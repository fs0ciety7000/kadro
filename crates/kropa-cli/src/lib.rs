// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line client of Kropa (`kropa`), free of any GTK dependency.

use std::ffi::OsString;

use clap::{Arg, ArgAction, Command};
use kropa_core::ExitCode;
use kropa_i18n::{I18nError, Lang, Localizer};

/// Name of the global language option.
const LANG_ARG: &str = "lang";

/// Finds the value of `--lang` in raw arguments, before the real parsing: the help text must already be
/// in the right language when clap renders it. Stops at `--`. Returns the last occurrence.
#[must_use]
pub fn prescan_lang(args: &[OsString]) -> Option<String> {
    let mut found = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else { continue };
        if arg == "--" {
            break;
        }
        if arg == "--lang" {
            if let Some(value) = iter.next().and_then(|v| v.to_str()) {
                found = Some(value.to_owned());
            }
        } else if let Some(value) = arg.strip_prefix("--lang=") {
            found = Some(value.to_owned());
        }
    }
    found
}

fn parse_lang(value: &str) -> Result<String, String> {
    Lang::parse(value)
        .map(|l| l.code().to_owned())
        .map_err(|e| e.to_string())
}

/// Builds the `kropa` command with every visible string taken from Fluent.
#[must_use]
pub fn build_command(loc: &Localizer) -> Command {
    let template = format!(
        "{{about}}\n\n{} {{usage}}\n\n{}\n{{options}}\n",
        loc.message("cli-usage-heading"),
        loc.message("cli-options-heading"),
    );
    Command::new("kropa")
        .version(env!("CARGO_PKG_VERSION"))
        .about(loc.message("cli-about"))
        .disable_help_flag(true)
        .disable_version_flag(true)
        .help_template(template)
        .arg(
            Arg::new(LANG_ARG)
                .long("lang")
                .value_name("LANG")
                .global(true)
                .value_parser(parse_lang)
                .help(loc.message("cli-lang-flag")),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .global(true)
                .help(loc.message("cli-help-flag")),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(loc.message("cli-version-flag")),
        )
}

/// Runs the command line: `args` includes the program name, `lookup` reads the standard locale
/// variables. Output goes to stdout/stderr; the result is the process exit code.
#[must_use]
pub fn run(args: &[OsString], lookup: &dyn Fn(&str) -> Option<String>) -> ExitCode {
    match run_inner(args, lookup) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kropa: {e}");
            ExitCode::Internal
        }
    }
}

fn run_inner(
    args: &[OsString],
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<ExitCode, I18nError> {
    let lang = kropa_i18n::negotiate(prescan_lang(args).as_deref(), lookup);
    let loc = Localizer::new(lang)?;
    match build_command(&loc).try_get_matches_from(args) {
        Ok(_) => Ok(ExitCode::Ok),
        Err(e) => {
            // Help and version go to stdout with code 0; every parse error is a usage error (2).
            let code = if e.use_stderr() {
                ExitCode::Usage
            } else {
                ExitCode::Ok
            };
            // Nothing sensible can be done if the terminal is closed.
            let _ = e.print();
            Ok(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use kropa_i18n::{Lang, Localizer};

    use super::{build_command, prescan_lang};

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn help(lang: Lang) -> String {
        let mut cmd = build_command(&Localizer::new(lang).unwrap());
        cmd.render_long_help().to_string()
    }

    #[test]
    fn help_differs_by_language() {
        let fr = help(Lang::Fr);
        let en = help(Lang::En);
        assert_ne!(fr, en);
        assert!(fr.contains("Capturer, annoter"), "{fr}");
        assert!(fr.contains("Utilisation :"), "{fr}");
        assert!(en.contains("Capture, annotate"), "{en}");
        assert!(en.contains("Usage:"), "{en}");
    }

    #[test]
    fn prescan() {
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang", "fr"])).as_deref(),
            Some("fr")
        );
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang=en", "--help"])).as_deref(),
            Some("en")
        );
        assert_eq!(
            prescan_lang(&args(&["kropa", "--lang", "fr", "--lang=en"])).as_deref(),
            Some("en")
        );
        assert_eq!(prescan_lang(&args(&["kropa", "--", "--lang", "fr"])), None);
        assert_eq!(prescan_lang(&args(&["kropa", "--lang"])), None);
    }

    #[test]
    fn lang_option_is_validated_and_case_insensitive() {
        let cmd = build_command(&Localizer::new(Lang::En).unwrap());
        let m = cmd
            .clone()
            .try_get_matches_from(["kropa", "--lang", "FR"])
            .unwrap();
        assert_eq!(m.get_one::<String>("lang").map(String::as_str), Some("fr"));
        assert!(cmd.try_get_matches_from(["kropa", "--lang", "de"]).is_err());
    }
}
