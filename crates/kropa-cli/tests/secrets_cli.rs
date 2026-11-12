// SPDX-License-Identifier: GPL-3.0-or-later

//! `secrets status|set|remove|allow` end to end, in process, on an in-memory store and a scripted
//! input. The real keyring is tested in `crates/kropa-store/tests/gnome_keyring.rs`.
//!
//! The value of a secret must be nowhere in stdout or stderr, whatever the command, the outcome, the
//! language and the output mode: `no_command_prints_the_value` runs them all.

#![allow(
    clippy::too_many_lines,
    clippy::struct_field_names,
    clippy::assert_is_empty
)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;

use kropa_capture::{ClipboardReason, UnavailableClipboard};
use kropa_cli::secret_input::{InputError, InputFrom, SecretInput};
use kropa_cli::{Io, Services, run_with_services};
use kropa_core::FixedClock;
use kropa_upload::secret::{
    Authorization, EntryInfo, MemorySecretSource, NewSecret, Placement, SecretEntry, SecretError,
    SecretSource, SecretStore, SecretValue, StoreState, StoreStatus, UnavailableSecretSource,
};
use zeroize::Zeroizing;

/// Built at run time: nothing in the source looks like a token.
fn sentinel() -> String {
    ["Sn7-", "Qx4Lm9", "Vt2Rw-zz"].concat()
}

#[derive(Default)]
struct ScriptedInput {
    values: Mutex<VecDeque<Result<String, InputError>>>,
    asked: Mutex<Vec<InputFrom>>,
}

impl ScriptedInput {
    fn with(value: &str) -> Self {
        let input = Self::default();
        input.values.lock().unwrap().push_back(Ok(value.to_owned()));
        input
    }

    fn asked(&self) -> Vec<InputFrom> {
        self.asked.lock().unwrap().clone()
    }
}

impl SecretInput for ScriptedInput {
    fn read(&self, from: InputFrom, _prompt: &str) -> Result<Zeroizing<String>, InputError> {
        self.asked.lock().unwrap().push(from);
        self.values
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(InputError::Unreadable))
            .map(Zeroizing::new)
    }
}

struct Out {
    code: u8,
    out: String,
    err: String,
}

impl Out {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }

    fn no_leak(&self, text: &str) {
        assert!(!self.out.contains(text), "stdout leaks: {}", self.out);
        assert!(!self.err.contains(text), "stderr leaks: {}", self.err);
    }
}

fn run(
    args: &[&str],
    store: &dyn SecretStore,
    input: &dyn SecretInput,
    tty: bool,
    answer: bool,
) -> Out {
    let args: Vec<OsString> = std::iter::once("kropa")
        .chain(args.iter().copied())
        .map(OsString::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
    let code = {
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            stdin_is_tty: tty,
        };
        let services = Services {
            secrets: store,
            input,
            confirm: &move |_| answer,
            clipboard: &UnavailableClipboard::new(ClipboardReason::NoGraphicalSession),
        };
        run_with_services(
            &args,
            &|_| None,
            &mut io,
            &clock,
            Some("laptop".into()),
            None,
            &|_| {},
            &services,
        )
    };
    Out {
        code: code.code(),
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// A configuration folder with the preset `zipline` and an HTTP destination that uses a secret in
/// a query parameter.
fn config() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "").unwrap();
    std::fs::write(
        dir.path().join("destinations.toml"),
        r#"
[[destination]]
id = "zipline"
type = "zipline"
url = "https://Zipline.Example.org/"
[destination.secrets]
token = {}

[[destination]]
id = "other"
type = "http"
[destination.request]
method = "POST"
url = "https://other.example.org/upload"
query = { key = "{secret:apikey}" }
body = "multipart"
file_field = "file"
[destination.response]
url = "{json:$.url}"
[destination.secrets]
apikey = {}
"#,
    )
    .unwrap();
    dir
}

fn cfg(dir: &Path) -> String {
    dir.to_str().unwrap().to_owned()
}

#[test]
fn set_a_new_secret_ties_it_to_the_origin_and_place_of_the_profile() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&sentinel());
    let r = run(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "secrets",
            "set",
            "zipline",
            "token",
            "--secret-stdin",
            "--yes",
        ],
        &store,
        &input,
        false,
        false,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    r.no_leak(&sentinel());
    assert_eq!(input.asked(), vec![InputFrom::Stdin]);
    assert!(
        r.err.contains("https://zipline.example.org:443"),
        "{}",
        r.err
    );
    assert!(r.err.contains("header:authorization"), "{}", r.err);
    let entry = store.load("zipline", "token").unwrap().unwrap();
    assert_eq!(entry.value.expose(), sentinel());
    assert_eq!(entry.origins, vec!["https://zipline.example.org:443"]);
    assert_eq!(entry.placements, vec![Placement::header("Authorization")]);
}

#[test]
fn a_new_secret_without_yes_and_without_a_terminal_asks_for_nothing() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&sentinel());
    let r = run(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "secrets",
            "set",
            "zipline",
            "token",
            "--secret-fd",
            "3",
        ],
        &store,
        &input,
        false,
        true,
    );
    assert_eq!(r.code, 12, "{}", r.err);
    // The value was not even read: stdin keeps what it had.
    assert!(input.asked().is_empty());
    assert!(store.inspect("zipline", "token").unwrap().is_none());
}

#[test]
fn an_existing_secret_keeps_its_origins_and_places_and_asks_nothing() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    store
        .insert(
            "zipline",
            "token",
            "old-value-1234",
            &["https://elsewhere.example.org:443"],
            vec![Placement::Query("k".into())],
        )
        .unwrap();
    let input = ScriptedInput::with(&sentinel());
    let r = run(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "secrets",
            "set",
            "zipline",
            "token",
            "--secret-stdin",
        ],
        &store,
        &input,
        false,
        false,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    r.no_leak(&sentinel());
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert_eq!(info.origins, vec!["https://elsewhere.example.org:443"]);
    assert_eq!(info.placements, vec![Placement::Query("k".into())]);
    assert_eq!(
        store
            .load("zipline", "token")
            .unwrap()
            .unwrap()
            .value
            .expose(),
        sentinel()
    );
}

#[test]
fn an_empty_entry_left_by_an_import_shows_its_limits_and_asks_again() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    store
        .put(
            "zipline",
            "token",
            NewSecret {
                value: None,
                origins: vec!["https://zipline.example.org:443".into()],
                placements: vec![Placement::Query("api".into())],
            },
        )
        .unwrap();
    let input = ScriptedInput::with(&sentinel());
    // On a terminal, refused: nothing read, nothing stored.
    let r = run(
        &[
            "--lang", "en", "--config", &c, "secrets", "set", "zipline", "token",
        ],
        &store,
        &input,
        true,
        false,
    );
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(r.err.contains("query:api"), "{}", r.err);
    assert!(r.err.contains("logs of the server"), "{}", r.err);
    assert!(input.asked().is_empty());
    assert!(
        !store
            .inspect("zipline", "token")
            .unwrap()
            .unwrap()
            .has_value
    );
    // Accepted: the hidden prompt is used, the limits of the import are kept.
    let r = run(
        &[
            "--lang", "en", "--config", &c, "secrets", "set", "zipline", "token",
        ],
        &store,
        &input,
        true,
        true,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(input.asked(), vec![InputFrom::Terminal]);
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert!(info.has_value);
    assert_eq!(info.placements, vec![Placement::Query("api".into())]);
}

fn set_args(config: &str, profile: &str, name: &str, extra: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = [
        "--lang",
        "en",
        "--config",
        config,
        "secrets",
        "set",
        profile,
        name,
        "--secret-stdin",
        "--yes",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    v.extend(extra.iter().map(|s| (*s).to_owned()));
    v
}

fn run_owned(
    args: &[String],
    store: &dyn SecretStore,
    input: &dyn SecretInput,
    tty: bool,
    answer: bool,
) -> Out {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs, store, input, tty, answer)
}

#[test]
fn bad_input_is_refused_with_the_right_codes() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    // Too short: security refusal, not a usage error.
    let r = run_owned(
        &set_args(&c, "zipline", "token", &[]),
        &store,
        &ScriptedInput::with("abc"),
        false,
        false,
    );
    assert_eq!(r.code, 11, "{}", r.err);
    let r = run_owned(
        &set_args(&c, "zipline", "token", &["--json"]),
        &store,
        &ScriptedInput::with("abc"),
        false,
        false,
    );
    assert_eq!(r.json()["error"]["reason"], "secret_too_short");
    r.no_leak("abc");
    // Too long, not text, unreadable.
    for (error, code) in [
        (InputError::TooLong, 2),
        (InputError::NotText, 2),
        (InputError::Unreadable, 15),
    ] {
        let input = ScriptedInput::default();
        input.values.lock().unwrap().push_back(Err(error));
        let r = run_owned(
            &set_args(&c, "zipline", "token", &[]),
            &store,
            &input,
            false,
            false,
        );
        assert_eq!(r.code, code, "{error:?}: {}", r.err);
    }
    // A secret the profile does not declare, and a profile that does not exist.
    for (profile, name) in [("zipline", "nope"), ("ghost", "token")] {
        let r = run_owned(
            &set_args(&c, profile, name, &[]),
            &store,
            &ScriptedInput::with(&sentinel()),
            false,
            false,
        );
        assert_eq!(r.code, 6, "{}", r.err);
        r.no_leak(&sentinel());
    }
    assert!(store.status().profiles.is_empty());
    // Declined confirmation on a terminal.
    let r = run(
        &[
            "--lang", "en", "--config", &c, "secrets", "set", "zipline", "token",
        ],
        &store,
        &ScriptedInput::with(&sentinel()),
        true,
        false,
    );
    assert_eq!(r.code, 3);
    // A short value is accepted with a warning that names the secret, never its value.
    let r = run_owned(
        &set_args(&c, "zipline", "token", &[]),
        &store,
        &ScriptedInput::with("abcde"),
        false,
        false,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.err.contains("short"), "{}", r.err);
    r.no_leak("abcde");
}

#[test]
fn status_counts_entries_and_never_shows_a_value() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    store
        .insert("zipline", "token", &sentinel(), &["https://z:443"], vec![])
        .unwrap();
    store
        .put(
            "other",
            "apikey",
            NewSecret {
                value: None,
                origins: vec![],
                placements: vec![],
            },
        )
        .unwrap();
    for lang in ["en", "fr"] {
        let r = run(
            &["--lang", lang, "--config", &c, "secrets", "status"],
            &store,
            &ScriptedInput::default(),
            false,
            false,
        );
        assert_eq!(r.code, 0, "{}", r.err);
        r.no_leak(&sentinel());
        assert!(r.out.contains("memory"), "{}", r.out);
        assert!(r.out.contains("zipline"), "{}", r.out);
        assert!(!r.out.contains("apikey"), "{}", r.out);
        let r = run(
            &[
                "--lang", lang, "--json", "--config", &c, "secrets", "status",
            ],
            &store,
            &ScriptedInput::default(),
            false,
            false,
        );
        r.no_leak(&sentinel());
        let v = r.json();
        assert_eq!(v["schema"], "kropa.secrets.status/1");
        assert_eq!(v["state"], "available");
        assert_eq!(v["store"], "memory");
        assert_eq!(
            v["profiles"],
            serde_json::json!([
                {"profile": "other", "entries": 1, "empty": 1},
                {"profile": "zipline", "entries": 1, "empty": 0},
            ])
        );
        assert!(v["fallback"].is_null());
    }
}

/// A store that is there but cannot be used, as the keyring is when locked or absent.
struct Broken {
    reason: &'static str,
    state: StoreState,
}

impl Broken {
    fn error(&self) -> SecretError {
        SecretError::Store {
            reason: self.reason,
            detail: "fixed text".into(),
        }
    }
}

impl SecretSource for Broken {
    fn load(&self, _: &str, _: &str) -> Result<Option<SecretEntry>, SecretError> {
        Err(self.error())
    }
    fn profile_exists(&self, _: &str) -> Result<bool, SecretError> {
        Err(self.error())
    }
    fn put(&self, _: &str, _: &str, _: NewSecret) -> Result<(), SecretError> {
        Err(self.error())
    }
}

impl SecretStore for Broken {
    fn status(&self) -> StoreStatus {
        StoreStatus {
            store: "keyring",
            state: self.state,
            detail: Some("fixed text".into()),
            profiles: std::collections::BTreeMap::new(),
        }
    }
    fn names(&self, _: &str) -> Result<Vec<String>, SecretError> {
        Err(self.error())
    }
    fn inspect(&self, _: &str, _: &str) -> Result<Option<EntryInfo>, SecretError> {
        Err(self.error())
    }
    fn set(&self, _: &str, _: &str, _: SecretValue, _: Authorization) -> Result<(), SecretError> {
        Err(self.error())
    }
    fn remove(&self, _: &str, _: &str) -> Result<bool, SecretError> {
        Err(self.error())
    }
    fn authorize(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: Option<Placement>,
    ) -> Result<(), SecretError> {
        Err(self.error())
    }
}

#[test]
fn a_locked_or_missing_keyring_is_exit_7_with_its_reason_and_never_a_fallback() {
    let dir = config();
    let c = cfg(dir.path());
    for (reason, state, words) in [
        ("keyring_locked", StoreState::Locked, "locked"),
        (
            "keyring_timeout",
            StoreState::Unavailable,
            "did not respond",
        ),
        ("keyring_ambiguous", StoreState::Ambiguous, "more than once"),
        ("keyring_unavailable", StoreState::Unavailable, "vault"),
    ] {
        let store = Broken { reason, state };
        let input = ScriptedInput::with(&sentinel());
        let r = run(
            &[
                "--lang",
                "en",
                "--json",
                "--config",
                &c,
                "secrets",
                "set",
                "zipline",
                "token",
                "--secret-stdin",
                "--yes",
            ],
            &store,
            &input,
            false,
            false,
        );
        assert_eq!(r.code, 7, "{reason}: {}", r.err);
        assert_eq!(r.json()["error"]["reason"], reason);
        assert!(r.err.contains(words), "{reason}: {}", r.err);
        r.no_leak(&sentinel());
        // The value was never read: the store was asked first.
        assert!(input.asked().is_empty(), "{reason}");
        // `remove` and `allow` too.
        let r = run(
            &[
                "--lang", "en", "secrets", "remove", "zipline", "token", "--yes",
            ],
            &store,
            &input,
            false,
            false,
        );
        assert_eq!(r.code, 7, "{reason}");
        let r = run(
            &[
                "--lang",
                "en",
                "--config",
                &c,
                "secrets",
                "allow",
                "zipline",
                "token",
                "--origin",
                "https://n.example.org",
            ],
            &store,
            &input,
            true,
            true,
        );
        assert_eq!(r.code, 7, "{reason}: {}", r.err);
        // `status` reports the state: it is a report, exit 0.
        let r = run(
            &["--lang", "en", "--json", "secrets", "status"],
            &store,
            &input,
            false,
            false,
        );
        assert_eq!(r.code, 0);
        assert_eq!(r.json()["state"], state.as_str());
    }
}

#[test]
fn without_a_store_in_the_build_everything_says_exit_5() {
    let dir = config();
    let c = cfg(dir.path());
    let store = UnavailableSecretSource;
    for args in [
        vec!["secrets", "status"],
        vec![
            "secrets",
            "set",
            "zipline",
            "token",
            "--secret-stdin",
            "--yes",
        ],
        vec!["secrets", "remove", "zipline", "token", "--yes"],
    ] {
        let mut argv = vec!["--lang", "en", "--json", "--config", &c];
        argv.extend(args);
        let r = run(
            &argv,
            &store,
            &ScriptedInput::with("abcdefgh"),
            false,
            false,
        );
        assert_eq!(r.code, 5, "{argv:?}: {}", r.err);
        let v = r.json();
        assert_eq!(v["error"]["reason"], "secret_store_not_delivered");
        assert_eq!(v["error"]["lots"][0], "1.8m");
    }
}

#[test]
fn remove_asks_and_is_idempotent() {
    let store = MemorySecretSource::new();
    store
        .insert("zipline", "token", &sentinel(), &[], vec![])
        .unwrap();
    let none = ScriptedInput::default();
    // No terminal and no --yes: code 12, nothing removed.
    let r = run(
        &["--lang", "en", "secrets", "remove", "zipline", "token"],
        &store,
        &none,
        false,
        true,
    );
    assert_eq!(r.code, 12);
    assert!(store.inspect("zipline", "token").unwrap().is_some());
    // Declined on a terminal.
    let r = run(
        &["--lang", "en", "secrets", "remove", "zipline", "token"],
        &store,
        &none,
        true,
        false,
    );
    assert_eq!(r.code, 3);
    assert!(store.inspect("zipline", "token").unwrap().is_some());
    // Accepted.
    let r = run(
        &["--lang", "en", "secrets", "remove", "zipline", "token"],
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    r.no_leak(&sentinel());
    assert!(store.inspect("zipline", "token").unwrap().is_none());
    // Again: nothing to remove, said so, exit 0.
    let r = run(
        &[
            "--lang", "en", "secrets", "remove", "zipline", "token", "--yes",
        ],
        &store,
        &none,
        false,
        false,
    );
    assert_eq!(r.code, 0);
    assert!(r.err.contains("There was no secret"), "{}", r.err);
}

#[test]
fn allow_is_interactive_only_and_shows_before_and_after() {
    let dir = config();
    let c = cfg(dir.path());
    let store = MemorySecretSource::new();
    store
        .insert(
            "zipline",
            "token",
            &sentinel(),
            &["https://zipline.example.org:443"],
            vec![Placement::header("Authorization")],
        )
        .unwrap();
    let none = ScriptedInput::default();
    let base = |extra: &[&'static str]| {
        let mut v = vec![
            "--lang",
            "en",
            "--config",
            c.as_str(),
            "secrets",
            "allow",
            "zipline",
            "token",
        ];
        v.extend_from_slice(extra);
        v
    };
    // No terminal: 12, `--yes` changes nothing, `--no-input` wins over a terminal.
    for (extra, tty) in [
        (vec!["--origin", "https://n.example.org"], false),
        (vec!["--origin", "https://n.example.org", "--yes"], false),
        (
            vec!["--origin", "https://n.example.org", "--no-input"],
            true,
        ),
    ] {
        let r = run(&base(&extra), &store, &none, tty, true);
        assert_eq!(r.code, 12, "{extra:?}: {}", r.err);
    }
    // Declined.
    let r = run(
        &base(&["--origin", "https://New.Example.org/path"]),
        &store,
        &none,
        true,
        false,
    );
    assert_eq!(r.code, 3);
    assert_eq!(
        store
            .inspect("zipline", "token")
            .unwrap()
            .unwrap()
            .origins
            .len(),
        1
    );
    // Accepted: the old values and the new one were shown, the origin is canonical.
    let r = run(
        &base(&["--origin", "https://New.Example.org/path"]),
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    r.no_leak(&sentinel());
    assert!(
        r.err.contains("https://zipline.example.org:443"),
        "{}",
        r.err
    );
    assert!(
        r.err
            .contains("Adding the origin: https://new.example.org:443"),
        "{}",
        r.err
    );
    assert_eq!(
        store.inspect("zipline", "token").unwrap().unwrap().origins,
        vec![
            "https://zipline.example.org:443".to_owned(),
            "https://new.example.org:443".to_owned()
        ]
    );
    // A place: normalised, `query:` is warned about.
    let r = run(
        &base(&["--placement", "query:Key"]),
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.err.contains("logs of the server"), "{}", r.err);
    assert!(
        store
            .inspect("zipline", "token")
            .unwrap()
            .unwrap()
            .placements
            .contains(&Placement::Query("Key".into()))
    );
    // Never the host or the path, never anything else.
    for place in ["host", "path:/x", "url:abc", "header:", "body:abc", ":x"] {
        let r = run(&base(&["--placement", place]), &store, &none, true, true);
        assert_eq!(r.code, 11, "{place}: {}", r.err);
    }
    let r = run(
        &base(&["--placement", "path:/x", "--json"]),
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.json()["error"]["reason"], "placement_not_allowed");
    // `body` is tied to the template of the profile: this one has none.
    let r = run(&base(&["--placement", "body"]), &store, &none, true, true);
    assert_eq!(r.code, 6, "{}", r.err);
    // A secret in the address, a bad origin, a missing entry.
    let r = run(
        &base(&["--origin", "https://u:p@n.example.org"]),
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 11, "{}", r.err);
    let r = run(
        &base(&["--origin", "https://[bad"]),
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 6, "{}", r.err);
    let r = run(
        &[
            "--lang",
            "en",
            "--config",
            &c,
            "secrets",
            "allow",
            "zipline",
            "ghost",
            "--origin",
            "https://n.example.org",
        ],
        &store,
        &none,
        true,
        true,
    );
    assert_eq!(r.code, 7, "{}", r.err);
    // One of the two is required.
    let r = run(&base(&[]), &store, &none, true, true);
    assert_eq!(r.code, 2);
}

/// Fails the second `put`, as a keyring that goes away in the middle of an import.
struct FailsOnSecondPut {
    inner: MemorySecretSource,
    puts: Mutex<usize>,
}

impl SecretSource for FailsOnSecondPut {
    fn load(&self, p: &str, n: &str) -> Result<Option<SecretEntry>, SecretError> {
        self.inner.load(p, n)
    }
    fn profile_exists(&self, p: &str) -> Result<bool, SecretError> {
        self.inner.profile_exists(p)
    }
    fn put(&self, p: &str, n: &str, e: NewSecret) -> Result<(), SecretError> {
        let mut puts = self.puts.lock().unwrap();
        *puts += 1;
        if *puts == 2 {
            return Err(SecretError::Store {
                reason: "keyring_locked",
                detail: "fixed text".into(),
            });
        }
        self.inner.put(p, n, e)
    }
}

impl SecretStore for FailsOnSecondPut {
    fn status(&self) -> StoreStatus {
        self.inner.status()
    }
    fn names(&self, p: &str) -> Result<Vec<String>, SecretError> {
        self.inner.names(p)
    }
    fn inspect(&self, p: &str, n: &str) -> Result<Option<EntryInfo>, SecretError> {
        self.inner.inspect(p, n)
    }
    fn set(&self, p: &str, n: &str, v: SecretValue, a: Authorization) -> Result<(), SecretError> {
        self.inner.set(p, n, v, a)
    }
    fn remove(&self, p: &str, n: &str) -> Result<bool, SecretError> {
        self.inner.remove(p, n)
    }
    fn authorize(
        &self,
        p: &str,
        n: &str,
        o: Option<&str>,
        pl: Option<Placement>,
    ) -> Result<(), SecretError> {
        self.inner.authorize(p, n, o, pl)
    }
}

#[test]
fn an_import_that_fails_half_way_leaves_no_orphan_entry() {
    let dir = config();
    let c = cfg(dir.path());
    let file = dir.path().join("two.toml");
    std::fs::write(
        &file,
        r#"
[[destination]]
id = "two"
type = "http"
[destination.request]
method = "POST"
url = "https://two.example.org/upload"
headers = { Authorization = "Bearer {secret:first}", X-Other = "{secret:second}" }
body = "multipart"
file_field = "file"
[destination.response]
url = "{json:$.url}"
[destination.secrets]
first = {}
second = {}
"#,
    )
    .unwrap();
    let f = file.to_str().unwrap();
    let before = std::fs::read_to_string(dir.path().join("destinations.toml")).unwrap();
    let args = [
        "--lang",
        "en",
        "--config",
        &c,
        "dest",
        "import",
        f,
        "--yes",
        "--expect-host",
        "two.example.org",
    ];
    let store = FailsOnSecondPut {
        inner: MemorySecretSource::new(),
        puts: Mutex::new(0),
    };
    let r = run(&args, &store, &ScriptedInput::default(), false, false);
    assert_eq!(r.code, 7, "{}", r.err);
    // The first entry was taken back: the next import of `two` is not blocked by an orphan.
    assert!(!store.profile_exists("two").unwrap());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("destinations.toml")).unwrap(),
        before
    );
    // The store works again: both entries are created empty, then the profile.
    let r = run(&args, &store, &ScriptedInput::default(), false, false);
    assert_eq!(r.code, 0, "{}", r.err);
    for name in ["first", "second"] {
        let info = store.inspect("two", name).unwrap().unwrap();
        assert!(!info.has_value);
        assert_eq!(info.origins, vec!["https://two.example.org:443"]);
    }
}

/// A store whose second `put` fails with `put_reason` and whose `remove` always fails.
struct CannotRollBack {
    inner: MemorySecretSource,
    puts: Mutex<u32>,
    put_reason: &'static str,
}

impl SecretSource for CannotRollBack {
    fn load(&self, p: &str, n: &str) -> Result<Option<SecretEntry>, SecretError> {
        self.inner.load(p, n)
    }
    fn profile_exists(&self, p: &str) -> Result<bool, SecretError> {
        self.inner.profile_exists(p)
    }
    fn put(&self, p: &str, n: &str, e: NewSecret) -> Result<(), SecretError> {
        let mut puts = self.puts.lock().unwrap();
        *puts += 1;
        if *puts == 2 {
            return Err(SecretError::Store {
                reason: self.put_reason,
                detail: "fixed text".into(),
            });
        }
        self.inner.put(p, n, e)
    }
}

impl SecretStore for CannotRollBack {
    fn status(&self) -> StoreStatus {
        self.inner.status()
    }
    fn names(&self, p: &str) -> Result<Vec<String>, SecretError> {
        self.inner.names(p)
    }
    fn inspect(&self, p: &str, n: &str) -> Result<Option<EntryInfo>, SecretError> {
        self.inner.inspect(p, n)
    }
    fn set(&self, p: &str, n: &str, v: SecretValue, a: Authorization) -> Result<(), SecretError> {
        self.inner.set(p, n, v, a)
    }
    fn remove(&self, _: &str, _: &str) -> Result<bool, SecretError> {
        Err(SecretError::Store {
            reason: "keyring_locked",
            detail: "fixed text".into(),
        })
    }
    fn authorize(
        &self,
        p: &str,
        n: &str,
        o: Option<&str>,
        pl: Option<Placement>,
    ) -> Result<(), SecretError> {
        self.inner.authorize(p, n, o, pl)
    }
}

#[test]
fn a_rollback_that_fails_and_a_timeout_are_said_not_hushed() {
    let dir = config();
    let c = cfg(dir.path());
    let file = dir.path().join("two.toml");
    std::fs::write(
        &file,
        r#"
[[destination]]
id = "two"
type = "http"
[destination.request]
method = "POST"
url = "https://two.example.org/upload"
headers = { Authorization = "Bearer {secret:first}", X-Other = "{secret:second}" }
body = "multipart"
file_field = "file"
[destination.response]
url = "{json:$.url}"
[destination.secrets]
first = {}
second = {}
"#,
    )
    .unwrap();
    let f = file.to_str().unwrap();
    for (lang, reason, left, unknown) in [
        ("en", "secret_store_error", "Could not remove", false),
        ("en", "keyring_timeout", "Could not remove", true),
        ("fr", "keyring_timeout", "Impossible de retirer", true),
    ] {
        let store = CannotRollBack {
            inner: MemorySecretSource::new(),
            puts: Mutex::new(0),
            put_reason: reason,
        };
        let r = run(
            &[
                "--lang",
                lang,
                "--config",
                &c,
                "dest",
                "import",
                f,
                "--yes",
                "--expect-host",
                "two.example.org",
            ],
            &store,
            &ScriptedInput::default(),
            false,
            false,
        );
        assert_eq!(r.code, 7, "{}", r.err);
        // The entry that stayed is named, with what to do about it.
        assert!(r.err.contains(left), "{lang}: {}", r.err);
        assert!(r.err.contains("first"), "{}", r.err);
        assert!(store.profile_exists("two").unwrap());
        // A timeout says that the outcome is unknown, twice: on the error and on the entry.
        let said = |words: &[&str]| words.iter().any(|w| r.err.contains(w));
        assert_eq!(
            said(&["may have been stored", "a pu être enregistré"]),
            unknown
        );
        assert_eq!(
            said(&["result is unknown", "résultat est inconnu"]),
            unknown
        );
    }
}

#[test]
fn import_warnings_survive_quiet_and_reach_the_json_error() {
    let dir = config();
    let c = cfg(dir.path());
    let file = dir.path().join("two.toml");
    std::fs::write(
        &file,
        r#"
[[destination]]
id = "two"
type = "http"
[destination.request]
method = "POST"
url = "https://two.example.org/upload"
headers = { Authorization = "Bearer {secret:first}", X-Other = "{secret:second}" }
body = "multipart"
file_field = "file"
[destination.response]
url = "{json:$.url}"
[destination.secrets]
first = {}
second = {}
"#,
    )
    .unwrap();
    let f = file.to_str().unwrap();
    let store = CannotRollBack {
        inner: MemorySecretSource::new(),
        puts: Mutex::new(0),
        put_reason: "keyring_timeout",
    };
    let r = run(
        &[
            "--lang",
            "en",
            "--quiet",
            "--json",
            "--config",
            &c,
            "dest",
            "import",
            f,
            "--yes",
            "--expect-host",
            "two.example.org",
        ],
        &store,
        &ScriptedInput::default(),
        false,
        false,
    );
    assert_eq!(r.code, 7, "{}", r.err);
    // `--quiet` keeps what the user must act on.
    assert!(r.err.contains("Could not remove"), "{}", r.err);
    assert!(r.err.contains("may have been stored"), "{}", r.err);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!(v["error"]["reason"], "keyring_timeout");
    let warnings = v["error"]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2, "{v}");
    assert!(warnings[0].as_str().unwrap().contains("second"), "{v}");
    assert!(warnings[1].as_str().unwrap().contains("first"), "{v}");
    // A write that timed out may have been done; the message says so.
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("may or may not have been made")
    );
}

#[test]
fn no_command_prints_the_value() {
    let dir = config();
    let c = cfg(dir.path());
    let secret = sentinel();
    for lang in ["en", "fr"] {
        for json in [false, true] {
            let global: Vec<&str> = if json {
                vec!["--lang", lang, "--json", "--config", &c]
            } else {
                vec!["--lang", lang, "--config", &c]
            };
            let store = MemorySecretSource::new();
            let go = |tail: &[&str], input: &ScriptedInput, tty: bool, answer: bool| {
                let argv = [&global[..], tail].concat();
                let r = run(&argv, &store, input, tty, answer);
                r.no_leak(&secret);
                r
            };
            // Stored, replaced, refused on every path that can see the value.
            let set = [
                "secrets",
                "set",
                "zipline",
                "token",
                "--secret-stdin",
                "--yes",
            ];
            assert_eq!(
                go(&set, &ScriptedInput::with(&secret), false, false).code,
                0
            );
            assert_eq!(
                go(&set, &ScriptedInput::with(&secret), false, false).code,
                0
            );
            let bad = [
                "secrets",
                "set",
                "zipline",
                "nope",
                "--secret-stdin",
                "--yes",
            ];
            assert_eq!(
                go(&bad, &ScriptedInput::with(&secret), false, false).code,
                6
            );
            let mut unreadable = ScriptedInput::default();
            unreadable
                .values
                .get_mut()
                .unwrap()
                .push_back(Err(InputError::NotText));
            assert_eq!(go(&set, &unreadable, false, false).code, 2);
            // Everything else that reads the entry.
            assert_eq!(
                go(
                    &["secrets", "status"],
                    &ScriptedInput::default(),
                    false,
                    false
                )
                .code,
                0
            );
            let allow = [
                "secrets",
                "allow",
                "zipline",
                "token",
                "--origin",
                "https://n.example.org",
            ];
            assert_eq!(go(&allow, &ScriptedInput::default(), true, true).code, 0);
            assert_eq!(go(&allow, &ScriptedInput::default(), false, true).code, 12);
            let remove = ["secrets", "remove", "zipline", "token", "--yes"];
            assert_eq!(go(&remove, &ScriptedInput::default(), false, false).code, 0);
            // An entry that cannot be read (corrupt in a real keyring) is reported without it.
            let broken = Broken {
                reason: "secret_store_error",
                state: StoreState::Unavailable,
            };
            let argv = [&global[..], &set[..]].concat();
            run(&argv, &broken, &ScriptedInput::with(&secret), false, false).no_leak(&secret);
        }
    }
}

/// N3: what the question is about is not hidden by `--quiet`; with `--yes`, nothing is asked, so the
/// quiet flag keeps its meaning.
#[test]
fn n3_quiet_does_not_hide_what_a_new_entry_is_confirmed_for() {
    let dir = config();
    let c = cfg(dir.path());
    let base = [
        "--quiet",
        "--lang",
        "en",
        "--config",
        c.as_str(),
        "secrets",
        "set",
        "zipline",
        "token",
        "--secret-stdin",
    ];
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&sentinel());
    let r = run(&base, &store, &input, true, true);
    assert_eq!(r.code, 0, "{}", r.err);
    r.no_leak(&sentinel());
    assert!(
        r.err.contains("https://zipline.example.org:443") && r.err.contains("header:authorization"),
        "{}",
        r.err
    );
    // Declined: the question was seen, nothing is stored.
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&sentinel());
    let r = run(&base, &store, &input, true, false);
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(
        r.err.contains("https://zipline.example.org:443"),
        "{}",
        r.err
    );
    assert!(store.inspect("zipline", "token").unwrap().is_none());
    // `--yes` answers by itself: `--quiet` still hides the lines.
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&sentinel());
    let mut yes = base.to_vec();
    yes.push("--yes");
    let r = run(&yes, &store, &input, true, false);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!r.err.contains("zipline.example.org"), "{}", r.err);
}
