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

use lsp_server::{Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};
use slides::markdown::code_cells;
use slides::paths::parent;

use super::{Event, file_path, file_uri};
use crate::notebook::{FOREIGN, activated};

/// The servers looked for, in order, by the command that starts one.
const SERVERS: &[(&str, &[&str])] = &[
    ("basedpyright-langserver", &["--stdio"]),
    ("ty", &["server"]),
    ("pyright-langserver", &["--stdio"]),
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
    /// Whether it resolves its completions, as ty does not.
    resolves: bool,
    /// Whether it resolves its code actions.
    resolves_actions: bool,
    /// Whether it resolves its inlay hints.
    resolves_hints: bool,
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
                    "no Python language server for {where_}: install basedpyright or ty in it, for the code cells to have one"
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
            resolves: false,
            resolves_actions: false,
            resolves_hints: false,
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
                self.documents.insert(
                    uri.to_string(),
                    Tracked {
                        server: index,
                        shadow,
                    },
                );
            }
        }
    }

    /// Asks the deck's server what the extension asks within its cells, and
    /// whether it did: outside them, or without a server, the deck answers.
    pub fn forward(&mut self, request: &Request, text: &str) -> bool {
        let method = request.method.as_str();
        if matches!(
            method,
            "completionItem/resolve" | "codeAction/resolve" | "inlayHint/resolve"
        ) {
            return self.resolve(request);
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
        ) {
            return false;
        }
        let Some(uri) = request.params["textDocument"]["uri"]
            .as_str()
            .map(String::from)
        else {
            return false;
        };
        self.track(&uri, text);
        let Some(tracked) = self.documents.get(&uri) else {
            return false;
        };
        if !asks_of_cells(method, &request.params, &tracked.shadow.cells) {
            return false;
        }
        let index = tracked.server;
        let mut params = request.params.clone();
        params["textDocument"]["uri"] = json!(tracked.shadow.uri);
        let asked = match method {
            "textDocument/completion" => Asked::Completion { document: uri },
            "textDocument/codeAction" => Asked::Actions { document: uri },
            "textDocument/documentHighlight" => Asked::Ranges { document: uri },
            "textDocument/inlayHint" => Asked::Hints { document: uri },
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
        let (asked, resolves) = match method {
            "codeAction/resolve" => (Asked::Actions { document }, server.resolves_actions),
            "inlayHint/resolve" => (Asked::Hints { document }, server.resolves_hints),
            _ => (Asked::Completion { document }, server.resolves),
        };
        // A server that does not resolve what it gives gave it whole.
        if !resolves {
            return false;
        }
        let id = server.request(method, item);
        self.pending
            .insert((index, id), (request.id.clone(), asked));
        true
    }

    /// Cancels what a server was asked, once the extension cancels it.
    pub fn cancel(&mut self, id: &RequestId) {
        let asked = self
            .pending
            .iter()
            .find(|(_, (client, _))| client == id)
            .map(|((index, asked), _)| (*index, asked.clone()));
        if let Some((index, asked)) = asked
            && let Some(server) = self.servers[index].as_mut()
        {
            server.notify("$/cancelRequest", json!({ "id": asked }));
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
            server.resolves = result["capabilities"]["completionProvider"]["resolveProvider"]
                .as_bool()
                .unwrap_or(false);
            server.resolves_actions =
                result["capabilities"]["codeActionProvider"]["resolveProvider"]
                    .as_bool()
                    .unwrap_or(false);
            server.resolves_hints = result["capabilities"]["inlayHintProvider"]["resolveProvider"]
                .as_bool()
                .unwrap_or(false);
            let _ = server.writer.send(Message::Notification(Notification::new(
                "initialized".to_string(),
                json!({}),
            )));
            server.ready = true;
            for message in std::mem::take(&mut server.queue) {
                let _ = server.writer.send(message);
            }
            return;
        }
        let Some((id, asked)) = self.pending.remove(&(index, response.id)) else {
            return;
        };
        let response = match response.response_result {
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
        if matches!(
            request.method.as_str(),
            "workspace/configuration" | "workspace/inlayHint/refresh"
        ) {
            // Asked of the extension: its configuration, with the cells'
            // interpreter for pyright, or to ask for the inlay hints again.
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
            if let Some((id, _)) = self.pending.remove(&key) {
                (self.client)(Message::Response(Response::new_ok(id, Value::Null)));
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
        "textDocument/inlayHint" => {
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
