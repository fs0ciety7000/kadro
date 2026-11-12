// SPDX-License-Identifier: GPL-3.0-or-later

//! Links in the three formats of `--copy` (`docs/uploads.md` section 5.3): the URL, a Markdown link
//! and an HTML snippet. The URL and the name are escaped for the format, and the result goes through
//! [`CheckedOutput`] again, so that no text built here can carry a secret out.

use url::Url;

use crate::engine::{is_hostile_char, mime_of};
use crate::masked::{CheckedOutput, SecretInOutput, SecretSet};

/// A format of link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormat {
    /// The URL alone.
    Url,
    /// `![name](url)` for an image, `[name](url)` otherwise.
    Markdown,
    /// `<img src="url" alt="name">` for an image, `<a href="url">name</a>` otherwise.
    Html,
}

impl LinkFormat {
    /// Name used in the JSON output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Markdown => "markdown",
            Self::Html => "html",
        }
    }
}

/// Whether a file name is an image, by its extension.
#[must_use]
pub fn is_image_name(name: &str) -> bool {
    mime_of(name).starts_with("image/")
}

/// Longest URL accepted in a link, in bytes (what browsers and most servers accept).
pub const MAX_URL_LEN: usize = 2048;
/// Longest file name shown in a link, in characters; the rest is cut.
pub const MAX_NAME_LEN: usize = 255;

/// Why a link could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// A secret appears in the link.
    #[error("the text holds a secret")]
    SecretInOutput,
    /// The URL is not a usable `http(s)` address (scheme, control character, length, user info).
    #[error("the URL is not usable in a link")]
    InvalidUrl,
}

impl From<SecretInOutput> for LinkError {
    fn from(_: SecretInOutput) -> Self {
        Self::SecretInOutput
    }
}

/// Validates the address of a link: `http` or `https` only, a host, no user info, no control,
/// bidirectional or zero-width character, at most [`MAX_URL_LEN`] bytes. Returns it normalised
/// (the serialisation of the URL parser: spaces and quotes of the path and query are encoded).
///
/// # Errors
/// [`LinkError::InvalidUrl`].
pub fn validate_url(text: &str) -> Result<Url, LinkError> {
    if text.len() > MAX_URL_LEN || text.chars().any(is_hostile_char) {
        return Err(LinkError::InvalidUrl);
    }
    let url = Url::parse(text.trim()).map_err(|_| LinkError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > MAX_URL_LEN
    {
        return Err(LinkError::InvalidUrl);
    }
    Ok(url)
}

/// The file name as shown in a link: cut, with controls and invisible characters turned into spaces.
fn display_name(name: &str) -> String {
    name.chars()
        .take(MAX_NAME_LEN)
        .map(|c| if is_hostile_char(c) { ' ' } else { c })
        .collect()
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '`' => out.push_str("&#96;"),
            c if is_hostile_char(c) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn escape_markdown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '<' | '>' | '!' | '#' | '|' | '~'
            | '&' => {
                out.push('\\');
                out.push(c);
            }
            c if is_hostile_char(c) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A URL for a Markdown destination: nothing that closes it, opens a title or an autolink. A `&`
/// is escaped (`\&`: CommonMark would decode `&amp;` in a destination) and a `|` is percent-encoded
/// (it would split a cell of a GFM table).
fn markdown_url(url: &Url) -> String {
    let mut out = String::with_capacity(url.as_str().len());
    for c in url.as_str().chars() {
        match c {
            ' ' | '(' | ')' | '<' | '>' | '[' | ']' | '\\' | '`' | '"' | '\'' | '|' => {
                out.push_str(&format!("%{:02X}", u32::from(c)));
            }
            '&' => out.push_str("\\&"),
            c => out.push(c),
        }
    }
    out
}

/// The link for `url`, named after the file `name`. The URL is validated again ([`validate_url`]):
/// it comes from a response.
///
/// # Errors
/// [`LinkError::InvalidUrl`] for an address that is not a plain `http(s)` one,
/// [`LinkError::SecretInOutput`] if the link holds a secret.
pub fn format_link(
    format: LinkFormat,
    url: &CheckedOutput,
    name: &str,
    secrets: &SecretSet,
) -> Result<CheckedOutput, LinkError> {
    let parsed = validate_url(url.as_str())?;
    let shown = display_name(name);
    // The secret is searched for in what is shown **before** any escaping: a Markdown escape
    // (`sk\_live`) or an HTML entity hides it from a check made on the result alone. The check of
    // the result below stays: it is the final one.
    CheckedOutput::check(name, secrets)?;
    CheckedOutput::check(url.as_str(), secrets)?;
    CheckedOutput::check(shown.as_str(), secrets)?;
    CheckedOutput::check(parsed.as_str(), secrets)?;
    let image = is_image_name(&shown);
    let text = match format {
        LinkFormat::Url => parsed.as_str().to_owned(),
        LinkFormat::Markdown => {
            let label = escape_markdown_text(&shown);
            let target = markdown_url(&parsed);
            if image {
                format!("![{label}]({target})")
            } else {
                format!("[{label}]({target})")
            }
        }
        LinkFormat::Html => {
            let href = escape_html(parsed.as_str());
            let label = escape_html(&shown);
            if image {
                format!("<img src=\"{href}\" alt=\"{label}\">")
            } else {
                format!("<a href=\"{href}\">{label}</a>")
            }
        }
    };
    Ok(CheckedOutput::check(text, secrets)?)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        LinkError, LinkFormat, MAX_NAME_LEN, MAX_URL_LEN, display_name, format_link, is_image_name,
        validate_url,
    };
    use crate::engine::is_hostile_char;
    use crate::masked::{CheckedOutput, SecretSet};

    fn link(format: LinkFormat, url: &str, name: &str) -> String {
        let secrets = SecretSet::new();
        let url = CheckedOutput::check(url, &secrets).unwrap();
        format_link(format, &url, name, &secrets)
            .unwrap()
            .as_str()
            .to_owned()
    }

    #[test]
    fn the_three_formats() {
        let u = "https://z.example.org/u/a.png";
        assert_eq!(link(LinkFormat::Url, u, "a.png"), u);
        assert_eq!(
            link(LinkFormat::Markdown, u, "a.png"),
            "![a.png](https://z.example.org/u/a.png)"
        );
        assert_eq!(
            link(LinkFormat::Html, u, "a.png"),
            "<img src=\"https://z.example.org/u/a.png\" alt=\"a.png\">"
        );
        assert_eq!(
            link(LinkFormat::Markdown, u, "notes.txt"),
            "[notes.txt](https://z.example.org/u/a.png)"
        );
        assert_eq!(
            link(LinkFormat::Html, u, "notes.txt"),
            "<a href=\"https://z.example.org/u/a.png\">notes.txt</a>"
        );
    }

    #[test]
    fn names_and_urls_are_escaped() {
        let hostile = "a](https://evil.example)\"><script>x</script>.png";
        let md = link(
            LinkFormat::Markdown,
            "https://z.example.org/a b(1).png",
            hostile,
        );
        assert!(md.starts_with("![a\\]\\(https://evil.example\\)"), "{md}");
        assert!(
            md.ends_with("](https://z.example.org/a%20b%281%29.png)"),
            "{md}"
        );
        assert!(!md.replace("\\<", "").contains('<'), "{md}");
        let html = link(
            LinkFormat::Html,
            "https://z.example.org/x?a=1&b='2'",
            hostile,
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&amp;"), "{html}");
        assert!(html.contains("&quot;&gt;&lt;script&gt;"), "{html}");
        assert!(html.contains("%272%27"), "{html}");
    }

    #[test]
    fn a_secret_in_the_name_is_refused() {
        let secrets = SecretSet::from_values(&["name-with-secret-1"]);
        let url = CheckedOutput::check("https://z.example.org/a", &secrets).unwrap();
        assert!(
            format_link(
                LinkFormat::Markdown,
                &url,
                "name-with-secret-1.png",
                &secrets
            )
            .is_err()
        );
    }

    #[test]
    fn ampersands_and_pipes_of_a_destination_are_neutralised() {
        let md = link(
            LinkFormat::Markdown,
            "https://z.example.org/a|b?x=1&amp;y=2&z=3",
            "a.png",
        );
        assert_eq!(
            md,
            "![a.png](https://z.example.org/a%7Cb?x=1\\&amp;y=2\\&z=3)"
        );
    }

    /// A secret that holds characters Markdown and HTML escape: the name or the address that carry
    /// it are refused whatever the format, because the check is made before the escaping (I1).
    #[test]
    fn a_secret_with_markdown_punctuation_is_refused_in_the_name_and_the_url() {
        for secret in [
            ["sk_", "live_", "Zq7kLm9Xt2"].concat(),
            ["sk*", "live*", "Zq7kLm9Xt2"].concat(),
            ["sk#", "live!", "Zq7kLm9Xt2"].concat(),
            ["sk|", "live&", "Zq7kLm9Xt2~"].concat(),
            ["(sk", "[live]", "Zq7kLm9)"].concat(),
        ] {
            let secrets = SecretSet::from_values(&[secret.as_str()]);
            let plain = CheckedOutput::check("https://z.example.org/a", &secrets).unwrap();
            for format in [LinkFormat::Url, LinkFormat::Markdown, LinkFormat::Html] {
                for name in [format!("{secret}.png"), format!("a{secret}b.txt")] {
                    assert_eq!(
                        format_link(format, &plain, &name, &secrets).unwrap_err(),
                        LinkError::SecretInOutput,
                        "{secret:?} in the name, as {format:?}"
                    );
                }
            }
            // In the address: a secret that the URL parser keeps as it is.
            let raw = format!("https://z.example.org/a?k={secret}");
            if let Ok(url) = CheckedOutput::check(raw.clone(), &SecretSet::new()) {
                for format in [LinkFormat::Url, LinkFormat::Markdown, LinkFormat::Html] {
                    assert!(
                        format_link(format, &url, "a.png", &secrets).is_err(),
                        "{raw:?} as {format:?}"
                    );
                }
            }
        }
        // A name cut at 255 characters: the secret that straddles the cut is refused as well.
        let secret = ["cut-", "Zq7kLm9Xt2vRw"].concat();
        let secrets = SecretSet::from_values(&[secret.as_str()]);
        let url = CheckedOutput::check("https://z.example.org/a", &secrets).unwrap();
        let name = format!("{}{secret}.png", "n".repeat(MAX_NAME_LEN - 6));
        assert!(format_link(LinkFormat::Markdown, &url, &name, &secrets).is_err());
    }

    #[test]
    fn urls_that_are_not_plain_http_are_refused() {
        let secrets = SecretSet::new();
        let long = format!("https://z.example.org/{}", "a".repeat(MAX_URL_LEN));
        for bad in [
            "javascript:alert(1)",
            "data:text/html,<script>",
            "ftp://z.example.org/a",
            "file:///etc/passwd",
            "https://user:pw@z.example.org/a",
            "https://z.example.org/a\nb",
            "https://z.example.org/a\u{202E}b",
            "https://z.example.org/a\u{200B}b",
            "/relative/path",
            "https://",
            &long,
        ] {
            let url = CheckedOutput::check(bad, &secrets).unwrap();
            for format in [LinkFormat::Url, LinkFormat::Markdown, LinkFormat::Html] {
                assert_eq!(
                    format_link(format, &url, "a.png", &secrets).unwrap_err(),
                    LinkError::InvalidUrl,
                    "{bad:?} as {format:?}"
                );
            }
        }
    }

    #[test]
    fn a_long_name_is_cut_and_invisible_characters_are_blanked() {
        let name = format!("{}\u{202E}x", "n".repeat(MAX_NAME_LEN + 40));
        let md = link(LinkFormat::Markdown, "https://z.example.org/a", &name);
        assert!(md.len() < MAX_NAME_LEN + 60, "{}", md.len());
        let html = link(
            LinkFormat::Html,
            "https://z.example.org/a",
            "a\u{200B}\nb.png",
        );
        assert!(html.contains("alt=\"a  b.png\""), "{html}");
    }

    // ---- no output can break the format or inject a tag (proptest) ----

    /// The label and the destination of `![label](dest)` or `[label](dest)`, read the way a
    /// Markdown parser does: a backslash escapes the next character, the first unescaped `]` closes
    /// the label, which must be followed by `(`.
    fn split_markdown(out: &str) -> (String, String) {
        let body = out.strip_prefix('!').unwrap_or(out);
        let body = body.strip_prefix('[').expect("starts with [");
        let mut label = String::new();
        let mut chars = body.chars();
        loop {
            match chars.next().expect("label is closed") {
                '\\' => label.push(chars.next().expect("escaped char")),
                ']' => break,
                '[' | '(' | ')' | '<' | '>' => panic!("unescaped {out:?}"),
                c => label.push(c),
            }
        }
        let rest: String = chars.collect();
        let dest = rest
            .strip_prefix('(')
            .and_then(|r| r.strip_suffix(')'))
            .expect("(dest) follows");
        (label, dest.to_owned())
    }

    fn assert_markdown_safe(out: &str, name: &str) {
        let (label, dest) = split_markdown(out);
        // `\&` is the one escape of a destination; every `&` of it must carry one.
        let bare = dest.replace("\\&", "");
        assert!(
            !bare.contains([
                ' ', '(', ')', '<', '>', '[', ']', '\\', '"', '\'', '`', '|', '&'
            ]),
            "{out:?}"
        );
        assert!(!dest.chars().any(char::is_control), "{out:?}");
        assert!(dest.starts_with("http"), "{out:?}");
        assert!(!label.chars().any(char::is_control), "{out:?}");
        let expected: String = name
            .chars()
            .take(MAX_NAME_LEN)
            .map(|c| if is_hostile_char(c) { ' ' } else { c })
            .collect();
        assert_eq!(label, expected, "{out:?}");
    }

    /// Undoes [`escape_html`]; panics on a `&` that is not one of its entities.
    fn unescape_html(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(i) = rest.find('&') {
            out.push_str(&rest[..i]);
            rest = &rest[i..];
            let (entity, c) = ["&amp;", "&lt;", "&gt;", "&quot;", "&#39;", "&#96;"]
                .iter()
                .zip(['&', '<', '>', '"', '\'', '`'])
                .find(|(e, _)| rest.starts_with(**e))
                .expect("a bare & in the output");
            out.push(c);
            rest = &rest[entity.len()..];
        }
        out.push_str(rest);
        out
    }

    fn assert_html_safe(out: &str, name: &str, image: bool) {
        let (head, mid, tail) = if image {
            ("<img src=\"", "\" alt=\"", "\">")
        } else {
            ("<a href=\"", "\">", "</a>")
        };
        let body = out.strip_prefix(head).expect("head");
        let body = body.strip_suffix(tail).expect("tail");
        let (href, label) = body.split_once(mid).expect("middle");
        for part in [href, label] {
            assert!(!part.contains(['<', '>', '"', '\'', '`']), "{out:?}");
            assert!(!part.chars().any(char::is_control), "{out:?}");
        }
        assert_eq!(
            out.matches('<').count(),
            if image { 1 } else { 2 },
            "{out:?}"
        );
        assert_eq!(
            out.matches('>').count(),
            if image { 1 } else { 2 },
            "{out:?}"
        );
        let expected: String = name
            .chars()
            .take(MAX_NAME_LEN)
            .map(|c| if is_hostile_char(c) { ' ' } else { c })
            .collect();
        assert_eq!(unescape_html(label), expected, "{out:?}");
        assert!(unescape_html(href).starts_with("http"), "{out:?}");
    }

    proptest! {
        #[test]
        fn markdown_cannot_be_broken_out_of(
            name in any::<String>(),
            path in any::<String>(),
            query in any::<String>(),
        ) {
            let raw = format!("https://z.example.org/{path}?{query}");
            let secrets = SecretSet::new();
            let url = CheckedOutput::check(raw, &secrets).unwrap();
            if let Ok(out) = format_link(LinkFormat::Markdown, &url, &name, &secrets) {
                assert_markdown_safe(out.as_str(), &name);
            }
        }

        #[test]
        fn html_cannot_be_broken_out_of(
            name in any::<String>(),
            path in any::<String>(),
            query in any::<String>(),
        ) {
            let raw = format!("http://z.example.org/{path}?{query}");
            let secrets = SecretSet::new();
            let url = CheckedOutput::check(raw, &secrets).unwrap();
            if let Ok(out) = format_link(LinkFormat::Html, &url, &name, &secrets) {
                assert_html_safe(out.as_str(), &name, is_image_name(&display_name(&name)));
            }
        }

        #[test]
        fn hostile_but_printable_input_always_gives_a_link(
            name in "[a-z\\]\\[()<>\"'`&*_!#|~\\\\ ]{0,40}(\\.png|\\.txt)",
            path in "[a-z0-9 ()<>\"'`\\[\\]\\\\|^{}%&=+#]{0,40}",
        ) {
            let raw = format!("https://z.example.org/{path}");
            let secrets = SecretSet::new();
            let url = CheckedOutput::check(raw, &secrets).unwrap();
            let md = format_link(LinkFormat::Markdown, &url, &name, &secrets).unwrap();
            assert_markdown_safe(md.as_str(), &name);
            let html = format_link(LinkFormat::Html, &url, &name, &secrets).unwrap();
            assert_html_safe(html.as_str(), &name, is_image_name(&name));
            let plain = format_link(LinkFormat::Url, &url, &name, &secrets).unwrap();
            assert!(!plain.as_str().contains([' ', '"', '<', '>']), "{plain}");
        }

        #[test]
        fn a_validated_url_has_no_control_and_a_known_scheme(text in any::<String>()) {
            if let Ok(url) = validate_url(&text) {
                assert!(matches!(url.scheme(), "http" | "https"));
                assert!(url.as_str().len() <= MAX_URL_LEN);
                assert!(!url.as_str().chars().any(is_hostile_char));
            }
        }
    }
}
