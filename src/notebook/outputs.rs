//! What a kernel publishes for a cell, turned into the outputs to save.

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jupyter_protocol::media::MediaType;
use jupyter_protocol::{ErrorOutput, Media, StreamContent};

use super::svg;
use crate::store::Output;

impl Output {
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
            MediaType::Html(html) => Ok(Output::text("html", html)),
            MediaType::Svg(svg) => Ok(Output::text("svg", &svg::inline(svg)?)),
            MediaType::Markdown(markdown) => Ok(Output::text("md", markdown)),
            MediaType::Plain(text) => Ok(Output::text("txt", text)),
            MediaType::Png(data) => Output::image("png", data),
            MediaType::Jpeg(data) => Output::image("jpeg", data),
            MediaType::Gif(data) => Output::image("gif", data),
            other => anyhow::bail!("unsupported media type {}", other.mime_type()),
        }
    }
}

/// How much we prefer each representation of the same output. A rank of 0
/// means we cannot put it in an HTML document, so it is never chosen.
fn rank(media: &MediaType) -> usize {
    match media {
        MediaType::Html(_) => 7,
        MediaType::Svg(_) => 6,
        MediaType::Png(_) => 5,
        MediaType::Jpeg(_) => 4,
        MediaType::Gif(_) => 3,
        MediaType::Markdown(_) => 2,
        MediaType::Plain(_) => 1,
        _ => 0,
    }
}

/// Collects what a kernel publishes for one cell, in the order it arrives.
#[derive(Default)]
pub struct Outputs {
    outputs: Vec<Output>,
    /// Which stream the last output came from, if it was a stream at all.
    open_stream: Option<&'static str>,
}

impl Outputs {
    /// A kernel splits a long `print` across several messages, so consecutive
    /// text from the same stream belongs in one file.
    pub fn push_stream(&mut self, stream: &StreamContent) {
        let name = match stream.name {
            jupyter_protocol::Stdio::Stdout => "stdout",
            jupyter_protocol::Stdio::Stderr => "stderr",
        };
        match self.outputs.last_mut() {
            Some(last) if self.open_stream == Some(name) => {
                last.bytes.extend_from_slice(stream.text.as_bytes());
            }
            _ => {
                self.outputs.push(Output::text("txt", &stream.text));
                self.open_stream = Some(name);
            }
        }
    }

    /// Keep the richest representation the kernel offered; a figure arrives as
    /// both a PNG and a `<Figure ...>` repr, and only the PNG belongs on a slide.
    pub fn push_media(&mut self, media: &Media) -> Result<()> {
        self.open_stream = None;
        let Some(richest) = media.richest(rank) else {
            return Ok(());
        };
        self.outputs.push(Output::from_media(richest)?);
        Ok(())
    }

    pub fn push_error(&mut self, error: &ErrorOutput) {
        self.open_stream = None;
        let traceback = error.traceback.join("\n");
        self.outputs
            .push(Output::text("txt", &strip_ansi(&traceback)));
    }

    pub fn into_vec(self) -> Vec<Output> {
        self.outputs
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

    #[test]
    fn a_figure_is_saved_as_an_image_not_as_its_repr() {
        let media = Media {
            content: vec![
                MediaType::Plain("<Figure size 640x480>".to_string()),
                MediaType::Png(BASE64.encode(b"not really a png")),
            ],
        };
        let mut outputs = Outputs::default();
        outputs.push_media(&media).unwrap();
        let outputs = outputs.into_vec();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].extension, "png");
        assert_eq!(outputs[0].bytes, b"not really a png");
    }

    #[test]
    fn an_svg_is_saved_ready_to_be_inlined() {
        let media = Media {
            content: vec![MediaType::Svg(
                r#"<?xml version="1.0"?><svg><g id="fragment 1"/></svg>"#.to_string(),
            )],
        };
        let mut outputs = Outputs::default();
        outputs.push_media(&media).unwrap();
        assert_eq!(
            outputs.into_vec()[0].bytes,
            br#"<svg><g fragment="1"/></svg>"#
        );
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
