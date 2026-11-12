// SPDX-License-Identifier: GPL-3.0-or-later

//! `dest add|edit|remove|set-default|export` end to end, in process, on an in-memory store, a scripted
//! input, a scripted confirmation and a fake editor (`sh script`). The real keyring is exercised by
//! `tests/keyring_cli.rs`.
//!
//! The value of a secret must be nowhere in stdout, stderr or the files: `no_command_prints_or_writes_the_value`
//! runs the commands and looks for it everywhere.

#![allow(
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::struct_field_names
)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;

use kropa_capture::{ClipboardReason, UnavailableClipboard};
use kropa_cli::secret_input::{InputError, InputFrom, SecretInput};
use kropa_cli::{Io, Services, run_with_services};
use kropa_core::{Config, FixedClock};
use kropa_upload::secret::{
    Authorization, MemorySecretSource, Placement, SecretSource, SecretStore, SecretValue,
};
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zeroize::Zeroizing;

/// Built at run time: nothing in the source looks like a token.
fn sentinel() -> String {
    ["Zq7-", "Hx3Lm9", "Vt2Rw-yy"].concat()
}

#[derive(Default)]
struct ScriptedInput {
    values: Mutex<VecDeque<String>>,
    asked: Mutex<Vec<InputFrom>>,
}

impl ScriptedInput {
    fn with(value: &str) -> Self {
        let input = Self::default();
        input.values.lock().unwrap().push_back(value.to_owned());
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
            .map(Zeroizing::new)
            .ok_or(InputError::Unreadable)
    }
}

struct Out {
    code: u8,
    out: String,
    err: String,
    prompts: Vec<String>,
}

impl Out {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }
}

struct Setup<'a> {
    store: &'a dyn SecretStore,
    input: &'a ScriptedInput,
    tty: bool,
    answer: bool,
    editor: Option<String>,
    /// `XDG_RUNTIME_DIR`.
    runtime: Option<std::path::PathBuf>,
}

fn run(args: &[&str], dir: &Path, setup: &Setup<'_>) -> Out {
    let mut full: Vec<OsString> = vec!["kropa".into(), "--config".into(), dir.into()];
    full.extend(args.iter().map(OsString::from));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let clock = FixedClock::new("2026-11-11T14:03:22+01:00[+01:00]".parse().unwrap());
    let prompts = Mutex::new(Vec::new());
    let editor = setup.editor.clone();
    let runtime = setup.runtime.clone();
    let code = {
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            stdin_is_tty: setup.tty,
        };
        let services = Services {
            secrets: setup.store,
            input: setup.input,
            confirm: &|p| {
                prompts.lock().unwrap().push(p.to_owned());
                setup.answer
            },
            clipboard: &UnavailableClipboard::new(ClipboardReason::NoGraphicalSession),
        };
        run_with_services(
            &full,
            &move |name| match name {
                "EDITOR" => editor.clone(),
                "XDG_RUNTIME_DIR" => runtime.as_ref().map(|p| p.display().to_string()),
                _ => None,
            },
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
        prompts: prompts.into_inner().unwrap(),
    }
}

fn plain<'a>(store: &'a MemorySecretSource, input: &'a ScriptedInput) -> Setup<'a> {
    Setup {
        store,
        input,
        tty: false,
        answer: false,
        editor: None,
        runtime: None,
    }
}

const COMMENTED: &str = "\
# my destinations
[[destination]]
# the main one
id = \"zipline\"
type = \"zipline\"
url = \"https://zipline.example.org\" # the server

[destination.secrets]
token = {} # filled by 'secrets set'
";

fn config(destinations: &str, config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), config).unwrap();
    std::fs::write(dir.path().join("destinations.toml"), destinations).unwrap();
    dir
}

fn read(dir: &tempfile::TempDir, file: &str) -> String {
    std::fs::read_to_string(dir.path().join(file)).unwrap()
}

fn no_temporary_left(dir: &tempfile::TempDir) {
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with('.')),
        "temporary files left: {names:?}"
    );
}

fn with_entry(store: &MemorySecretSource, profile: &str, value: &str) {
    store
        .set(
            profile,
            "token",
            SecretValue::new(value).unwrap(),
            Authorization {
                origins: vec!["https://zipline.example.org:443".into()],
                placements: vec![Placement::header("Authorization")],
            },
        )
        .unwrap();
}

// ---------------------------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------------------------

#[test]
fn add_stores_the_token_for_the_origin_and_the_place_and_never_prints_it() {
    let secret = sentinel();
    let dir = config(COMMENTED, "language = \"en\"\n");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with(&secret);
    let out = run(
        &[
            "--json",
            "dest",
            "add",
            "team",
            "--type",
            "zipline",
            "--url",
            "https://Z.Team.example/",
            "--secret-stdin",
            "--default",
        ],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(input.asked(), [InputFrom::Stdin]);
    let doc = out.json();
    assert_eq!(doc["schema"], "kropa.dest.change/1");
    assert_eq!(doc["action"], "add");
    assert_eq!(doc["id"], "team");
    assert_eq!(doc["default"], "team");
    assert_eq!(doc["secrets"][0]["name"], "token");
    assert_eq!(doc["secrets"][0]["action"], "created");
    // The entry exists, bound to the origin of --url and to the place the preset puts it.
    let info = store.inspect("team", "token").unwrap().unwrap();
    assert!(info.has_value);
    assert_eq!(info.origins, ["https://z.team.example:443"]);
    assert_eq!(info.placements, [Placement::header("authorization")]);
    assert_eq!(
        store.load("team", "token").unwrap().unwrap().value.expose(),
        secret
    );
    // The file kept its comments and gained the profile; the default is in the configuration.
    let file = read(&dir, "destinations.toml");
    assert!(file.starts_with("# my destinations\n"), "{file}");
    assert!(file.contains("# the main one"), "{file}");
    assert!(
        file.contains("token = {} # filled by 'secrets set'"),
        "{file}"
    );
    assert!(file.contains("id = \"team\""), "{file}");
    assert!(read(&dir, "config.toml").contains("default_destination = \"team\""));
    assert!(read(&dir, "config.toml").contains("language = \"en\""));
    no_temporary_left(&dir);
    for text in [&out.out, &out.err, &file, &read(&dir, "config.toml")] {
        assert!(!text.contains(&secret), "leak: {text}");
    }
}

#[test]
fn add_reads_the_token_hidden_on_a_terminal_and_refuses_a_short_one() {
    let dir = config("", "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with("abcdefghij");
    let mut setup = plain(&store, &input);
    setup.tty = true;
    let out = run(
        &[
            "dest",
            "add",
            "z",
            "--type",
            "zipline",
            "--url",
            "https://z.example.org",
        ],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(input.asked(), [InputFrom::Terminal]);
    assert!(out.prompts.is_empty(), "adding asks nothing but the token");
    assert_eq!(out.out, "z\n");
    assert_eq!(
        read(&dir, "config.toml"),
        "",
        "no --default: config untouched"
    );

    let short = ScriptedInput::with("abc");
    let out = run(
        &[
            "--json",
            "dest",
            "add",
            "y",
            "--type",
            "zipline",
            "--url",
            "https://y.example.org",
        ],
        dir.path(),
        &Setup {
            tty: true,
            ..plain(&store, &short)
        },
    );
    assert_eq!(out.code, 11, "{}", out.err);
    assert_eq!(out.json()["error"]["reason"], "secret_too_short");
    assert!(store.inspect("y", "token").unwrap().is_none());
    assert!(!read(&dir, "destinations.toml").contains("\"y\""));
}

#[test]
fn add_without_a_terminal_and_without_a_source_asks_nothing_and_writes_nothing() {
    let dir = config("", "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &[
            "dest",
            "add",
            "z",
            "--type",
            "zipline",
            "--url",
            "https://z.example.org",
        ],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 12);
    assert!(input.asked().is_empty());
    assert_eq!(read(&dir, "destinations.toml"), "");
    assert!(!store.profile_exists("z").unwrap());
}

#[test]
fn add_refuses_an_id_taken_by_a_profile_or_by_the_store_before_reading_anything() {
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    // A leftover of a removed profile (N2): the store alone blocks the name.
    with_entry(&store, "ghost", "abcdefghij");
    for id in ["zipline", "ghost"] {
        let input = ScriptedInput::with("abcdefghij");
        let out = run(
            &[
                "dest",
                "add",
                id,
                "--type",
                "zipline",
                "--url",
                "https://z.example.org",
                "--secret-stdin",
            ],
            dir.path(),
            &plain(&store, &input),
        );
        assert_eq!(out.code, 6, "{id}: {}", out.err);
        assert!(input.asked().is_empty(), "{id}: stdin was read");
    }
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
}

#[test]
fn add_refuses_http_and_waits_for_s3_and_sftp_without_touching_anything() {
    let dir = config("", "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::with("abcdefghij");
    let go = |args: &[&str]| run(args, dir.path(), &plain(&store, &input));
    let out = go(&["dest", "add", "h", "--type", "http"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("dest import"), "{}", out.err);
    assert!(out.err.contains("docs/examples/imgur.toml"), "{}", out.err);
    let out = go(&[
        "--json",
        "dest",
        "add",
        "s",
        "--type",
        "s3",
        "--bucket",
        "b",
        "--region",
        "r",
        "--secret-stdin",
    ]);
    assert_eq!(out.code, 5);
    assert_eq!(out.json()["error"]["lots"][0], "2.7");
    let out = go(&[
        "dest",
        "add",
        "s",
        "--type",
        "sftp",
        "--host",
        "h",
        "--secret-stdin",
    ]);
    assert_eq!(out.code, 5);
    assert!(out.err.contains("4.9"), "{}", out.err);
    let out = go(&["dest", "add", "z", "--type", "zipline", "--secret-stdin"]);
    assert_eq!(out.code, 2, "--url is needed");
    let out = go(&[
        "dest",
        "add",
        "z",
        "--type",
        "zipline",
        "--url",
        "https://z.example.org",
        "--prefix",
        "p",
        "--secret-stdin",
    ]);
    assert_eq!(out.code, 2, "--prefix is not for zipline");
    assert!(out.err.contains("--prefix"), "{}", out.err);
    let out = go(&[
        "dest",
        "add",
        "z",
        "--type",
        "zipline",
        "--url",
        "http://z.example.org",
        "--secret-stdin",
    ]);
    assert_eq!(out.code, 11, "a token over http");
    assert!(input.asked().is_empty());
    assert_eq!(read(&dir, "destinations.toml"), "");
}

// ---------------------------------------------------------------------------------------------
// edit with options
// ---------------------------------------------------------------------------------------------

#[test]
fn edit_set_changes_the_label_and_kinds_keeping_every_comment_and_asks_nothing() {
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &[
            "--json",
            "dest",
            "edit",
            "zipline",
            "--set",
            "label=Home",
            "--set",
            "kinds=image",
        ],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.prompts.is_empty());
    let doc = out.json();
    assert_eq!(doc["action"], "edit");
    assert_eq!(doc["secrets"][0]["action"], "unchanged");
    let file = read(&dir, "destinations.toml");
    for kept in [
        "# my destinations\n",
        "# the main one\n",
        "url = \"https://zipline.example.org\" # the server",
        "token = {} # filled by 'secrets set'",
    ] {
        assert!(file.contains(kept), "{kept:?} lost:\n{file}");
    }
    assert!(file.contains("label = \"Home\""), "{file}");
    assert!(file.contains("kinds = [\"image\"]"), "{file}");
    // Clearing a label, and the same value again.
    let out = run(
        &["dest", "edit", "zipline", "--set", "label="],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0);
    assert!(!read(&dir, "destinations.toml").contains("label"));
    let before = read(&dir, "destinations.toml");
    let out = run(
        &["dest", "edit", "zipline", "--set", "kinds=image"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0);
    assert_eq!(read(&dir, "destinations.toml"), before);
    no_temporary_left(&dir);
}

#[test]
fn edit_set_refuses_keys_values_and_templates_with_nothing_changed() {
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let setup = plain(&store, &input);
    let key = sentinel();
    for (args, code) in [
        (vec!["--set", "request.url=https://x.example.org"], 2),
        (vec!["--set", "kinds=video"], 2),
        (vec!["--set", "label={secret:token}"], 11),
        (vec!["--set", "label=Bearer {secret:token}"], 11),
        (vec!["--set", "note=sk-live-0123456789abcdef"], 11),
    ] {
        let mut full = vec!["dest", "edit", "zipline"];
        full.extend(args.iter());
        let out = run(&full, dir.path(), &setup);
        assert_eq!(out.code, code, "{args:?}: {}", out.err);
    }
    // A key that looks like a secret is never repeated in the message.
    let arg = format!("{key}=x");
    let out = run(
        &["dest", "edit", "zipline", "--set", &arg],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 2);
    assert!(!out.err.contains(&key), "{}", out.err);
    // `--var` does not exist here.
    let out = run(
        &["dest", "edit", "zipline", "--var", "X=1"],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 2);
    // Unknown profile.
    let out = run(
        &["dest", "edit", "nope", "--set", "label=x"],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 6);
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
}

#[test]
fn moving_the_address_of_a_profile_with_a_secret_needs_a_terminal_and_a_yes_that_yes_does_not_give()
{
    let secret = sentinel();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    with_entry(&store, "zipline", &secret);
    let input = ScriptedInput::default();
    let new_url = ["--url", "https://other.example.org"];
    // No terminal: 12, even with --yes, and the confirmation is not even asked.
    let out = run(
        &["dest", "edit", "zipline", "--url", new_url[1], "--yes"],
        dir.path(),
        &Setup {
            answer: true,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 12, "{}", out.err);
    assert!(out.prompts.is_empty());
    // A terminal, answer no: cancelled, nothing changed, nothing authorized.
    let out = run(
        &["dest", "edit", "zipline", "--url", new_url[1]],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert_eq!(out.prompts.len(), 1);
    assert!(
        out.err.contains("https://other.example.org:443"),
        "{}",
        out.err
    );
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert_eq!(info.origins, ["https://zipline.example.org:443"]);
    // --no-input is the same as no terminal.
    let out = run(
        &["--no-input", "dest", "edit", "zipline", "--url", new_url[1]],
        dir.path(),
        &Setup {
            tty: true,
            answer: true,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 12);
    // A terminal, answer yes: the profile moves and the secret follows (the old origin, which the
    // profile no longer uses, is withdrawn in the same answer).
    let out = run(
        &["--json", "dest", "edit", "zipline", "--url", new_url[1]],
        dir.path(),
        &Setup {
            tty: true,
            answer: true,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(out.json()["secrets"][0]["action"], "authorized");
    let file = read(&dir, "destinations.toml");
    assert!(
        file.contains("url = \"https://other.example.org\" # the server"),
        "{file}"
    );
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert_eq!(info.origins, ["https://other.example.org:443"]);
    assert_eq!(
        store
            .load("zipline", "token")
            .unwrap()
            .unwrap()
            .value
            .expose(),
        secret
    );
    for text in [&out.out, &out.err, &file] {
        assert!(!text.contains(&secret), "leak: {text}");
    }
    // http:// for a profile with a secret is refused outright.
    let out = run(
        &[
            "dest",
            "edit",
            "zipline",
            "--url",
            "http://other.example.org",
        ],
        dir.path(),
        &Setup {
            tty: true,
            answer: true,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 11);
}

#[test]
fn an_address_change_on_a_profile_without_a_secret_asks_nothing() {
    let dir = config(
        "[[destination]]\nid = \"open\"\ntype = \"http\"\n[destination.request]\nurl = \"https://a.example.org/up\"\n[destination.response]\nurl = \"{json:$.url}\"\n",
        "",
    );
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &["dest", "edit", "open", "--url", "http://b.example.org/up"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.prompts.is_empty());
    assert!(read(&dir, "destinations.toml").contains("http://b.example.org/up"));
}

// ---------------------------------------------------------------------------------------------
// edit in $EDITOR
// ---------------------------------------------------------------------------------------------

/// An editor that is `sh script`, the script being `body` (`$1` is the file to edit).
fn editor(dir: &Path, body: &str) -> String {
    let script = dir.join("editor.sh");
    std::fs::write(&script, body).unwrap();
    format!("sh {}", script.display())
}

fn terminal<'a>(
    store: &'a MemorySecretSource,
    input: &'a ScriptedInput,
    editor: String,
    answer: bool,
) -> Setup<'a> {
    Setup {
        tty: true,
        answer,
        editor: Some(editor),
        ..plain(store, input)
    }
}

#[test]
fn the_editor_gets_a_private_copy_and_its_result_replaces_the_profile_atomically() {
    let work = tempfile::tempdir().unwrap();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let log = work.path().join("seen");
    let body = format!(
        "{{ stat -c %a \"$1\" 2>/dev/null || stat -f %Lp \"$1\"; }} > {log}.mode\ncp \"$1\" {log}.copy\nperl -pi -e 's|^# the main one|# my edit|; s|# the server|# still the server|' \"$1\"\nprintf 'label = \"Edited\"\\n' >> {log}.unused\nperl -pi -e 's|^type = \"zipline\"|type = \"zipline\"\\nlabel = \"Edited\"|' \"$1\"\n",
        log = log.display()
    );
    let out = run(
        &["--json", "dest", "edit", "zipline"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), &body), false),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        std::fs::read_to_string(work.path().join("seen.mode"))
            .unwrap()
            .trim(),
        "600"
    );
    let seen = std::fs::read_to_string(work.path().join("seen.copy")).unwrap();
    assert!(
        seen.contains("# the main one") && seen.contains("# the server"),
        "{seen}"
    );
    let file = read(&dir, "destinations.toml");
    assert!(file.contains("# my edit"), "{file}");
    assert!(file.contains("# still the server"), "{file}");
    assert!(file.contains("label = \"Edited\""), "{file}");
    assert!(file.starts_with("# my destinations\n"), "{file}");
    assert_eq!(out.json()["action"], "edit");
    // The copy is gone, whatever happened.
    no_temporary_left(&dir);
}

#[test]
fn the_editor_needs_a_terminal_an_editor_and_a_change_to_do_anything() {
    let work = tempfile::tempdir().unwrap();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    // No terminal.
    let mut setup = plain(&store, &input);
    setup.editor = Some(editor(work.path(), "exit 0\n"));
    assert_eq!(
        run(&["dest", "edit", "zipline"], dir.path(), &setup).code,
        12
    );
    // No EDITOR.
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &Setup {
            tty: true,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 6, "{}", out.err);
    assert!(out.err.contains("EDITOR"), "{}", out.err);
    // An editor that cannot start, one that fails, one that changes nothing.
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &terminal(&store, &input, "/nonexistent/editor".into(), false),
    );
    assert_eq!(out.code, 15, "{}", out.err);
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &terminal(
            &store,
            &input,
            editor(work.path(), "echo 'label = \"x\"' >> \"$1\"\nexit 1\n"),
            false,
        ),
    );
    assert_eq!(out.code, 3, "{}", out.err);
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), "exit 0\n"), false),
    );
    assert_eq!(out.code, 0);
    assert!(out.err.contains("Nothing changed"), "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    no_temporary_left(&dir);
}

#[test]
fn an_invalid_result_keeps_the_old_file_and_can_be_fixed_in_a_second_round() {
    let work = tempfile::tempdir().unwrap();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    // The user declines to try again: 6, old file intact.
    let breaks = "perl -pi -e 's|^url = .*|url = \"not a url\"|' \"$1\"\n";
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), breaks), false),
    );
    assert_eq!(out.code, 6, "{}", out.err);
    assert_eq!(out.prompts.len(), 1);
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    // Another id, or two profiles, are refused too.
    for body in [
        "perl -pi -e 's|^id = \"zipline\"|id = \"other\"|' \"$1\"\n",
        "cat \"$1\" \"$1\" > \"$1.x\" && mv \"$1.x\" \"$1\"\n",
        "printf '\\377\\376' > \"$1\"\n",
    ] {
        let out = run(
            &["dest", "edit", "zipline"],
            dir.path(),
            &terminal(&store, &input, editor(work.path(), body), false),
        );
        assert_eq!(out.code, 6, "{body}: {}", out.err);
    }
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    no_temporary_left(&dir);
}

#[test]
fn a_secret_value_typed_in_the_editor_is_refused_and_never_written() {
    let work = tempfile::tempdir().unwrap();
    let secret = sentinel();
    let original = "[[destination]]\nid = \"api\"\ntype = \"http\"\n[destination.request]\nurl = \"https://api.example.org/up\"\n[destination.request.headers]\nX-Api-Key = \"{secret:key}\"\n[destination.secrets.key]\n[destination.response]\nurl = \"{json:$.url}\"\n";
    let dir = config(original, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let body = format!("perl -pi -e 's|{{secret:key}}|{secret}|' \"$1\"\n");
    let out = run(
        &["--json", "dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), &body), true),
    );
    assert_eq!(out.code, 11, "{}", out.err);
    assert_eq!(out.json()["error"]["reason"], "secret_in_profile");
    assert_eq!(read(&dir, "destinations.toml"), original);
    for text in [&out.out, &out.err] {
        assert!(!text.contains(&secret), "leak: {text}");
    }
    assert!(out.prompts.is_empty(), "no second round for a refusal");
    no_temporary_left(&dir);
}

#[test]
fn moving_a_secret_place_in_the_editor_asks_like_a_new_address() {
    let work = tempfile::tempdir().unwrap();
    let original = "[[destination]]\nid = \"api\"\ntype = \"http\"\n[destination.request]\nurl = \"https://api.example.org/up\"\n[destination.request.headers]\nX-Api-Key = \"{secret:key}\"\n[destination.secrets.key]\n[destination.response]\nurl = \"{json:$.url}\"\n";
    let dir = config(original, "");
    let store = MemorySecretSource::new();
    store
        .set(
            "api",
            "key",
            SecretValue::new("abcdefghij").unwrap(),
            Authorization {
                origins: vec!["https://api.example.org:443".into()],
                placements: vec![Placement::header("x-api-key")],
            },
        )
        .unwrap();
    let input = ScriptedInput::default();
    let body = "perl -pi -e 's|^X-Api-Key = |Other-Header = |' \"$1\"\n";
    // Declined: nothing changes.
    let out = run(
        &["dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), body), false),
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert!(out.err.contains("header:other-header"), "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), original);
    // Accepted: the new place is added to the entry.
    let out = run(
        &["dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), body), true),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let info = store.inspect("api", "key").unwrap().unwrap();
    assert_eq!(info.placements, [Placement::header("other-header")]);
    assert!(read(&dir, "destinations.toml").contains("Other-Header"));
}

// ---------------------------------------------------------------------------------------------
// remove, set-default, export
// ---------------------------------------------------------------------------------------------

const TWO: &str = "\
[[destination]]
id = \"zipline\"
type = \"zipline\"
url = \"https://zipline.example.org\" # first
[destination.secrets]
token = {}

# the second
[[destination]]
id = \"team\"
type = \"zipline\"
url = \"https://z.team.example\"
[destination.secrets]
token = {}
";

#[test]
fn remove_deletes_the_profile_its_entries_and_the_default_after_a_confirmation() {
    let secret = sentinel();
    let dir = config(
        TWO,
        "default_destination = \"zipline\"\nlanguage = \"fr\"\n",
    );
    let store = MemorySecretSource::new();
    with_entry(&store, "zipline", &secret);
    with_entry(&store, "team", "abcdefghij");
    let input = ScriptedInput::default();
    // No terminal and no --yes: 12, nothing removed.
    assert_eq!(
        run(
            &["dest", "remove", "zipline"],
            dir.path(),
            &plain(&store, &input)
        )
        .code,
        12
    );
    // A terminal, answer no: 3.
    let out = run(
        &["dest", "remove", "zipline"],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 3);
    assert!(
        out.prompts[0].contains("zipline") && out.prompts[0].contains('1'),
        "{:?}",
        out.prompts
    );
    assert!(store.profile_exists("zipline").unwrap());
    assert_eq!(read(&dir, "destinations.toml"), TWO);
    // --yes without a terminal.
    let out = run(
        &["--json", "dest", "remove", "zipline", "--yes"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let doc = out.json();
    assert_eq!(doc["action"], "remove");
    assert_eq!(doc["default"], serde_json::Value::Null);
    assert_eq!(doc["secrets"][0]["action"], "removed");
    assert!(!store.profile_exists("zipline").unwrap());
    assert!(
        store.profile_exists("team").unwrap(),
        "another profile is untouched"
    );
    let file = read(&dir, "destinations.toml");
    assert!(
        file.contains("# the second") && file.contains("id = \"team\""),
        "{file}"
    );
    assert!(!file.contains("zipline.example.org"), "{file}");
    let config = read(&dir, "config.toml");
    assert!(
        !config.contains("default_destination") && config.contains("language"),
        "{config}"
    );
    assert!(!out.out.contains(&secret) && !out.err.contains(&secret));
    // Nothing left to remove.
    let out = run(
        &["dest", "remove", "zipline", "--yes"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 6, "{}", out.err);
    // The default is kept when it was another profile.
    std::fs::write(
        dir.path().join("config.toml"),
        "default_destination = \"zipline\"\n",
    )
    .unwrap();
    let out = run(
        &["dest", "remove", "team", "--yes"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0);
    assert!(read(&dir, "config.toml").contains("default_destination = \"zipline\""));
    assert!(read(&dir, "destinations.toml").trim().is_empty());
    no_temporary_left(&dir);
}

#[test]
fn remove_also_clears_entries_a_removed_profile_left_behind() {
    let dir = config("", "");
    let store = MemorySecretSource::new();
    with_entry(&store, "ghost", "abcdefghij");
    let input = ScriptedInput::default();
    let out = run(
        &["dest", "remove", "ghost", "--yes"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(!store.profile_exists("ghost").unwrap());
    assert!(out.err.contains("no destination 'ghost'"), "{}", out.err);
}

#[test]
fn set_default_needs_an_existing_profile_and_none_clears_it() {
    let dir = config(TWO, "language = \"en\" # mine\n");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let setup = plain(&store, &input);
    let out = run(
        &["--json", "dest", "set-default", "team"],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let doc = out.json();
    assert_eq!(
        (
            doc["action"].as_str(),
            doc["id"].as_str(),
            doc["default"].as_str()
        ),
        (Some("set-default"), Some("team"), Some("team"))
    );
    let config = read(&dir, "config.toml");
    assert!(
        config.contains("default_destination = \"team\"")
            && config.contains("language = \"en\" # mine"),
        "{config}"
    );
    assert_eq!(
        Config::load_from(Some(dir.path()))
            .unwrap()
            .default_destination
            .as_deref(),
        Some("team")
    );
    let out = run(&["dest", "set-default", "nope"], dir.path(), &setup);
    assert_eq!(out.code, 6);
    assert!(read(&dir, "config.toml").contains("team"));
    let out = run(
        &["--json", "dest", "set-default", "--none"],
        dir.path(),
        &setup,
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.json()["default"], serde_json::Value::Null);
    assert!(!read(&dir, "config.toml").contains("default_destination"));
}

#[test]
fn export_writes_references_only_and_never_a_value() {
    let secret = sentinel();
    let dir = config(TWO, "");
    let store = MemorySecretSource::new();
    with_entry(&store, "zipline", &secret);
    with_entry(&store, "team", &format!("{secret}-2"));
    let input = ScriptedInput::default();
    let setup = plain(&store, &input);
    let all = run(&["dest", "export"], dir.path(), &setup);
    assert_eq!(all.code, 0, "{}", all.err);
    assert!(
        all.out.contains("id = \"zipline\"") && all.out.contains("id = \"team\""),
        "{}",
        all.out
    );
    assert!(
        all.out.contains("[destination.secrets.token]"),
        "{}",
        all.out
    );
    // The export is a valid file for `dest import`, one profile at a time.
    let one = run(
        &["--json", "dest", "export", "team", "team"],
        dir.path(),
        &setup,
    );
    let doc = one.json();
    assert_eq!(doc["schema"], "kropa.dest.export/1");
    assert_eq!(doc["destinations"], serde_json::json!(["team"]));
    let toml = doc["toml"].as_str().unwrap();
    assert!(
        toml.contains("id = \"team\"") && !toml.contains("id = \"zipline\""),
        "{toml}"
    );
    for out in [&all, &one] {
        assert!(
            !out.out.contains(&secret) && !out.err.contains(&secret),
            "leak"
        );
    }
    let file = dir.path().join("exported.toml");
    std::fs::write(&file, toml).unwrap();
    let other = config("", "");
    let out = run(
        &[
            "dest",
            "import",
            file.to_str().unwrap(),
            "--yes",
            "--expect-host",
            "z.team.example",
        ],
        other.path(),
        &plain(&MemorySecretSource::new(), &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(run(&["dest", "export", "nope"], dir.path(), &setup).code, 6);
}

#[test]
fn export_refuses_a_file_where_someone_left_a_value() {
    let secret = sentinel();
    let hand_written = format!(
        "[[destination]]\nid = \"api\"\ntype = \"http\"\n[destination.request]\nurl = \"https://api.example.org/up\"\n[destination.request.headers]\nX-Api-Key = \"{secret}\"\n[destination.response]\nurl = \"{{json:$.url}}\"\n"
    );
    let dir = config(&hand_written, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &["--json", "dest", "export"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 11, "{}", out.err);
    assert_eq!(out.json()["error"]["reason"], "secret_in_profile");
    assert!(
        !out.out.contains(&secret) && !out.err.contains(&secret),
        "the refusal repeats the value"
    );
}

// ---------------------------------------------------------------------------------------------
// The Imgur model
// ---------------------------------------------------------------------------------------------

const IMGUR: &str = include_str!("../../../docs/examples/imgur.toml");

#[test]
fn the_imgur_model_imports_with_an_empty_secret_and_holds_no_value() {
    assert!(
        !IMGUR.contains("0123456789"),
        "the model carries no client ID"
    );
    let dir = config("", "");
    let file = dir.path().join("imgur.toml");
    std::fs::write(&file, IMGUR).unwrap();
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &[
            "--json",
            "dest",
            "import",
            file.to_str().unwrap(),
            "--yes",
            "--expect-host",
            "api.imgur.com",
        ],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let info = store.inspect("imgur", "client_id").unwrap().unwrap();
    assert!(!info.has_value, "nothing is invented");
    assert_eq!(info.origins, ["https://api.imgur.com:443"]);
    assert_eq!(info.placements, [Placement::header("authorization")]);
    // It exports back as the same profile, and shows its secret as missing.
    let shown = run(
        &["dest", "show", "imgur"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(shown.code, 0, "{}", shown.err);
    assert!(
        shown.out.contains("Secret client_id: missing"),
        "{}",
        shown.out
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_imgur_model_sends_the_file_in_the_image_field_and_reads_data_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/3/image"))
        .and(header_exists("content-type"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"link": "https://i.imgur.example/abc.png", "deletehash": "dh1"}, "success": true, "status": 200
        })))
        .mount(&server)
        .await;
    // The model, pointed at the local server and without its secret: only the request shape and the
    // reading of the answer are under test here (a secret needs https, `tests/upload_cli.rs`).
    let model = IMGUR
        .replace(
            "https://api.imgur.com/3/image",
            &format!("{}/3/image", server.uri()),
        )
        .replace(
            "Authorization = \"Client-ID {secret:client_id}\"",
            "X-Test = \"1\"",
        )
        .replace(
            "[destination.secrets.client_id]\ndescription = \"Your own Imgur client ID\"\n",
            "",
        );
    let dir = config(&model, "");
    let shot = dir.path().join("shot.png");
    std::fs::write(&shot, b"\x89PNG fake screenshot").unwrap();
    let shot_text = shot.to_str().unwrap().to_owned();
    let dir_path = dir.path().to_owned();
    let out = tokio::task::spawn_blocking(move || {
        let store = MemorySecretSource::new();
        let input = ScriptedInput::default();
        let args = [
            "--json", "upload", &shot_text, "--to", "imgur", "--copy", "none",
        ];
        run(&args, &dir_path, &plain(&store, &input))
    })
    .await
    .unwrap();
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        out.json()["upload"]["url"],
        "https://i.imgur.example/abc.png"
    );
    let requests = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&requests[0].body);
    assert!(body.contains("name=\"image\""), "{body}");
}

// ---------------------------------------------------------------------------------------------
// Nothing leaks, whatever the command
// ---------------------------------------------------------------------------------------------

#[test]
fn no_command_prints_or_writes_the_value() {
    let secret = sentinel();
    let dir = config(TWO, "");
    let store = MemorySecretSource::new();
    with_entry(&store, "zipline", &secret);
    let mut seen = String::new();
    for lang in ["en", "fr"] {
        for json in [false, true] {
            let mut base: Vec<&str> = vec!["--lang", lang];
            if json {
                base.push("--json");
            }
            for tail in [
                vec!["dest", "export"],
                vec!["dest", "show", "zipline"],
                vec!["dest", "set-default", "zipline"],
                vec!["dest", "edit", "zipline", "--set", "label=A"],
                vec![
                    "dest",
                    "edit",
                    "zipline",
                    "--url",
                    "https://moved.example.org",
                ],
                vec![
                    "dest",
                    "add",
                    "zipline",
                    "--type",
                    "zipline",
                    "--url",
                    "https://z.example.org",
                    "--secret-stdin",
                ],
                vec!["dest", "remove", "nope", "--yes"],
            ] {
                let input = ScriptedInput::with(&secret);
                let full: Vec<&str> = base.iter().chain(tail.iter()).copied().collect();
                let out = run(&full, dir.path(), &plain(&store, &input));
                seen.push_str(&out.out);
                seen.push_str(&out.err);
            }
        }
    }
    seen.push_str(&read(&dir, "destinations.toml"));
    seen.push_str(&read(&dir, "config.toml"));
    assert!(!seen.contains(&secret), "the value leaked");
    assert!(seen.len() > 1000, "the commands ran");
}

// ---------------------------------------------------------------------------------------------
// Review of 2.6 (2.6b): B1, I1, I2, I3, m1, m2, m4
// ---------------------------------------------------------------------------------------------

/// An HTTP profile `api` with one secret, `key`.
fn api(url: &str, headers: &str, query: &str, extra_request: &str) -> String {
    format!(
        "[[destination]]\nid = \"api\"\ntype = \"http\"\n[destination.request]\nurl = \"{url}\"\n{extra_request}[destination.request.headers]\n{headers}[destination.request.query]\n{query}[destination.secrets.key]\n[destination.response]\nurl = \"{{json:$.url}}\"\n"
    )
}

/// The entry `api/key`, allowed only for `api.example.org` and the header `x-api-key`.
fn api_entry(store: &MemorySecretSource) {
    store
        .set(
            "api",
            "key",
            SecretValue::new("abcdefghij").unwrap(),
            Authorization {
                origins: vec!["https://api.example.org:443".into()],
                placements: vec![Placement::header("x-api-key")],
            },
        )
        .unwrap();
}

fn assert_entry_untouched(store: &MemorySecretSource) {
    let info = store.inspect("api", "key").unwrap().unwrap();
    assert_eq!(info.origins, ["https://api.example.org:443"]);
    assert_eq!(info.placements, [Placement::header("x-api-key")]);
}

#[test]
fn b1_an_origin_the_file_gained_behind_the_stores_back_is_shown_and_never_allowed_silently() {
    let work = tempfile::tempdir().unwrap();
    // Someone changed `url` in the file; the store still only knows api.example.org.
    let tampered = api(
        "https://evil.example.net/up",
        "X-Api-Key = \"{secret:key}\"\n",
        "",
        "",
    );
    let dir = config(&tampered, "");
    let store = MemorySecretSource::new();
    api_entry(&store);
    let input = ScriptedInput::default();
    let body = "perl -pi -e 's|^X-Api-Key = |Other-Header = |' \"$1\"\n";
    // The user only renames a header. The question shows the foreign origin too, even with --quiet,
    // and "old" is what the store allows.
    let out = run(
        &["--quiet", "dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), body), false),
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert!(
        out.err.contains("https://evil.example.net:443"),
        "{}",
        out.err
    );
    assert!(
        out.err.contains("https://api.example.org:443"),
        "{}",
        out.err
    );
    assert!(out.err.contains("header:other-header"), "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), tampered);
    assert_entry_untouched(&store);
    // Answering yes allows exactly what was shown: the foreign origin and the new place.
    let out = run(
        &["--quiet", "dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), body), true),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let info = store.inspect("api", "key").unwrap().unwrap();
    assert_eq!(
        info.origins,
        [
            "https://api.example.org:443",
            "https://evil.example.net:443"
        ]
    );
    assert_eq!(info.placements, [Placement::header("other-header")]);
}

#[test]
fn b1_a_place_the_file_gained_behind_the_stores_back_is_shown_and_does_not_ride_along_with_an_address()
 {
    let tampered = api(
        "https://api.example.org/up",
        "",
        "key = \"{secret:key}\"\n",
        "",
    );
    let dir = config(&tampered, "");
    let store = MemorySecretSource::new();
    api_entry(&store);
    let input = ScriptedInput::default();
    // `--url` moves the address: the question shows the new origin AND the query place the file has
    // and the store does not allow.
    let out = run(
        &[
            "dest",
            "edit",
            "api",
            "--url",
            "https://api2.example.org/up",
        ],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert!(
        out.err.contains("https://api2.example.org:443"),
        "{}",
        out.err
    );
    assert!(out.err.contains("query:key"), "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), tampered);
    assert_entry_untouched(&store);
    // A change that touches neither the address nor a place allows nothing, and says the file
    // differs from the store. No terminal is needed for that, and the store is left alone.
    let out = run(
        &["--quiet", "dest", "edit", "api", "--set", "label=Mine"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.prompts.is_empty());
    assert!(
        out.err.contains("differs from the secret store") && out.err.contains("key"),
        "{}",
        out.err
    );
    assert!(read(&dir, "destinations.toml").contains("Mine"));
    assert_entry_untouched(&store);
}

#[test]
fn b1_what_the_store_allows_already_is_not_asked_again() {
    // The address goes back to what the store knows: nothing to add, so no question on a terminal;
    // without one the command still answers 12, as for any move of a profile with secrets.
    let dir = config(
        &api(
            "https://api2.example.org/up",
            "X-Api-Key = \"{secret:key}\"\n",
            "",
            "",
        ),
        "",
    );
    let store = MemorySecretSource::new();
    api_entry(&store);
    let input = ScriptedInput::default();
    let out = run(
        &["dest", "edit", "api", "--url", "https://api.example.org/up"],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.prompts.is_empty());
    assert_entry_untouched(&store);
    let out = run(
        &[
            "dest",
            "edit",
            "api",
            "--url",
            "https://api3.example.org/up",
        ],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 12, "{}", out.err);
}

#[test]
fn i1_quiet_does_not_hide_what_the_question_is_about() {
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    with_entry(&store, "zipline", "abcdefghij");
    let input = ScriptedInput::default();
    let out = run(
        &[
            "--quiet",
            "dest",
            "edit",
            "zipline",
            "--url",
            "https://other.example.org",
        ],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 3);
    assert!(
        out.err.contains("https://other.example.org:443"),
        "{}",
        out.err
    );
    // `secrets allow` is the same.
    let out = run(
        &[
            "--quiet",
            "secrets",
            "allow",
            "zipline",
            "token",
            "--origin",
            "https://x.example.org",
        ],
        dir.path(),
        &Setup {
            tty: true,
            answer: false,
            ..plain(&store, &input)
        },
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert!(out.err.contains("https://x.example.org:443"), "{}", out.err);
    assert!(
        out.err.contains("https://zipline.example.org:443"),
        "{}",
        out.err
    );
}

/// A string of hexadecimal digits, made at run time.
fn hex_tail() -> String {
    use std::fmt::Write as _;
    (0..18u8).fold(String::new(), |mut text, i| {
        let _ = write!(text, "{:02x}", i * 13 + 7);
        text
    })
}

#[test]
fn i2_export_and_edit_refuse_every_literal_an_import_would_presume_secret() {
    let work = tempfile::tempdir().unwrap();
    let t = sentinel();
    let cases = [
        // A neutral name, a value that is only hexadecimal.
        api(
            "https://api.example.org/up",
            &format!("X-Request-Tag = \"{}\"\n", hex_tail()),
            "",
            "",
        ),
        // A reference with a literal behind it.
        api(
            "https://api.example.org/up",
            &format!("X-Neutral = \"{{secret:key}}:{t}\"\n"),
            "",
            "",
        ),
        // A literal in the body template.
        api(
            "https://api.example.org/up",
            "",
            "",
            &format!("body = \"json\"\nbody_template = '{{\"note\":\"{t}\"}}'\n"),
        ),
        // A literal in a query parameter and in a field.
        api(
            "https://api.example.org/up",
            "",
            &format!("folder = \"{t}\"\n"),
            "",
        ),
    ];
    for (n, text) in cases.iter().enumerate() {
        let dir = config(text, "");
        let store = MemorySecretSource::new();
        let input = ScriptedInput::default();
        let out = run(
            &["--json", "dest", "export"],
            dir.path(),
            &plain(&store, &input),
        );
        assert_eq!(out.code, 11, "case {n}: {}", out.err);
        assert!(out.out.contains("secret_in_profile"), "case {n}");
        let leaks = [hex_tail(), t.clone()];
        for leak in &leaks {
            assert!(
                !out.out.contains(leak) && !out.err.contains(leak),
                "case {n}"
            );
        }
    }
    // The editor refuses the mixed value too (it used to let it through), without a second round.
    let clean = api(
        "https://api.example.org/up",
        "X-Neutral = \"{secret:key}\"\n",
        "",
        "",
    );
    let dir = config(&clean, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let body = format!("perl -pi -e 's|\\{{secret:key\\}}|{{secret:key}}:{t}|' \"$1\"\n");
    let out = run(
        &["--json", "dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), &body), true),
    );
    assert_eq!(out.code, 11, "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), clean);
    assert!(out.prompts.is_empty());
    assert!(!out.out.contains(&t) && !out.err.contains(&t));
    // So does an import, with the same detector.
    let file = work.path().join("mixed.toml");
    std::fs::write(&file, &cases[1]).unwrap();
    let other = config("", "");
    let out = run(
        &[
            "dest",
            "import",
            file.to_str().unwrap(),
            "--yes",
            "--expect-host",
            "api.example.org",
        ],
        other.path(),
        &plain(&MemorySecretSource::new(), &input),
    );
    assert_ne!(out.code, 0);
    assert!(!out.out.contains(&t) && !out.err.contains(&t));
}

#[test]
fn i3_the_editor_only_suspects_what_the_edit_added_or_changed() {
    let work = tempfile::tempdir().unwrap();
    let t = sentinel();
    // A public field that was in the file already.
    let original = "[[destination]]\nid = \"api\"\ntype = \"http\"\n[destination.request]\nurl = \"https://api.example.org/up\"\nbody = \"multipart\"\nfile_field = \"file\"\n[destination.request.fields]\nfolder = \"shots\"\n[destination.response]\nurl = \"{json:$.url}\"\n";
    let dir = config(original, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let label = "perl -pi -e 's|^type = \"http\"|type = \"http\"\\nlabel = \"Edited\"|' \"$1\"\n";
    let out = run(
        &["dest", "edit", "api"],
        dir.path(),
        &terminal(&store, &input, editor(work.path(), label), false),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    let file = read(&dir, "destinations.toml");
    assert!(
        file.contains("label = \"Edited\"") && file.contains("folder = \"shots\""),
        "{file}"
    );
    // A value the edit adds or changes is still refused.
    for body in [
        format!("perl -pi -e 's|^folder = .*|folder = \"{t}\"|' \"$1\"\n"),
        format!(
            "perl -pi -e 's|^folder = \"shots\"|folder = \"shots\"\\nnote = \"{t}\"|' \"$1\"\n"
        ),
    ] {
        let before = read(&dir, "destinations.toml");
        let out = run(
            &["dest", "edit", "api"],
            dir.path(),
            &terminal(&store, &input, editor(work.path(), &body), true),
        );
        assert_eq!(out.code, 11, "{body}: {}", out.err);
        assert_eq!(read(&dir, "destinations.toml"), before);
        assert!(!out.err.contains(&t));
    }
}

#[test]
fn m1_the_editor_copy_is_a_private_directory_under_the_runtime_dir_and_goes_away_whole() {
    let work = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let log = work.path().join("seen");
    let body = format!(
        "d=$(dirname \"$1\")\n{{ stat -c %a \"$d\" 2>/dev/null || stat -f %Lp \"$d\"; }} > {log}.dirmode\necho \"$d\" > {log}.dir\ntouch \"$d/.swap\"\nperl -pi -e 's|^# the main one|# edited|' \"$1\"\n",
        log = log.display()
    );
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &Setup {
            runtime: Some(runtime.path().to_owned()),
            ..terminal(&store, &input, editor(work.path(), &body), false)
        },
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        std::fs::read_to_string(work.path().join("seen.dirmode"))
            .unwrap()
            .trim(),
        "700"
    );
    let seen = std::fs::read_to_string(work.path().join("seen.dir")).unwrap();
    let seen = Path::new(seen.trim());
    assert_eq!(seen.parent().unwrap(), runtime.path().join("kropa"));
    assert!(
        seen.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".edit-"),
        "{seen:?}"
    );
    // The directory, with the swap file the editor left in it, is gone.
    assert!(!seen.exists());
    assert_eq!(
        std::fs::read_dir(runtime.path().join("kropa"))
            .unwrap()
            .count(),
        0
    );
    assert!(read(&dir, "destinations.toml").contains("# edited"));
    no_temporary_left(&dir);
}

/// The scenario of `a_signal_while_the_editor_runs...`, run in a process of its own by that test: a
/// signal sent to the test process must not reach the other tests.
#[test]
#[ignore = "run by a_signal_while_the_editor_runs_cancels_and_removes_the_private_copy"]
fn signal_child_scenario() {
    let work = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let dir = config(COMMENTED, "");
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    // The editor is hit by Ctrl-C (SIGINT goes to the process group: here, to us), then ends well.
    let body = "perl -pi -e 's|^# the main one|# edited|' \"$1\"\nkill -INT $PPID\nsleep 1\nkill -HUP $PPID\nsleep 1\nexit 0\n";
    let out = run(
        &["dest", "edit", "zipline"],
        dir.path(),
        &Setup {
            runtime: Some(runtime.path().to_owned()),
            ..terminal(&store, &input, editor(work.path(), body), true)
        },
    );
    assert_eq!(out.code, 3, "{}", out.err);
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    assert_eq!(
        std::fs::read_dir(runtime.path().join("kropa"))
            .unwrap()
            .count(),
        0
    );
    no_temporary_left(&dir);
}

#[test]
fn a_signal_while_the_editor_runs_cancels_and_removes_the_private_copy() {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args([
            "--exact",
            "signal_child_scenario",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed"), "{text}");
}

#[cfg(unix)]
#[test]
fn m2_an_edit_keeps_the_mode_of_the_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = config(COMMENTED, "");
    let path = dir.path().join("destinations.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let store = MemorySecretSource::new();
    let input = ScriptedInput::default();
    let out = run(
        &["dest", "edit", "zipline", "--set", "label=Mine"],
        dir.path(),
        &plain(&store, &input),
    );
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn an_unreadable_store_never_lets_an_address_move_unasked() {
    use kropa_upload::secret::UnavailableSecretSource;
    let dir = config(COMMENTED, "");
    let store = UnavailableSecretSource;
    let input = ScriptedInput::default();
    let move_url = [
        "dest",
        "edit",
        "zipline",
        "--url",
        "https://other.example.org",
    ];
    let setup = |tty, answer| Setup {
        store: &store,
        input: &input,
        tty,
        answer,
        editor: None,
        runtime: None,
    };
    // No terminal: 12, with --yes too. A terminal: the error of the store. Nothing is written.
    let out = run(&move_url, dir.path(), &setup(false, true));
    assert_eq!(out.code, 12, "{}", out.err);
    let out = run(&move_url, dir.path(), &setup(true, true));
    assert_eq!(out.code, 5, "{}", out.err);
    assert!(out.prompts.is_empty());
    assert_eq!(read(&dir, "destinations.toml"), COMMENTED);
    // A change that moves nothing goes on without the store.
    let out = run(
        &["dest", "edit", "zipline", "--set", "label=Mine"],
        dir.path(),
        &setup(false, false),
    );
    assert_eq!(out.code, 0, "{}", out.err);
}
