// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end checks of the language selection on the real `kropa` binary.

use std::process::Command;

fn kropa() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kropa"));
    cmd.env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG");
    cmd
}

fn help(locale: &str, extra: &[&str]) -> String {
    let out = kropa()
        .env("LANG", locale)
        .args(extra)
        .arg("--help")
        .output()
        .expect("run kropa");
    assert!(out.status.success());
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn lang_variable_selects_the_help_language() {
    let fr = help("fr_FR.UTF-8", &[]);
    let en = help("en_US.UTF-8", &[]);
    assert_ne!(fr, en);
    assert!(fr.contains("Capturer"));
    assert!(en.contains("Capture, annotate"));
    // Unsupported or C locale: English.
    assert_eq!(help("C", &[]), en);
    assert_eq!(help("ja_JP.UTF-8", &[]), en);
}

#[test]
fn lang_option_wins_over_the_environment() {
    assert_eq!(
        help("en_US.UTF-8", &["--lang", "fr"]),
        help("fr_FR.UTF-8", &[])
    );
    assert_eq!(
        help("fr_FR.UTF-8", &["--lang=en"]),
        help("en_US.UTF-8", &[])
    );
}

#[test]
fn unsupported_lang_is_a_usage_error() {
    let out = kropa().args(["--lang", "de"]).output().expect("run kropa");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn version_is_printed() {
    let out = kropa().arg("--version").output().expect("run kropa");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("kropa "));
}
