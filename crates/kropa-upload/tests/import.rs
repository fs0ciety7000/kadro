// SPDX-License-Identifier: GPL-3.0-or-later

//! Import of `.sxcu` and `.toml` files: conversions in `insta` snapshots, and the refusals.
//!
//! The snapshots show the profile that would be created and the secrets that would go to the store
//! with their place and a masked preview, never a value: the tests also check that no secret of the
//! fixtures appears in any snapshot.

use std::fmt::Write as _;
use std::path::PathBuf;

use kropa_upload::error::UploadError;
use kropa_upload::import::{ImportFormat, ImportPlan, Slot, import};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn plan(name: &str) -> ImportPlan {
    let format = if name.ends_with(".toml") {
        ImportFormat::Toml
    } else {
        ImportFormat::Sxcu
    };
    import(&fixture(name), format, None).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A `.sxcu` whose path holds a token. Built here so that no token-like literal sits in the source.
fn token_in_path() -> Vec<u8> {
    let token = format!("{}{}", "a8F3kQ9z", "TESTONLY0pX2vB7nC4mR1");
    format!(
        r#"{{"Version":"16.0.1","Name":"Token in the path","DestinationType":"ImageUploader","RequestURL":"https://up.example.org/api/{token}/upload","Body":"MultipartFormData","URL":"{{json:link}}"}}"#
    )
    .into_bytes()
}

fn refused(name: &str) -> UploadError {
    if name == "token-in-path.sxcu" {
        return import(&token_in_path(), ImportFormat::Sxcu, None).expect_err(name);
    }
    let format = if name.ends_with(".toml") {
        ImportFormat::Toml
    } else {
        ImportFormat::Sxcu
    };
    import(&fixture(name), format, None).expect_err(name)
}

/// What the user would see and what would be stored, without any value.
fn render(plan: &ImportPlan) -> String {
    let mut out = String::new();
    writeln!(out, "origin: {} (host {}, https {})", plan.origin, plan.host, plan.https).unwrap();
    out.push_str("\n--- profile ---\n");
    out.push_str(&plan.destination.to_toml().unwrap());
    out.push_str("\n--- secrets ---\n");
    for s in &plan.secrets {
        writeln!(
            out,
            "{} | {:?} | {} | {} | {} | {}",
            s.name,
            s.slot,
            s.placement,
            s.origin,
            s.preview,
            if s.value.is_some() { "value" } else { "to enter" }
        )
        .unwrap();
    }
    out.push_str("\n--- notes ---\n");
    for n in &plan.notes {
        writeln!(out, "{}: {}", n.code, n.detail).unwrap();
    }
    out
}

const FIXTURE_SECRETS: [&str; 8] = [
    "ZIPLINE-FAKE-TOKEN",
    "0123456789abcde",
    "xbb-0123456789abcdef",
    "xbb-fedcba9876543210",
    "live-0123456789abcdef",
    "FAKE-literal-credential-0001",
    "abcdef0123456789",
    "hunter2",
];

macro_rules! snapshot {
    ($name:literal) => {{
        let text = render(&plan($name));
        for secret in FIXTURE_SECRETS {
            assert!(!text.contains(secret), "{} leaks {secret}", $name);
        }
        insta::assert_snapshot!($name, text);
    }};
}

#[test]
fn zipline_sxcu() {
    snapshot!("zipline.sxcu");
    // The token goes to the store tied to the header it was found in.
    let p = plan("zipline.sxcu");
    assert_eq!(p.secrets.len(), 1);
    assert_eq!(p.secrets[0].value.as_ref().unwrap().expose(), "ZIPLINE-FAKE-TOKEN-0123456789abcdef");
    assert_eq!(p.secrets[0].origin, "https://zipline.example.org:443");
    assert!(p.host_matches("zipline.example.org"));
    assert!(p.host_matches("Zipline.Example.Org."));
    assert!(p.host_matches("zipline.example.org:443"));
    assert!(!p.host_matches("evil.example.org"));
    assert!(!p.host_matches(""));
}

#[test]
fn imgur_sxcu() {
    snapshot!("imgur.sxcu");
}

#[test]
fn xbackbone_sxcu_query_and_field_secrets() {
    snapshot!("xbackbone.sxcu");
    let p = plan("xbackbone.sxcu");
    assert!(p.notes.iter().any(|n| n.code == "query_secret"));
}

#[test]
fn prompts_become_secrets_or_variables() {
    snapshot!("prompts.sxcu");
    let p = plan("prompts.sxcu");
    // The prompt in Authorization is a secret to enter; the ones in plain fields are variables.
    assert!(p.secrets.iter().all(|s| s.value.is_none()));
    assert!(p.destination.vars.contains_key("album") || p.destination.vars.len() >= 2);
}

#[test]
fn json_body_is_stored_as_a_secret() {
    snapshot!("json-body.sxcu");
}

#[test]
fn legacy_syntax_is_migrated() {
    snapshot!("legacy.sxcu");
}

#[test]
fn redirection_response_type() {
    snapshot!("redirect.sxcu");
}

#[test]
fn toml_zipline_preset_has_an_empty_token_to_fill() {
    snapshot!("team.toml");
    let p = plan("team.toml");
    assert_eq!(p.secrets.len(), 1);
    assert!(p.secrets[0].value.is_none());
}

#[test]
fn received_toml_follows_the_same_rules_as_sxcu() {
    snapshot!("received.toml");
    let p = plan("received.toml");
    // The literal credential was taken out; the reference is an empty entry; Accept stays.
    let literal = p.secrets.iter().find(|s| s.value.is_some()).unwrap();
    assert_eq!(literal.slot, Slot::Header("Authorization".into()));
    let reference = p.secrets.iter().find(|s| s.name == "sig").unwrap();
    assert!(reference.value.is_none());
    let text = p.destination.to_toml().unwrap();
    assert!(!text.contains("FAKE-literal"), "{text}");
    assert!(text.contains("Accept = \"application/json\""), "{text}");
}

#[test]
fn a_reference_to_another_profile_is_refused() {
    match refused("steals.toml") {
        UploadError::Config { reason, .. } => assert_eq!(reason, "invalid_destination"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn refusals_have_a_reason_and_an_exit_code() {
    for (name, reason, code) in [
        ("token-in-path.sxcu", "secret_in_path", 11),
        ("http-with-secret.sxcu", "https_required", 11),
        ("old-version.sxcu", "import_unsupported", 6),
        ("url-shortener.sxcu", "import_unsupported", 6),
        ("unknown-field.sxcu", "import_unsupported", 6),
        ("xml-function.sxcu", "import_unsupported", 6),
        ("userinfo.sxcu", "import_invalid", 6),
    ] {
        let e = refused(name);
        assert_eq!(e.reason(), reason, "{name}: {e}");
        assert_eq!(e.exit_code().code(), code, "{name}");
        let text = e.to_string();
        for secret in FIXTURE_SECRETS {
            assert!(!text.contains(secret), "{name}: {text}");
        }
    }
    // The messages, once.
    let messages: Vec<String> = [
        "token-in-path.sxcu",
        "http-with-secret.sxcu",
        "old-version.sxcu",
        "url-shortener.sxcu",
        "unknown-field.sxcu",
        "xml-function.sxcu",
        "userinfo.sxcu",
    ]
    .iter()
    .map(|n| format!("{n}: {}", refused(n)))
    .collect();
    insta::assert_snapshot!("refusals", messages.join("\n"));
}

#[test]
fn bad_input_is_refused_without_quoting_it() {
    let big = vec![b' '; 64 * 1024 + 1];
    assert_eq!(
        import(&big, ImportFormat::Sxcu, None).unwrap_err().reason(),
        "import_too_large"
    );
    for (bytes, format) in [
        (b"not json".as_slice(), ImportFormat::Sxcu),
        (b"[1,2]".as_slice(), ImportFormat::Sxcu),
        (b"\xff\xfe".as_slice(), ImportFormat::Sxcu),
        (b"= = =".as_slice(), ImportFormat::Toml),
    ] {
        let e = import(bytes, format, None).unwrap_err();
        assert_eq!(e.exit_code().code(), 6, "{e}");
    }
    // Two destinations in one TOML.
    let two = format!(
        "{}\n{}",
        String::from_utf8(fixture("team.toml")).unwrap(),
        String::from_utf8(fixture("team.toml")).unwrap().replace("\"team\"", "\"other\"")
    );
    assert_eq!(
        import(two.as_bytes(), ImportFormat::Toml, None).unwrap_err().reason(),
        "import_invalid"
    );
}

#[test]
fn the_id_can_be_chosen() {
    let p = import(&fixture("zipline.sxcu"), ImportFormat::Sxcu, Some("work")).unwrap();
    assert_eq!(p.destination.id, "work");
    assert_eq!(plan("zipline.sxcu").destination.id, "zipline-team");
    assert!(import(&fixture("zipline.sxcu"), ImportFormat::Sxcu, Some("a/b")).is_err());
    let t = import(&fixture("team.toml"), ImportFormat::Toml, Some("work")).unwrap();
    assert_eq!(t.destination.id, "work");
}

#[test]
fn a_value_can_be_taken_back_as_public() {
    let mut p = plan("xbackbone.sxcu");
    let name = p.secrets.iter().find(|s| matches!(&s.slot, Slot::Form(n) if n == "token")).unwrap().name.clone();
    let before = p.secrets.len();
    p.mark_public(&name).unwrap();
    assert_eq!(p.secrets.len(), before - 1);
    let text = p.destination.to_toml().unwrap();
    assert!(text.contains("xbb-fedcba9876543210"), "{text}");
    assert!(p.mark_public("nope").is_err());
}

#[test]
fn an_imported_profile_loads_and_is_ready_for_the_store() {
    for name in ["zipline.sxcu", "imgur.sxcu", "xbackbone.sxcu", "prompts.sxcu", "json-body.sxcu", "legacy.sxcu", "redirect.sxcu", "team.toml", "received.toml"] {
        let p = plan(name);
        let compiled = p.destination.compile().unwrap_or_else(|e| panic!("{name}: {e}"));
        // Every secret slot of the profile has an entry in the plan, with the same place.
        for slot in compiled.secret_slots() {
            let entry = p.secrets.iter().find(|s| s.name == slot.name).unwrap_or_else(|| panic!("{name}: {}", slot.name));
            assert_eq!(entry.placement, slot.placement, "{name}: {}", slot.name);
            assert_eq!(Some(entry.origin.as_str()), compiled.origin().as_deref());
        }
    }
}
