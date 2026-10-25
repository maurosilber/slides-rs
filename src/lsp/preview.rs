//! The decks the extension shows: each rendered by `slides-rs --watch`, again
//! whenever it or a file it imports is saved, as a file another one imports
//! is rendered as it is in that deck. Its page follows the cursor of the text
//! editor, which the extension tells of, from a server on this machine that
//! sends each page the slide and step to go to.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use lsp_server::RequestId;
use slides::paths::parent;

use super::Event;

/// A deck being rendered, for a file shown.
struct Watch {
    file: PathBuf,
    deck: Option<PathBuf>,
    /// What the page names it by, to be sent where the cursor is.
    id: String,
    child: Child,
    /// The page, once it is first rendered.
    page: Option<PathBuf>,
    /// Where to send the page once it is rendered again, after a save.
    next: Option<String>,
}

/// A request to show a file, answered once its page is rendered.
pub struct Showing {
    pub id: RequestId,
    pub fragment: Option<String>,
    pub query: Option<String>,
}

/// What the extension opens: the page, at `#slide.step` if given, with the
/// query the page follows the cursor by.
pub struct Shown {
    pub page: PathBuf,
    pub fragment: Option<String>,
    pub query: Option<String>,
}

#[derive(Default)]
pub struct Watches {
    watches: Vec<Option<Watch>>,
    /// The requests to show each deck, waiting for it to be rendered.
    waiting: HashMap<usize, Vec<Showing>>,
    pub follow: Follow,
}

impl Watches {
    /// The deck rendering `file` as it is in `deck`, started if it is not yet.
    pub fn start(
        &mut self,
        executable: &Path,
        file: &Path,
        deck: Option<&Path>,
        events: &Sender<Event>,
    ) -> Result<usize, String> {
        let found = self.watches.iter().position(|watch| {
            watch
                .as_ref()
                .is_some_and(|watch| watch.file == file && watch.deck.as_deref() == deck)
        });
        if let Some(index) = found {
            return Ok(index);
        }
        let dir = parent(file).to_path_buf();
        let mut command = Command::new(executable);
        command
            .arg(file)
            .arg("--watch")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(deck) = deck {
            command.arg("--deck").arg(deck);
        }
        // A group of its own, so that stopping it stops the kernel it may be
        // running too.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
        let index = self.watches.len();
        let stderr = BufReader::new(child.stderr.take().unwrap());
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let events = events.clone();
        std::thread::spawn(move || {
            for line in stderr.lines() {
                let Ok(line) = line else {
                    break;
                };
                eprintln!("{name}: {line}");
                if let Some(page) = rendered(&line) {
                    let _ = events.send(Event::Rendered(index, dir.join(page)));
                }
            }
            let _ = events.send(Event::Stopped(index));
        });
        let id = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
        self.watches.push(Some(Watch {
            file: file.to_path_buf(),
            deck: deck.map(Path::to_path_buf),
            id,
            child,
            page: None,
            next: None,
        }));
        Ok(index)
    }

    /// What the page names the deck by.
    pub fn id(&self, index: usize) -> Option<&str> {
        Some(self.watches.get(index)?.as_ref()?.id.as_str())
    }

    /// Shows the deck's page once it is rendered: at once, if it is already.
    pub fn show(&mut self, index: usize, showing: Showing) -> Option<(RequestId, Shown)> {
        let watch = self.watches.get(index)?.as_ref()?;
        match &watch.page {
            Some(page) => Some((
                showing.id,
                Shown {
                    page: page.clone(),
                    fragment: showing.fragment,
                    query: showing.query,
                },
            )),
            None => {
                self.waiting.entry(index).or_default().push(showing);
                None
            }
        }
    }

    /// The decks rendering the file at `path`.
    pub fn of(&self, path: &Path) -> Vec<usize> {
        (0..self.watches.len())
            .filter(|&index| {
                self.watches[index]
                    .as_ref()
                    .is_some_and(|watch| watch.file == path)
            })
            .collect()
    }

    /// Sends the page where the cursor is once it is rendered again.
    pub fn after_render(&mut self, index: usize, fragment: String) {
        if let Some(Some(watch)) = self.watches.get_mut(index) {
            watch.next = Some(fragment);
        }
    }

    /// The deck's page, rendered: the requests waiting for it, and where to
    /// open it, after a save, if no page follows the cursor to be sent there.
    pub fn rendered(
        &mut self,
        index: usize,
        page: PathBuf,
    ) -> (Vec<(RequestId, Shown)>, Option<Shown>) {
        let Some(Some(watch)) = self.watches.get_mut(index) else {
            return (Vec::new(), None);
        };
        watch.page = Some(page.clone());
        let answers = self
            .waiting
            .remove(&index)
            .unwrap_or_default()
            .into_iter()
            .map(|showing| {
                let shown = Shown {
                    page: page.clone(),
                    fragment: showing.fragment,
                    query: showing.query,
                };
                (showing.id, shown)
            })
            .collect();
        let opened = watch.next.take().and_then(|fragment| {
            // What the page that reloads is sent as it connects again.
            self.follow.send(&watch.id, &fragment);
            (!self.follow.connected(&watch.id)).then_some(Shown {
                page,
                fragment: Some(fragment),
                query: None,
            })
        });
        (answers, opened)
    }

    /// Forgets a deck that stopped rendering, and returns the requests that
    /// were waiting for it.
    pub fn stopped(&mut self, index: usize) -> Vec<RequestId> {
        if let Some(Some(watch)) = self.watches.get_mut(index) {
            let _ = watch.child.wait();
            eprintln!("{}: stopped rendering", watch.file.display());
            self.follow.forget(&watch.id);
        }
        if let Some(slot) = self.watches.get_mut(index) {
            *slot = None;
        }
        self.waiting
            .remove(&index)
            .unwrap_or_default()
            .into_iter()
            .map(|showing| showing.id)
            .collect()
    }

    /// Stops rendering every deck.
    pub fn stop(&mut self) {
        for watch in self.watches.iter_mut().flatten() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(watch.child.id() as i32), libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = watch.child.kill();
        }
    }
}

/// The page a line of `slides-rs` says it rendered, as it is already or
/// anew.
fn rendered(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("rendered ") {
        let (page, _) = rest.rsplit_once(" in ")?;
        return Some(page);
    }
    let (page, _) = line.rsplit_once(" is unchanged, after ")?;
    Some(page)
}

/// A server on this machine from which each page, by the id of the deck it
/// shows, is sent the slide and step to go to, as `slide.step`, and the last
/// of them as it connects.
#[derive(Default)]
pub struct Follow {
    port: Option<u16>,
    pages: Arc<Mutex<Pages>>,
}

#[derive(Default)]
struct Pages {
    /// The pages connected, by the id of their deck, each by a number of its
    /// own.
    connected: HashMap<String, HashMap<u64, TcpStream>>,
    /// The last place sent, by the id of the deck.
    last: HashMap<String, String>,
    next: u64,
}

impl Follow {
    /// The port of the server, started the first time.
    pub fn port(&mut self) -> std::io::Result<u16> {
        if let Some(port) = self.port {
            return Ok(port);
        }
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let pages = self.pages.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let pages = pages.clone();
                std::thread::spawn(move || connect(stream, &pages));
            }
        });
        self.port = Some(port);
        Ok(port)
    }

    /// Whether a page of the deck is connected.
    pub fn connected(&self, id: &str) -> bool {
        let pages = self.pages.lock().unwrap();
        pages
            .connected
            .get(id)
            .is_some_and(|pages| !pages.is_empty())
    }

    /// Sends the pages of the deck to `position`, unless it is where they
    /// were sent last.
    pub fn send(&self, id: &str, position: &str) {
        let mut pages = self.pages.lock().unwrap();
        if pages.last.get(id).map(String::as_str) == Some(position) {
            return;
        }
        pages.last.insert(id.to_string(), position.to_string());
        if let Some(connected) = pages.connected.get_mut(id) {
            connected.retain(|_, page| write!(page, "data: {position}\n\n").is_ok());
        }
    }

    /// Disconnects the pages of the deck, which stopped rendering.
    pub fn forget(&self, id: &str) {
        let mut pages = self.pages.lock().unwrap();
        for page in pages
            .connected
            .remove(id)
            .into_iter()
            .flat_map(HashMap::into_values)
        {
            let _ = page.shutdown(std::net::Shutdown::Both);
        }
        pages.last.remove(id);
    }
}

/// Serves a page that connects, as a stream of events, until it disconnects.
fn connect(stream: TcpStream, pages: &Mutex<Pages>) {
    let Ok(reading) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reading);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // The headers, up to the blank line after them.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
        header.clear();
    }
    let id = request
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.strip_prefix('/'))
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(String::from);
    let mut writing = stream;
    let Some(id) = id else {
        let _ = writing.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    // The page is a file, from no origin.
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\n\r\nretry: 1000\n\n";
    let number = {
        let mut pages = pages.lock().unwrap();
        let mut sent = writing.write_all(head.as_bytes());
        if let Some(last) = pages.last.get(&id) {
            sent = sent.and_then(|()| write!(writing, "data: {last}\n\n"));
        }
        if sent.is_err() {
            return;
        }
        let Ok(page) = writing.try_clone() else {
            return;
        };
        pages.next += 1;
        let number = pages.next;
        pages
            .connected
            .entry(id.clone())
            .or_default()
            .insert(number, page);
        number
    };
    // The page sends nothing more, so a read ends as it disconnects.
    let mut rest = Vec::new();
    let _ = std::io::Read::read_to_end(&mut reader, &mut rest);
    let mut pages = pages.lock().unwrap();
    if let Some(connected) = pages.connected.get_mut(&id) {
        connected.remove(&number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{Duration, Instant};

    #[test]
    fn the_page_rendered_is_read_from_what_slides_rs_says() {
        assert_eq!(
            rendered("rendered _outputs/talk.html in 12ms"),
            Some("_outputs/talk.html")
        );
        assert_eq!(
            rendered("_outputs/talk.html is unchanged, after 3ms"),
            Some("_outputs/talk.html")
        );
        assert_eq!(rendered("running 2 cells"), None);
    }

    #[test]
    fn a_page_is_sent_where_to_go_and_the_last_place_as_it_connects() {
        let mut follow = Follow::default();
        let port = follow.port().unwrap();
        follow.send("deck", "2.1");
        let mut page = TcpStream::connect(("127.0.0.1", port)).unwrap();
        page.write_all(b"GET /deck HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let start = Instant::now();
        while !follow.connected("deck") {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the page did not connect"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        follow.send("deck", "3.2");
        follow.forget("deck");
        let mut received = String::new();
        page.read_to_string(&mut received).unwrap();
        assert!(received.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(received.ends_with("retry: 1000\n\ndata: 2.1\n\ndata: 3.2\n\n"));
    }
}
