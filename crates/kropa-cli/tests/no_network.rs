// SPDX-License-Identifier: GPL-3.0-or-later

//! "Local first": without a configured destination, `kropa` makes no network connection at all
//! (`docs/quality-release.md` section 1.2, criterion of lot 1; `docs/cli.md` principle 8).
//!
//! Two proofs for each command that must send nothing (`--help`, `version`, `doctor`, `dest list`,
//! `secrets status`, `upload` with no destination, ...):
//!
//! 1. it runs inside an empty network namespace (`unshare -rn`: only a down loopback), so it
//!    succeeds, or fails with its own code, without any network;
//! 2. `strace -f` records every socket call and the test asserts that no socket of a network family
//!    (`AF_INET`, `AF_INET6`, ...) is created, bound, connected or written to. Local sockets
//!    (`AF_UNIX`: the D-Bus session, `AF_NETLINK`) are allowed. The check is not vacuous: a control
//!    run (`bash` opening a TCP connection) must be caught by the same detector.
//!
//! Each command runs twice: with no session bus, and on a private D-Bus session (what a desktop
//! offers: `doctor` and `secrets status` then talk to it over a Unix socket).
//!
//! Without `strace` the test says so and passes, unless `KROPA_REQUIRE_NO_NETWORK_TESTS` is set (CI
//! does). Where unprivileged user namespaces are refused (the `AppArmor` policy of recent Ubuntu
//! releases), proof 2 still runs alone and the output says "strace only"; with the variable set, that
//! is a failure instead.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

/// Commands that must not reach the network, with the exit code they have here (no destination, no
/// graphical session, no keyring).
const COMMANDS: &[(&[&str], i32)] = &[
    (&["--help"], 0),
    (&["--version"], 0),
    (&["version"], 0),
    (&["--json", "version"], 0),
    (&["doctor"], 0),
    (&["--json", "doctor"], 0),
    (&["dest", "list"], 0),
    (&["--json", "dest", "list"], 0),
    // An S3 profile (written below) is only read: listing and showing it opens no connection.
    (&["dest", "show", "garage"], 0),
    (&["--json", "dest", "show", "garage"], 0),
    (&["secrets", "status"], 0),
    (&["--json", "secrets", "status"], 0),
    // An existing file, no destination given and none configured: refused before anything is sent (code 6).
    (&["upload", "{png}"], 6),
];

/// An S3 profile: loading it, listing it and showing it must send nothing.
const S3_PROFILE: &str = r#"
[[destination]]
id = "garage"
type = "s3"
endpoint = "https://s3.example.org"
region = "garage"
bucket = "kropa-test"
path_style = true
public_base_url = "https://cdn.example.org/pub"
access_key_id = "GKtest0123456789abcdef"

[destination.secrets]
secret_access_key = {}
"#;

/// The address families that stay on the machine.
const LOCAL_FAMILIES: &[&str] = &["AF_UNIX", "AF_LOCAL", "AF_NETLINK"];

/// The lines of an `strace` log that touch a socket of a network family.
fn network_calls(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| {
            line.match_indices("AF_").any(|(at, _)| {
                let family: String = line[at..]
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                !LOCAL_FAMILIES.contains(&family.as_str())
            })
        })
        .collect()
}

fn required() -> bool {
    std::env::var("KROPA_REQUIRE_NO_NETWORK_TESTS").is_ok_and(|v| !v.is_empty())
}

fn works(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `[unshare -rn] [dbus-run-session --] strace -f ... <program> <args>`, the log going to `log`.
fn traced(netns: bool, bus: bool, log: &Path, program: &str, args: &[&str]) -> Command {
    let mut words: Vec<&str> = Vec::new();
    if netns {
        words.extend(["unshare", "-rn"]);
    }
    if bus {
        words.extend(["dbus-run-session", "--"]);
    }
    let log = log.to_str().unwrap();
    words.extend([
        "strace",
        "-f",
        "-qq",
        "-e",
        "trace=%network",
        "-s",
        "64",
        "-o",
        log,
        program,
    ]);
    words.extend(args);
    let mut command = Command::new(words[0]);
    command.args(&words[1..]);
    command
}

#[test]
fn the_detector_tells_a_network_socket_from_a_local_one() {
    let log = "\
10 socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC, 0) = 3
10 connect(3, {sa_family=AF_UNIX, sun_path=\"/run/user/1000/bus\"}, 20) = 0
10 socket(AF_NETLINK, SOCK_RAW|SOCK_CLOEXEC, NETLINK_ROUTE) = 4
10 sendmsg(4, {msg_name={sa_family=AF_NETLINK, nl_pid=0, nl_groups=00000000}, msg_namelen=12}, 0) = 20
11 socket(AF_INET, SOCK_STREAM, IPPROTO_TCP) = 5
11 connect(5, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr(\"192.0.2.1\")}, 16) = -1
12 sendto(6, \"x\", 1, 0, {sa_family=AF_INET6, sin6_port=htons(53)}, 28) = 1
13 socket(AF_PACKET, SOCK_RAW, 0) = 7
";
    let found = network_calls(log);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(found[0].contains("socket(AF_INET,"));
    assert!(found[1].contains("sin_port=htons(443)"));
    assert!(found[2].contains("AF_INET6"));
    assert!(found[3].contains("AF_PACKET"));
    let local = network_calls("1 connect(3, {sa_family=AF_UNIX, sun_path=\"/x\"}, 8) = 0");
    assert_eq!(local.len(), 0, "{local:?}");
}

#[test]
fn without_a_destination_kropa_opens_no_network_socket() {
    if !works("strace", &["-V"]) {
        assert!(
            !required(),
            "strace is required (KROPA_REQUIRE_NO_NETWORK_TESTS is set)"
        );
        eprintln!("SKIPPED: strace is not installed");
        return;
    }
    let netns = works("unshare", &["-rn", "true"]);
    if netns {
        eprintln!("empty network namespace (unshare -rn) and strace");
    } else {
        assert!(
            !required(),
            "unshare -rn is refused here and KROPA_REQUIRE_NO_NETWORK_TESTS is set"
        );
        eprintln!("NETWORK NAMESPACE UNAVAILABLE (unshare -rn refused): strace only");
    }
    let bus_variants: &[bool] = if works("dbus-run-session", &["--version"]) {
        &[false, true]
    } else {
        assert!(
            !required(),
            "dbus-run-session is required (KROPA_REQUIRE_NO_NETWORK_TESTS is set)"
        );
        eprintln!("no dbus-run-session: the variant on a session bus is skipped");
        &[false]
    };

    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config");
    std::fs::create_dir(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "").unwrap();
    std::fs::write(cfg.join("destinations.toml"), S3_PROFILE).unwrap();
    let png = dir.path().join("capture.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
    let png = png.to_str().unwrap().to_owned();
    let kropa = env!("CARGO_BIN_EXE_kropa");

    // Control: the detector must see a TCP connection attempt (it fails, in an empty namespace, with
    // "Network is unreachable", but the call is traced all the same).
    let log = dir.path().join("control.log");
    let _ = traced(
        netns,
        false,
        &log,
        "bash",
        &["-c", "echo > /dev/tcp/127.0.0.1/9"],
    )
    .output()
    .unwrap();
    let control = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !network_calls(&control).is_empty(),
        "the detector missed a TCP connection attempt:\n{control}"
    );

    let mut failures = Vec::new();
    let mut runs = 0_usize;
    for &bus in bus_variants {
        for (index, (args, expected)) in COMMANDS.iter().enumerate() {
            let log = dir.path().join(format!("run-{bus}-{index}.log"));
            let mut command = traced(netns, bus, &log, kropa, &[]);
            // The global options first (a private configuration folder), then the command.
            command.args(["--lang", "en", "--config"]).arg(&cfg);
            command.args(args.iter().map(|a| a.replace("{png}", &png)));
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", dir.path())
                .env("XDG_CONFIG_HOME", dir.path().join("xdg-config"))
                .env("XDG_DATA_HOME", dir.path().join("xdg-data"))
                .env("XDG_CACHE_HOME", dir.path().join("xdg-cache"))
                .env("XDG_RUNTIME_DIR", dir.path());
            let output = command.output().unwrap();
            runs += 1;
            let name = format!("kropa {} (session bus: {bus})", args.join(" "));
            let code = output.status.code();
            let trace = std::fs::read_to_string(&log).unwrap_or_default();
            if code != Some(*expected) {
                failures.push(format!(
                    "{name}: exit {code:?}, expected {expected}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            // An empty trace is fine for `--help`; a missing log means strace itself did not run.
            if !log.exists() {
                failures.push(format!(
                    "{name}: strace left no log\n{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            let calls = network_calls(&trace);
            if !calls.is_empty() {
                failures.push(format!("{name}: network calls:\n{}", calls.join("\n")));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
    eprintln!(
        "{runs} runs ({} commands, {} bus variant(s)): no network socket opened, control caught",
        COMMANDS.len(),
        bus_variants.len()
    );
}
