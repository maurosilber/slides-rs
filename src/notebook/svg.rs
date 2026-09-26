//! SVG outputs, prepared before they are saved to be inlined in the deck
//! rather than linked.

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::{Context, Result};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::{Reader, Writer};
use sha2::{Digest, Sha256};

use crate::step::Step;

/// The SVG as markup for an html document, with every id that marks a step,
/// as matplotlib's `gid="step=2..4"`, `gid="step"` or `gid="also"` gives an
/// artist's group, turned into `step="2..4"`, `step=""` or `also=""` for the
/// slides to step through.
///
/// The XML declaration, doctype, comments and metadata are dropped, as they
/// have no place in the middle of an html body, and the metadata holds the
/// date the figure was drawn. Ids nothing refers to are dropped too, and the
/// rest are renamed after the figure: matplotlib salts them at random, and
/// they would clash with those of other figures on the page. The same figure
/// is then always the same markup.
pub fn inline(svg: &str) -> Result<String> {
    let ids = referenced(svg)?;
    // The figure is named by its markup with the ids numbered, which leaves
    // out the random salt.
    let numbered = write(svg, &ids, "")?;
    let digest = Sha256::digest(&numbered);
    let prefix: String = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(String::from_utf8(write(
        svg,
        &ids,
        &format!("f{prefix}-"),
    )?)?)
}

/// The ids the SVG refers to, by `url(#id)` or `href="#id"`, numbered in
/// the order they are first referred to.
fn referenced(svg: &str) -> Result<HashMap<String, usize>> {
    let mut ids = HashMap::new();
    let mut reader = Reader::from_str(svg);
    loop {
        let element = match reader.read_event().context("the SVG is not valid XML")? {
            Event::Eof => break,
            Event::Start(element) | Event::Empty(element) => element,
            _ => continue,
        };
        for attribute in element.attributes().flatten() {
            for id in references(attribute.key, &attribute.value) {
                let next = ids.len();
                ids.entry(id.to_string()).or_insert(next);
            }
        }
    }
    Ok(ids)
}

/// The SVG with its elements rewritten by `element`, and what is not
/// needed in an html body dropped.
fn write(svg: &str, ids: &HashMap<String, usize>, prefix: &str) -> Result<Vec<u8>> {
    let mut reader = Reader::from_str(svg);
    let mut writer = Writer::new(Vec::new());
    loop {
        let event = match reader.read_event().context("the SVG is not valid XML")? {
            Event::Eof => break,
            Event::Decl(_) | Event::DocType(_) | Event::Comment(_) => continue,
            Event::Start(start) if start.name().as_ref() == "metadata" => {
                reader.read_to_end(start.name())?;
                continue;
            }
            Event::Empty(start) if start.name().as_ref() == "metadata" => continue,
            Event::Start(start) => Event::Start(element(start, ids, prefix)),
            Event::Empty(start) => Event::Empty(element(start, ids, prefix)),
            event => event,
        };
        writer.write_event(event)?;
    }
    Ok(writer.into_inner())
}

/// The element, with an id that marks a step replaced by the attribute the
/// slides read, the other ids referred to renamed, and the rest dropped.
/// Only the step is kept, as ids like this repeat across figures.
fn element<'a>(
    element: BytesStart<'a>,
    ids: &HashMap<String, usize>,
    prefix: &str,
) -> BytesStart<'a> {
    let rename = |id: &str| ids.get(id).map(|n| format!("{prefix}{n}"));
    let mut rewritten = BytesStart::new(element.name().as_ref().to_owned());
    for attribute in element.attributes().flatten() {
        if attribute.key.as_ref() == "id" {
            let value = &attribute.value;
            if let Some(step) = Step::parse(value) {
                let (key, value) = match step {
                    Step::Range(range) => ("step", range.to_string()),
                    Step::Next => ("step", String::new()),
                    Step::Also => ("also", String::new()),
                };
                rewritten.push_attribute(Attribute {
                    key: QName(key),
                    value: Cow::Owned(value),
                });
            } else if let Some(id) = rename(value) {
                rewritten.push_attribute(Attribute {
                    key: attribute.key,
                    value: Cow::Owned(id),
                });
            }
            continue;
        }
        let value = rewrite(attribute.key, &attribute.value, |id| rename(id).unwrap());
        rewritten.push_attribute(Attribute {
            key: attribute.key,
            value: Cow::Owned(value),
        });
    }
    rewritten
}

/// The ids an attribute refers to: the `#id` of an `href`, or each
/// `url(#id)` of any other attribute.
fn references<'v>(key: QName<'_>, value: &'v str) -> Vec<&'v str> {
    let mut ids = Vec::new();
    rewrite(key, value, |id| {
        ids.push(id);
        String::new()
    });
    ids
}

/// The attribute's value, with each id it refers to replaced by `rename`.
fn rewrite<'v>(
    key: QName<'_>,
    value: &'v str,
    mut rename: impl FnMut(&'v str) -> String,
) -> String {
    if matches!(key.as_ref(), "href" | "xlink:href") {
        return match value.strip_prefix('#') {
            Some(id) => format!("#{}", rename(id)),
            None => value.to_string(),
        };
    }
    let mut rewritten = String::new();
    let mut rest = value;
    while let Some((before, after)) = rest.split_once("url(#")
        && let Some((id, after)) = after.split_once(')')
    {
        rewritten.push_str(before);
        rewritten.push_str("url(#");
        rewritten.push_str(&rename(id));
        rewritten.push(')');
        rest = after;
    }
    rewritten.push_str(rest);
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_id_becomes_a_step_attribute() {
        let svg = r#"<svg><g id="step=2" class="a"><path d="M 0 0"/></g></svg>"#;
        assert_eq!(
            inline(svg).unwrap(),
            r#"<svg><g step="2" class="a"><path d="M 0 0"/></g></svg>"#
        );
    }

    #[test]
    fn a_step_id_can_be_the_next_step_or_along_with_the_latest() {
        let svg = r#"<svg><g id="step"/><g id="also"/></svg>"#;
        assert_eq!(
            inline(svg).unwrap(),
            r#"<svg><g step=""/><g also=""/></svg>"#
        );
    }

    #[test]
    fn a_step_id_can_be_a_range() {
        let svg = r#"<svg><g id="step=2..4"/><g id="step=..3"/><g id="step=5.."/></svg>"#;
        assert_eq!(
            inline(svg).unwrap(),
            r#"<svg><g step="2..4"/><g step="..3"/><g step="5"/></svg>"#
        );
    }

    #[test]
    fn ids_nothing_refers_to_are_dropped() {
        let svg = r#"<svg><g id="line2d_1"/><g id="step=x"/></svg>"#;
        assert_eq!(inline(svg).unwrap(), "<svg><g/><g/></svg>");
    }

    #[test]
    fn ids_referred_to_are_renamed_after_the_figure() {
        let figure = |salt: &str| {
            format!(
                concat!(
                    r#"<svg><g clip-path="url(#p{0})" style="fill: url(#h{0})"/>"#,
                    r##"<use xlink:href="#m{0}"/>"##,
                    r#"<clipPath id="p{0}"/><pattern id="h{0}"/><path id="m{0}"/></svg>"#,
                ),
                salt
            )
        };
        let inlined = inline(&figure("1a2b")).unwrap();
        assert_eq!(inlined, inline(&figure("3c4d")).unwrap());

        let prefix = &inlined[inlined.find("url(#").unwrap() + 5..][..10];
        assert_eq!(
            inlined,
            format!(
                concat!(
                    r#"<svg><g clip-path="url(#{0}0)" style="fill: url(#{0}1)"/>"#,
                    r##"<use xlink:href="#{0}2"/>"##,
                    r#"<clipPath id="{0}0"/><pattern id="{0}1"/><path id="{0}2"/></svg>"#,
                ),
                prefix
            )
        );
    }

    #[test]
    fn different_figures_have_different_ids() {
        let id = |d: &str| {
            let svg = format!(r##"<svg><use href="#a"/><path id="a" d="{d}"/></svg>"##);
            inline(&svg).unwrap().split('"').nth(1).unwrap().to_string()
        };
        assert_ne!(id("M 0 0"), id("M 1 1"));
    }

    #[test]
    fn the_declaration_doctype_comments_and_metadata_are_dropped() {
        let svg = concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\n",
            "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"\n",
            "  \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n",
            "<svg><metadata><dc:date>2026-09-26</dc:date></metadata><!-- 0.5 --></svg>\n",
        );
        assert_eq!(inline(svg).unwrap().trim(), "<svg></svg>");
    }
}
