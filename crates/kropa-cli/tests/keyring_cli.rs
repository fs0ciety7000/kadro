// SPDX-License-Identifier: GPL-3.0-or-later

//! The real `kropa` binary on a real GNOME Keyring, headless: `secrets set` with `--secret-stdin` and
//! `--secret-fd`, `secrets status`, `dest show`, `secrets remove` (`docs/security/secrets.md` sections 2.1
//! and 7). A private `dbus-run-session`, a private home and a keyring daemon unlocked with a test
//! password: nothing touches the session of the person who runs the test.
//!
//! The same rule as `crates/kropa-store/tests/gnome_keyring.rs`: without `dbus-run-session` and
//! `gnome-keyring-daemon` the test says so and passes, unless `KROPA_REQUIRE_KEYRING_TESTS` is set.

#![cfg(target_os = "linux")]

use std::process::Command;

const SCRIPT: &str = r#"
set -u
home="$1"; kropa="$2"; cfg="$3"; secret="$4"
export HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" XDG_CACHE_HOME="$home/cache" XDG_RUNTIME_DIR="$home/run"
unset GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
# A new keyring in the private home, unlocked with a test password, serving the Secret Service. In the
# foreground and in the background, so that it can be stopped: it does not end with the bus.
printf '%s' 'kropa-test-password' | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
daemon=$!
trap 'kill "$daemon" 2>/dev/null' EXIT
# Wait until the daemon owns the name. NameHasOwner never activates anything: calling the service
# earlier would make the bus start a second, locked daemon from its service file.
i=0
until dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; do
  i=$((i + 1)); [ "$i" -gt 100 ] && { echo "keyring daemon never appeared"; exit 99; }
  sleep 0.2
done
# The daemon is on the bus a moment before its default collection exists.
i=0
until "$kropa" --json secrets status 2>/dev/null | grep -q '"state":"available"'; do
  i=$((i + 1)); [ "$i" -gt 100 ] && { echo "keyring never became available"; exit 99; }
  sleep 0.2
done
run() { echo "== $*"; "$kropa" --lang en --config "$cfg" "$@"; echo "-> exit $?"; }

echo "== status (empty)"
"$kropa" --lang en --json --config "$cfg" secrets status; echo "-> exit $?"
printf '%s\n' "$secret" | run secrets set zipline token --secret-stdin --yes
run secrets status
"$kropa" --lang en --json --config "$cfg" secrets status; echo "-> exit $?"
run dest show zipline
# The value of a second secret from a descriptor; a refused value; an unknown secret.
printf '%s\n' "$secret-fd" > "$home/fd-secret"
run secrets set zipline token --secret-fd 3 3< "$home/fd-secret"
printf 'abc\n' | run secrets set zipline token --secret-stdin --yes
printf '%s\n' "$secret" | run secrets set zipline nope --secret-stdin --yes
# Without --yes and without a terminal, a new entry is not created and stdin is not read.
run secrets remove zipline token --yes
printf '%s\n' "$secret" | run secrets set zipline token --secret-stdin
run dest show zipline
run secrets remove zipline token --yes
run secrets remove zipline token --yes
"#;

/// CI sets the variable: a missing tool is then a failure, not a skipped test.
fn required() -> bool {
    std::env::var("KROPA_REQUIRE_KEYRING_TESTS").is_ok_and(|v| !v.is_empty())
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

#[test]
fn the_binary_stores_reads_and_removes_a_secret_in_a_real_keyring() {
    if !(have("dbus-run-session") && have("gnome-keyring-daemon") && have("dbus-send")) {
        assert!(
            !required(),
            "dbus-run-session and gnome-keyring-daemon are required (KROPA_REQUIRE_KEYRING_TESTS is set)"
        );
        eprintln!("SKIPPED: dbus-run-session or gnome-keyring-daemon is not installed");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(cfg.path().join("config.toml"), "").unwrap();
    std::fs::write(
        cfg.path().join("destinations.toml"),
        "[[destination]]\nid = \"zipline\"\ntype = \"zipline\"\nurl = \"https://zipline.example.org\"\n[destination.secrets]\ntoken = {}\n",
    )
    .unwrap();
    // Built at run time: nothing in the source looks like a token.
    let secret = ["Kb8-", "Tn4Lm2", "Vx7Rw-qq"].concat();
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
        .arg(env!("CARGO_BIN_EXE_kropa"))
        .arg(cfg.path())
        .arg(&secret)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    let all = format!("{out}\n--- stderr\n{err}");
    assert!(output.status.success(), "{all}");
    // The value is nowhere, in either stream.
    assert!(!all.contains(&secret), "the value leaked:\n{all}");

    // Empty at first, one entry after `set`, the state of the secret in `dest show`, then gone.
    assert!(
        out.contains(r#""state":"available","detail":null,"profiles":[]"#),
        "{all}"
    );
    assert!(
        out.contains(r#""profiles":[{"profile":"zipline","entries":1,"empty":0}]"#),
        "{all}"
    );
    assert!(out.contains("Secret store: keyring (available)"), "{all}");
    assert!(out.contains("zipline: 1 entries, 0 still empty"), "{all}");
    assert!(out.contains("Secret token: set"), "{all}");
    assert!(out.contains("Secret token: missing"), "{all}");
    // The new entry was tied to the address and place of the profile, and shown.
    assert!(err.contains("https://zipline.example.org:443"), "{all}");
    assert!(err.contains("header:authorization"), "{all}");
    // Exit codes, in order: status 0, set 0, status 0, status 0, show 0, set (fd) 0, short 11,
    // undeclared 6, remove 0, set without --yes 12, show 0, remove 0, remove (nothing) 0.
    let in_order: Vec<&str> = out
        .lines()
        .filter_map(|l| l.strip_prefix("-> exit "))
        .collect();
    assert_eq!(
        in_order,
        [
            "0", "0", "0", "0", "0", "0", "11", "6", "0", "12", "0", "0", "0"
        ],
        "{all}"
    );
}
