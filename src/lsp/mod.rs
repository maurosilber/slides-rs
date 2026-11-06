//! The language server of the VS Code extension, `slides-rs lsp`, which the
//! extension starts in the workspace's pixi environment. It completes and
//! describes the frontmatter and the attributes the deck reads, links each
//! import to the file it imports, warns of what is wrong with how the slides
//! step, and tells the extension which files are decks. The code cells have
//! what a Python language server it starts has of them. It renders the decks
//! the extension shows, whose pages follow the cursor. Its own requests,
//! under `slides/`, answer what the extension's notebook and slide editor ask
//! of the deck: the cells of a file, the file the cells make, where its
//! slides break and how they step, and the outputs saved for its cells.

mod attributes;
mod decks;
mod frontmatter;
mod notebook;
mod outputs;
mod preview;
mod python;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CompletionOptions, Diagnostic, DiagnosticSeverity, DocumentLink, DocumentLinkOptions, OneOf,
    Position, Range, SaveOptions, ServerCapabilities, SignatureHelpOptions,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Uri,
};
use serde::Deserialize;
use serde_json::{Value, json};
use slides::markdown::{Files, NoFiles, breaks, steps};
use slides::paths::{joined, parent};

use decks::Decks;
use outputs::{Disk, Place};
use preview::{Showing, Shown, Watches};
use python::Pythons;

/// What the server hears of: the extension, a Python language server, the
/// environment of a directory's cells, once it is activated, or a deck it
/// renders.
pub enum Event {
    Client(Message),
    Python(usize, Message),
    Exited(usize),
    Activated(PathBuf, Result<Option<python::Activated>, String>),
    Rendered(usize, PathBuf),
    Stopped(usize),
}

/// How long the edits of a document stop before what is wrong with its
/// slides is found again.
const DELAY: Duration = Duration::from_millis(150);

/// Serves the extension on stdin and stdout, until it exits.
pub fn serve() -> Result<()> {
    let (connection, io) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(false),
                })),
                ..Default::default()
            },
        )),
        // Values after the space that follows a key, attributes and their
        // values as a block or a tag is written, and Python after a `.`.
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(["{", " ", "=", "\"", "."].map(String::from).to_vec()),
            resolve_provider: Some(true),
            ..Default::default()
        }),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        // The cells' quick fixes, as an import, from their Python server.
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
            lsp_types::CodeActionOptions {
                resolve_provider: Some(true),
                ..Default::default()
            },
        )),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(["(", ","].map(String::from).to_vec()),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        // This very command, for the extension to run the cells and render the
        // slides with, rather than starting the environment again for each.
        experimental: Some(json!({ "executable": std::env::current_exe()? })),
        ..Default::default()
    };
    let params = connection.initialize(serde_json::to_value(capabilities)?)?;
    let (events, heard) = std::sync::mpsc::channel();
    let receiver = connection.receiver.clone();
    let client = events.clone();
    std::thread::spawn(move || {
        for message in receiver {
            if client.send(Event::Client(message)).is_err() {
                return;
            }
        }
    });
    let sender = connection.sender.clone();
    let to_client: python::ToClient = Arc::new(move |message| {
        let _ = sender.send(message);
    });
    let roots = roots(&params);
    let mut server = Server::new(
        connection,
        heard,
        events.clone(),
        Pythons::new(&params, roots.clone(), events, to_client),
    );
    server.decks.scan_all(&roots);
    server.decks.update();
    server.publish_decks();
    let run = server.run();
    server.pythons.shutdown();
    server.watches.stop();
    run?;
    drop(server);
    io.join()?;
    Ok(())
}

/// The folders of the workspace, from the parameters it was initialized with.
fn roots(params: &Value) -> Vec<PathBuf> {
    let folders = params["workspaceFolders"].as_array().into_iter().flatten();
    let mut roots: Vec<PathBuf> = folders
        .filter_map(|folder| file_path(folder["uri"].as_str()?))
        .collect();
    if roots.is_empty()
        && let Some(root) = params["rootUri"].as_str().and_then(file_path)
    {
        roots.push(root);
    }
    roots
}

/// A document the extension has open.
struct Document {
    text: String,
    language: String,
}

impl Document {
    fn lines(&self) -> Vec<&str> {
        self.text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect()
    }
}

struct Server {
    connection: Connection,
    events: Receiver<Event>,
    /// What the threads it starts tell it by.
    sender: Sender<Event>,
    /// The open documents, by uri.
    documents: HashMap<String, Document>,
    decks: Decks,
    pythons: Pythons,
    watches: Watches,
    /// The line of the cursor in each document whose deck is shown, by uri.
    cursors: HashMap<String, usize>,
    /// The documents whose warnings are found again once their edits stop.
    stale: HashSet<String>,
    deadline: Option<Instant>,
    /// What is wrong with how the slides of each document step.
    warnings: HashMap<String, Vec<Value>>,
    /// The documents given diagnostics, which are taken back once they have
    /// none.
    diagnosed: HashSet<String>,
    /// Whether the extension asked it to shut down, after which it exits.
    shutting_down: bool,
}

impl Server {
    fn new(
        connection: Connection,
        events: Receiver<Event>,
        sender: Sender<Event>,
        pythons: Pythons,
    ) -> Server {
        Server {
            connection,
            events,
            sender,
            documents: HashMap::new(),
            decks: Decks::default(),
            pythons,
            watches: Watches::default(),
            cursors: HashMap::new(),
            stale: HashSet::new(),
            deadline: None,
            warnings: HashMap::new(),
            diagnosed: HashSet::new(),
            shutting_down: false,
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            let event = match self.deadline {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    match self.events.recv_timeout(wait) {
                        Ok(event) => event,
                        Err(RecvTimeoutError::Timeout) => {
                            self.diagnose();
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => return Ok(()),
                    }
                }
                None => match self.events.recv() {
                    Ok(event) => event,
                    Err(_) => return Ok(()),
                },
            };
            let message = match event {
                Event::Client(message) => message,
                Event::Python(index, message) => {
                    if let Some(uri) = self.pythons.received(index, message) {
                        self.publish(&uri);
                    }
                    continue;
                }
                Event::Exited(index) => {
                    for uri in self.pythons.exited(index) {
                        self.publish(&uri);
                    }
                    continue;
                }
                Event::Activated(dir, activated) => {
                    self.pythons.activated(dir, activated);
                    self.later(self.documents.keys().cloned().collect::<Vec<_>>());
                    continue;
                }
                Event::Rendered(index, page) => {
                    let (answers, opened) = self.watches.rendered(index, page);
                    for (id, shown) in answers {
                        self.connection
                            .sender
                            .send(Message::Response(Response::new_ok(id, shown_json(shown))))?;
                    }
                    // A page that follows no cursor, which the extension
                    // opens there, in the integrated browser.
                    if let Some(shown) = opened {
                        self.notify("slides/open", shown_json(shown));
                    }
                    continue;
                }
                Event::Stopped(index) => {
                    for id in self.watches.stopped(index) {
                        let message = "slides-rs exited before rendering it. Its output is in the Slides Notebook log.";
                        let response = Response::new_err(
                            id,
                            ErrorCode::InternalError as i32,
                            message.to_string(),
                        );
                        self.connection.sender.send(Message::Response(response))?;
                    }
                    continue;
                }
            };
            match message {
                Message::Request(request) => {
                    if request.method == "shutdown" {
                        self.shutting_down = true;
                        let response = Response::new_ok(request.id, Value::Null);
                        self.connection.sender.send(Message::Response(response))?;
                        continue;
                    }
                    let text = request.params["textDocument"]["uri"]
                        .as_str()
                        .and_then(|uri| self.documents.get(uri))
                        .map(|document| document.text.clone())
                        .unwrap_or_default();
                    if self.pythons.forward(&request, &text) {
                        continue;
                    }
                    if request.method == "slides/show" {
                        if let Some(response) = self.show(request) {
                            self.connection.sender.send(Message::Response(response))?;
                        }
                        continue;
                    }
                    let id = request.id.clone();
                    let response = match self.respond(request) {
                        Ok(result) => Response::new_ok(id, result),
                        Err(Failure::NotFound(method)) => Response::new_err(
                            id,
                            ErrorCode::MethodNotFound as i32,
                            format!("no method {method}"),
                        ),
                        Err(Failure::Error(error)) => Response::new_err(
                            id,
                            ErrorCode::InternalError as i32,
                            format!("{error:#}"),
                        ),
                    };
                    self.connection.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => {
                    if notification.method == "exit" {
                        return Ok(());
                    }
                    self.notified(notification);
                }
                Message::Response(response) => self.pythons.client_answered(response),
            }
        }
    }

    fn respond(&mut self, request: Request) -> Result<Value, Failure> {
        let Request { method, params, .. } = request;
        let value = match method.as_str() {
            "textDocument/completion" => {
                let at: lsp_types::TextDocumentPositionParams = parse(params)?;
                let items = self.with_lines(&at, |lines, position, deck| {
                    let mut items = frontmatter::completion(lines, position).unwrap_or_default();
                    if deck {
                        items.extend(attributes::completion(lines, position).unwrap_or_default());
                    }
                    items
                });
                serde_json::to_value(items.unwrap_or_default())?
            }
            "textDocument/hover" => {
                let at: lsp_types::TextDocumentPositionParams = parse(params)?;
                let hover = self.with_lines(&at, |lines, position, deck| {
                    frontmatter::hover(lines, position)
                        .or_else(|| deck.then(|| attributes::hover(lines, position)).flatten())
                });
                serde_json::to_value(hover.flatten())?
            }
            // Outside the code cells, there is no Python to ask of.
            "textDocument/signatureHelp"
            | "textDocument/definition"
            | "textDocument/references"
            | "textDocument/codeAction" => Value::Null,
            // The deck's own completions, and a server's that it does not
            // resolve, are whole already.
            "completionItem/resolve" | "codeAction/resolve" => params,
            "textDocument/documentLink" => {
                let params: lsp_types::DocumentLinkParams = parse(params)?;
                serde_json::to_value(self.import_links(params.text_document.uri.as_str()))?
            }
            "slides/cells" => {
                let MarkdownOf { markdown } = parse(params)?;
                serde_json::to_value(notebook::cells(&markdown))?
            }
            "slides/markdown" => {
                let CellsOf { cells } = parse(params)?;
                serde_json::to_value(notebook::markdown(&cells))?
            }
            "slides/breaks" => {
                let MarkdownOf { markdown } = parse(params)?;
                breaks_json(&markdown)
            }
            "slides/steps" => {
                let StepsOf { markdown, uri } = parse(params)?;
                steps_json(&markdown, place(&uri).as_ref())
            }
            "slides/position" => {
                let PositionOf {
                    markdown,
                    line,
                    uri,
                } = parse(params)?;
                let position = match file(&uri) {
                    Some(path) => Disk.position(&path, &markdown, line),
                    // Not saved, it has no place for its imports to be found from.
                    None => NoFiles.position(
                        Path::new(uri.rsplit('/').next().unwrap_or("")),
                        &markdown,
                        line,
                    ),
                };
                json!({ "slide": position.slide, "step": position.step })
            }
            "slides/load" => {
                let LoadOf { uri, sources } = parse(params)?;
                let place = place(&uri).with_context(|| format!("{uri} is not a file"))?;
                serde_json::to_value(outputs::load(&place, &sources))?
            }
            "slides/save" => {
                let SaveOf { uri, cells } = parse(params)?;
                let place = place(&uri).with_context(|| format!("{uri} is not a file"))?;
                serde_json::to_value(outputs::save(&place, &cells)?)?
            }
            "slides/stop" => {
                self.watches.stop();
                Value::Null
            }
            _ => return Err(Failure::NotFound(method)),
        };
        Ok(value)
    }

    /// Runs `answer` on the lines of the document at a position, the position
    /// itself and whether the document is a deck.
    fn with_lines<T>(
        &self,
        at: &lsp_types::TextDocumentPositionParams,
        answer: impl FnOnce(&[&str], Position, bool) -> T,
    ) -> Option<T> {
        let uri = at.text_document.uri.as_str();
        let document = self.documents.get(uri)?;
        let lines = document.lines();
        if at.position.line as usize >= lines.len() {
            return None;
        }
        Some(answer(&lines, at.position, self.is_deck(uri, document)))
    }

    fn notified(&mut self, notification: Notification) {
        let Notification { method, params } = notification;
        match method.as_str() {
            "textDocument/didOpen" => {
                let Ok(params) = parse::<lsp_types::DidOpenTextDocumentParams>(params) else {
                    return;
                };
                let document = params.text_document;
                let uri = document.uri.as_str().to_string();
                self.documents.insert(
                    uri.clone(),
                    Document {
                        text: document.text,
                        language: document.language_id,
                    },
                );
                // A file outside the workspace, which the watcher does not see.
                self.rescan(&uri);
                self.later([uri]);
            }
            "textDocument/didChange" => {
                let Ok(params) = parse::<lsp_types::DidChangeTextDocumentParams>(params) else {
                    return;
                };
                let uri = params.text_document.uri.as_str().to_string();
                if let (Some(document), Some(change)) = (
                    self.documents.get_mut(&uri),
                    params.content_changes.into_iter().last(),
                ) {
                    document.text = change.text;
                }
                self.later([uri]);
            }
            "textDocument/didSave" => {
                if let Ok(params) = parse::<lsp_types::DidSaveTextDocumentParams>(params) {
                    let uri = params.text_document.uri.as_str();
                    self.rescan(uri);
                    // Where the cursor is, found as the save has it, before
                    // the render that follows it is done.
                    if let (Some(path), Some(fragment)) = (file_path(uri), self.cursor(uri)) {
                        for index in self.watches.of(&path) {
                            self.watches.after_render(index, fragment.clone());
                        }
                    }
                }
            }
            "slides/cursor" => {
                let Ok(CursorOf { uri, line, follow }) = parse(params) else {
                    return;
                };
                self.cursors.insert(uri.clone(), line);
                let (Some(path), true) = (file_path(&uri), follow) else {
                    return;
                };
                let Some(fragment) = self.cursor(&uri) else {
                    return;
                };
                for index in self.watches.of(&path) {
                    if let Some(id) = self.watches.id(index) {
                        self.watches.follow.send(id, &fragment);
                    }
                }
            }
            "textDocument/didClose" => {
                let Ok(params) = parse::<lsp_types::DidCloseTextDocumentParams>(params) else {
                    return;
                };
                let uri = params.text_document.uri.as_str();
                self.documents.remove(uri);
                self.stale.remove(uri);
                self.warnings.remove(uri);
                self.pythons.close(uri);
                self.publish(uri);
            }
            "$/cancelRequest" => {
                if let Ok(id) = serde_json::from_value::<RequestId>(params["id"].clone()) {
                    self.pythons.cancel(&id);
                }
            }
            "workspace/didChangeWatchedFiles" => {
                let Ok(params) = parse::<lsp_types::DidChangeWatchedFilesParams>(params) else {
                    return;
                };
                let mut saved = false;
                for change in params.changes {
                    let Some(path) = file_path(change.uri.as_str()) else {
                        continue;
                    };
                    if path.file_name().is_some_and(|name| name == "outputs.txt") {
                        // The outputs of the cells step as they do on the
                        // slides, so a run that saves them moves the steps,
                        // though the documents stay as they are.
                        saved = true;
                    } else if change.typ == lsp_types::FileChangeType::DELETED {
                        self.decks.remove(&path);
                    } else {
                        self.decks.scan(&path);
                    }
                }
                if self.decks.update() {
                    self.publish_decks();
                    saved = true;
                }
                if saved {
                    self.later(self.documents.keys().cloned().collect::<Vec<_>>());
                }
            }
            _ => {}
        }
    }

    /// Renders the deck of a file, if it is not yet, and answers with its
    /// page once it is rendered: as it is in the deck that imports it, if one
    /// does, in its theme and shape, at the slide and step the cursor is at.
    fn show(&mut self, request: Request) -> Option<Response> {
        let failed = |message: String| {
            Response::new_err(request.id.clone(), ErrorCode::InternalError as i32, message)
        };
        let Ok(ShowOf { uri, line, follow }) = parse(request.params.clone()) else {
            return Some(failed("invalid parameters".to_string()));
        };
        let Some(path) = file_path(&uri) else {
            return Some(failed(
                "Only a deck saved on disk can be rendered.".to_string(),
            ));
        };
        if let Some(line) = line {
            self.cursors.insert(uri.clone(), line);
        }
        let deck = self.decks.deck_of(&path);
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => return Some(failed(error.to_string())),
        };
        let index = match self
            .watches
            .start(&executable, &path, deck.as_deref(), &self.sender)
        {
            Ok(index) => index,
            Err(error) => return Some(failed(error)),
        };
        let id = self.watches.id(index)?.to_string();
        // A tab that shows the page already goes there rather than back to
        // the first slide.
        let fragment = line.and_then(|_| self.cursor(&uri));
        if let Some(fragment) = &fragment {
            self.watches.follow.send(&id, fragment);
        }
        // Opened as before, not following the cursor, if the server cannot
        // start.
        let query = if follow {
            match self.watches.follow.port() {
                Ok(port) => Some(format!("follow={port}/{id}")),
                Err(error) => {
                    eprintln!(
                        "could not start the server the page follows the cursor from: {error}"
                    );
                    None
                }
            }
        } else {
            None
        };
        let showing = Showing {
            id: request.id,
            fragment,
            query,
        };
        let (id, shown) = self.watches.show(index, showing)?;
        Some(Response::new_ok(id, shown_json(shown)))
    }

    /// Where the cursor of a document is on its page, as `slide.step`.
    fn cursor(&self, uri: &str) -> Option<String> {
        let path = file_path(uri)?;
        let line = *self.cursors.get(uri)?;
        let text = match self.documents.get(uri) {
            Some(document) => document.text.clone(),
            None => std::fs::read_to_string(&path).ok()?,
        };
        let position = Disk.position(&path, &text, line);
        Some(format!("{}.{}", position.slide, position.step))
    }

    /// Reads the file of a document again, from the disk, for whether it is a
    /// deck and what it imports.
    fn rescan(&mut self, uri: &str) {
        let Some(path) = file_path(uri) else {
            return;
        };
        self.decks.scan(&path);
        if self.decks.update() {
            self.publish_decks();
            self.later(self.documents.keys().cloned().collect::<Vec<_>>());
        }
    }

    /// Tells the extension which files are decks, for it to mark their slides
    /// and show the deck's commands on them alone.
    fn publish_decks(&self) {
        let paths: Vec<&PathBuf> = self.decks.paths().collect();
        self.notify("slides/decks", json!({ "paths": paths }));
    }

    fn notify(&self, method: &str, params: Value) {
        let notification = Notification::new(method.to_string(), params);
        let _ = self
            .connection
            .sender
            .send(Message::Notification(notification));
    }

    /// Whether the document is a deck, or part of one.
    fn is_deck(&self, uri: &str, document: &Document) -> bool {
        document.language == "slides"
            || (document.language == "markdown"
                && path_in(uri).is_some_and(|path| self.decks.contains(&path)))
    }

    /// Finds what is wrong with the documents' slides again, once their edits
    /// stop.
    fn later(&mut self, uris: impl IntoIterator<Item = String>) {
        self.stale.extend(uris);
        self.deadline = Some(Instant::now() + DELAY);
    }

    /// Warns of what is wrong with how the slides of the stale documents step,
    /// and tells the Python language servers of their cells.
    fn diagnose(&mut self) {
        self.deadline = None;
        for uri in std::mem::take(&mut self.stale) {
            let Some(document) = self.documents.get(&uri) else {
                continue;
            };
            if !self.is_deck(&uri, document) {
                self.warnings.remove(&uri);
                self.publish(&uri);
                continue;
            }
            self.pythons.track(&uri, &document.text);
            let lines = document.lines();
            let place = place(&uri);
            let warnings: Vec<Value> = steps(&document.text, |codes| match &place {
                Some(place) => place.html(codes),
                None => Vec::new(),
            })
            .into_iter()
            .flat_map(|slide| slide.warnings)
            .filter(|warning| warning.line < lines.len())
            .map(|warning| Diagnostic {
                range: Range::new(
                    Position::new(warning.line as u32, 0),
                    Position::new(warning.line as u32, utf16(lines[warning.line])),
                ),
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("slides".to_string()),
                message: warning.message,
                ..Default::default()
            })
            .filter_map(|diagnostic| serde_json::to_value(diagnostic).ok())
            .collect();
            self.warnings.insert(uri.clone(), warnings);
            self.publish(&uri);
        }
    }

    /// Shows the document's diagnostics: what is wrong with how its slides
    /// step, and what its Python language server says of its cells.
    fn publish(&mut self, uri: &str) {
        let diagnostics: Vec<&Value> = self
            .warnings
            .get(uri)
            .into_iter()
            .chain(self.pythons.diagnostics.get(uri))
            .flatten()
            .collect();
        if diagnostics.is_empty() && !self.diagnosed.remove(uri) {
            return;
        }
        if !diagnostics.is_empty() {
            self.diagnosed.insert(uri.to_string());
        }
        self.notify(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": diagnostics }),
        );
    }

    /// A link on the `src` of each import of the document, to the file it
    /// imports.
    fn import_links(&self, uri: &str) -> Vec<DocumentLink> {
        let (Some(document), Some(path)) = (self.documents.get(uri), file_path(uri)) else {
            return Vec::new();
        };
        let lines = document.lines();
        breaks(&document.text)
            .imports
            .into_iter()
            .filter_map(|import| {
                let text = lines.get(import.line)?;
                // Within the quotes of the `src`.
                let start = text.find(&format!("\"{}\"", import.src))? + 1;
                let line = import.line as u32;
                let target = file_uri(&joined(parent(&path), &import.src));
                Some(DocumentLink {
                    range: Range::new(
                        Position::new(line, utf16(&text[..start])),
                        Position::new(line, utf16(&text[..start + import.src.len()])),
                    ),
                    target: Uri::from_str(&target).ok(),
                    tooltip: Some("Open the imported slides".to_string()),
                    data: None,
                })
            })
            .collect()
    }
}

/// Why a request was not answered.
enum Failure {
    NotFound(String),
    Error(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Failure {
    fn from(error: E) -> Failure {
        Failure::Error(error.into())
    }
}

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T> {
    serde_json::from_value(params).context("invalid parameters")
}

#[derive(Deserialize)]
struct MarkdownOf {
    markdown: String,
}

#[derive(Deserialize)]
struct CellsOf {
    cells: Vec<notebook::Cell>,
}

#[derive(Deserialize)]
struct StepsOf {
    markdown: String,
    uri: String,
}

#[derive(Deserialize)]
struct PositionOf {
    markdown: String,
    line: usize,
    uri: String,
}

#[derive(Deserialize)]
struct LoadOf {
    uri: String,
    sources: Vec<String>,
}

#[derive(Deserialize)]
struct SaveOf {
    uri: String,
    cells: Vec<outputs::Cell>,
}

#[derive(Deserialize)]
struct ShowOf {
    uri: String,
    /// The line of the cursor, if the file is in a text editor.
    line: Option<usize>,
    /// Whether the page follows the cursor.
    follow: bool,
}

#[derive(Deserialize)]
struct CursorOf {
    uri: String,
    line: usize,
    follow: bool,
}

fn shown_json(shown: Shown) -> Value {
    json!({ "page": shown.page, "fragment": shown.fragment, "query": shown.query })
}

/// Where the slides of a markdown file break.
fn breaks_json(markdown: &str) -> Value {
    let breaks = breaks(markdown);
    json!({
        "deck": breaks.deck,
        "start": breaks.start,
        "rules": breaks.rules,
        "imports": breaks.imports.iter().map(|import| json!({
            "line": import.line,
            "src": import.src,
        })).collect::<Vec<_>>(),
        "headings": breaks.headings.iter().map(|heading| json!({
            "line": heading.line,
            "last": heading.last,
            "level": heading.level,
        })).collect::<Vec<_>>(),
    })
}

/// The steps of the slides of a markdown file, by line, with the saved
/// outputs of its cells, if it is in a place they are saved.
fn steps_json(markdown: &str, place: Option<&Place>) -> Value {
    let slides = steps(markdown, |codes| match place {
        Some(place) => place.html(codes),
        None => Vec::new(),
    });
    let slides: Vec<Value> = slides
        .into_iter()
        .map(|slide| {
            json!({
                "line": slide.line,
                "count": slide.count,
                "steps": slide.steps.iter().map(|step| json!({
                    "line": step.line,
                    "from": step.from,
                    "to": step.to,
                    "collapse": step.collapse,
                })).collect::<Vec<_>>(),
                "warnings": slide.warnings.iter().map(|warning| json!({
                    "line": warning.line,
                    "message": warning.message,
                })).collect::<Vec<_>>(),
                "columns": slide.columns.iter().map(|column| json!({
                    "line": column.line,
                    "last": column.last,
                    "index": column.index,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Value::Array(slides)
}

/// Where the outputs of a document's cells are saved: next to its file, or,
/// for a notebook's cell, its notebook's. A document not saved has none.
fn place(uri: &str) -> Option<Place> {
    file(uri).map(|path| Place::of(&path))
}

/// The file a document is of, unless it is not saved.
fn file(uri: &str) -> Option<PathBuf> {
    if uri.starts_with("untitled:") {
        return None;
    }
    path_in(uri)
}

/// The path of a `file:` uri.
fn file_path(uri: &str) -> Option<PathBuf> {
    uri.starts_with("file:").then(|| path_in(uri)).flatten()
}

/// The path a uri holds, whatever its scheme, as a notebook's cell holds its
/// notebook's.
fn path_in(uri: &str) -> Option<PathBuf> {
    let (_, rest) = uri.split_once(':')?;
    let rest = match rest.strip_prefix("//") {
        Some(rest) => &rest[rest.find('/').unwrap_or(rest.len())..],
        None => rest,
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    Some(PathBuf::from(decoded(&rest[..end])))
}

/// The `file:` uri of a path.
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// A uri's path with what is percent-encoded in it decoded.
fn decoded(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let hex = after
            .get(..2)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, hex) {
            (b'%', Some(decoded)) => {
                bytes.push(decoded);
                rest = &after[2..];
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The byte of a line at a character, as the protocol counts them, in UTF-16.
fn byte(line: &str, character: u32) -> usize {
    let mut counted = 0;
    for (at, c) in line.char_indices() {
        if counted >= character as usize {
            return at;
        }
        counted += c.len_utf16();
    }
    line.len()
}

/// How many characters the protocol counts in a text, in UTF-16.
fn utf16(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i32, method: &str, params: Value) -> Request {
        Request::new(lsp_server::RequestId::from(id), method.to_string(), params)
    }

    #[test]
    fn a_path_and_its_uri_go_both_ways() {
        let path = Path::new("/a deck/talk #1.md");
        let uri = file_uri(path);
        assert_eq!(uri, "file:///a%20deck/talk%20%231.md");
        assert_eq!(file_path(&uri).as_deref(), Some(path));
        // A notebook's cell holds its notebook's path.
        assert_eq!(
            path_in("vscode-notebook-cell:/a%20deck/talk.md#W0sZmlsZQ%3D%3D").as_deref(),
            Some(Path::new("/a deck/talk.md"))
        );
        assert_eq!(file("untitled:Untitled-1"), None);
    }

    #[test]
    fn a_character_is_counted_in_utf16() {
        let line = "é😀x";
        assert_eq!(byte(line, 1), 2);
        assert_eq!(byte(line, 3), 6);
        assert_eq!(byte(line, 9), line.len());
        assert_eq!(utf16(line), 4);
    }

    #[test]
    fn the_deck_s_requests_are_answered() {
        let (client, server) = Connection::memory();
        let (events, heard) = std::sync::mpsc::channel();
        let sender = events.clone();
        let pythons = Pythons::new(&json!({}), Vec::new(), events, Arc::new(|_| {}));
        let mut server = Server::new(server, heard, sender, pythons);
        let markdown = "# One\n\n~~~python\nx = 1\n~~~\n\n---\n\n# Two\n";
        let cells = server
            .respond(request(1, "slides/cells", json!({ "markdown": markdown })))
            .ok()
            .unwrap();
        assert_eq!(cells.as_array().unwrap().len(), 3);
        let written = server
            .respond(request(2, "slides/markdown", json!({ "cells": cells })))
            .ok()
            .unwrap();
        assert_eq!(written, markdown);
        let breaks = server
            .respond(request(3, "slides/breaks", json!({ "markdown": markdown })))
            .ok()
            .unwrap();
        assert_eq!(breaks["rules"], json!([6]));
        assert!(matches!(
            server.respond(request(4, "slides/nothing", Value::Null)),
            Err(Failure::NotFound(_))
        ));
        drop(client);
    }
}
