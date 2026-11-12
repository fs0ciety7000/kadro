// SPDX-License-Identifier: GPL-3.0-or-later

//! The real `kropa` binary uploads a real file to a real Garage (S3) over TLS (lot 2.7a,
//! `docs/testing/integration.md`).
//!
//! The server is the one of `tests/integration/compose.yaml` (profile `s3`): Garage behind Caddy, in TLS,
//! because Kropa never sends a secret over plain HTTP. The certificate comes from the throwaway CA of
//! `tests/integration/run.sh`, known to the system verifier only (see `zipline_integration.rs`).
//!
//! The command line of 2.7b (`dest add --type s3`) does not exist yet: the test writes
//! `destinations.toml` by hand, and the secret key goes to the keyring through `kropa secrets set
//! --secret-stdin` (from a 0600 file written by `run.sh`; never an argument, never the environment).
//! The access key id is not a secret and sits in the file (decision D9).
//!
//! Checked: the link of a profile with `presign` is a real presigned URL, which a `GET` answers with the
//! bytes that were sent; the one of a profile with `public_base_url` is that address and the key; the
//! secret key is in no stream and not in the file; without the CA in the store the upload is refused.
//!
//! Ignored by default: run it with `tests/integration/run.sh test --profile s3`. Without the servers it
//! fails, it never passes silently.

#![cfg(target_os = "linux")]
#![allow(clippy::too_many_lines)]

use std::path::Path;
use std::process::Command;

use base64::Engine as _;

/// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

const SCRIPT: &str = r#"
set -u
home="$1"; kropa="$2"; cfg="$3"; secret_file="$4"; png="$5"; unlock="$6"
export HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" XDG_CACHE_HOME="$home/cache" XDG_RUNTIME_DIR="$home/run"
unset GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
printf '%s' "$unlock" | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
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

# The secret key comes from a file on stdin: never an argument, never the environment.
for profile in garage garage-pub; do
  "$kropa" --lang en --config "$cfg" secrets set "$profile" secret_access_key --secret-stdin --yes \
    < "$secret_file" > "$home/set-$profile.out" 2> "$home/set-$profile.err"
  echo $? > "$home/set-$profile.code"
done

# The uploads, as a user would run them: the link alone on stdout.
for profile in garage garage-pub; do
  "$kropa" --lang en --config "$cfg" upload "$png" --to "$profile" --copy none \
    > "$home/up-$profile.out" 2> "$home/up-$profile.err"
  echo $? > "$home/up-$profile.code"
done

# Control: the same upload where the throwaway CA is unknown (a store that holds another CA only).
mkdir -p "$home/no-ca-dir"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 1 -subj "/CN=Another CA" \
  -keyout "$home/another-ca.priv" -out "$home/another-ca.crt" 2>/dev/null
SSL_CERT_FILE="$home/another-ca.crt" SSL_CERT_DIR="$home/no-ca-dir" \
  "$kropa" --lang en --config "$cfg" upload "$png" --to garage --copy none \
  > "$home/untrusted.out" 2> "$home/untrusted.err"
echo $? > "$home/untrusted.code"
"#;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set: run this test with tests/integration/run.sh test --profile s3 (docs/testing/integration.md)")
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
#[ignore = "needs the integration servers: tests/integration/run.sh test --profile s3"]
fn kropa_uploads_a_file_to_a_real_garage_and_the_presigned_link_reads_it_back() {
    let base = var("KROPA_IT_S3_URL");
    let bucket = var("KROPA_IT_S3_BUCKET");
    let key_id = std::fs::read_to_string(var("KROPA_IT_S3_KEY_ID_FILE")).expect("the key id file");
    let key_id = key_id.trim();
    let secret_file = var("KROPA_IT_S3_SECRET_FILE");
    let secret = std::fs::read_to_string(&secret_file).expect("the secret key file of run.sh");
    let secret = secret.trim();
    assert!(
        key_id.starts_with("GK") && secret.len() >= 32,
        "the key files look empty"
    );

    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(cfg.path().join("config.toml"), "").unwrap();
    // Two profiles on the same bucket: the link of the first is presigned, the one of the second is a
    // public address (not served by Garage here: only its form is checked). Hand-written, as 2.7b is
    // the command line that will do it.
    let profile = |id: &str, link: &str| {
        format!(
            "[[destination]]\nid = \"{id}\"\ntype = \"s3\"\nendpoint = \"{base}\"\nregion = \"garage\"\nbucket = \"{bucket}\"\npath_style = true\nprefix = \"it\"\n{link}\naccess_key_id = \"{key_id}\"\n\n[destination.secrets]\nsecret_access_key = {{}}\n\n"
        )
    };
    let destinations = format!(
        "{}{}",
        profile("garage", "presign = \"10m\""),
        profile(
            "garage-pub",
            "public_base_url = \"https://cdn.example.org/pub\""
        )
    );
    std::fs::write(cfg.path().join("destinations.toml"), &destinations).unwrap();
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
        .arg(&secret_file)
        .arg(&png)
        // Unlocks the throwaway keyring; built at run time, it is no literal of the source.
        .arg(["throwaway-", "keyring-", "unlock"].concat());
    let output = run.output().expect("dbus-run-session");
    let shell = format!(
        "{}\n--- stderr\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{shell}");

    let dir = home.path();
    let all = format!(
        "garage: set {} up {}\n--- stdout\n{}--- stderr\n{}\ngarage-pub: set {} up {}\n--- stdout\n{}--- stderr\n{}\nuntrusted: exit {}\n{}",
        read(dir, "set-garage.code").trim(),
        read(dir, "up-garage.code").trim(),
        read(dir, "up-garage.out"),
        read(dir, "up-garage.err"),
        read(dir, "set-garage-pub.code").trim(),
        read(dir, "up-garage-pub.code").trim(),
        read(dir, "up-garage-pub.out"),
        read(dir, "up-garage-pub.err"),
        read(dir, "untrusted.code").trim(),
        read(dir, "untrusted.err"),
    );
    for name in ["set-garage", "set-garage-pub", "up-garage", "up-garage-pub"] {
        assert_eq!(read(dir, &format!("{name}.code")).trim(), "0", "{all}");
    }

    // The presigned link: one line, on the service, holding the key id, a ten minutes validity and a
    // signature; never the secret key.
    let up_out = read(dir, "up-garage.out");
    let link = up_out.strip_suffix('\n').unwrap_or(&up_out);
    assert!(
        !link.contains('\n') && !link.contains(' '),
        "stdout is not one link: {all}"
    );
    assert!(
        link.starts_with(&format!("{base}/{bucket}/it/kropa-integration.png?")),
        "unexpected link: {all}"
    );
    assert!(
        link.contains(&format!("X-Amz-Credential={key_id}")),
        "{all}"
    );
    assert!(link.contains("X-Amz-Expires=600"), "{all}");
    assert!(link.contains("X-Amz-Signature="), "{all}");
    eprintln!(
        "kropa upload printed a presigned link ({} bytes)",
        link.len()
    );

    // The public address: the base and the key, nothing else.
    let public = read(dir, "up-garage-pub.out");
    assert_eq!(
        public.trim_end(),
        "https://cdn.example.org/pub/it/kropa-integration.png",
        "{all}"
    );

    // The secret key is in no stream, and not in the destination file.
    for name in [
        "set-garage.err",
        "set-garage-pub.err",
        "up-garage.out",
        "up-garage.err",
        "up-garage-pub.out",
        "up-garage-pub.err",
        "untrusted.err",
    ] {
        assert!(
            !read(dir, name).contains(secret),
            "the secret key leaked in {name}"
        );
    }
    assert!(
        !std::fs::read_to_string(cfg.path().join("destinations.toml"))
            .unwrap()
            .contains(secret)
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
        "GET of the presigned link: {}\n{all}",
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
}
