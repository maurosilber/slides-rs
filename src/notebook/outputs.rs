//! What a kernel publishes for a cell, turned into the outputs to save.

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jupyter_protocol::media::MediaType;
use jupyter_protocol::{ErrorOutput, Media, StreamContent};

use super::svg;
use crate::store::{self, Output};

fn text(extension: &'static str, text: &str) -> Output {
    Output {
        extension,
        bytes: text.as_bytes().to_vec(),
    }
}

/// Image payloads arrive base64-encoded; store the decoded bytes so the
/// file on disk is a real image.
fn image(extension: &'static str, base64: &str) -> Result<Output> {
    let bytes = BASE64
        .decode(base64.trim())
        .with_context(|| format!("{extension} output is not valid base64"))?;
    Ok(Output { extension, bytes })
}

fn from_media(media: &MediaType) -> Result<Output> {
    match media {
        MediaType::Html(html) => Ok(text("html", html)),
        MediaType::Svg(svg) => Ok(text("svg", &svg::inline(svg)?)),
        MediaType::Markdown(markdown) => Ok(text("md", markdown)),
        MediaType::Plain(plain) => Ok(text("txt", plain)),
        MediaType::Png(data) => image("png", data),
        MediaType::Jpeg(data) => image("jpeg", data),
        MediaType::Gif(data) => image("gif", data),
        other => anyhow::bail!("unsupported media type {}", other.mime_type()),
    }
}

/// How much we prefer each representation of the same output, in the order
/// of the media types the store saves. A rank of 0 means we cannot put it in
/// an HTML document, so it is never chosen.
fn rank(media: &MediaType) -> usize {
    store::MEDIA_TYPES
        .iter()
        .position(|&(mime, _)| mime == media.mime_type())
        .map_or(0, |index| store::MEDIA_TYPES.len() - index)
}

/// Collects what a kernel publishes for one cell, in the order it arrives,
/// as a notebook shows it once the cell is done: without what it cleared, and
/// with each display as it was last updated.
#[derive(Default)]
pub struct Outputs {
    /// Each output, with the id of the display it shows, if it has one.
    outputs: Vec<(Output, Option<String>)>,
    /// Which stream the last output came from, if it was a stream at all.
    open_stream: Option<&'static str>,
    /// Whether to clear what there is once the next output arrives, as
    /// `clear_output(wait=True)` asks, to redraw without flickering.
    clear_on_next: bool,
}

impl Outputs {
    /// A kernel splits a long `print` across several messages, so consecutive
    /// text from the same stream belongs in one file.
    pub fn push_stream(&mut self, stream: &StreamContent) {
        self.next();
        let name = match stream.name {
            jupyter_protocol::Stdio::Stdout => "stdout",
            jupyter_protocol::Stdio::Stderr => "stderr",
        };
        match self.outputs.last_mut() {
            Some((last, _)) if self.open_stream == Some(name) => {
                last.bytes.extend_from_slice(stream.text.as_bytes());
            }
            _ => {
                self.outputs.push((text("txt", &stream.text), None));
                self.open_stream = Some(name);
            }
        }
    }

    /// Keep the richest representation the kernel offered; a figure arrives as
    /// both a PNG and a `<Figure ...>` repr, and only the PNG belongs on a slide.
    /// A display with an id can be updated later, by `update_media`.
    pub fn push_media(&mut self, media: &Media, display: Option<&str>) -> Result<()> {
        self.next();
        self.open_stream = None;
        let Some(richest) = media.richest(rank) else {
            return Ok(());
        };
        self.outputs
            .push((from_media(richest)?, display.map(str::to_string)));
        Ok(())
    }

    /// Shows `media` in place of what each display with the id `display`
    /// shows, as `display_handle.update` asks.
    pub fn update_media(&mut self, media: &Media, display: &str) -> Result<()> {
        let Some(richest) = media.richest(rank) else {
            return Ok(());
        };
        let updated = from_media(richest)?;
        for (output, id) in &mut self.outputs {
            if id.as_deref() == Some(display) {
                *output = Output {
                    extension: updated.extension,
                    bytes: updated.bytes.clone(),
                };
            }
        }
        Ok(())
    }

    pub fn push_error(&mut self, error: &ErrorOutput) {
        self.next();
        self.open_stream = None;
        let traceback = error.traceback.join("\n");
        self.outputs
            .push((text("txt", &strip_ansi(&traceback)), None));
    }

    /// Clears what the cell has shown so far, as `clear_output` asks: now, or,
    /// with `wait`, once the next output arrives.
    pub fn clear(&mut self, wait: bool) {
        if wait {
            self.clear_on_next = true;
        } else {
            self.outputs.clear();
            self.open_stream = None;
            self.clear_on_next = false;
        }
    }

    /// Before an output arrives, clears what was to be cleared then.
    fn next(&mut self) {
        if self.clear_on_next {
            self.clear(false);
        }
    }

    pub fn into_vec(self) -> Vec<Output> {
        self.outputs.into_iter().map(|(output, _)| output).collect()
    }
}

/// Tracebacks come coloured with ANSI escapes, which are noise in HTML.
fn strip_ansi(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            clean.push(c);
            continue;
        }
        // Drop the escape up to and including its final byte, `@` to `~`.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_escapes_are_removed() {
        let traceback = "\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero";
        assert_eq!(strip_ansi(traceback), "ZeroDivisionError: division by zero");
    }

    fn stdout(text: &str) -> StreamContent {
        StreamContent {
            name: jupyter_protocol::Stdio::Stdout,
            text: text.to_string(),
        }
    }

    fn plain(text: &str) -> Media {
        Media {
            content: vec![MediaType::Plain(text.to_string())],
        }
    }

    fn texts(outputs: Outputs) -> Vec<String> {
        outputs
            .into_vec()
            .into_iter()
            .map(|output| String::from_utf8(output.bytes).unwrap())
            .collect()
    }

    #[test]
    fn what_is_cleared_is_not_saved() {
        let mut outputs = Outputs::default();
        outputs.push_stream(&stdout("1\n"));
        outputs.clear(false);
        outputs.push_stream(&stdout("2\n"));
        outputs.push_media(&plain("a"), None).unwrap();
        // Cleared once the next output arrives, as an animation redraws.
        outputs.clear(true);
        outputs.push_media(&plain("b"), None).unwrap();
        outputs.clear(true);
        assert_eq!(texts(outputs), ["b"]);
    }

    #[test]
    fn a_display_is_saved_as_last_updated() {
        let mut outputs = Outputs::default();
        outputs.push_media(&plain("0%"), Some("bar")).unwrap();
        outputs.push_stream(&stdout("working\n"));
        outputs.update_media(&plain("50%"), "bar").unwrap();
        outputs.update_media(&plain("100%"), "bar").unwrap();
        outputs.update_media(&plain("other"), "none").unwrap();
        assert_eq!(texts(outputs), ["100%", "working\n"]);
    }

    #[test]
    fn a_figure_is_saved_as_an_image_not_as_its_repr() {
        let media = Media {
            content: vec![
                MediaType::Plain("<Figure size 640x480>".to_string()),
                MediaType::Png(BASE64.encode(b"not really a png")),
            ],
        };
        let mut outputs = Outputs::default();
        outputs.push_media(&media, None).unwrap();
        let outputs = outputs.into_vec();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].extension, "png");
        assert_eq!(outputs[0].bytes, b"not really a png");
    }

    #[test]
    fn an_svg_is_saved_ready_to_be_inlined() {
        let media = Media {
            content: vec![MediaType::Svg(
                r#"<?xml version="1.0"?><svg><g id="step=1"/></svg>"#.to_string(),
            )],
        };
        let mut outputs = Outputs::default();
        outputs.push_media(&media, None).unwrap();
        assert_eq!(outputs.into_vec()[0].bytes, br#"<svg><g step="1"/></svg>"#);
    }

    #[test]
    fn consecutive_stream_messages_become_one_output() {
        let mut outputs = Outputs::default();
        for text in ["one\n", "two\n"] {
            outputs.push_stream(&StreamContent {
                name: jupyter_protocol::Stdio::Stdout,
                text: text.to_string(),
            });
        }
        let outputs = outputs.into_vec();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].bytes, b"one\ntwo\n");
    }
}
