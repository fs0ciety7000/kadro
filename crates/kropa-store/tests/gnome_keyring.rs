// SPDX-License-Identifier: GPL-3.0-or-later

//! The Secret Service store against a real GNOME Keyring, headless (`docs/security/secrets.md`
//! sections 2.1 and 7).
//!
//! Each `#[test]` below starts `dbus-run-session`, a private home and a keyring daemon unlocked with
//! a test password, and runs the matching `#[ignore]`d `inner_*` test of this same binary inside it.
//! So nothing touches the session of the person who runs the tests, and the bus and the daemon die
//! with the test.
//!
//! The tools are `dbus-run-session`, `dbus-send`, `dbus-monitor` and `gnome-keyring-daemon` (Ubuntu:
//! packages `dbus`, which brings `dbus-bin`, and `gnome-keyring`). Without them the outer tests say so and pass, unless the variable
//! `KROPA_REQUIRE_KEYRING_TESTS` is set (CI sets it), in which case they fail.

#![cfg(target_os = "linux")]
#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kropa_core::SystemClock;
use kropa_store::{SERVICE, SecretServiceStore, Timeouts};
use kropa_upload::secret::{
    Authorization, NewSecret, Placement, SecretError, SecretSource, SecretStore, SecretValue,
    StoreState,
};
use secret_service::EncryptionType;
use secret_service::blocking::SecretService;

const SCRIPT: &str = r#"
set -u
home="$1"; exe="$2"; name="$3"
export HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" XDG_CACHE_HOME="$home/cache" XDG_RUNTIME_DIR="$home/run"
unset GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
# A new keyring in the private home, unlocked with a test password, serving the Secret Service. In the
# foreground and in the background, so that it can be stopped: it does not end with the bus.
printf '%s' 'kropa-test-password' | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
daemon=$!
# Wait until the daemon owns the name. NameHasOwner never activates anything: calling the service
# earlier would make the bus start a second, locked daemon from its service file.
i=0
until dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; do
  i=$((i + 1)); [ "$i" -gt 100 ] && { echo "keyring daemon never appeared"; exit 99; }
  sleep 0.2
done
"$exe" --exact "$name" --ignored --nocapture --test-threads=1
status=$?
kill "$daemon" 2>/dev/null
exit "$status"
"#;

/// CI sets the variable: a missing tool is then a failure, not a skipped test.
fn required() -> bool {
    std::env::var("KROPA_REQUIRE_KEYRING_TESTS").is_ok_and(|v| !v.is_empty())
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// Runs the `inner_*` test `name` inside a private session bus with an unlocked keyring.
fn in_private_session(name: &str) {
    if !(have("dbus-run-session")
        && have("gnome-keyring-daemon")
        && have("dbus-send")
        && have("dbus-monitor"))
    {
        assert!(
            !required(),
            "dbus-run-session and gnome-keyring-daemon are required (KROPA_REQUIRE_KEYRING_TESTS is set)"
        );
        eprintln!("SKIPPED {name}: dbus-run-session or gnome-keyring-daemon is not installed");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let output = Command::new("dbus-run-session")
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env_remove("GNOME_KEYRING_CONTROL")
        .env_remove("SSH_AUTH_SOCK")
        .args(["--", "sh", "-c", SCRIPT, "sh"])
        .arg(home.path())
        .arg(exe)
        .arg(name)
        .output()
        .unwrap();
    let text = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{name} failed\n{text}");
    assert!(text.contains("1 passed"), "{name} did not run\n{text}");
}

#[test]
fn unlocked_keyring_round_trip_and_attributes() {
    in_private_session("inner_unlocked");
}

#[test]
fn unlocked_keyring_duplicates_and_corrupt_items_are_refused() {
    in_private_session("inner_duplicates_and_corruption");
}

#[test]
fn locked_keyring_without_a_way_to_ask_fails_at_once_and_shows_no_prompt() {
    in_private_session("inner_locked_not_interactive");
}

#[test]
fn locked_keyring_whose_prompt_is_dismissed_stays_locked() {
    in_private_session("inner_locked_prompt_unanswered");
}

#[test]
fn a_call_that_misses_its_deadline_is_a_timeout() {
    in_private_session("inner_timeout");
}

// ---------------------------------------------------------------------------------------------
// Inside the private session
// ---------------------------------------------------------------------------------------------

fn store() -> SecretServiceStore {
    SecretServiceStore::new(Arc::new(SystemClock))
}

fn reason(error: &SecretError) -> &'static str {
    match error {
        SecretError::Store { reason, .. } => reason,
        other => panic!("not a store error: {other:?}"),
    }
}

/// A value that looks like nothing in particular, built at run time.
fn value(tail: &str) -> String {
    ["Kr9-", "Tn4Lm2", "Vx7Rw-", tail].concat()
}

fn auth(origin: &str, placement: Placement) -> Authorization {
    Authorization {
        origins: vec![origin.to_owned()],
        placements: vec![placement],
    }
}

/// The items of Kropa, seen from outside the store: attributes, label and the stored document.
struct Raw {
    attributes: HashMap<String, String>,
    label: String,
    secret: String,
}

fn raw_items() -> Vec<Raw> {
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    let found = service
        .search_items(HashMap::from([("service", SERVICE)]))
        .unwrap();
    found
        .unlocked
        .iter()
        .map(|item| Raw {
            attributes: item.get_attributes().unwrap(),
            label: item.get_label().unwrap(),
            secret: String::from_utf8(item.get_secret().unwrap()).unwrap(),
        })
        .collect()
}

fn raw_create(profile: &str, name: &str, secret: &[u8]) {
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    let collection = service.get_default_collection().unwrap();
    collection
        .create_item(
            "raw test item",
            HashMap::from([("service", SERVICE), ("profile", profile), ("name", name)]),
            secret,
            false,
            "application/json",
        )
        .unwrap();
}

fn raw_delete_all() {
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    let found = service
        .search_items(HashMap::from([("service", SERVICE)]))
        .unwrap();
    for item in found.unlocked {
        item.delete().unwrap();
    }
}

/// A `dbus-monitor` on the private bus, writing every method call to a file in the private runtime
/// directory. It is how the tests see what the store sends: a refusal that is quick proves little, a
/// bus that never carried an `Unlock` or a `Prompt` call proves that nobody was asked.
struct Monitor {
    child: Child,
    log: PathBuf,
    /// Size of the log once the monitor was ready: what comes before is the test's own traffic.
    seen: usize,
}

impl Monitor {
    /// Starts it and waits until it sees a call, so that nothing after this point is missed.
    fn start() -> Self {
        let dir = std::env::var("XDG_RUNTIME_DIR").expect("run inside the private session");
        let log = PathBuf::from(dir).join("monitor.log");
        let child = Command::new("dbus-monitor")
            .args(["--session", "type='method_call'"])
            .stdout(File::create(&log).unwrap())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut monitor = Self {
            child,
            log,
            seen: 0,
        };
        let started = Instant::now();
        while !monitor.calls().contains("member=SearchItems") {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the monitor sees nothing"
            );
            // A harmless call, repeated until the monitor has joined the bus.
            let _ = raw_items();
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(300));
        monitor.seen = monitor.calls().len();
        monitor
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Stops it and returns the calls seen since `start` returned (never a value: it is encrypted
    /// on the bus).
    fn stop(mut self) -> String {
        // Calls still in flight reach the log before the monitor is killed.
        std::thread::sleep(Duration::from_millis(300));
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        self.calls().split_off(self.seen)
    }
}

/// Calls that ask the user for something: unlocking, or any step of a prompt.
fn asks_the_user(calls: &str) -> bool {
    calls.lines().any(|line| {
        line.starts_with("method call")
            && (line.contains("member=Unlock")
                || line.contains("interface=org.freedesktop.Secret.Prompt")
                || line.contains("member=Prompt"))
    })
}

/// The daemon registers on the bus a moment before its default collection exists.
fn wait_for_the_default_collection() {
    let started = Instant::now();
    loop {
        let service = SecretService::connect(EncryptionType::Dh).unwrap();
        if service.get_default_collection().is_ok() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "no default collection"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn lock_default_collection() {
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    let collection = service.get_default_collection().unwrap();
    collection.lock().unwrap();
    assert!(collection.is_locked().unwrap());
}

#[test]
#[ignore = "runs inside the private session of `unlocked_keyring_round_trip_and_attributes`"]
fn inner_unlocked() {
    wait_for_the_default_collection();
    let store = store();
    let status = store.status();
    assert_eq!(status.state, StoreState::Available, "{status:?}");
    assert!(status.profiles.is_empty());
    assert!(store.load("zipline", "token").unwrap().is_none());
    assert!(!store.profile_exists("zipline").unwrap());

    // An import creates an empty entry, tied to an origin and a place.
    store
        .put(
            "zipline",
            "token",
            NewSecret {
                value: None,
                origins: vec!["https://zipline.example.org:443".into()],
                placements: vec![Placement::header("Authorization")],
            },
        )
        .unwrap();
    assert!(store.profile_exists("zipline").unwrap());
    assert!(!store.profile_exists("zip").unwrap());
    assert!(store.load("zipline", "token").unwrap().is_none());
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert!(!info.has_value);
    // An import never reuses an entry.
    assert!(
        store
            .put(
                "zipline",
                "token",
                NewSecret {
                    value: None,
                    origins: vec![],
                    placements: vec![]
                }
            )
            .is_err()
    );

    // `set` fills it and keeps what the import tied it to, whatever `fresh` says.
    let secret = value("one");
    store
        .set(
            "zipline",
            "token",
            SecretValue::new(secret.clone()).unwrap(),
            auth("https://evil.example.org:443", Placement::Query("k".into())),
        )
        .unwrap();
    let entry = store.load("zipline", "token").unwrap().unwrap();
    assert_eq!(entry.value.expose(), secret);
    assert_eq!(entry.origins, vec!["https://zipline.example.org:443"]);
    assert_eq!(entry.placements, vec![Placement::header("authorization")]);

    // A new entry gets `fresh`; the value can be replaced.
    let other = value("two");
    store
        .set(
            "other",
            "apikey",
            SecretValue::new(other.clone()).unwrap(),
            auth(
                "https://other.example.org:443",
                Placement::Query("key".into()),
            ),
        )
        .unwrap();
    let entry = store.load("other", "apikey").unwrap().unwrap();
    assert_eq!(entry.origins, vec!["https://other.example.org:443"]);
    let replaced = value("three");
    store
        .set(
            "other",
            "apikey",
            SecretValue::new(replaced.clone()).unwrap(),
            Authorization::default(),
        )
        .unwrap();
    assert_eq!(
        store
            .load("other", "apikey")
            .unwrap()
            .unwrap()
            .value
            .expose(),
        replaced
    );

    // What is in the keyring: one item per secret, three attributes of ours and nothing else, the label with
    // names only, and a version 2 document as the secret.
    let items = raw_items();
    assert_eq!(items.len(), 2);
    for item in &items {
        // GNOME Keyring adds `xdg:schema` by itself; the others are ours and only these.
        let mut keys: Vec<&str> = item
            .attributes
            .keys()
            .map(String::as_str)
            .filter(|k| !k.starts_with("xdg:"))
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["name", "profile", "service"]);
        assert_eq!(item.attributes["service"], SERVICE);
        assert_eq!(
            item.label,
            format!(
                "Kropa: {}/{}",
                item.attributes["profile"], item.attributes["name"]
            )
        );
        for text in item.attributes.values().chain([&item.label]) {
            assert!(
                !text.contains(&secret) && !text.contains(&replaced),
                "{text}"
            );
        }
        let doc: serde_json::Value = serde_json::from_str(&item.secret).unwrap();
        assert_eq!(doc["v"], 2);
        let key = format!("{}/{}", item.attributes["profile"], item.attributes["name"]);
        assert!(doc["entries"][&key]["origins"].is_array(), "{doc}");
        assert!(doc["entries"][&key]["created"].is_string(), "{doc}");
    }

    // `authorize` adds, once.
    for _ in 0..2 {
        store
            .authorize(
                "zipline",
                "token",
                Some("https://new.example.org:443"),
                Some(Placement::Form("title".into())),
            )
            .unwrap();
    }
    let info = store.inspect("zipline", "token").unwrap().unwrap();
    assert_eq!(info.origins.len(), 2);
    assert_eq!(info.placements.len(), 2);
    assert!(info.has_value);
    assert_eq!(
        reason(&store.authorize("zipline", "nope", None, None).unwrap_err()),
        "secret_missing"
    );

    // Counts per profile, never a value.
    let status = store.status();
    assert_eq!(status.state, StoreState::Available);
    assert_eq!(status.profiles["zipline"].entries, 1);
    assert_eq!(status.profiles["other"].entries, 1);
    assert!(!format!("{status:?}").contains(&secret));

    // Remove.
    assert!(store.remove("other", "apikey").unwrap());
    assert!(!store.remove("other", "apikey").unwrap());
    assert!(store.load("other", "apikey").unwrap().is_none());
    assert!(!store.profile_exists("other").unwrap());
    raw_delete_all();
    assert_eq!(store.status().profiles.len(), 0);
}

#[test]
#[ignore = "runs inside the private session of `unlocked_keyring_duplicates_and_corrupt_items_are_refused`"]
fn inner_duplicates_and_corruption() {
    wait_for_the_default_collection();
    let store = store();
    let secret = value("dup");
    store
        .set(
            "p",
            "n",
            SecretValue::new(secret.clone()).unwrap(),
            auth("https://a.example.org:443", Placement::header("x")),
        )
        .unwrap();
    // The same profile and name twice (someone copied the item): nothing is chosen.
    let doc = raw_items().remove(0).secret;
    raw_create("p", "n", doc.as_bytes());
    for error in [
        store.load("p", "n").err().unwrap(),
        store.inspect("p", "n").err().unwrap(),
        store.remove("p", "n").unwrap_err(),
        store
            .set(
                "p",
                "n",
                SecretValue::new(value("x")).unwrap(),
                Authorization::default(),
            )
            .unwrap_err(),
    ] {
        assert_eq!(reason(&error), "keyring_ambiguous");
        assert!(!error.to_string().contains(&secret));
    }
    let status = store.status();
    assert_eq!(status.state, StoreState::Ambiguous, "{status:?}");
    raw_delete_all();

    // An item that is not a document of ours, and one whose attributes were edited to point at
    // another name: refused, with a fixed text, never overwritten, never quoted.
    let junk = format!("not json {secret}");
    raw_create("p", "junk", junk.as_bytes());
    let error = store.load("p", "junk").err().unwrap();
    assert_eq!(reason(&error), "secret_store_error");
    assert!(!error.to_string().contains(&secret), "{error}");
    let error = store
        .set(
            "p",
            "junk",
            SecretValue::new(value("y")).unwrap(),
            Authorization::default(),
        )
        .unwrap_err();
    assert_eq!(reason(&error), "secret_store_error");
    assert_eq!(raw_items().len(), 1);
    assert_eq!(
        raw_items()[0].secret,
        junk,
        "a bad item is never overwritten"
    );
    raw_delete_all();

    store
        .set(
            "p",
            "a",
            SecretValue::new(secret.clone()).unwrap(),
            auth("https://a.example.org:443", Placement::header("x")),
        )
        .unwrap();
    let doc = raw_items().remove(0).secret;
    raw_delete_all();
    raw_create("p", "b", doc.as_bytes());
    let error = store.load("p", "b").err().unwrap();
    assert_eq!(reason(&error), "secret_store_error");
    assert!(error.to_string().contains("does not match"), "{error}");
    raw_delete_all();
}

#[test]
#[ignore = "runs inside the private session of `locked_keyring_without_a_way_to_ask_fails_at_once_and_shows_no_prompt`"]
fn inner_locked_not_interactive() {
    wait_for_the_default_collection();
    let store = store();
    let secret = value("lock");
    store
        .set(
            "p",
            "n",
            SecretValue::new(secret.clone()).unwrap(),
            auth("https://a.example.org:443", Placement::header("x")),
        )
        .unwrap();
    lock_default_collection();
    store.set_interactive(false);
    let monitor = Monitor::start();

    // Every call answers `keyring_locked` at once. The call deadline is 5 s: a prompt would have
    // shown as a timeout (nobody answers it here) and as a slow call.
    let started = Instant::now();
    let errors = [
        store.load("p", "n").err().unwrap(),
        store.inspect("p", "n").err().unwrap(),
        store.remove("p", "n").unwrap_err(),
        store
            .set(
                "p",
                "n",
                SecretValue::new(value("z")).unwrap(),
                Authorization::default(),
            )
            .unwrap_err(),
        store
            .put(
                "p",
                "new",
                NewSecret {
                    value: None,
                    origins: vec![],
                    placements: vec![],
                },
            )
            .unwrap_err(),
        store
            .authorize("p", "n", Some("https://b.example.org:443"), None)
            .unwrap_err(),
    ];
    let calls = monitor.stop();
    for error in &errors {
        assert_eq!(reason(error), "keyring_locked", "{error}");
        assert!(error.to_string().contains("no way to ask"), "{error}");
        assert!(!error.to_string().contains(&secret));
    }
    // The proof: the calls did reach the Secret Service (a search at least), and none of them asked
    // it to unlock anything or opened a prompt.
    assert!(calls.contains("member=SearchItems"), "{calls}");
    assert!(
        !asks_the_user(&calls),
        "a locked keyring was asked to unlock without anyone to answer:\n{calls}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}: a call waited for something",
        started.elapsed()
    );
    // Looking is allowed: the keyring says what it is, and the collection is still locked.
    let status = store.status();
    assert_eq!(status.state, StoreState::Locked, "{status:?}");
    assert!(store.profile_exists("p").unwrap());
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    assert!(
        service
            .get_default_collection()
            .unwrap()
            .is_locked()
            .unwrap()
    );
}

#[test]
#[ignore = "runs inside the private session of `locked_keyring_whose_prompt_is_dismissed_stays_locked`"]
fn inner_locked_prompt_unanswered() {
    wait_for_the_default_collection();
    let store = SecretServiceStore::with_timeouts(
        Arc::new(SystemClock),
        Timeouts {
            call: Duration::from_secs(20),
            prompt: Duration::from_secs(20),
        },
    );
    store
        .set(
            "p",
            "n",
            SecretValue::new(value("prompt")).unwrap(),
            auth("https://a.example.org:443", Placement::header("x")),
        )
        .unwrap();
    lock_default_collection();
    store.set_interactive(true);
    let monitor = Monitor::start();
    let started = Instant::now();
    let error = store.load("p", "n").err().unwrap();
    // Control of the monitor of the test above: here the store does ask, and the monitor sees it.
    let calls = monitor.stop();
    assert!(
        asks_the_user(&calls),
        "the monitor did not see the unlock call:\n{calls}"
    );
    // GNOME Keyring has no prompter in this session and dismisses the prompt: the call ends at once,
    // with the reason of a keyring that stayed locked, and nothing else is tried.
    assert_eq!(reason(&error), "keyring_locked", "{error}");
    assert!(error.to_string().contains("prompt"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    let service = SecretService::connect(EncryptionType::Dh).unwrap();
    assert!(
        service
            .get_default_collection()
            .unwrap()
            .is_locked()
            .unwrap()
    );
}

#[test]
#[ignore = "runs inside the private session of `a_call_that_misses_its_deadline_is_a_timeout`"]
fn inner_timeout() {
    let store = SecretServiceStore::with_timeouts(
        Arc::new(SystemClock),
        Timeouts {
            call: Duration::ZERO,
            prompt: Duration::ZERO,
        },
    );
    let error = store.load("p", "n").err().unwrap();
    assert_eq!(reason(&error), "keyring_timeout", "{error}");
    assert_eq!(store.status().state, StoreState::Unavailable);
}
