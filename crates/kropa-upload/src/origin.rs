// SPDX-License-Identifier: GPL-3.0-or-later

//! Origins (`docs/security/secrets.md` section 4.1): scheme, IDNA host in lower case without the
//! final dot, and the port, explicit or the default of the scheme.

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Canonical origin of a URL, `scheme://host:port`; `None` for anything but `http` and `https`, a
/// URL without host, or a URL with `user:pass@` (refused everywhere).
#[must_use]
pub fn origin_of(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = match url.host()? {
        Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        Host::Ipv4(a) => a.to_string(),
        Host::Ipv6(a) => format!("[{a}]"),
    };
    Some(format!(
        "{}://{}:{}",
        url.scheme(),
        host,
        url.port_or_known_default()?
    ))
}

/// Canonical origin of a URL given as text (path, query and fragment are ignored).
#[must_use]
pub fn canonical_origin(text: &str) -> Option<String> {
    origin_of(&Url::parse(text).ok()?)
}

/// Whether the host is the literal loopback address `127.0.0.1` or `[::1]`. `localhost` is never
/// accepted: it goes through the resolver (decision U3, `docs/security/secrets.md` section 4.5).
#[must_use]
pub fn is_loopback_literal(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(a)) => a == Ipv4Addr::LOCALHOST,
        Some(Host::Ipv6(a)) => a == Ipv6Addr::LOCALHOST,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{canonical_origin, is_loopback_literal, origin_of};

    #[test]
    fn canonical_forms() {
        assert_eq!(
            canonical_origin("https://Zipline.Example.org/a?b#c").as_deref(),
            Some("https://zipline.example.org:443")
        );
        assert_eq!(
            canonical_origin("https://z.example.org.:443/").as_deref(),
            Some("https://z.example.org:443")
        );
        assert_eq!(
            canonical_origin("http://127.0.0.1:8080/x").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            canonical_origin("http://[::1]/").as_deref(),
            Some("http://[::1]:80")
        );
        assert_eq!(
            canonical_origin("https://bücher.example/").as_deref(),
            Some("https://xn--bcher-kva.example:443")
        );
    }

    #[test]
    fn refused_forms() {
        assert_eq!(canonical_origin("https://user:pw@host/"), None);
        assert_eq!(canonical_origin("https://user@host/"), None);
        assert_eq!(canonical_origin("ftp://host/"), None);
        assert_eq!(canonical_origin("not a url"), None);
    }

    #[test]
    fn loopback_literals_only() {
        let yes = |s: &str| is_loopback_literal(&Url::parse(s).unwrap());
        assert!(yes("http://127.0.0.1:9/"));
        assert!(yes("http://[::1]:9/"));
        assert!(!yes("http://localhost:9/"));
        assert!(!yes("http://127.0.0.2/"));
        assert!(!yes("http://10.0.0.1/"));
        assert!(origin_of(&Url::parse("https://a/").unwrap()).is_some());
    }
}
