// SPDX-License-Identifier: GPL-3.0-or-later

//! The `file://` URI a portal answers with.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

/// Why a URI is not a local file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    /// Not a `file:` URI.
    #[error("not a file: URI")]
    Scheme,
    /// The host part names another machine.
    #[error("file: URI with a remote host")]
    Host,
    /// The path is empty, relative, malformed or holds a NUL byte.
    #[error("malformed path in file: URI")]
    Path,
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex(*bytes.get(i + 1)?)?;
            let low = hex(*bytes.get(i + 2)?)?;
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Absolute path of a `file:` URI (`file:///tmp/a%20b.png`, `file://localhost/tmp/x`).
///
/// A query or fragment is refused rather than guessed at.
///
/// # Errors
/// [`UriError`] when the URI is not a plain local file path.
pub fn file_uri_to_path(uri: &str) -> Result<PathBuf, UriError> {
    let scheme = uri.get(..5).ok_or(UriError::Scheme)?;
    if !scheme.eq_ignore_ascii_case("file:") {
        return Err(UriError::Scheme);
    }
    let rest = uri[5..].strip_prefix("//").ok_or(UriError::Path)?;
    let (host, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
        return Err(UriError::Host);
    }
    if path.is_empty() || path.contains(['?', '#']) {
        return Err(UriError::Path);
    }
    let decoded = percent_decode(path).ok_or(UriError::Path)?;
    if decoded.contains(&0) {
        return Err(UriError::Path);
    }
    Ok(PathBuf::from(OsString::from_vec(decoded)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{UriError, file_uri_to_path};

    #[test]
    fn plain_and_encoded_paths() {
        assert_eq!(
            file_uri_to_path("file:///tmp/shot.png").unwrap(),
            PathBuf::from("/tmp/shot.png")
        );
        assert_eq!(
            file_uri_to_path("file:///home/l%C3%A9a/Capture%20d%27%C3%A9cran.png").unwrap(),
            PathBuf::from("/home/léa/Capture d'écran.png")
        );
        assert_eq!(
            file_uri_to_path("FILE://localhost/run/user/1000/doc/x/a.png").unwrap(),
            PathBuf::from("/run/user/1000/doc/x/a.png")
        );
    }

    #[test]
    fn refuses_anything_that_is_not_a_local_file() {
        assert_eq!(
            file_uri_to_path("https://example.org/a.png"),
            Err(UriError::Scheme)
        );
        assert_eq!(file_uri_to_path(""), Err(UriError::Scheme));
        assert_eq!(file_uri_to_path("/tmp/a.png"), Err(UriError::Scheme));
        assert_eq!(
            file_uri_to_path("file://other-host/tmp/a.png"),
            Err(UriError::Host)
        );
        assert_eq!(file_uri_to_path("file:/tmp/a.png"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file://"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file:///a%00b"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file:///a%zz"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file:///a%2"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file:///a?x=1"), Err(UriError::Path));
        assert_eq!(file_uri_to_path("file:///a#frag"), Err(UriError::Path));
    }
}
