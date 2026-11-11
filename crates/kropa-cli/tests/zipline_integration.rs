// SPDX-License-Identifier: GPL-3.0-or-later

//! The real `kropa` binary uploads a real file to a real Zipline 4 (lot 1.11, `docs/testing/integration.md`).
//!
//! The server is the one of `tests/integration/compose.yaml`: Zipline and PostgreSQL behind Caddy, in
//! TLS, because Kropa never sends a secret over plain HTTP and has no option to relax that. The
//! certificate comes from a throwaway CA that only the trust store of the system knows (installed by
//! `tests/integration/run.sh`, or a copy of the system bundle handed through `SSL_CERT_FILE`, which
//! the native store loader reads): the test makes no exception for it, and a control run proves that
//! without that CA the very same upload is refused.
//!
//! The token never goes through an argument or the environment of `kropa`: the test pipes the file
//! written by `run.sh` to `kropa secrets set --secret-stdin`, into a real GNOME Keyring on a private
//! D-Bus session (as `keyring_cli.rs`), then `kropa upload` reads it from the keyring.
//!
//! Ignored by default: run it with `tests/integration/run.sh test`. Without the servers it fails,
//! it never passes silently.

#![cfg(target_os = "linux")]
#![allow(clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine as _;

/// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

const SCRIPT: &str = r#"
set -u
home="$1"; kropa="$2"; cfg="$3"; token_file="$4"; png="$5"
export HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" XDG_CACHE_HOME="$home/cache" XDG_RUNTIME_DIR="$home/run"
unset GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
printf '%s' 'kropa-test-password' | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
daemon=$!
trap 'kill "$daemon" 2>/dev/null' EXIT
i=0
until dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; do
  i=$((i + 1)); [ "$i" -gt 100 ] && { echo "keyring daemon never appeared" >&2; exit 99; }
  sleep 0.2
done
i=0
until "$kropa" --json secrets status 2>/dev/null | grep -q '"state":"available"'; do
  i=$((i + 1)); [ "$i" -gt 100 ] && { echo "keyring never became available" >&2; exit 99; }
  sleep 0.2
done

# The token comes from a file on stdin: never an argument, never the environment.
"$kropa" --lang en --config "$cfg" secrets set zipline token --secret-stdin --yes \
  < "$token_file" > "$home/set.out" 2> "$home/set.err"
echo $? > "$home/set.code"

# The upload, as a user would run it: the link alone on stdout.
"$kropa" --lang en --config "$cfg" upload "$png" --to zipline --copy none \
  > "$home/up.out" 2> "$home/up.err"
echo $? > "$home/up.code"

# Control: the same upload where the throwaway CA is unknown (a store that holds another CA only).
# It must be refused, which proves that the certificate is really checked.
mkdir -p "$home/no-ca-dir"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 1 -subj "/CN=Another CA" \
  -keyout "$home/another-ca.priv" -out "$home/another-ca.crt" 2>/dev/null
SSL_CERT_FILE="$home/another-ca.crt" SSL_CERT_DIR="$home/no-ca-dir" \
  "$kropa" --lang en --config "$cfg" upload "$png" --to zipline --copy none \
  > "$home/untrusted.out" 2> "$home/untrusted.err"
echo $? > "$home/untrusted.code"
"#;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: run this test with tests/integration/run.sh test (docs/testing/integration.md)")
    })
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// What the child processes get for the trust store: nothing by default (the system store), or the
/// bundle copy made by `run.sh --trust bundle`.
fn trust(command: &mut Command) -> &mut Command {
    command
        .env_remove("SSL_CERT_FILE")
        .env_remove("SSL_CERT_DIR");
    match std::env::var("KROPA_IT_SSL_CERT_FILE") {
        Ok(bundle) if !bundle.is_empty() => command.env("SSL_CERT_FILE", bundle),
        _ => command,
    }
}

#[test]
#[ignore = "needs the integration servers: tests/integration/run.sh test"]
fn kropa_uploads_a_file_to_a_real_zipline_and_prints_its_link() {
    let base = var("KROPA_IT_ZIPLINE_URL");
    let token_file = PathBuf::from(var("KROPA_IT_ZIPLINE_TOKEN_FILE"));
    let token = std::fs::read_to_string(&token_file).expect("the token file of run.sh");
    let token = token.trim();
    assert!(token.len() >= 16, "the token file looks empty");

    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(cfg.path().join("config.toml"), "").unwrap();
    // The Zipline preset of `destinations.toml`: an address and the name of a secret, no value.
    std::fs::write(
        cfg.path().join("destinations.toml"),
        format!(
            "[[destination]]\nid = \"zipline\"\ntype = \"zipline\"\nurl = \"{base}\"\n[destination.secrets]\ntoken = {{}}\n"
        ),
    )
    .unwrap();
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap();
    let png = home.path().join("kropa-integration.png");
    std::fs::write(&png, &png_bytes).unwrap();

    let mut run = Command::new("dbus-run-session");
    trust(&mut run)
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
        .arg(&token_file)
        .arg(&png);
    let output = run.output().expect("dbus-run-session");
    let shell = format!(
        "{}\n--- stderr\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{shell}");

    let dir = home.path();
    let (set_err, up_out, up_err) = (
        read(dir, "set.err"),
        read(dir, "up.out"),
        read(dir, "up.err"),
    );
    let all = format!(
        "set: exit {}\n{set_err}\nupload: exit {}\n--- stdout\n{up_out}--- stderr\n{up_err}\nuntrusted: exit {}\n{}",
        read(dir, "set.code").trim(),
        read(dir, "up.code").trim(),
        read(dir, "untrusted.code").trim(),
        read(dir, "untrusted.err"),
    );

    // The token reached the keyring, the upload worked.
    assert_eq!(read(dir, "set.code").trim(), "0", "{all}");
    assert_eq!(read(dir, "up.code").trim(), "0", "{all}");
    // stdout is the link, alone, on one line: the one Zipline returned for this server.
    let link = up_out.strip_suffix('\n').unwrap_or(&up_out);
    assert!(
        !link.contains('\n') && !link.contains(' '),
        "stdout is not one link: {all}"
    );
    assert!(
        link.starts_with(&format!("{base}/u/"))
            && Path::new(link)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png")),
        "unexpected link: {all}"
    );
    eprintln!("kropa upload printed: {link}");
    // The secret is in no stream, and not in the destination file.
    for (name, text) in [
        ("set stderr", &set_err),
        ("stdout", &up_out),
        ("stderr", &up_err),
    ] {
        assert!(!text.contains(token), "the token leaked in {name}");
    }
    assert!(
        !std::fs::read_to_string(cfg.path().join("destinations.toml"))
            .unwrap()
            .contains(token)
    );

    // The link is real: a GET returns the file that was sent (curl uses the same system trust store).
    let fetched = dir.join("fetched.png");
    let mut curl = Command::new("curl");
    trust(&mut curl).args(["--silent", "--show-error", "--fail", "--max-time", "30"]);
    // Debian's curl has its own default bundle and ignores `SSL_CERT_FILE`: give it the bundle (if any).
    if let Ok(bundle) = std::env::var("KROPA_IT_SSL_CERT_FILE")
        && !bundle.is_empty()
    {
        curl.args(["--cacert", &bundle]);
    }
    let status = curl
        .arg("--output")
        .arg(&fetched)
        .arg(link)
        .output()
        .expect("curl");
    assert!(
        status.status.success(),
        "GET {link}: {}\n{all}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        std::fs::read(&fetched).unwrap(),
        png_bytes,
        "the served file differs\n{all}"
    );

    // Control: without the CA in the store the upload fails (no link, a non-zero exit) and says why.
    eprintln!(
        "control without the CA: {}",
        read(dir, "untrusted.err").trim()
    );
    assert_ne!(read(dir, "untrusted.code").trim(), "0", "{all}");
    assert_eq!(read(dir, "untrusted.out"), "", "{all}");
    assert!(
        !read(dir, "untrusted.err").contains(token),
        "the token leaked in the control run"
    );
}
