//! The Python of a deck's code cells, as a Python language server has it: one
//! started in the environment the cells run in, as for the kernel, so that it
//! resolves the imports as the cells do when they run. It is the one the
//! `slides.pythonServer` setting names, or else the first of `SERVERS` that
//! the environment has, or the `PATH`. The server is told of a Python file of
//! a deck's cells, in order, each line where it is in the markdown and the
//! rest blank, which is never written to disk, so that its answers hold for
//! the markdown as they are. What the extension asks within the cells is
//! asked of it, and its diagnostics are shown on the markdown, but for an
//! expression left unused at the end of a cell, which the kernel shows.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use lsp_server::{ErrorCode, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};
use slides::markdown::{breaks, code_cells};
use slides::paths::parent;

use super::{Event, file_path, file_uri};
use crate::notebook::{FOREIGN, activated};

/// The servers looked for, in order, by the command that starts one.
const SERVERS: &[(&str, &[&str])] = &[
    ("basedpyright-langserver", &["--stdio"]),
    ("ty", &["server"]),
    ("pyright-langserver", &["--stdio"]),
    ("pyrefly", &["lsp"]),
];

/// The kinds of semantic tokens the deck declares, which each server's are
/// told as: those of the protocol, and those of a Python server that are not.
pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
    "selfParameter",
    "clsParameter",
    "builtinConstant",
];

/// The modifiers of semantic tokens the deck declares, as `TOKEN_TYPES`.
pub const TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
    "builtin",
    "classMember",
    "parameter",
];

/// What sends a message to the extension.
pub type ToClient = Arc<dyn Fn(Message) + Send + Sync>;

/// The environment the cells of the files in a directory run in.
enum Activation {
    Pending,
    Failed,
    Ready(Option<Arc<Activated>>),
}

/// An environment pinned by a lock file, activated.
pub struct Activated {
    lock: PathBuf,
    prefix: PathBuf,
    vars: HashMap<String, String>,
}

/// Activates the environment of the cells of the files in `dir`, which may
/// install it first, and so is done apart.
pub fn activate(dir: &Path) -> Result<Option<Activated>, String> {
    match activated(dir) {
        Ok(environment) => Ok(environment.map(|(lock, environment)| Activated {
            lock,
            prefix: environment.prefix,
            vars: environment.vars,
        })),
        Err(error) => Err(format!("{error:#}")),
    }
}

/// A Python file of a deck's cells, as a server knows it.
struct Shadow {
    uri: String,
    version: i32,
    text: String,
    /// The lines of its cells, which only its answers within are kept of.
    cells: Vec<(usize, usize)>,
}

/// A started server, for the decks whose cells run in one environment.
struct Server {
    name: String,
    child: Child,
    writer: Sender<Message>,
    /// Whether it has answered `initialize`, before which what it is told waits.
    ready: bool,
    /// What it said it does, in `initialize` or registering it since.
    capabilities: Value,
    queue: Vec<Message>,
    /// The cells' interpreter, which pyright asks for as `python.pythonPath`.
    python: Option<PathBuf>,
    /// The directories outside the workspace it was told of as folders.
    folders: HashSet<PathBuf>,
    next: i32,
}

impl Server {
    fn send(&mut self, message: Message) {
        if self.ready {
            let _ = self.writer.send(message);
        } else {
            self.queue.push(message);
        }
    }

    fn request(&mut self, method: &str, params: Value) -> RequestId {
        self.next += 1;
        let id = RequestId::from(self.next);
        self.send(Message::Request(Request::new(
            id.clone(),
            method.to_string(),
            params,
        )));
        id
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(Message::Notification(Notification::new(
            method.to_string(),
            params,
        )));
    }

    /// Whether it does what a method asks, as it said: as basedpyright does
    /// not select ranges, nor ty resolve its completions. Before it says,
    /// it is taken to.
    fn provides(&self, method: &str) -> bool {
        if !self.ready {
            return true;
        }
        let Some(path) = capability(method) else {
            return true;
        };
        path.iter()
            .try_fold(&self.capabilities, |value, key| value.get(*key))
            .is_some_and(|value| !value.is_null() && *value != Value::Bool(false))
    }
}

/// Where in a server's capabilities it says it does what a method asks.
fn capability(method: &str) -> Option<&'static [&'static str]> {
    Some(match method {
        "textDocument/completion" => &["completionProvider"],
        "completionItem/resolve" => &["completionProvider", "resolveProvider"],
        "textDocument/hover" => &["hoverProvider"],
        "textDocument/signatureHelp" => &["signatureHelpProvider"],
        "textDocument/definition" => &["definitionProvider"],
        "textDocument/typeDefinition" => &["typeDefinitionProvider"],
        "textDocument/declaration" => &["declarationProvider"],
        "textDocument/implementation" => &["implementationProvider"],
        "textDocument/references" => &["referencesProvider"],
        "textDocument/documentHighlight" => &["documentHighlightProvider"],
        "textDocument/selectionRange" => &["selectionRangeProvider"],
        "textDocument/inlayHint" => &["inlayHintProvider"],
        "inlayHint/resolve" => &["inlayHintProvider", "resolveProvider"],
        "textDocument/codeAction" => &["codeActionProvider"],
        "codeAction/resolve" => &["codeActionProvider", "resolveProvider"],
        "textDocument/prepareRename" => &["renameProvider", "prepareProvider"],
        "textDocument/rename" => &["renameProvider"],
        // As a server registers them, for both.
        "textDocument/semanticTokens" | "textDocument/semanticTokens/range" => {
            &["semanticTokensProvider"]
        }
        "textDocument/semanticTokens/full" => &["semanticTokensProvider", "full"],
        "textDocument/foldingRange" => &["foldingRangeProvider"],
        "textDocument/documentSymbol" => &["documentSymbolProvider"],
        "workspace/symbol" => &["workspaceSymbolProvider"],
        "textDocument/prepareCallHierarchy"
        | "callHierarchy/incomingCalls"
        | "callHierarchy/outgoingCalls" => &["callHierarchyProvider"],
        "textDocument/prepareTypeHierarchy"
        | "typeHierarchy/supertypes"
        | "typeHierarchy/subtypes" => &["typeHierarchyProvider"],
        _ => return None,
    })
}

/// A deck whose cells a server was told of.
struct Tracked {
    server: usize,
    shadow: Shadow,
}

/// What a request asked of a server was, to answer the extension with.
enum Asked {
    /// Its answer goes to the extension as it is.
    Plain,
    /// Completions, whose edits are kept within the cells.
    Completion { document: String },
    /// Locations, in the cells' file, which are in the markdown.
    Locations { document: String },
    /// Code actions, whose edits of the cells' file are of the markdown.
    Actions { document: String },
    /// Ranges, as of highlights, of which those in the cells are kept.
    Ranges { document: String },
    /// Inlay hints, of which those in the cells are kept.
    Hints { document: String },
    /// Semantic tokens, of the server's kinds, which are told as the deck's,
    /// in the cells, and on the lines asked for, if a range was.
    Tokens {
        document: String,
        types: Vec<Option<u32>>,
        modifiers: Vec<Option<u32>>,
        lines: Option<(usize, usize)>,
    },
    /// Folding ranges, of which those within a cell are kept.
    Folds { document: String },
    /// The symbols of the cells' file, of which those in the cells are kept,
    /// under the deck's headings they are under.
    Symbols {
        document: String,
        headings: Vec<Value>,
    },
    /// Call or type hierarchy items, which say which server gave them.
    Items,
    /// Calls, whose hierarchy items say which server gave them.
    Calls,
    /// Workspace symbols, one server's of those asked of all, of which those
    /// in the decks' cells are kept.
    Gather { key: i32 },
    /// A workspace edit, as a rename's, whose edits of the cells' file are of
    /// the markdown, and of other files as they are.
    Edit { document: String },
}

pub struct Pythons {
    /// The `slides.pythonServer` setting: a command, or `off`.
    setting: String,
    /// What the extension can do, which the servers are told, but for asking
    /// for their diagnostics, which they publish instead.
    capabilities: Value,
    roots: Vec<PathBuf>,
    events: Sender<Event>,
    client: ToClient,
    environments: HashMap<PathBuf, Activation>,
    servers: Vec<Option<Server>>,
    /// The server of each environment, by its lock file, or nothing where none
    /// was found.
    by_lock: HashMap<Option<PathBuf>, Option<usize>>,
    /// The decks the servers were told of, by uri.
    documents: HashMap<String, Tracked>,
    /// The extension's requests asked of a server, by the server and its id.
    pending: HashMap<(usize, RequestId), (RequestId, Asked)>,
    /// The workspace symbols asked of every server, by a key of their own:
    /// the extension's request, how many servers are still to answer, and
    /// the symbols the others did.
    gathers: HashMap<i32, (RequestId, usize, Vec<Value>)>,
    /// A server's requests asked of the extension, by their id there.
    forwarded: HashMap<RequestId, (usize, RequestId, Value)>,
    next: i32,
    /// The servers' diagnostics of each deck, by its uri.
    pub diagnostics: HashMap<String, Vec<Value>>,
}

impl Pythons {
    pub fn new(
        params: &Value,
        roots: Vec<PathBuf>,
        events: Sender<Event>,
        client: ToClient,
    ) -> Pythons {
        let mut capabilities = params["capabilities"].clone();
        if let Some(text) = capabilities["textDocument"].as_object_mut() {
            text.remove("diagnostic");
        }
        if let Some(workspace) = capabilities["workspace"].as_object_mut() {
            workspace.remove("diagnostics");
        }
        let setting = params["initializationOptions"]["pythonServer"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        Pythons {
            setting,
            capabilities,
            roots,
            events,
            client,
            environments: HashMap::new(),
            servers: Vec::new(),
            by_lock: HashMap::new(),
            documents: HashMap::new(),
            pending: HashMap::new(),
            gathers: HashMap::new(),
            forwarded: HashMap::new(),
            next: 0,
            diagnostics: HashMap::new(),
        }
    }

    /// Tells the server of a deck's cells as they are now, starting the server
    /// of its environment the first time one of its decks asks.
    pub fn track(&mut self, uri: &str, text: &str) {
        if self.setting == "off" {
            return;
        }
        let Some(path) = file_path(uri) else {
            return;
        };
        let dir = parent(&path).to_path_buf();
        let environment = match self.environments.get(&dir) {
            Some(Activation::Ready(environment)) => environment.clone(),
            Some(Activation::Pending | Activation::Failed) => return,
            None => {
                self.environments.insert(dir.clone(), Activation::Pending);
                let events = self.events.clone();
                std::thread::spawn(move || {
                    let activated = activate(&dir);
                    let _ = events.send(Event::Activated(dir, activated));
                });
                return;
            }
        };
        let lock = environment
            .as_ref()
            .map(|environment| environment.lock.clone());
        let server = match self.by_lock.get(&lock) {
            Some(server) => *server,
            None => {
                let server = self.launch(environment.as_deref());
                self.by_lock.insert(lock, server);
                server
            }
        };
        if let Some(server) = server {
            self.sync(uri, &path, text, server);
        }
    }

    /// The environment of a directory, activated, for its decks to be tracked
    /// again.
    pub fn activated(&mut self, dir: PathBuf, activated: Result<Option<Activated>, String>) {
        let activation = match activated {
            Ok(environment) => Activation::Ready(environment.map(Arc::new)),
            Err(error) => {
                eprintln!(
                    "{}: could not activate the environment of its cells: {error}",
                    dir.display()
                );
                Activation::Failed
            }
        };
        self.environments.insert(dir, activation);
    }

    /// Forgets a deck once it closes.
    pub fn close(&mut self, uri: &str) {
        let Some(tracked) = self.documents.remove(uri) else {
            return;
        };
        self.diagnostics.remove(uri);
        if let Some(server) = self.servers[tracked.server].as_mut() {
            server.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": tracked.shadow.uri } }),
            );
        }
    }

    /// Starts the server of an environment, or of the one the extension runs
    /// in without one.
    fn launch(&mut self, environment: Option<&Activated>) -> Option<usize> {
        let mut vars: HashMap<String, String> = std::env::vars().collect();
        let where_ = match environment {
            Some(environment) => {
                for name in FOREIGN {
                    vars.remove(*name);
                }
                vars.extend(environment.vars.clone());
                environment.lock.display().to_string()
            }
            None => "the environment the extension runs in".to_string(),
        };
        let path = vars.get("PATH").cloned().unwrap_or_default();
        let Some((name, command, args)) = choose(&self.setting, &path) else {
            if self.setting.is_empty() {
                eprintln!(
                    "no Python language server for {where_}: install basedpyright, ty or pyrefly in it, for the code cells to have one"
                );
            } else {
                eprintln!(
                    "the Python language server `{}` is not in {where_} nor on the PATH",
                    self.setting
                );
            }
            return None;
        };
        let mut process = Command::new(&command);
        process
            .args(&args)
            .env_clear()
            .envs(&vars)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(environment) = environment {
            process.current_dir(parent(&environment.lock));
        }
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(error) => {
                eprintln!(
                    "could not start {name}, {}, for {where_}: {error}",
                    command.display()
                );
                return None;
            }
        };
        let index = self.servers.len();
        let (writer, written) = std::sync::mpsc::channel::<Message>();
        let mut stdin = BufWriter::new(child.stdin.take().unwrap());
        std::thread::spawn(move || {
            for message in written {
                if message.write(&mut stdin).is_err() {
                    break;
                }
            }
        });
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let events = self.events.clone();
        std::thread::spawn(move || {
            while let Ok(Some(message)) = Message::read(&mut stdout) {
                if events.send(Event::Python(index, message)).is_err() {
                    return;
                }
            }
            let _ = events.send(Event::Exited(index));
        });
        let python = environment.and_then(|environment| python_of(&environment.prefix));
        let server = Server {
            name: name.clone(),
            child,
            writer,
            ready: false,
            capabilities: Value::Null,
            queue: Vec::new(),
            python,
            folders: HashSet::new(),
            next: 0,
        };
        let folders: Vec<Value> = self
            .roots
            .iter()
            .map(|root| json!({ "uri": file_uri(root), "name": root.file_name().map(|name| name.to_string_lossy()) }))
            .collect();
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "slides-rs" },
            "rootUri": self.roots.first().map(|root| file_uri(root)),
            "workspaceFolders": folders,
            "capabilities": self.capabilities,
        });
        // Sent ahead of what waits for its answer.
        let initialize = Request::new(RequestId::from(0), "initialize".to_string(), params);
        let _ = server.writer.send(Message::Request(initialize));
        eprintln!("started {name}, {}, for {where_}", command.display());
        self.servers.push(Some(server));
        Some(index)
    }

    /// Tells the server of the deck's cells as they are now.
    fn sync(&mut self, uri: &str, path: &Path, text: &str, index: usize) {
        let Some(server) = self.servers[index].as_mut() else {
            return;
        };
        let (python, cells) = shadow_text(text);
        match self.documents.get_mut(uri) {
            Some(tracked) if tracked.server == index => {
                tracked.shadow.cells = cells;
                if tracked.shadow.text != python {
                    tracked.shadow.text = python;
                    tracked.shadow.version += 1;
                    server.notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": tracked.shadow.uri, "version": tracked.shadow.version },
                            "contentChanges": [{ "text": tracked.shadow.text }],
                        }),
                    );
                }
            }
            _ => {
                // A server may check only the files in its folders, as ty does.
                let dir = parent(path).to_path_buf();
                if !self.roots.iter().any(|root| dir.starts_with(root))
                    && server.folders.insert(dir.clone())
                {
                    server.notify(
                        "workspace/didChangeWorkspaceFolders",
                        json!({ "event": { "added": [{ "uri": file_uri(&dir), "name": dir.file_name().map(|name| name.to_string_lossy()) }], "removed": [] } }),
                    );
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let shadow = Shadow {
                    uri: file_uri(&dir.join(format!(".{name}.py"))),
                    version: 1,
                    text: python,
                    cells,
                };
                server.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": { "uri": shadow.uri, "languageId": "python", "version": 1, "text": shadow.text } }),
                );
                let ready = server.ready;
                self.documents.insert(
                    uri.to_string(),
                    Tracked {
                        server: index,
                        shadow,
                    },
                );
                if ready {
                    self.refresh();
                }
            }
        }
    }

    /// Asks the deck's server what the extension asks within its cells, and
    /// whether it did: outside them, or without a server, the deck answers.
    pub fn forward(&mut self, request: &Request, text: &str) -> bool {
        let method = request.method.as_str();
        match method {
            "completionItem/resolve" | "codeAction/resolve" | "inlayHint/resolve" => {
                return self.resolve(request);
            }
            "callHierarchy/incomingCalls"
            | "callHierarchy/outgoingCalls"
            | "typeHierarchy/supertypes"
            | "typeHierarchy/subtypes" => return self.hierarchy(request),
            "workspace/symbol" => return self.workspace_symbols(request),
            _ => {}
        }
        if !matches!(
            method,
            "textDocument/completion"
                | "textDocument/hover"
                | "textDocument/signatureHelp"
                | "textDocument/definition"
                | "textDocument/typeDefinition"
                | "textDocument/declaration"
                | "textDocument/implementation"
                | "textDocument/references"
                | "textDocument/documentHighlight"
                | "textDocument/selectionRange"
                | "textDocument/inlayHint"
                | "textDocument/codeAction"
                | "textDocument/prepareRename"
                | "textDocument/rename"
                | "textDocument/semanticTokens/full"
                | "textDocument/semanticTokens/range"
                | "textDocument/foldingRange"
                | "textDocument/documentSymbol"
                | "textDocument/prepareCallHierarchy"
                | "textDocument/prepareTypeHierarchy"
        ) {
            return false;
        }
        let Some(uri) = request.params["textDocument"]["uri"]
            .as_str()
            .map(String::from)
        else {
            return false;
        };
        // What the editor asks of every document it shows, rather than of
        // what the cursor is on, starts no server for a markdown file that is
        // not a deck: a deck's is started as its slides are checked.
        let unasked = matches!(
            method,
            "textDocument/inlayHint"
                | "textDocument/semanticTokens/full"
                | "textDocument/semanticTokens/range"
                | "textDocument/foldingRange"
                | "textDocument/documentSymbol"
        );
        if unasked && !self.documents.contains_key(&uri) {
            return false;
        }
        self.track(&uri, text);
        let Some(tracked) = self.documents.get(&uri) else {
            return false;
        };
        if !asks_of_cells(method, &request.params, &tracked.shadow.cells) {
            return false;
        }
        let index = tracked.server;
        let Some(server) = self.servers[index].as_ref() else {
            return false;
        };
        if !server.provides(method) {
            return false;
        }
        let mut params = request.params.clone();
        params["textDocument"]["uri"] = json!(tracked.shadow.uri);
        let mut method = method;
        let asked = match method {
            "textDocument/semanticTokens/full" | "textDocument/semanticTokens/range" => {
                // Its kinds are not known before it says them.
                if !server.ready {
                    return false;
                }
                let provider = &server.capabilities["semanticTokensProvider"];
                let lines = (method == "textDocument/semanticTokens/range").then(|| {
                    let line = |at: &str| params["range"][at]["line"].as_u64().unwrap_or(0);
                    (line("start") as usize, line("end") as usize)
                });
                // Asked for the whole file, of a server that has no range.
                if lines.is_some() && !provider["range"].as_bool().unwrap_or(false) {
                    method = "textDocument/semanticTokens/full";
                    if let Some(params) = params.as_object_mut() {
                        params.remove("range");
                    }
                }
                Asked::Tokens {
                    document: uri,
                    types: legend(&provider["legend"]["tokenTypes"], TOKEN_TYPES),
                    modifiers: legend(&provider["legend"]["tokenModifiers"], TOKEN_MODIFIERS),
                    lines,
                }
            }
            "textDocument/foldingRange" => Asked::Folds { document: uri },
            "textDocument/documentSymbol" => Asked::Symbols {
                document: uri,
                headings: outline(text, Vec::new()),
            },
            "textDocument/prepareCallHierarchy" | "textDocument/prepareTypeHierarchy" => {
                Asked::Items
            }
            "textDocument/completion" => Asked::Completion { document: uri },
            "textDocument/codeAction" => Asked::Actions { document: uri },
            "textDocument/documentHighlight" => Asked::Ranges { document: uri },
            "textDocument/inlayHint" => Asked::Hints { document: uri },
            "textDocument/rename" => Asked::Edit { document: uri },
            "textDocument/definition"
            | "textDocument/typeDefinition"
            | "textDocument/declaration"
            | "textDocument/implementation"
            | "textDocument/references" => Asked::Locations { document: uri },
            _ => Asked::Plain,
        };
        let Some(server) = self.servers[index].as_mut() else {
            return false;
        };
        let id = server.request(method, params);
        self.pending
            .insert((index, id), (request.id.clone(), asked));
        true
    }

    /// Resolves a completion, a code action or an inlay hint a server gave,
    /// which says which it was.
    fn resolve(&mut self, request: &Request) -> bool {
        let method = request.method.as_str();
        let given = &request.params["data"];
        let (Some(index), Some(document)) = (
            given["slidesServer"].as_u64(),
            given["slidesDocument"].as_str(),
        ) else {
            return false;
        };
        let index = index as usize;
        let mut item = request.params.clone();
        item["data"] = given["data"].clone();
        let document = document.to_string();
        let Some(server) = self.servers.get_mut(index).and_then(Option::as_mut) else {
            return false;
        };
        // A server that does not resolve what it gives gave it whole.
        if !server.provides(method) {
            return false;
        }
        let asked = match method {
            "codeAction/resolve" => Asked::Actions { document },
            "inlayHint/resolve" => Asked::Hints { document },
            _ => Asked::Completion { document },
        };
        let id = server.request(method, item);
        self.pending
            .insert((index, id), (request.id.clone(), asked));
        true
    }

    /// The calls or the types of a call or type hierarchy item, asked of the
    /// server that gave it.
    fn hierarchy(&mut self, request: &Request) -> bool {
        let method = request.method.as_str();
        let item = &request.params["item"];
        let Some(index) = item["data"]["slidesServer"].as_u64() else {
            return false;
        };
        let index = index as usize;
        let mut params = request.params.clone();
        params["item"]["data"] = item["data"]["data"].clone();
        if let Some(tracked) = item["uri"].as_str().and_then(|uri| self.documents.get(uri))
            && tracked.server == index
        {
            params["item"]["uri"] = json!(tracked.shadow.uri);
        }
        let Some(server) = self.servers.get_mut(index).and_then(Option::as_mut) else {
            return false;
        };
        if !server.provides(method) {
            return false;
        }
        let asked = if method.starts_with("callHierarchy/") {
            Asked::Calls
        } else {
            Asked::Items
        };
        let id = server.request(method, params);
        self.pending
            .insert((index, id), (request.id.clone(), asked));
        true
    }

    /// The symbols of the decks' cells, asked of every server that was told
    /// of a deck, and answered once all have.
    fn workspace_symbols(&mut self, request: &Request) -> bool {
        let asked: Vec<usize> = (0..self.servers.len())
            .filter(|&index| {
                self.servers[index]
                    .as_ref()
                    .is_some_and(|server| server.ready && server.provides(&request.method))
                    && self
                        .documents
                        .values()
                        .any(|tracked| tracked.server == index)
            })
            .collect();
        if asked.is_empty() {
            return false;
        }
        self.next += 1;
        let key = self.next;
        self.gathers
            .insert(key, (request.id.clone(), asked.len(), Vec::new()));
        for index in asked {
            if let Some(server) = self.servers[index].as_mut() {
                let id = server.request(&request.method, request.params.clone());
                self.pending
                    .insert((index, id), (request.id.clone(), Asked::Gather { key }));
            }
        }
        true
    }

    /// One server's workspace symbols, answering the extension once all the
    /// servers asked have.
    fn gathered(&mut self, key: i32, symbols: Vec<Value>) {
        let Some((_, remaining, found)) = self.gathers.get_mut(&key) else {
            return;
        };
        found.extend(symbols);
        *remaining -= 1;
        if *remaining == 0
            && let Some((id, _, found)) = self.gathers.remove(&key)
        {
            (self.client)(Message::Response(Response::new_ok(id, found)));
        }
    }

    /// Has the extension ask again for what it asks of every document it
    /// shows, as it does not once a deck has a server to answer.
    fn refresh(&mut self) {
        for kind in ["inlayHint", "semanticTokens", "foldingRange"] {
            if self.capabilities["workspace"][kind]["refreshSupport"] == true {
                self.next += 1;
                (self.client)(Message::Request(Request::new(
                    RequestId::from(format!("python-{}", self.next)),
                    format!("workspace/{kind}/refresh"),
                    Value::Null,
                )));
            }
        }
    }

    /// The decks, by the uri of their cells' file, that a server was told of.
    fn decks_of(&self, index: usize) -> HashMap<String, String> {
        self.documents
            .iter()
            .filter(|(_, tracked)| tracked.server == index)
            .map(|(uri, tracked)| (tracked.shadow.uri.clone(), uri.clone()))
            .collect()
    }

    /// Cancels what servers were asked, once the extension cancels it.
    pub fn cancel(&mut self, id: &RequestId) {
        let asked: Vec<(usize, RequestId)> = self
            .pending
            .iter()
            .filter(|(_, (client, _))| client == id)
            .map(|(key, _)| key.clone())
            .collect();
        for (index, asked) in asked {
            if let Some(server) = self.servers[index].as_mut() {
                server.notify("$/cancelRequest", json!({ "id": asked }));
            }
        }
    }

    /// Handles what a server sends, and returns the deck whose diagnostics it
    /// changed, if it did.
    pub fn received(&mut self, index: usize, message: Message) -> Option<String> {
        match message {
            Message::Response(response) => {
                self.answered(index, response);
                None
            }
            Message::Request(request) => {
                self.asked(index, request);
                None
            }
            Message::Notification(notification) => self.notified(index, notification),
        }
    }

    fn answered(&mut self, index: usize, response: Response) {
        if response.id == RequestId::from(0) {
            let Some(server) = self.servers[index].as_mut() else {
                return;
            };
            let result = match response.response_result {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("{} could not start: {}", server.name, error.message);
                    self.stop(index);
                    return;
                }
            };
            server.capabilities = result["capabilities"].clone();
            let _ = server.writer.send(Message::Notification(Notification::new(
                "initialized".to_string(),
                json!({}),
            )));
            server.ready = true;
            for message in std::mem::take(&mut server.queue) {
                let _ = server.writer.send(message);
            }
            // The decks asked before the server could answer.
            self.refresh();
            return;
        }
        let Some((id, asked)) = self.pending.remove(&(index, response.id)) else {
            return;
        };
        let result = match response.response_result {
            // What a server does not do, it has nothing of.
            Err(error) if error.code == ErrorCode::MethodNotFound as i32 => Ok(Value::Null),
            result => result,
        };
        if let Asked::Gather { key } = asked {
            let decks = self.decks_of(index);
            let symbols = match result {
                Ok(Value::Array(symbols)) => in_decks_symbols(symbols, &decks),
                _ => Vec::new(),
            };
            self.gathered(key, symbols);
            return;
        }
        let response = match result {
            Err(error) => Response {
                id,
                response_result: Err(error),
            },
            Ok(mut result) => {
                match asked {
                    Asked::Plain => {}
                    Asked::Completion { document } => {
                        self.completions(index, &document, &mut result)
                    }
                    Asked::Actions { document } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            actions(&mut result, index, &document, &tracked.shadow);
                        }
                    }
                    Asked::Locations { document } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            in_document(&mut result, &tracked.shadow.uri, &document);
                        }
                    }
                    Asked::Ranges { document } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            ranges_in_cells(&mut result, &tracked.shadow.cells);
                        }
                    }
                    Asked::Hints { document } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            hints(&mut result, index, &document, &tracked.shadow);
                        }
                    }
                    Asked::Tokens {
                        document,
                        types,
                        modifiers,
                        lines,
                    } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            tokens(
                                &mut result,
                                &types,
                                &modifiers,
                                &tracked.shadow.cells,
                                lines,
                            );
                        }
                    }
                    Asked::Folds { document } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            folds_in_cells(&mut result, &tracked.shadow.cells);
                        }
                    }
                    Asked::Symbols { document, headings } => {
                        if let Some(tracked) = self.documents.get(&document) {
                            symbols_in_cells(&mut result, &tracked.shadow, &document);
                        }
                        let symbols = match result {
                            Value::Array(symbols) => symbols,
                            _ => Vec::new(),
                        };
                        result = json!(nest(headings, symbols));
                    }
                    Asked::Items => {
                        let decks = self.decks_of(index);
                        if let Value::Array(items) = &mut result {
                            for item in items {
                                hierarchy_item(item, index, &decks);
                            }
                        }
                    }
                    Asked::Calls => {
                        let decks = self.decks_of(index);
                        if let Value::Array(calls) = &mut result {
                            for call in calls {
                                for key in ["from", "to"] {
                                    if let Some(item) = call.get_mut(key) {
                                        hierarchy_item(item, index, &decks);
                                    }
                                }
                            }
                        }
                    }
                    Asked::Gather { .. } => {}
                    Asked::Edit { document } => {
                        if let Some(tracked) = self.documents.get(&document)
                            && result.is_object()
                        {
                            let shadow = &tracked.shadow;
                            in_deck_edit(&mut result, &shadow.uri, &document, &shadow.cells);
                        }
                    }
                }
                Response::new_ok(id, result)
            }
        };
        (self.client)(Message::Response(response));
    }

    /// The completions of a deck's cells, each saying which server gave it, to
    /// be resolved, and with its edits within the cells.
    fn completions(&self, index: usize, document: &str, result: &mut Value) {
        let Some(tracked) = self.documents.get(document) else {
            return;
        };
        // A resolved item.
        if result.get("label").is_some() {
            completion(result, index, document, &tracked.shadow.cells);
            return;
        }
        let items = match result {
            Value::Array(items) => items,
            Value::Object(list) => match list.get_mut("items") {
                Some(Value::Array(items)) => items,
                _ => return,
            },
            _ => return,
        };
        for item in items.iter_mut() {
            completion(item, index, document, &tracked.shadow.cells);
        }
    }

    /// Answers what a server asks of the extension.
    fn asked(&mut self, index: usize, request: Request) {
        if request.method == "client/registerCapability"
            && let Some(server) = self.servers[index].as_mut()
        {
            // As it said it does in `initialize`.
            let registrations = request.params["registrations"].as_array();
            for registration in registrations.into_iter().flatten() {
                let method = registration["method"].as_str().unwrap_or("");
                if let Some(&[key, ..]) = capability(method)
                    && let Some(capabilities) = server.capabilities.as_object_mut()
                {
                    let options = match &registration["registerOptions"] {
                        Value::Null => json!(true),
                        options => options.clone(),
                    };
                    capabilities.insert(key.to_string(), options);
                }
            }
        }
        if matches!(
            request.method.as_str(),
            "workspace/configuration"
                | "workspace/inlayHint/refresh"
                | "workspace/semanticTokens/refresh"
                | "workspace/foldingRange/refresh"
        ) {
            // Asked of the extension: its configuration, with the cells'
            // interpreter for pyright, or to ask for the inlay hints, tokens
            // or folding ranges again.
            self.next += 1;
            let id = RequestId::from(format!("python-{}", self.next));
            self.forwarded
                .insert(id.clone(), (index, request.id, request.params.clone()));
            (self.client)(Message::Request(Request::new(
                id,
                request.method,
                request.params,
            )));
            return;
        }
        // As registering what it watches, or a progress: taken, but not done.
        if let Some(server) = self.servers[index].as_mut() {
            let _ = server
                .writer
                .send(Message::Response(Response::new_ok(request.id, Value::Null)));
        }
    }

    /// The extension's answer to what a server asked of it, if it was a
    /// server's.
    pub fn client_answered(&mut self, response: Response) {
        let Some((index, id, params)) = self.forwarded.remove(&response.id) else {
            return;
        };
        let Some(server) = self.servers[index].as_mut() else {
            return;
        };
        let mut result = response.response_result.unwrap_or(Value::Null);
        if let (Some(python), Value::Array(values)) = (&server.python, &mut result) {
            let items = params["items"].as_array().cloned().unwrap_or_default();
            for (item, value) in items.iter().zip(values.iter_mut()) {
                if item["section"] == "python" {
                    if !value.is_object() {
                        *value = json!({});
                    }
                    value["pythonPath"] = json!(python);
                }
            }
        }
        let _ = server
            .writer
            .send(Message::Response(Response::new_ok(id, result)));
    }

    fn notified(&mut self, index: usize, notification: Notification) -> Option<String> {
        let name = self.servers[index]
            .as_ref()
            .map_or("", |server| server.name.as_str());
        match notification.method.as_str() {
            "textDocument/publishDiagnostics" => {
                let uri = notification.params["uri"].as_str()?;
                let (document, tracked) = self
                    .documents
                    .iter()
                    .find(|(_, tracked)| tracked.server == index && tracked.shadow.uri == uri)?;
                let diagnostics = notification.params["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let kept = kept(&tracked.shadow, diagnostics);
                let document = document.clone();
                self.diagnostics.insert(document.clone(), kept);
                Some(document)
            }
            "window/logMessage" | "window/showMessage" => {
                let message = notification.params["message"].as_str().unwrap_or("");
                eprintln!("{name}: {message}");
                None
            }
            _ => None,
        }
    }

    /// Forgets a server that exited, and the decks it was told of, which have
    /// none until the extension restarts.
    pub fn exited(&mut self, index: usize) -> Vec<String> {
        if let Some(server) = &self.servers[index] {
            eprintln!("{} exited", server.name);
        }
        self.stop(index);
        let documents: Vec<String> = self
            .documents
            .iter()
            .filter(|(_, tracked)| tracked.server == index)
            .map(|(uri, _)| uri.clone())
            .collect();
        for uri in &documents {
            self.documents.remove(uri);
            self.diagnostics.remove(uri);
        }
        let pending: Vec<(usize, RequestId)> = self
            .pending
            .keys()
            .filter(|(server, _)| *server == index)
            .cloned()
            .collect();
        for key in pending {
            match self.pending.remove(&key) {
                Some((_, Asked::Gather { key })) => self.gathered(key, Vec::new()),
                Some((id, _)) => {
                    (self.client)(Message::Response(Response::new_ok(id, Value::Null)))
                }
                None => {}
            }
        }
        documents
    }

    fn stop(&mut self, index: usize) {
        if let Some(mut server) = self.servers[index].take() {
            let _ = server.child.kill();
            let _ = server.child.wait();
        }
    }

    /// Stops every server, as the extension does.
    pub fn shutdown(&mut self) {
        for index in 0..self.servers.len() {
            self.stop(index);
        }
    }
}

/// A completion of a deck's cells, saying which server gave it, with its
/// edits within the cells.
fn completion(item: &mut Value, index: usize, document: &str, cells: &[(usize, usize)]) {
    let data = item.get("data").cloned().unwrap_or(Value::Null);
    item["data"] = json!({ "slidesServer": index, "slidesDocument": document, "data": data });
    if let Some(edits) = item
        .get_mut("additionalTextEdits")
        .and_then(Value::as_array_mut)
    {
        *edits = keep_in_cells(std::mem::take(edits), cells);
    }
}

/// The edits of a completion that are in the cells: an import added at the
/// top of the file goes at the top of the first cell, and any other edit,
/// where the markdown is, is left out.
fn keep_in_cells(edits: Vec<Value>, cells: &[(usize, usize)]) -> Vec<Value> {
    let Some(&(first, _)) = cells.first() else {
        return Vec::new();
    };
    edits
        .into_iter()
        .filter_map(|mut edit| {
            let range = &edit["range"];
            let start = range["start"]["line"].as_u64()? as usize;
            let end = range["end"]["line"].as_u64()? as usize;
            if range["start"] == range["end"] && start == 0 {
                let top = json!({ "line": first, "character": 0 });
                edit["range"] = json!({ "start": top, "end": top });
                return Some(edit);
            }
            cells
                .iter()
                .any(|&(from, to)| from <= start && end < to)
                .then_some(edit)
        })
        .collect()
}

/// Code actions on a deck's cells, or one resolved, each saying which server
/// gave it, to be resolved, and with its edits of the cells' file made of the
/// markdown, within the cells.
fn actions(result: &mut Value, index: usize, document: &str, shadow: &Shadow) {
    let actions = match result {
        Value::Array(actions) => actions,
        Value::Object(_) => std::slice::from_mut(result),
        _ => return,
    };
    for action in actions.iter_mut() {
        // A command alone, run by the server, has no edits nor data.
        if action.get("title").is_none() || action["command"].is_string() {
            continue;
        }
        let data = action.get("data").cloned().unwrap_or(Value::Null);
        action["data"] = json!({ "slidesServer": index, "slidesDocument": document, "data": data });
        if let Some(edit) = action.get_mut("edit") {
            in_deck_edit(edit, &shadow.uri, document, &shadow.cells);
        }
    }
}

/// A workspace edit of the cells' file, made of the markdown, within the cells.
fn in_deck_edit(edit: &mut Value, shadow: &str, document: &str, cells: &[(usize, usize)]) {
    if let Some(changes) = edit.get_mut("changes").and_then(Value::as_object_mut)
        && let Some(Value::Array(edits)) = changes.remove(shadow)
    {
        changes.insert(document.to_string(), json!(keep_in_cells(edits, cells)));
    }
    if let Some(Value::Array(changes)) = edit.get_mut("documentChanges") {
        for change in changes.iter_mut() {
            if change["textDocument"]["uri"].as_str() != Some(shadow) {
                continue;
            }
            // The version is the server's of the cells' file, not the deck's.
            change["textDocument"] = json!({ "uri": document, "version": null });
            if let Some(Value::Array(edits)) = change.get_mut("edits") {
                *edits = keep_in_cells(std::mem::take(edits), cells);
            }
        }
    }
}

/// Locations in the cells' file, which are in the markdown.
fn in_document(result: &mut Value, shadow: &str, document: &str) {
    match result {
        Value::Array(locations) => locations
            .iter_mut()
            .for_each(|location| in_document(location, shadow, document)),
        Value::Object(location) => {
            for key in ["uri", "targetUri"] {
                if location.get(key).and_then(Value::as_str) == Some(shadow) {
                    location.insert(key.to_string(), json!(document));
                }
            }
        }
        _ => {}
    }
}

/// Inlay hints on a deck's cells, or one resolved: those in the cells, each
/// saying which server gave it, to be resolved, with its edits within the
/// cells and the locations of its label in the markdown.
fn hints(result: &mut Value, index: usize, document: &str, shadow: &Shadow) {
    let hints = match result {
        Value::Array(hints) => {
            hints.retain(|hint| {
                let line = hint["position"]["line"].as_u64();
                line.is_some_and(|line| in_cells(&shadow.cells, line as usize))
            });
            hints
        }
        Value::Object(_) => std::slice::from_mut(result),
        _ => return,
    };
    for hint in hints.iter_mut() {
        let data = hint.get("data").cloned().unwrap_or(Value::Null);
        hint["data"] = json!({ "slidesServer": index, "slidesDocument": document, "data": data });
        if let Some(edits) = hint.get_mut("textEdits").and_then(Value::as_array_mut) {
            *edits = keep_in_cells(std::mem::take(edits), &shadow.cells);
        }
        if let Some(parts) = hint.get_mut("label").and_then(Value::as_array_mut) {
            for part in parts {
                if let Some(location) = part.get_mut("location") {
                    in_document(location, &shadow.uri, document);
                }
            }
        }
    }
}

/// Whether what the extension asks with these parameters is of the cells: at
/// a position in them, or for a range that overlaps them, as inlay hints are
/// asked for what the editor shows.
fn asks_of_cells(method: &str, params: &Value, cells: &[(usize, usize)]) -> bool {
    let line = |at: &Value| at["line"].as_u64().map(|line| line as usize);
    match method {
        "textDocument/codeAction" => starts_in_cells(&params["range"], cells),
        // Each of the positions, or else the editor selects as it would.
        "textDocument/selectionRange" => params["positions"].as_array().is_some_and(|positions| {
            !positions.is_empty()
                && positions
                    .iter()
                    .all(|at| line(at).is_some_and(|line| in_cells(cells, line)))
        }),
        // Of the whole file.
        "textDocument/semanticTokens/full"
        | "textDocument/foldingRange"
        | "textDocument/documentSymbol" => !cells.is_empty(),
        "textDocument/inlayHint" | "textDocument/semanticTokens/range" => {
            let (Some(start), Some(end)) = (
                line(&params["range"]["start"]),
                line(&params["range"]["end"]),
            ) else {
                return false;
            };
            cells.iter().any(|&(from, to)| from <= end && start < to)
        }
        _ => line(&params["position"]).is_some_and(|line| in_cells(cells, line)),
    }
}

/// The index in `ours` of each of a server's names, of its semantic tokens'
/// kinds or modifiers.
fn legend(names: &Value, ours: &[&str]) -> Vec<Option<u32>> {
    names
        .as_array()
        .into_iter()
        .flatten()
        .map(|name| {
            let name = name.as_str()?;
            ours.iter()
                .position(|our| *our == name)
                .map(|index| index as u32)
        })
        .collect()
}

/// Semantic tokens of the cells' file, as `{ data }`, with the deck's kinds
/// and modifiers: those of a kind the deck has, in the cells, and on the
/// lines asked for, if any.
fn tokens(
    result: &mut Value,
    types: &[Option<u32>],
    modifiers: &[Option<u32>],
    cells: &[(usize, usize)],
    lines: Option<(usize, usize)>,
) {
    let Some(data) = result.get_mut("data").and_then(Value::as_array_mut) else {
        return;
    };
    let numbers: Vec<u64> = data.iter().map(|n| n.as_u64().unwrap_or(0)).collect();
    let (mut line, mut start) = (0, 0);
    let (mut last_line, mut last_start) = (0, 0);
    let mut kept = Vec::new();
    for &[delta_line, delta_start, length, kind, bits] in numbers.as_chunks::<5>().0 {
        if delta_line > 0 {
            line += delta_line;
            start = delta_start;
        } else {
            start += delta_start;
        }
        let Some(Some(kind)) = types.get(kind as usize) else {
            continue;
        };
        let at = line as usize;
        if !in_cells(cells, at) || lines.is_some_and(|(from, to)| at < from || to < at) {
            continue;
        }
        let bits = (0..modifiers.len().min(64))
            .filter(|bit| bits & (1 << bit) != 0)
            .filter_map(|bit| modifiers[bit])
            .fold(0u64, |bits, ours| bits | (1 << ours));
        let delta_start = if line == last_line {
            start - last_start
        } else {
            start
        };
        kept.extend([
            line - last_line,
            delta_start,
            length,
            u64::from(*kind),
            bits,
        ]);
        (last_line, last_start) = (line, start);
    }
    *data = kept.into_iter().map(Value::from).collect();
}

/// The folding ranges that are within a cell.
fn folds_in_cells(result: &mut Value, cells: &[(usize, usize)]) {
    if let Value::Array(folds) = result {
        folds.retain(|fold| {
            let line = |key: &str| fold[key].as_u64().map(|line| line as usize);
            let (Some(start), Some(end)) = (line("startLine"), line("endLine")) else {
                return false;
            };
            cells.iter().any(|&(from, to)| from <= start && end < to)
        });
    }
}

/// The symbols of the cells' file that are in the cells, as document symbols
/// or as symbols with their location, which is then in the markdown.
fn symbols_in_cells(result: &mut Value, shadow: &Shadow, document: &str) {
    let Value::Array(symbols) = result else {
        return;
    };
    symbols.retain(|symbol| match symbol.get("location") {
        Some(location) => {
            location["uri"].as_str() == Some(shadow.uri.as_str())
                && starts_in_cells(&location["range"], &shadow.cells)
        }
        None => starts_in_cells(&symbol["range"], &shadow.cells),
    });
    for symbol in symbols {
        if let Some(location) = symbol.get_mut("location") {
            location["uri"] = json!(document);
        }
    }
}

/// The outline of a deck: its slides' headings, each over what follows it up
/// to the next of its level or above, with the symbols of its cells, as
/// document symbols or with their location, under the heading they are in.
pub fn outline(markdown: &str, symbols: Vec<Value>) -> Vec<Value> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let end_of = |line: usize| {
        let text = lines.get(line).copied().unwrap_or("");
        json!({ "line": line, "character": text.trim_end_matches('\r').encode_utf16().count() })
    };
    let headings = breaks(markdown).headings;
    let mut outline = Vec::new();
    for (i, heading) in headings.iter().enumerate() {
        let end = headings[i + 1..]
            .iter()
            .find(|next| next.level <= heading.level)
            .map_or(lines.len().saturating_sub(1), |next| {
                next.line.saturating_sub(1)
            })
            .max(heading.last);
        let start = json!({ "line": heading.line, "character": 0 });
        outline.push(json!({
            "name": heading_name(lines.get(heading.line).copied().unwrap_or("")),
            // As the editor's own markdown outline has them.
            "kind": 15,
            "range": { "start": start, "end": end_of(end) },
            "selectionRange": { "start": start, "end": end_of(heading.last) },
        }));
    }
    outline.extend(symbols);
    nest(Vec::new(), outline)
}

/// The text of a heading's line, without its `#`s nor its attributes.
fn heading_name(line: &str) -> String {
    let text = line
        .trim()
        .trim_start_matches('#')
        .trim_end_matches('#')
        .trim();
    let text = match text.rfind('{') {
        Some(at) if text.ends_with('}') => text[..at].trim_end(),
        _ => text,
    };
    if text.is_empty() {
        line.trim().to_string()
    } else {
        text.to_string()
    }
}

/// Document symbols, each under the deepest of `outline`, or of the symbols
/// before it, whose range holds it. A symbol with its location is made one.
fn nest(mut outline: Vec<Value>, symbols: Vec<Value>) -> Vec<Value> {
    let mut symbols: Vec<Value> = symbols
        .into_iter()
        .map(|mut symbol| {
            if let Some(location) = symbol.as_object_mut().and_then(|s| s.remove("location")) {
                symbol["range"] = location["range"].clone();
                symbol["selectionRange"] = location["range"].clone();
                if let Some(symbol) = symbol.as_object_mut() {
                    symbol.remove("containerName");
                }
            }
            symbol
        })
        .collect();
    let at = |position: &Value| {
        (
            position["line"].as_u64().unwrap_or(0),
            position["character"].as_u64().unwrap_or(0),
        )
    };
    symbols.sort_by_key(|symbol| {
        let range = &symbol["range"];
        (at(&range["start"]), std::cmp::Reverse(at(&range["end"])))
    });
    fn insert(nodes: &mut Vec<Value>, symbol: Value, at: &dyn Fn(&Value) -> (u64, u64)) {
        let (inner, outer) = (&symbol["range"], |node: &Value| node["range"].clone());
        let holds = |node: &Value| {
            let range = outer(node);
            at(&range["start"]) <= at(&inner["start"]) && at(&inner["end"]) <= at(&range["end"])
        };
        match nodes.iter_mut().rev().find(|node| holds(node)) {
            Some(node) => {
                if !node["children"].is_array() {
                    node["children"] = json!([]);
                }
                if let Value::Array(children) = &mut node["children"] {
                    insert(children, symbol, at);
                }
            }
            None => nodes.push(symbol),
        }
    }
    for symbol in symbols {
        insert(&mut outline, symbol, &at);
    }
    outline
}

/// The workspace symbols that are in the decks' cells, by the uri of their
/// cells' file, which are then in the markdown. Those of other files, the
/// extension has of Python's own servers.
fn in_decks_symbols(symbols: Vec<Value>, decks: &HashMap<String, String>) -> Vec<Value> {
    symbols
        .into_iter()
        .filter_map(|mut symbol| {
            let deck = decks.get(symbol["location"]["uri"].as_str()?)?;
            symbol["location"]["uri"] = json!(deck);
            Some(symbol)
        })
        .collect()
}

/// A call or type hierarchy item, in a deck if it is in its cells' file,
/// saying which server gave it, to ask it of its calls or types.
fn hierarchy_item(item: &mut Value, index: usize, decks: &HashMap<String, String>) {
    if let Some(deck) = item["uri"].as_str().and_then(|uri| decks.get(uri)) {
        item["uri"] = json!(deck);
    }
    let data = item.get("data").cloned().unwrap_or(Value::Null);
    item["data"] = json!({ "slidesServer": index, "data": data });
}

/// The items with a range, as highlights, that are in the cells.
fn ranges_in_cells(result: &mut Value, cells: &[(usize, usize)]) {
    if let Value::Array(items) = result {
        items.retain(|item| starts_in_cells(&item["range"], cells));
    }
}

/// Whether a range starts in one of the cells.
fn starts_in_cells(range: &Value, cells: &[(usize, usize)]) -> bool {
    range["start"]["line"]
        .as_u64()
        .is_some_and(|line| in_cells(cells, line as usize))
}

/// The diagnostics of a deck's cells, but for an expression left unused at
/// the end of a cell, which the kernel shows.
fn kept(shadow: &Shadow, diagnostics: Vec<Value>) -> Vec<Value> {
    let lines: Vec<&str> = shadow.text.split('\n').collect();
    let last_line = |end: usize| {
        let mut line = end.saturating_sub(1);
        while line > 0 && lines.get(line).is_some_and(|text| text.trim().is_empty()) {
            line -= 1;
        }
        line
    };
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            let start = diagnostic["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
            let end = diagnostic["range"]["end"]["line"].as_u64().unwrap_or(0) as usize;
            let Some(&(_, cell_end)) = shadow
                .cells
                .iter()
                .find(|&&(from, to)| from <= start && end < to)
            else {
                return false;
            };
            let code = match &diagnostic["code"] {
                Value::Object(code) => code.get("value").cloned().unwrap_or(Value::Null),
                code => code.clone(),
            };
            let code = match code {
                Value::String(code) => code,
                code => code.to_string(),
            };
            !(is_unused_expression(&code) && start == last_line(cell_end))
        })
        .collect()
}

/// Whether a diagnostic's code is of an expression left unused, as
/// `reportUnusedExpression` is.
fn is_unused_expression(code: &str) -> bool {
    let code = code.to_lowercase();
    code.match_indices("unused").any(|(at, _)| {
        let rest = &code[at + "unused".len()..];
        rest.starts_with("expression")
            || rest
                .chars()
                .next()
                .is_some_and(|c| rest[c.len_utf8()..].starts_with("expression"))
    })
}

/// Whether a line is in one of the cells.
fn in_cells(cells: &[(usize, usize)], line: usize) -> bool {
    cells
        .iter()
        .any(|&(start, end)| start <= line && line < end)
}

/// The Python file of a deck's cells, each line where it is in the markdown
/// and the rest blank, and the lines of each cell's code.
fn shadow_text(markdown: &str) -> (String, Vec<(usize, usize)>) {
    let cells = python_cells(markdown);
    let lines: Vec<&str> = markdown
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let python: Vec<&str> = lines
        .iter()
        .enumerate()
        .map(|(line, text)| if in_cells(&cells, line) { *text } else { "" })
        .collect();
    (python.join("\n"), cells)
}

/// The lines of each Python cell's code, from the one after its opening fence
/// up to its closing one, excluded, as the deck finds them: in Python unless
/// its fence names another language.
fn python_cells(markdown: &str) -> Vec<(usize, usize)> {
    let line_of = |offset: usize| markdown[..offset].matches('\n').count();
    code_cells(markdown)
        .into_iter()
        .filter(|cell| {
            let language = cell.info.split_whitespace().next().unwrap_or("");
            let language = language
                .trim_start_matches('{')
                .trim_start_matches('.')
                .trim_end_matches('}');
            language.is_empty() || language == "python"
        })
        .map(|cell| {
            let start = line_of(cell.range.start) + 1;
            (start, start + cell.code.lines().count())
        })
        .collect()
}

/// The server to start: the one the setting names, a command or a path, or
/// else the first of `SERVERS` on the `PATH`, which the environment's comes
/// first on.
fn choose(setting: &str, path: &str) -> Option<(String, PathBuf, Vec<String>)> {
    let args = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect();
    if !setting.is_empty() {
        let command = executable(setting, path)?;
        let name = Path::new(setting)
            .file_stem()?
            .to_string_lossy()
            .into_owned();
        let known = SERVERS.iter().find(|(server, _)| *server == name);
        return Some((
            name,
            command,
            known.map_or(vec!["--stdio".to_string()], |(_, known)| args(known)),
        ));
    }
    SERVERS
        .iter()
        .find_map(|(name, known)| Some((name.to_string(), executable(name, path)?, args(known))))
}

/// The path of the command `name`, on the `PATH`, unless it is a path itself.
fn executable(name: &str, path: &str) -> Option<PathBuf> {
    let name = Path::new(name);
    if name.is_absolute() {
        return name.exists().then(|| name.to_path_buf());
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The Python of the environment installed at `prefix`, by conda or pixi, or
/// by uv.
fn python_of(prefix: &Path) -> Option<PathBuf> {
    let python = prefix.join("bin").join("python");
    python.exists().then_some(python)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK: &str = "---\ntheme: dark\n---\n\n# Slide\n\n```python\nnot_a_cell = 1\n```\n\n~~~python\nimport numpy as np\nnp.\n~~~\n\n~~~\nx = 1\n~~~\n\n~~~rust\nlet y = 2;\n~~~\n";

    #[test]
    fn the_cells_file_holds_the_python_cells_each_line_in_its_place() {
        let (text, cells) = shadow_text(DECK);
        assert_eq!(cells, [(11, 13), (16, 17)]);
        let expected: Vec<&str> = DECK
            .split('\n')
            .enumerate()
            .map(|(i, line)| if [11, 12, 16].contains(&i) { line } else { "" })
            .collect();
        assert_eq!(text, expected.join("\n"));
    }

    #[test]
    fn an_import_goes_at_the_top_of_the_first_cell_and_an_edit_outside_them_not_at_all() {
        let at = |line: u32| json!({ "line": line, "character": 0 });
        let edits = vec![
            json!({ "range": { "start": at(0), "end": at(0) }, "newText": "import os\n" }),
            json!({ "range": { "start": at(4), "end": at(4) }, "newText": "no" }),
            json!({ "range": { "start": at(12), "end": at(12) }, "newText": "yes" }),
        ];
        let kept = keep_in_cells(edits, &[(11, 13), (16, 17)]);
        let lines: Vec<(u64, &str)> = kept
            .iter()
            .map(|edit| {
                (
                    edit["range"]["start"]["line"].as_u64().unwrap(),
                    edit["newText"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(lines, [(11, "import os\n"), (12, "yes")]);
    }

    #[test]
    fn an_expression_left_unused_at_the_end_of_a_cell_is_not_reported() {
        let (text, cells) = shadow_text("~~~python\nx = 1\nx\n~~~\n\n~~~python\nx\ny = 2\n~~~\n");
        let shadow = Shadow {
            uri: String::new(),
            version: 1,
            text,
            cells,
        };
        let diagnostic = |line: u32, code: &str| json!({ "range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 1 } }, "code": code });
        let kept = kept(
            &shadow,
            vec![
                diagnostic(2, "reportUnusedExpression"),
                diagnostic(6, "reportUnusedExpression"),
                diagnostic(2, "reportUndefinedVariable"),
                diagnostic(4, "reportUndefinedVariable"),
            ],
        );
        let lines: Vec<u64> = kept
            .iter()
            .map(|d| d["range"]["start"]["line"].as_u64().unwrap())
            .collect();
        assert_eq!(lines, [6, 2]);
    }

    #[test]
    fn a_code_actions_edits_of_the_cells_file_are_of_the_deck() {
        let at = |line: u32| json!({ "line": line, "character": 0 });
        let import = json!({ "range": { "start": at(0), "end": at(0) }, "newText": "import os\n" });
        let shadow = Shadow {
            uri: "file:///d/.talk.md.py".to_string(),
            version: 3,
            text: String::new(),
            cells: vec![(11, 13)],
        };
        let mut result = json!([
            { "title": "Import os", "kind": "quickfix", "data": 7, "edit": { "changes": { "file:///d/.talk.md.py": [import] } } },
            { "title": "Import os", "edit": { "documentChanges": [{ "textDocument": { "uri": "file:///d/.talk.md.py", "version": 3 }, "edits": [import] }] } },
            { "title": "Run", "command": "server.run" },
        ]);
        actions(&mut result, 1, "file:///d/talk.md", &shadow);
        let first = &result[0];
        assert_eq!(
            first["data"],
            json!({ "slidesServer": 1, "slidesDocument": "file:///d/talk.md", "data": 7 })
        );
        let edits = &first["edit"]["changes"]["file:///d/talk.md"];
        assert_eq!(edits[0]["range"]["start"]["line"], 11);
        assert!(
            first["edit"]["changes"]
                .get("file:///d/.talk.md.py")
                .is_none()
        );
        let change = &result[1]["edit"]["documentChanges"][0];
        assert_eq!(
            change["textDocument"],
            json!({ "uri": "file:///d/talk.md", "version": null })
        );
        assert_eq!(change["edits"][0]["range"]["start"]["line"], 11);
        assert!(result[2].get("data").is_none());
    }

    #[test]
    fn what_is_asked_at_positions_or_for_a_range_is_of_the_cells_if_they_are() {
        let cells = [(3, 5), (10, 12)];
        let at = |line: u32| json!({ "line": line, "character": 0 });
        let asks = |method: &str, params: Value| asks_of_cells(method, &params, &cells);
        assert!(asks("textDocument/hover", json!({ "position": at(4) })));
        assert!(!asks("textDocument/hover", json!({ "position": at(5) })));
        assert!(asks(
            "textDocument/codeAction",
            json!({ "range": { "start": at(10), "end": at(10) } })
        ));
        // Each position, or else none.
        assert!(asks(
            "textDocument/selectionRange",
            json!({ "positions": [at(3), at(11)] })
        ));
        assert!(!asks(
            "textDocument/selectionRange",
            json!({ "positions": [at(3), at(7)] })
        ));
        assert!(!asks(
            "textDocument/selectionRange",
            json!({ "positions": [] })
        ));
        // Inlay hints for what the editor shows, which starts before a cell.
        assert!(asks(
            "textDocument/inlayHint",
            json!({ "range": { "start": at(0), "end": at(20) } })
        ));
        assert!(asks(
            "textDocument/inlayHint",
            json!({ "range": { "start": at(0), "end": at(3) } })
        ));
        assert!(!asks(
            "textDocument/inlayHint",
            json!({ "range": { "start": at(5), "end": at(9) } })
        ));
        assert!(asks(
            "textDocument/semanticTokens/range",
            json!({ "range": { "start": at(9), "end": at(10) } })
        ));
        assert!(asks("textDocument/foldingRange", json!({})));
        assert!(!asks_of_cells(
            "textDocument/documentSymbol",
            &json!({}),
            &[]
        ));
    }

    #[test]
    fn the_highlights_in_the_cells_are_kept() {
        let at = |line: u32| json!({ "line": line, "character": 0 });
        let highlight =
            |line: u32| json!({ "range": { "start": at(line), "end": at(line) }, "kind": 1 });
        let mut result = json!([highlight(4), highlight(7), highlight(11)]);
        ranges_in_cells(&mut result, &[(3, 5), (10, 12)]);
        assert_eq!(result, json!([highlight(4), highlight(11)]));
    }

    #[test]
    fn the_inlay_hints_in_the_cells_are_kept_with_their_edits_and_locations_in_the_deck() {
        let at = |line: u32| json!({ "line": line, "character": 1 });
        let edit = |line: u32| json!({ "range": { "start": at(line), "end": at(line) }, "newText": ": float" });
        let shadow = Shadow {
            uri: "file:///d/.talk.md.py".to_string(),
            version: 1,
            text: String::new(),
            cells: vec![(3, 5)],
        };
        let place =
            json!({ "uri": "file:///d/.talk.md.py", "range": { "start": at(3), "end": at(3) } });
        let elsewhere =
            json!({ "uri": "file:///lib/math.pyi", "range": { "start": at(9), "end": at(9) } });
        let mut result = json!([
            { "position": at(4), "label": [{ "value": ": " }, { "value": "Point", "location": place }, { "value": "float", "location": elsewhere }], "textEdits": [edit(4), edit(7)], "data": 2 },
            { "position": at(8), "label": ": int" },
        ]);
        hints(&mut result, 1, "file:///d/talk.md", &shadow);
        let hints = result.as_array().unwrap();
        assert_eq!(hints.len(), 1);
        assert_eq!(
            hints[0]["data"],
            json!({ "slidesServer": 1, "slidesDocument": "file:///d/talk.md", "data": 2 })
        );
        assert_eq!(hints[0]["textEdits"], json!([edit(4)]));
        assert_eq!(hints[0]["label"][1]["location"]["uri"], "file:///d/talk.md");
        assert_eq!(hints[0]["label"][2]["location"], elsewhere);
    }

    #[test]
    fn a_rename_edits_the_cells_and_other_files_but_not_the_markdown() {
        let at = |line: u32| json!({ "line": line, "character": 4 });
        let edit = |line: u32| json!({ "range": { "start": at(line), "end": at(line) }, "newText": "twice" });
        let cells = [(5, 7), (12, 14)];
        let shadow = "file:///d/.talk.md.py";
        let module = "file:///d/module.py";
        let mut changes =
            json!({ "changes": { shadow: [edit(5), edit(9), edit(12)], module: [edit(1)] } });
        in_deck_edit(&mut changes, shadow, "file:///d/talk.md", &cells);
        assert_eq!(
            changes,
            json!({ "changes": { "file:///d/talk.md": [edit(5), edit(12)], module: [edit(1)] } })
        );
        let rename =
            json!({ "kind": "rename", "oldUri": "file:///d/a.py", "newUri": "file:///d/b.py" });
        let mut changes = json!({ "documentChanges": [
            { "textDocument": { "uri": shadow, "version": 2 }, "edits": [edit(5), edit(9)] },
            { "textDocument": { "uri": module, "version": 1 }, "edits": [edit(1)] },
            rename,
        ] });
        in_deck_edit(&mut changes, shadow, "file:///d/talk.md", &cells);
        assert_eq!(
            changes,
            json!({ "documentChanges": [
                { "textDocument": { "uri": "file:///d/talk.md", "version": null }, "edits": [edit(5)] },
                { "textDocument": { "uri": module, "version": 1 }, "edits": [edit(1)] },
                rename,
            ] })
        );
    }

    #[test]
    fn a_server_is_asked_only_what_it_said_it_does() {
        let (writer, _) = std::sync::mpsc::channel();
        let mut server = Server {
            name: "ty".to_string(),
            child: Command::new("true").spawn().unwrap(),
            writer,
            ready: false,
            capabilities: Value::Null,
            queue: Vec::new(),
            python: None,
            folders: HashSet::new(),
            next: 0,
        };
        // Before it says, it is asked, and answers what it does not do.
        assert!(server.provides("textDocument/foldingRange"));
        server.ready = true;
        server.capabilities = json!({
            "completionProvider": {},
            "inlayHintProvider": true,
            "renameProvider": { "prepareProvider": true },
            "semanticTokensProvider": { "full": { "delta": true }, "legend": {} },
            "foldingRangeProvider": false,
        });
        assert!(server.provides("textDocument/completion"));
        assert!(!server.provides("completionItem/resolve"));
        assert!(server.provides("textDocument/inlayHint"));
        assert!(!server.provides("inlayHint/resolve"));
        assert!(server.provides("textDocument/prepareRename"));
        assert!(server.provides("textDocument/semanticTokens/full"));
        assert!(!server.provides("textDocument/foldingRange"));
        assert!(!server.provides("textDocument/selectionRange"));
        assert!(!server.provides("callHierarchy/incomingCalls"));
        let _ = server.child.wait();
    }

    #[test]
    fn semantic_tokens_are_of_the_decks_kinds_and_in_the_cells() {
        let types = legend(&json!(["function", "unknownKind", "variable"]), TOKEN_TYPES);
        let modifiers = legend(
            &json!(["definition", "unknownModifier", "readonly"]),
            TOKEN_MODIFIERS,
        );
        let kind = |name: &str| TOKEN_TYPES.iter().position(|kind| *kind == name).unwrap() as u64;
        let bit = |name: &str| 1u64 << TOKEN_MODIFIERS.iter().position(|m| *m == name).unwrap();
        // Lines 1 (outside), 3, 3 (of an unknown kind), 3 and 4.
        let mut result = json!({ "resultId": "1", "data": [
            1, 0, 3, 0, 0,
            2, 4, 6, 0, 0b001,
            0, 7, 2, 1, 0,
            0, 3, 1, 2, 0b110,
            1, 2, 5, 2, 0,
        ] });
        tokens(&mut result, &types, &modifiers, &[(3, 5)], None);
        assert_eq!(
            result,
            json!({ "resultId": "1", "data": [
                3, 4, 6, kind("function"), bit("definition"),
                0, 10, 1, kind("variable"), bit("readonly"),
                1, 2, 5, kind("variable"), 0,
            ] })
        );
        // Asked for line 4 alone.
        let mut result = json!({ "data": [3, 4, 6, 0, 0, 1, 2, 5, 2, 0] });
        tokens(&mut result, &types, &modifiers, &[(3, 5)], Some((4, 4)));
        assert_eq!(result, json!({ "data": [4, 2, 5, kind("variable"), 0] }));
    }

    #[test]
    fn the_folds_within_a_cell_are_kept() {
        let fold = |start: u32, end: u32| json!({ "startLine": start, "endLine": end });
        let mut result = json!([fold(3, 4), fold(4, 11), fold(10, 12), fold(12, 13)]);
        folds_in_cells(&mut result, &[(3, 5), (10, 13)]);
        assert_eq!(result, json!([fold(3, 4), fold(10, 12)]));
    }

    #[test]
    fn the_symbols_in_the_cells_are_kept_in_the_deck() {
        let at = |line: u32| json!({ "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 3 } });
        let shadow = Shadow {
            uri: "file:///d/.talk.md.py".to_string(),
            version: 1,
            text: String::new(),
            cells: vec![(3, 5)],
        };
        let mut result = json!([
            { "name": "f", "kind": 12, "location": { "uri": shadow.uri, "range": at(3) } },
            { "name": "g", "kind": 12, "location": { "uri": shadow.uri, "range": at(7) } },
        ]);
        symbols_in_cells(&mut result, &shadow, "file:///d/talk.md");
        assert_eq!(
            result,
            json!([{ "name": "f", "kind": 12, "location": { "uri": "file:///d/talk.md", "range": at(3) } }])
        );
        let mut result = json!([
            { "name": "f", "kind": 12, "range": at(4), "selectionRange": at(4) },
            { "name": "g", "kind": 12, "range": at(0), "selectionRange": at(0) },
        ]);
        symbols_in_cells(&mut result, &shadow, "file:///d/talk.md");
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["name"], "f");
    }

    #[test]
    fn the_outline_is_the_headings_with_the_cells_symbols_under_theirs() {
        let markdown =
            "# One { center }\n\n~~~python\ndef f(x):\n    return x\n~~~\n\n## Two\n\n# Thrée\n";
        let at = |line: u32, character: u32| json!({ "line": line, "character": character });
        let range = |from, to| json!({ "start": from, "end": to });
        let symbols = vec![
            json!({ "name": "x", "kind": 13, "containerName": "f", "location": { "uri": "file:///d/talk.md", "range": range(at(3, 6), at(3, 7)) } }),
            json!({ "name": "f", "kind": 12, "location": { "uri": "file:///d/talk.md", "range": range(at(3, 0), at(4, 12)) } }),
        ];
        let outline = outline(markdown, symbols);
        let names = |nodes: &Value| -> Vec<String> {
            nodes
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names(&json!(outline)), ["One", "Thrée"]);
        assert_eq!(outline[0]["range"], range(at(0, 0), at(8, 0)));
        assert_eq!(outline[0]["selectionRange"], range(at(0, 0), at(0, 16)));
        assert_eq!(names(&outline[0]["children"]), ["f", "Two"]);
        let f = &outline[0]["children"][0];
        assert_eq!(f["selectionRange"], range(at(3, 0), at(4, 12)));
        assert_eq!(names(&f["children"]), ["x"]);
        assert!(f["children"][0].get("location").is_none());
        // Up to the end of the file, in UTF-16.
        assert_eq!(outline[1]["range"], range(at(9, 0), at(10, 0)));
        assert_eq!(outline[1]["selectionRange"]["end"], at(9, 7));
    }

    #[test]
    fn the_workspace_symbols_kept_are_those_of_the_decks() {
        let decks = HashMap::from([(
            "file:///d/.talk.md.py".to_string(),
            "file:///d/talk.md".to_string(),
        )]);
        let symbol = |uri: &str| json!({ "name": "f", "kind": 12, "location": { "uri": uri } });
        let kept = in_decks_symbols(
            vec![
                symbol("file:///d/.talk.md.py"),
                symbol("file:///d/module.py"),
            ],
            &decks,
        );
        assert_eq!(kept, [symbol("file:///d/talk.md")]);
    }

    #[test]
    fn a_hierarchy_item_is_in_its_deck_and_says_its_server() {
        let decks = HashMap::from([(
            "file:///d/.talk.md.py".to_string(),
            "file:///d/talk.md".to_string(),
        )]);
        let mut item = json!({ "name": "f", "uri": "file:///d/.talk.md.py", "data": 3 });
        hierarchy_item(&mut item, 2, &decks);
        assert_eq!(
            item,
            json!({ "name": "f", "uri": "file:///d/talk.md", "data": { "slidesServer": 2, "data": 3 } })
        );
        let mut item = json!({ "name": "g", "uri": "file:///d/module.py" });
        hierarchy_item(&mut item, 2, &decks);
        assert_eq!(item["uri"], "file:///d/module.py");
        assert_eq!(item["data"], json!({ "slidesServer": 2, "data": null }));
    }

    #[test]
    fn a_location_in_the_cells_file_is_in_the_deck() {
        let mut result = json!([
            { "uri": "file:///d/.talk.md.py", "range": {} },
            { "targetUri": "file:///lib/os.py", "targetRange": {} },
        ]);
        in_document(&mut result, "file:///d/.talk.md.py", "file:///d/talk.md");
        assert_eq!(result[0]["uri"], "file:///d/talk.md");
        assert_eq!(result[1]["targetUri"], "file:///lib/os.py");
    }
}
