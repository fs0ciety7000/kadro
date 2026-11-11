// SPDX-License-Identifier: GPL-3.0-or-later

//! Links in the three formats of `--copy` (`docs/uploads.md` section 5.3): the URL, a Markdown link
//! and an HTML snippet. The URL and the name are escaped for the format, and the result goes through
//! [`CheckedOutput`] again, so that no text built here can carry a secret out.

use url::Url;

use crate::engine::mime_of;
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

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn escape_markdown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '<' | '>' | '!' | '#' | '|' | '~' | '&' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A URL for a Markdown destination: parentheses and spaces cannot appear in it.
fn markdown_url(url: &Url) -> String {
    url.as_str()
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('<', "%3C")
        .replace('>', "%3E")
}

/// The link for `url`, named after the file `name`.
///
/// # Errors
/// [`SecretInOutput`] if the link holds a secret.
pub fn format_link(
    format: LinkFormat,
    url: &CheckedOutput,
    name: &str,
    secrets: &SecretSet,
) -> Result<CheckedOutput, SecretInOutput> {
    let text = match (format, Url::parse(url.as_str())) {
        (LinkFormat::Url, _) | (_, Err(_)) => url.as_str().to_owned(),
        (LinkFormat::Markdown, Ok(parsed)) => {
            let label = escape_markdown_text(name);
            let target = markdown_url(&parsed);
            if is_image_name(name) {
                format!("![{label}]({target})")
            } else {
                format!("[{label}]({target})")
            }
        }
        (LinkFormat::Html, Ok(parsed)) => {
            let href = escape_html(parsed.as_str());
            let label = escape_html(name);
            if is_image_name(name) {
                format!("<img src=\"{href}\" alt=\"{label}\">")
            } else {
                format!("<a href=\"{href}\">{label}</a>")
            }
        }
    };
    CheckedOutput::check(text, secrets)
}

#[cfg(test)]
mod tests {
    use super::{LinkFormat, format_link};
    use crate::masked::{CheckedOutput, SecretSet};

    fn link(format: LinkFormat, url: &str, name: &str) -> String {
        let secrets = SecretSet::new();
        let url = CheckedOutput::check(url, &secrets).unwrap();
        format_link(format, &url, name, &secrets).unwrap().as_str().to_owned()
    }

    #[test]
    fn the_three_formats() {
        let u = "https://z.example.org/u/a.png";
        assert_eq!(link(LinkFormat::Url, u, "a.png"), u);
        assert_eq!(link(LinkFormat::Markdown, u, "a.png"), "![a.png](https://z.example.org/u/a.png)");
        assert_eq!(
            link(LinkFormat::Html, u, "a.png"),
            "<img src=\"https://z.example.org/u/a.png\" alt=\"a.png\">"
        );
        assert_eq!(link(LinkFormat::Markdown, u, "notes.txt"), "[notes.txt](https://z.example.org/u/a.png)");
        assert_eq!(
            link(LinkFormat::Html, u, "notes.txt"),
            "<a href=\"https://z.example.org/u/a.png\">notes.txt</a>"
        );
    }

    #[test]
    fn names_and_urls_are_escaped() {
        let hostile = "a](https://evil.example)\"><script>x</script>.png";
        let md = link(LinkFormat::Markdown, "https://z.example.org/a b(1).png", hostile);
        assert!(md.starts_with("![a\\]\\(https://evil.example\\)"), "{md}");
        assert!(md.ends_with("](https://z.example.org/a%20b%281%29.png)"), "{md}");
        assert!(!md.contains("<script"), "{md}");
        let html = link(LinkFormat::Html, "https://z.example.org/x?a=1&b='2'", hostile);
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&amp;"), "{html}");
        assert!(html.contains("&quot;&gt;&lt;script&gt;"), "{html}");
        assert!(html.contains("&#39;2&#39;"), "{html}");
    }

    #[test]
    fn a_secret_in_the_name_is_refused() {
        let secrets = SecretSet::from_values(&["name-with-secret-1"]);
        let url = CheckedOutput::check("https://z.example.org/a", &secrets).unwrap();
        assert!(format_link(LinkFormat::Markdown, &url, "name-with-secret-1.png", &secrets).is_err());
    }
}
