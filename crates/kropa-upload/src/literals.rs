// SPDX-License-Identifier: GPL-3.0-or-later

//! The one detector of a literal value in a profile (`docs/security/secrets.md` section 5): what an
//! import would presume to be a secret. The import, `dest edit` and `dest export` all ask it, so that
//! none of them is stricter or looser than the others.
//!
//! A value is presumed secret when it is a header, query parameter, field or body text that is not a
//! reference, not a pure template (`{filename}`, `{var:x}`), not a public header and not shorter than a
//! secret can be. A reference does not excuse the text around it: in `{secret:key}:abcdef` the
//! `abcdef` is a value. What is left of a value once the references, the scheme of an `Authorization`
//! (`Bearer`, `Basic`...) and the separators (blanks, `:`) are taken out is its literal residue.
//!
//! The text of a JSON body is read as JSON, a string at a time: its keys and its structure are not
//! values. Any other body (XML) that holds a reference is left alone, its structure not being known.

use regex::Regex;

use crate::destination::Destination;
use crate::import::Slot;
use crate::secret::MIN_SECRET_LEN;
use crate::sensitive::{
    CREDENTIAL_SCHEMES, is_public_header, looks_like_credential_value, split_scheme,
};
use crate::template::{Side, Template};

/// Whether what is left of `template` once the references, the scheme and the separators are taken
/// out is long enough to be a secret.
#[must_use]
pub fn mixed_literal(template: &Template) -> bool {
    let text = template.literal_text();
    let lower = text.trim().to_lowercase();
    let without_scheme = CREDENTIAL_SCHEMES
        .iter()
        .find_map(|s| {
            lower
                .strip_prefix(s)
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        })
        .unwrap_or(&lower);
    without_scheme
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .count()
        >= MIN_SECRET_LEN
}

fn prompts() -> Regex {
    Regex::new(r"\{(inputbox|select):([^{}]*)\}").expect("constant pattern")
}

/// Whether the text of one slot (a header, a query parameter, a field or a whole body) holds a value
/// an import would take for a secret.
#[must_use]
pub fn presumed_value(slot: &Slot, value: &str) -> bool {
    if value.is_empty() || prompts().is_match(value) {
        return false;
    }
    if *slot == Slot::Body {
        return presumed_body(value);
    }
    presumed_text(slot, value)
}

fn presumed_text(slot: &Slot, value: &str) -> bool {
    let Ok(template) = Template::parse(value, Side::Request) else {
        // Not a template the engine would run: nothing safe to say about it.
        return true;
    };
    if !template.secrets().is_empty() {
        return mixed_literal(&template);
    }
    let public_slot = matches!(slot, Slot::Header(n) if is_public_header(n));
    if public_slot {
        // A public header holds a setting; a credential written there is still one.
        return looks_like_credential_value(value);
    }
    if template.is_pure_call() {
        return false;
    }
    let rest = match slot {
        Slot::Header(_) => split_scheme(value).map_or(value, |(_, rest)| rest),
        _ => value,
    };
    let Ok(rest_template) = Template::parse(rest, Side::Request) else {
        return true;
    };
    if !matches!(slot, Slot::Body) {
        if let Some(literal) = rest_template.base64_literal() {
            return literal.len() >= MIN_SECRET_LEN;
        }
        if rest_template.has_base64_text() {
            return true;
        }
    }
    rest.len() >= MIN_SECRET_LEN
}

fn presumed_body(value: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(json) => json_has_value(&json),
        Err(_) => match Template::parse(value, Side::Request) {
            // A body that is not JSON and holds a reference: the structure is unknown.
            Ok(t) if !t.secrets().is_empty() => false,
            Ok(t) if t.is_pure_call() => false,
            _ => value.len() >= MIN_SECRET_LEN,
        },
    }
}

fn json_has_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => presumed_text(&Slot::Form(String::new()), s),
        serde_json::Value::Array(items) => items.iter().any(json_has_value),
        serde_json::Value::Object(map) => map.values().any(json_has_value),
        _ => false,
    }
}

/// The slots of `destination` that hold a value an import would presume secret. With `old`, only the
/// values that are new or different there count (an edit does not accuse what was already in the
/// file). Names of slots only, never a value.
#[must_use]
pub fn presumed_values(destination: &Destination, old: Option<&Destination>) -> Vec<Slot> {
    let Some(request) = &destination.request else {
        return Vec::new();
    };
    let old_request = old.and_then(|o| o.request.as_ref());
    let mut found = Vec::new();
    let maps = [
        (&request.headers, old_request.map(|r| &r.headers), Slot::Header as fn(String) -> Slot),
        (&request.query, old_request.map(|r| &r.query), Slot::Query),
        (&request.fields, old_request.map(|r| &r.fields), Slot::Form),
    ];
    for (map, old_map, make) in maps {
        for (name, value) in map {
            let unchanged = old_map.is_some_and(|m| m.get(name) == Some(value));
            if !unchanged {
                let slot = make(name.clone());
                if presumed_value(&slot, value) {
                    found.push(slot);
                }
            }
        }
    }
    if let Some(body) = &request.body_template
        && old_request.and_then(|r| r.body_template.as_ref()) != Some(body)
        && presumed_value(&Slot::Body, body)
    {
        found.push(Slot::Body);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built at run time: nothing in the source looks like a token.
    fn token() -> String {
        ["Zq7-", "Hx3Lm9", "Vt2Rw-yy"].concat()
    }

    fn header(name: &str) -> Slot {
        Slot::Header(name.to_owned())
    }

    #[test]
    fn references_alone_and_pure_templates_hold_no_value() {
        for (slot, value) in [
            (header("X-Api-Key"), "{secret:key}".to_owned()),
            (header("Authorization"), "Bearer {secret:t}".to_owned()),
            (header("Authorization"), "Basic {base64:{secret:u}:{secret:p}}".to_owned()),
            (header("Authorization"), "{secret:a}:{secret:b}".to_owned()),
            (header("X-Name"), "{filename}".to_owned()),
            (Slot::Form("name".into()), "{var:name}".to_owned()),
            (Slot::Query("a".into()), String::new()),
            (header("Accept"), "application/json".to_owned()),
            (Slot::Query("n".into()), "abc".to_owned()),
            (Slot::Query("n".into()), "{secret:k}-v2".to_owned()),
            (Slot::Query("n".into()), "{inputbox:Title}".to_owned()),
        ] {
            assert!(!presumed_value(&slot, &value), "{slot:?} {value}");
        }
    }

    #[test]
    fn literals_are_presumed_secret_even_next_to_a_reference() {
        let t = token();
        for (slot, value) in [
            (header("X-Neutral"), t.clone()),
            (header("X-Neutral"), "0123456789abcdef0123456789abcdef0123".to_owned()),
            (Slot::Query("folder".into()), "shots".to_owned()),
            (Slot::Form("folder".into()), "shots".to_owned()),
            (header("Authorization"), format!("{{secret:key}}:{t}")),
            (header("Authorization"), format!("Bearer {{secret:key}}{t}")),
            (Slot::Query("q".into()), format!("{t}{{secret:key}}")),
            (header("Authorization"), format!("Basic {{base64:{t}}}")),
            (header("Accept"), format!("ghp_{t}{t}")),
        ] {
            assert!(presumed_value(&slot, &value), "{slot:?} {value}");
        }
    }

    #[test]
    fn a_json_body_is_read_a_string_at_a_time() {
        let t = token();
        assert!(!presumed_value(&Slot::Body, r#"{"token":"{secret:key}","n":1}"#));
        assert!(!presumed_value(&Slot::Body, r#"{"file":"{filename}"}"#));
        assert!(presumed_value(&Slot::Body, &format!(r#"{{"token":"{{secret:key}}","x":"{t}"}}"#)));
        assert!(presumed_value(&Slot::Body, &format!(r#"{{"a":["{t}"]}}"#)));
        assert!(presumed_value(&Slot::Body, &format!(r#"{{"x":"{{secret:key}}:{t}"}}"#)));
        // Not JSON, with a reference: the structure is not known.
        assert!(!presumed_value(&Slot::Body, "<k>{secret:key}</k>"));
        assert!(presumed_value(&Slot::Body, &format!("<k>{t}</k>")));
    }
}
