//! SVG outputs, prepared to be inlined in the deck rather than linked.

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};

/// The id that marks an element as a fragment, followed by its number. In
/// matplotlib, `gid="fragment 2"` gives an artist's group this id.
const FRAGMENT: &str = "fragment ";

/// The SVG as markup for an html document, with every `id="fragment n"`
/// turned into `fragment="n"` for the slides to step through.
///
/// The XML declaration and doctype are dropped, as they have no place in
/// the middle of an html body.
pub fn inline(svg: &str) -> Result<String> {
    let mut reader = Reader::from_str(svg);
    let mut writer = Writer::new(Vec::new());
    loop {
        let event = match reader.read_event().context("the SVG is not valid XML")? {
            Event::Eof => break,
            Event::Decl(_) | Event::DocType(_) => continue,
            Event::Start(element) => Event::Start(fragment(element)),
            Event::Empty(element) => Event::Empty(fragment(element)),
            event => event,
        };
        writer.write_event(event)?;
    }
    Ok(String::from_utf8(writer.into_inner())?)
}

/// The element, with an `id="fragment n"` replaced by `fragment="n"`.
/// Only the number is kept, as ids like this repeat across figures.
fn fragment(element: BytesStart<'_>) -> BytesStart<'_> {
    let number = element.attributes().flatten().find_map(|attribute| {
        let number = attribute.value.strip_prefix(FRAGMENT)?;
        let is_id = attribute.key.as_ref() == "id";
        (is_id && number.parse::<u32>().is_ok()).then(|| number.to_string())
    });
    let Some(number) = number else {
        return element;
    };

    let mut replaced = BytesStart::new(element.name().as_ref().to_owned());
    for attribute in element.attributes().flatten() {
        if attribute.key.as_ref() == "id" {
            replaced.push_attribute(("fragment", number.as_str()));
        } else {
            replaced.push_attribute(attribute);
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fragment_id_becomes_a_fragment_attribute() {
        let svg = r#"<svg><g id="fragment 2" class="a"><path d="M 0 0"/></g></svg>"#;
        assert_eq!(
            inline(svg).unwrap(),
            r#"<svg><g fragment="2" class="a"><path d="M 0 0"/></g></svg>"#
        );
    }

    #[test]
    fn other_ids_are_kept() {
        let svg = r#"<svg><g id="line2d_1"/><g id="fragment x"/></svg>"#;
        assert_eq!(inline(svg).unwrap(), svg);
    }

    #[test]
    fn the_declaration_and_doctype_are_dropped() {
        let svg = concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\n",
            "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"\n",
            "  \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n",
            "<svg/>\n",
        );
        assert_eq!(inline(svg).unwrap().trim(), "<svg/>");
    }
}
