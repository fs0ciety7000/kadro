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
//! values. Any other body (XML) that holds a reference is read as markup: tags, attribute names and
//! punctuation are taken out, the text between the tags and the attribute values are its residue.
//!
//! The default and the choices of a variable are values too: they sit in the file in the clear.

use std::collections::BTreeMap;

use regex::Regex;

use crate::destination::Destination;
use crate::import::Slot;
use crate::secret::MIN_SECRET_LEN;
use crate::sensitive::{
    CREDENTIAL_SCHEMES, is_public_header, is_sensitive_name, looks_like_credential_value,
    looks_like_token_segment, split_scheme,
};
use crate::template::{Side, Template};

/// Longest word taken for the scheme of an authorization that Kropa does not know (`Client-ID`).
const MAX_SCHEME_WORD: usize = 12;

/// Characters that are structure, not value, in what is left of a value: blanks and `:`.
fn is_separator(c: char) -> bool {
    c.is_whitespace() || c == ':'
}

/// Whether `head` (the text of a value before its first reference) is the scheme of an
/// authorization: a word of letters and `-` (no digit, at most [`MAX_SCHEME_WORD`] long) then blanks,
/// and only that. Only a header has a scheme.
fn is_scheme_word(slot: &Slot, head: &str) -> bool {
    let word = head.trim_end_matches(char::is_whitespace);
    matches!(slot, Slot::Header(_))
        && word.len() < head.len()
        && !word.is_empty()
        && word.len() <= MAX_SCHEME_WORD
        && word.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// Whether what is left of `template` (the value of `slot`, with at least one reference) once the
/// references, the scheme and the separators are taken out is long enough to be a secret.
///
/// The scheme is a known one (`Bearer`, `Basic`...) or, in a header only, a word of letters and `-`
/// that opens the value and comes right before the first reference (`Client-ID {secret:id}`). After
/// the first reference only separators are left alone.
#[must_use]
pub fn mixed_literal(slot: &Slot, template: &Template) -> bool {
    let (head, tail) = template.literal_head_tail();
    let lower = head.trim_start().to_lowercase();
    let known = CREDENTIAL_SCHEMES.iter().find_map(|s| {
        lower
            .strip_prefix(s)
            .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
    });
    let head_residue = match known {
        Some(rest) => rest,
        None if is_scheme_word(slot, &head) => "",
        None => head.as_str(),
    };
    head_residue
        .chars()
        .chain(tail.chars())
        .filter(|c| !is_separator(*c))
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
        return mixed_literal(slot, &template);
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
            // A body that is not JSON (XML) and holds a reference: the text between the tags and
            // the attribute values are the residue (n1).
            Ok(t) if !t.secrets().is_empty() => markup_residue(&t.literal_text()) >= MIN_SECRET_LEN,
            Ok(t) if t.is_pure_call() => false,
            _ => value.len() >= MIN_SECRET_LEN,
        },
    }
}

/// How many characters of value a markup text holds: what is left once declarations, tag names,
/// attribute names, quotes, punctuation and blanks are taken out.
fn markup_residue(text: &str) -> usize {
    let strip = Regex::new(
        r"(?s)<\?.*?\?>|<!DOCTYPE[^>]*>|<!\[CDATA\[|\]\]>|<!--|-->|</?[A-Za-z_][\w:.-]*|/?>|[A-Za-z_][\w:.-]*\s*=",
    )
    .expect("constant pattern");
    strip
        .replace_all(text, "")
        .chars()
        .filter(|c| !c.is_whitespace() && !"\"',:{}[]".contains(*c))
        .count()
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
        (
            &request.headers,
            old_request.map(|r| &r.headers),
            Slot::Header as fn(String) -> Slot,
        ),
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

/// Whether a var in a slot named like a secret (`X-Api-Key`, `token`...): an import turns a prompt
/// there into a secret, never a variable, so a value there goes through the same presumption as a
/// literal. Elsewhere a variable is a setting.
fn is_credential_slot(slot: &Slot) -> bool {
    match slot {
        Slot::Header(n) | Slot::Query(n) | Slot::Form(n) => is_sensitive_name(n),
        Slot::Body => false,
    }
}

/// For each variable of the request, whether some slot of credentials uses it.
fn var_uses(destination: &Destination) -> BTreeMap<String, bool> {
    let mut uses = BTreeMap::new();
    let Some(request) = &destination.request else {
        return uses;
    };
    let slots = request
        .headers
        .iter()
        .map(|(n, v)| (Slot::Header(n.clone()), v))
        .chain(
            request
                .query
                .iter()
                .map(|(n, v)| (Slot::Query(n.clone()), v)),
        )
        .chain(
            request
                .fields
                .iter()
                .map(|(n, v)| (Slot::Form(n.clone()), v)),
        )
        .chain(request.body_template.iter().map(|v| (Slot::Body, v)));
    for (slot, value) in slots {
        if let Ok(template) = Template::parse(value, Side::Request) {
            for name in template.vars() {
                let credential = is_credential_slot(&slot);
                let entry = uses.entry(name).or_insert(false);
                *entry |= credential;
            }
        }
    }
    uses
}

/// Whether a default or a choice of a variable is presumed to be a secret. In a slot of credentials
/// anything of the length of a secret is; elsewhere only what has the shape of a credential or of a
/// token (a public setting such as a folder name is what a variable is for).
fn presumed_var_value(value: &str, credential_slot: bool) -> bool {
    if value.is_empty() {
        return false;
    }
    if credential_slot {
        return value.len() >= MIN_SECRET_LEN;
    }
    looks_like_credential_value(value) || looks_like_token_segment(value)
}

/// The names of the variables of `destination` whose default or choices hold a value an import
/// would presume secret. With `old`, a variable that is the same there, used the same way, is not
/// accused again. Names only, never a value.
#[must_use]
pub fn presumed_vars(destination: &Destination, old: Option<&Destination>) -> Vec<String> {
    let uses = var_uses(destination);
    let old_uses = old.map(var_uses);
    destination
        .vars
        .iter()
        .filter(|(name, decl)| {
            let credential = uses.get(*name).copied().unwrap_or(false);
            let unchanged = old.zip(old_uses.as_ref()).is_some_and(|(o, ou)| {
                o.vars.get(*name) == Some(*decl)
                    && ou.get(*name).copied().unwrap_or(false) == credential
            });
            !unchanged
                && decl
                    .default
                    .iter()
                    .chain(&decl.choices)
                    .any(|v| presumed_var_value(v, credential))
        })
        .map(|(name, _)| name.clone())
        .collect()
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
            (
                header("Authorization"),
                "Basic {base64:{secret:u}:{secret:p}}".to_owned(),
            ),
            (header("Authorization"), "{secret:a}:{secret:b}".to_owned()),
            (header("X-Name"), "{filename}".to_owned()),
            (Slot::Form("name".into()), "{var:name}".to_owned()),
            (Slot::Query("a".into()), String::new()),
            (header("Accept"), "application/json".to_owned()),
            (
                header("Authorization"),
                "Client-ID {secret:client_id}".to_owned(),
            ),
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
            (
                header("X-Neutral"),
                "0123456789abcdef0123456789abcdef0123".to_owned(),
            ),
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
        assert!(!presumed_value(
            &Slot::Body,
            r#"{"token":"{secret:key}","n":1}"#
        ));
        assert!(!presumed_value(&Slot::Body, r#"{"file":"{filename}"}"#));
        assert!(presumed_value(
            &Slot::Body,
            &format!(r#"{{"token":"{{secret:key}}","x":"{t}"}}"#)
        ));
        assert!(presumed_value(&Slot::Body, &format!(r#"{{"a":["{t}"]}}"#)));
        assert!(presumed_value(
            &Slot::Body,
            &format!(r#"{{"x":"{{secret:key}}:{t}"}}"#)
        ));
        // Not JSON, with a reference: the structure is not known.
        assert!(!presumed_value(&Slot::Body, "<k>{secret:key}</k>"));
        assert!(presumed_value(&Slot::Body, &format!("<k>{t}</k>")));
    }

    /// 12 alphanumerics, built at run time, that start like a scheme but hold a digit.
    fn twelve() -> String {
        ["Zq7Hx3", "Lm9Vt2"].concat()
    }

    #[test]
    fn n2_a_scheme_word_is_only_a_header_before_the_first_reference() {
        let w = twelve();
        // Accepted: the Imgur model, and a longer word with a dash.
        for value in ["Client-ID {secret:id}", "Api-Key {secret:id}"] {
            assert!(!presumed_value(&header("Authorization"), value), "{value}");
        }
        // Refused: a digit in the word, a word anywhere but a header, text after a reference.
        for (slot, value) in [
            (header("Authorization"), format!("{w} {{secret:k}}")),
            (header("X-Neutral"), format!("{w} {{secret:k}}")),
            (Slot::Form("f".into()), format!("{w} {{secret:k}}")),
            (Slot::Query("q".into()), format!("{{secret:k}} {w} x")),
            (header("X-Neutral"), format!("{{secret:k}} {w} x")),
            (Slot::Query("q".into()), "Client-ID {secret:k}".to_owned()),
            (header("X-Neutral"), "Alpha Beta {secret:k}".to_owned()),
            (header("X-Neutral"), "Client-ID  x {secret:k}".to_owned()),
        ] {
            assert!(presumed_value(&slot, &value), "{slot:?} {value}");
        }
        // After the references only separators.
        assert!(!presumed_value(
            &header("Authorization"),
            "{secret:a} : {secret:b}"
        ));
    }

    #[test]
    fn n1_the_text_of_a_markup_body_is_a_value() {
        let t = token();
        assert!(!presumed_value(
            &Slot::Body,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><k a=\"1\">{secret:key}</k>"
        ));
        for body in [
            format!("<k>{{secret:key}}</k><t>{t}</t>"),
            format!("<k a=\"{t}\">{{secret:key}}</k>"),
            format!("<k>{{secret:key}}:{t}</k>"),
        ] {
            assert!(presumed_value(&Slot::Body, &body), "{body}");
        }
    }

    fn with_var(header_name: &str, template: &str, default: &str, choices: &[&str]) -> Destination {
        let text = format!(
            "id = \"a\"\ntype = \"http\"\n[request]\nurl = \"https://api.example.org/up\"\nbody = \"multipart\"\n[request.headers]\n{header_name} = {template:?}\n[response]\nurl = \"{{response}}\"\n[vars.k]\ndefault = {default:?}\nchoices = {choices:?}\n"
        );
        toml::from_str(&text).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn n1_the_default_and_the_choices_of_a_variable_are_read() {
        let t = token();
        // A token as the default of a variable standing in a header of credentials.
        let d = with_var("X-Api-Key", "{var:k}", &t, &[]);
        assert_eq!(presumed_vars(&d, None), ["k"]);
        // Unchanged since the old profile: not accused again (I3).
        assert!(presumed_vars(&d, Some(&d)).is_empty());
        // A choice, in a neutral header: only what has the shape of a token.
        let d = with_var("X-Neutral", "{var:k}", "", &["one", &format!("{t}{t}")]);
        assert_eq!(presumed_vars(&d, None), ["k"]);
        // Moved into a header of credentials: accused although the value is the same.
        let before = with_var("Accept", "{var:k}", "shots", &[]);
        let after = with_var("X-Api-Key", "{var:k}", "shots", &[]);
        assert!(presumed_vars(&before, None).is_empty());
        assert_eq!(presumed_vars(&after, Some(&before)), ["k"]);
        // A public setting of a variable in a public place stays fine.
        let d = with_var("Accept", "{var:k}", "shots", &["shots", "png"]);
        assert!(presumed_vars(&d, None).is_empty());
        // Ghp-like shape, wherever the variable stands.
        let d = with_var("Accept", "{var:k}", &format!("ghp_{t}{t}"), &[]);
        assert_eq!(presumed_vars(&d, None), ["k"]);
    }

    /// The import and the detector agree on every value of a header, query parameter or field.
    #[test]
    fn the_detector_agrees_with_the_import() {
        use crate::import::{ImportFormat, import};
        let t = token();
        let values = [
            "{secret:key}".to_owned(),
            "Bearer {secret:key}".to_owned(),
            "{filename}".to_owned(),
            "{var:v}".to_owned(),
            "abc".to_owned(),
            "shots".to_owned(),
            t.clone(),
            format!("Bearer {t}"),
            format!("Basic {{base64:{t}}}"),
            "Basic {base64:{secret:a}:{secret:b}}".to_owned(),
            "Client-ID {secret:key}".to_owned(),
            format!("{{secret:key}}:{t}"),
        ];
        for (kind, table) in [
            ("header", "headers"),
            ("query", "query"),
            ("field", "fields"),
        ] {
            for value in &values {
                let name = if kind == "header" { "X-Neutral" } else { "p" };
                let text = format!(
                    "[[destination]]\nid = \"a\"\ntype = \"http\"\n[destination.request]\nurl = \"https://api.example.org/up\"\nbody = \"multipart\"\n[destination.request.{table}]\n{name} = {value:?}\n[destination.response]\nurl = \"{{response}}\"\n[destination.vars.v]\n[destination.secrets]\nkey = {{}}\na = {{}}\nb = {{}}\n"
                );
                let slot = match kind {
                    "header" => Slot::Header(name.to_owned()),
                    "query" => Slot::Query(name.to_owned()),
                    _ => Slot::Form(name.to_owned()),
                };
                let detected = presumed_value(&slot, value);
                match import(text.as_bytes(), ImportFormat::Toml, None) {
                    Ok(plan) => assert_eq!(
                        plan.secrets.iter().any(|s| s.value.is_some()),
                        detected,
                        "{kind} {value}"
                    ),
                    // Refused outright: the detector must not call it clean.
                    Err(e) => assert!(detected, "{kind} {value}: {e}"),
                }
            }
        }
    }
}
