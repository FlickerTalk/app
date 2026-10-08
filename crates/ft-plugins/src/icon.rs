//! The image a plugin may carry for its tile (2026-10-08, plan of the apps grid, "Imagen por
//! plugin"): `icon.svg` at the root of the package, signed with everything else. The app draws it
//! as an `<img>`, where a browser runs nothing; this still refuses anything an SVG could do beyond
//! drawing shapes, so a package that tries is not taken at all.
//!
//! A strict scanner of its own, stricter than XML on purpose: no declaration, comment, doctype,
//! CDATA or processing instruction (nothing that starts with `<!` or `<?`), and no entity other
//! than the five of XML. What is left is elements, attributes in quotes and text.

use anyhow::{bail, ensure, Context, Result};

/// Where the image lives in the package.
pub const IMAGE: &str = "icon.svg";

/// The most an image may weigh, in bytes.
pub const IMAGE_LIMIT: usize = 4096;

/// The only elements an image may use: shapes, groups, gradients, clips and masks.
const ELEMENTS: [&str; 17] = [
    "svg", "g", "title", "desc", "defs", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "linearGradient",
    "radialGradient", "stop", "clipPath", "mask",
];

/// The five entities of XML; any other is refused.
const ENTITIES: [&str; 5] = ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"];

/// Checks an `icon.svg`: small, well formed, one `<svg>` root with a square `viewBox`, only the
/// elements above, and no event handler, link or style anywhere.
pub fn check(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= IMAGE_LIMIT, "icon.svg weighs {} bytes, more than {IMAGE_LIMIT}", bytes.len());
    let text = std::str::from_utf8(bytes).context("icon.svg is not UTF-8")?;
    let mut scanner = Scanner { text, at: 0 };
    let mut open: Vec<&str> = Vec::new();
    let mut root_done = false;
    let mut root_seen = false;
    while scanner.at < text.len() {
        let rest = &text[scanner.at..];
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            let words = &rest[..end];
            ensure!(!open.is_empty() || words.trim().is_empty(), "icon.svg has text outside its <svg>");
            entities(words)?;
            scanner.at += end;
            continue;
        }
        ensure!(!rest.starts_with("<!") && !rest.starts_with("<?"), "icon.svg may not declare, comment or instruct (<! or <?)");
        if let Some(closing) = rest.strip_prefix("</") {
            let name = name_of(closing).context("icon.svg has a closing tag without a name")?;
            scanner.at += 2 + name.len();
            scanner.spaces();
            ensure!(scanner.eat('>'), "icon.svg has a closing tag that does not end");
            ensure!(open.pop() == Some(name), "icon.svg closes <{name}> where it is not open");
            if open.is_empty() {
                root_done = true;
            }
            continue;
        }
        ensure!(!root_done, "icon.svg has something after its <svg>");
        let name = name_of(&rest[1..]).context("icon.svg has a tag without a name")?;
        ensure!(ELEMENTS.contains(&name), "icon.svg may not use <{name}>");
        if open.is_empty() {
            ensure!(!root_seen && name == "svg", "icon.svg must be one <svg>");
            root_seen = true;
        }
        scanner.at += 1 + name.len();
        let mut names: Vec<&str> = Vec::new();
        let mut view_box = None;
        let closed = loop {
            let spaced = scanner.spaces();
            if scanner.eat_str("/>") {
                break true;
            }
            if scanner.eat('>') {
                break false;
            }
            ensure!(spaced, "icon.svg has attributes run together in <{name}>");
            let attribute = name_of(&text[scanner.at..]).context("icon.svg has a tag that does not end")?;
            scanner.at += attribute.len();
            scanner.spaces();
            ensure!(scanner.eat('='), "icon.svg has an attribute without a value in <{name}>");
            scanner.spaces();
            let value = scanner.quoted().with_context(|| format!("icon.svg has a value without quotes in <{name}>"))?;
            ensure!(!value.contains('<'), "icon.svg has a '<' inside a value");
            entities(value)?;
            ensure!(!names.contains(&attribute), "icon.svg repeats {attribute} in <{name}>");
            names.push(attribute);
            allowed(attribute)?;
            if open.is_empty() && name == "svg" && attribute == "viewBox" {
                view_box = Some(value);
            }
        };
        if open.is_empty() {
            square(view_box.context("icon.svg has no viewBox")?)?;
        }
        if closed {
            if open.is_empty() {
                root_done = true;
            }
        } else {
            open.push(name);
        }
    }
    ensure!(root_seen && open.is_empty(), "icon.svg is not one whole <svg>");
    Ok(())
}

/// No event handler, no link of any kind (`href`, `xlink:href`…), no inline style.
fn allowed(attribute: &str) -> Result<()> {
    let lower = attribute.to_ascii_lowercase();
    let local = lower.rsplit(':').next().unwrap_or(&lower);
    if local.starts_with("on") || local == "href" || local == "style" || lower.starts_with("on") {
        bail!("icon.svg may not use the attribute {attribute}");
    }
    Ok(())
}

/// A `viewBox` of four numbers, as wide as it is tall.
fn square(view_box: &str) -> Result<()> {
    let numbers: Vec<f64> = view_box
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f64>().ok().filter(|number| number.is_finite()))
        .collect::<Option<_>>()
        .context("icon.svg has a viewBox that is not numbers")?;
    ensure!(numbers.len() == 4, "icon.svg has a viewBox that is not four numbers");
    ensure!(numbers[2] > 0.0 && numbers[2] == numbers[3], "icon.svg has a viewBox that is not square");
    Ok(())
}

/// Every `&` starts one of the five entities of XML.
fn entities(text: &str) -> Result<()> {
    for (at, _) in text.match_indices('&') {
        ensure!(ENTITIES.iter().any(|entity| text[at..].starts_with(entity)), "icon.svg uses an entity XML does not have");
    }
    Ok(())
}

/// The name at the start of `text`: a letter or `_`, then letters, digits, `_`, `-`, `.` and `:`.
fn name_of(text: &str) -> Option<&str> {
    let first = text.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let end = text.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))).unwrap_or(text.len());
    Some(&text[..end])
}

struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    /// Skips white space; says whether there was any.
    fn spaces(&mut self) -> bool {
        let rest = &self.text[self.at..];
        let skipped = rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len();
        self.at += skipped;
        skipped > 0
    }

    fn eat(&mut self, c: char) -> bool {
        self.eat_str(c.encode_utf8(&mut [0; 4]))
    }

    fn eat_str(&mut self, s: &str) -> bool {
        let found = self.text[self.at..].starts_with(s);
        if found {
            self.at += s.len();
        }
        found
    }

    /// A value in single or double quotes, without them.
    fn quoted(&mut self) -> Option<&'a str> {
        let rest = &self.text[self.at..];
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let end = rest[1..].find(quote)? + 1;
        self.at += end + 1;
        Some(&rest[1..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The icon of one of our plugins, as the script draws them: a coloured square and a glyph.
    const GOOD: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <title>Ajedrez · Échecs</title>
  <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f0742a"/><stop offset="1" stop-color="#d3661f"/></linearGradient></defs>
  <rect width="64" height="64" rx="18" fill="url(#g)"/>
  <g fill="#fff"><path d="M32 12 L44 20 V34 C44 44 32 52 32 52 C32 52 20 44 20 34 V20 Z"/><circle cx="32" cy="30" r="4"/></g>
</svg>"##;

    fn with(inner: &str) -> String {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">{inner}</svg>"#)
    }

    #[test]
    fn a_plain_icon_of_shapes_and_gradients_passes() {
        check(GOOD.as_bytes()).expect("a good icon");
        check(with(r#"<clipPath id="c"><circle cx="1" cy="1" r="1"/></clipPath><mask id="m"><rect width="1" height="1"/></mask>"#).as_bytes())
            .expect("clips and masks");
        check(br#"<svg viewBox="0,0,24,24"><desc>a &amp; b &lt;3</desc><ellipse rx='1' ry='2'/><line x1="0"/><polyline points="0 0"/><polygon points="0 0"/><radialGradient id="r"/></svg>"#)
            .expect("commas, single quotes and the five entities");
    }

    #[test]
    fn an_element_that_does_more_than_draw_is_refused() {
        for wrong in [
            "<script>alert(1)</script>",
            r#"<foreignObject width="1" height="1"><div/></foreignObject>"#,
            r#"<image width="1" height="1"/>"#,
            r#"<use/>"#,
            "<style>rect{fill:red}</style>",
            r#"<a><rect/></a>"#,
            r#"<animate attributeName="x"/>"#,
            r#"<filter id="f"/>"#,
            r#"<Rect/>"#,
        ] {
            assert!(check(with(wrong).as_bytes()).is_err(), "{wrong} should be refused");
        }
    }

    #[test]
    fn an_attribute_that_links_styles_or_runs_is_refused() {
        for wrong in [
            r#"<rect onload="alert(1)"/>"#,
            r#"<rect ONCLICK="x"/>"#,
            r#"<rect href="https://evil.example/"/>"#,
            r##"<rect xlink:href="#x"/>"##,
            r##"<rect XLINK:HREF="#x"/>"##,
            r#"<rect style="fill:red"/>"#,
            r#"<rect ev:onload="x"/>"#,
        ] {
            assert!(check(with(wrong).as_bytes()).is_err(), "{wrong} should be refused");
        }
        assert!(check(br#"<svg viewBox="0 0 1 1" onload="x"/>"#).is_err(), "nor on the root");
    }

    #[test]
    fn an_icon_must_be_small() {
        let filler = |size: usize| {
            let bare = with("<desc></desc>");
            with(&format!("<desc>{}</desc>", "x".repeat(size - bare.len())))
        };
        assert_eq!(filler(4096).len(), 4096);
        assert!(check(filler(4096).as_bytes()).is_ok(), "4096 bytes is fine");
        assert!(check(filler(4097).as_bytes()).is_err(), "4097 bytes is not");
    }

    #[test]
    fn an_icon_must_be_square_and_say_so() {
        assert!(check(br#"<svg viewBox="0 0 64 32"><rect/></svg>"#).is_err(), "not square");
        assert!(check(br#"<svg width="64" height="64"><rect/></svg>"#).is_err(), "no viewBox");
        assert!(check(br#"<svg viewBox="0 0 64"><rect/></svg>"#).is_err(), "three numbers");
        assert!(check(br#"<svg viewBox="0 0 0 0"/>"#).is_err(), "nothing to draw");
        assert!(check(br#"<svg viewBox="a b c d"/>"#).is_err(), "not numbers");
        assert!(check(br#"<g viewBox="0 0 1 1"/>"#).is_err(), "the root is an <svg>");
    }

    #[test]
    fn what_is_not_strict_xml_is_refused() {
        for wrong in [
            "not an svg at all",
            r#"<svg viewBox="0 0 1 1"><rect>"#,
            r#"<svg viewBox="0 0 1 1"><g></rect></svg>"#,
            r#"<svg viewBox="0 0 1 1"><desc>&nbsp;</desc></svg>"#,
            r#"<svg viewBox="0 0 1 1"><desc>&#60;</desc></svg>"#,
            r#"<?xml version="1.0"?><svg viewBox="0 0 1 1"/>"#,
            r#"<!DOCTYPE svg [<!ENTITY a "b">]><svg viewBox="0 0 1 1"/>"#,
            r#"<svg viewBox="0 0 1 1"><desc><![CDATA[x]]></desc></svg>"#,
            r#"<svg viewBox="0 0 1 1"><!-- a comment --></svg>"#,
            r#"<svg viewBox="0 0 1 1"/><svg viewBox="0 0 1 1"/>"#,
            r#"<svg viewBox="0 0 1 1"/>text after"#,
            r#"<svg viewBox=0 0 1 1/>"#,
            r#"<svg viewBox="0 0 1 1"><rect x="1"y="2"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><rect x="1" x="2"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><rect x="<"/></svg>"#,
        ] {
            assert!(check(wrong.as_bytes()).is_err(), "{wrong} should be refused");
        }
        assert!(check(&[0xff, 0xfe]).is_err(), "not UTF-8");
    }
}
