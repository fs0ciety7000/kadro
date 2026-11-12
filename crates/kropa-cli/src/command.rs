// SPDX-License-Identifier: GPL-3.0-or-later

//! The clap command tree, with every visible string taken from Fluent.
//!
//! The tree itself comes from the derive in [`crate::args`]. Here it is walked once to give each
//! command, option and value name its Fluent text:
//!
//! - command at path `dest add`: `cmd-dest-add` (the root uses `cli-about`);
//! - option `copy` of command `upload`: `arg-upload-copy` if it exists, else `arg-copy`;
//! - value name of option `file`: `val-file`.

use clap::{Arg, ArgAction, ColorChoice, Command, CommandFactory};
use kropa_i18n::Localizer;

use crate::args::{Cli, ColorWhen};

/// clap's colour choice for a value of `--color`.
#[must_use]
pub fn color_choice(color: ColorWhen) -> ColorChoice {
    match color {
        ColorWhen::Auto => ColorChoice::Auto,
        ColorWhen::Always => ColorChoice::Always,
        ColorWhen::Never => ColorChoice::Never,
    }
}

/// Builds the `kropa` command in the language of `loc`.
#[must_use]
pub fn build_command(loc: &Localizer, color: ColorChoice) -> Command {
    let cmd = Cli::command()
        .color(color)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .global(true),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version),
        );
    localize(cmd, &[], loc)
}

/// Global options in the order of the help, after the command's own options.
const GLOBAL_ORDER: [&str; 10] = [
    "json",
    "lang",
    "quiet",
    "verbose",
    "no_input",
    "color",
    "config",
    "password_fd",
    "help",
    "version",
];

/// `Some(text)` if Fluent has a message `id` (the localizer answers with the id itself otherwise).
fn lookup(loc: &Localizer, id: &str) -> Option<String> {
    let text = loc.message(id);
    (text != id).then_some(text)
}

fn arg_help(loc: &Localizer, path: &[String], id: &str) -> Option<String> {
    let id = id.replace('_', "-");
    match id.as_str() {
        "help" => return lookup(loc, "cli-help-flag"),
        "version" => return lookup(loc, "cli-version-flag"),
        "lang" => return lookup(loc, "cli-lang-flag"),
        _ => {}
    }
    let specific = format!("arg-{}-{id}", path.join("-"));
    lookup(loc, &specific).or_else(|| lookup(loc, &format!("arg-{id}")))
}

fn localize_arg(arg: Arg, path: &[String], loc: &Localizer) -> Arg {
    let id = arg.get_id().to_string();
    let mut arg = arg;
    if arg.get_action().takes_values() {
        // The values are listed in the Fluent help text; clap's own list is not translatable.
        arg = arg.hide_possible_values(true).hide_default_value(true);
    }
    if let Some(rank) = GLOBAL_ORDER.iter().position(|g| *g == id) {
        // Options valid everywhere come after the options of the command itself.
        arg = arg.display_order(1000 + rank);
    }
    if let Some(help) = arg_help(loc, path, &id) {
        arg = arg.help(help);
    }
    if let Some(name) = lookup(loc, &format!("val-{}", id.replace('_', "-"))) {
        arg = arg.value_name(name);
    }
    arg
}

fn localize(mut cmd: Command, path: &[String], loc: &Localizer) -> Command {
    let about_id = if path.is_empty() {
        "cli-about".to_owned()
    } else {
        format!("cmd-{}", path.join("-"))
    };
    cmd = cmd.about(loc.message(&about_id));
    if cmd.has_subcommands() {
        cmd = cmd.subcommand_value_name(loc.message("val-command"));
    }
    let ids: Vec<String> = cmd
        .get_arguments()
        .map(|a| a.get_id().to_string())
        .collect();
    for id in ids {
        cmd = cmd.mut_arg(id, |a| localize_arg(a, path, loc));
    }
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_owned())
        .collect();
    for name in names {
        let mut child = path.to_vec();
        child.push(name.clone());
        cmd = cmd.mut_subcommand(name, |sc| localize(sc, &child, loc));
    }
    let template = help_template(&cmd, loc);
    cmd.help_template(template)
}

/// Help layout with localised headings; sections that would be empty are left out.
fn help_template(cmd: &Command, loc: &Localizer) -> String {
    let mut parts = vec![format!(
        "{{about}}\n\n{} {{usage}}\n",
        loc.message("cli-usage-heading")
    )];
    if cmd.get_subcommands().next().is_some() {
        parts.push(format!(
            "\n{}\n{{subcommands}}\n",
            loc.message("cli-commands-heading")
        ));
    }
    if cmd.get_positionals().next().is_some() {
        parts.push(format!(
            "\n{}\n{{positionals}}\n",
            loc.message("cli-arguments-heading")
        ));
    }
    parts.push(format!(
        "\n{}\n{{options}}\n",
        loc.message("cli-options-heading")
    ));
    parts.concat()
}

#[cfg(test)]
mod tests {
    use clap::{ColorChoice, Command};
    use kropa_i18n::{Lang, Localizer};

    use super::{arg_help, build_command, lookup};

    fn walk(cmd: &Command, path: &mut Vec<String>, f: &mut dyn FnMut(&Command, &[String])) {
        f(cmd, path);
        for sc in cmd.get_subcommands() {
            path.push(sc.get_name().to_owned());
            walk(sc, path, f);
            path.pop();
        }
    }

    #[test]
    fn every_command_and_option_has_a_message_in_both_languages() {
        for lang in Lang::ALL {
            let loc = Localizer::new(lang).unwrap();
            let cmd = build_command(&loc, ColorChoice::Never);
            let mut missing = Vec::new();
            walk(&cmd, &mut Vec::new(), &mut |c, path| {
                if !path.is_empty() && lookup(&loc, &format!("cmd-{}", path.join("-"))).is_none() {
                    missing.push(format!("cmd-{}", path.join("-")));
                }
                for a in c.get_arguments() {
                    let id = a.get_id().as_str();
                    if arg_help(&loc, path, id).is_none() {
                        missing.push(format!("arg {id} of {path:?}"));
                    }
                }
            });
            assert!(missing.is_empty(), "{lang:?}: {missing:?}");
        }
    }

    #[test]
    fn help_is_localised_everywhere() {
        let loc = Localizer::new(Lang::Fr).unwrap();
        let cmd = build_command(&loc, ColorChoice::Never);
        let mut paths = Vec::new();
        walk(&cmd, &mut Vec::new(), &mut |_, path| {
            paths.push(path.to_vec());
        });
        assert!(paths.len() > 25, "{}", paths.len());
        for path in paths {
            // Real parsing, so that the global options are propagated as in a run.
            let mut argv = vec!["kropa".to_owned()];
            argv.extend(path.iter().cloned());
            argv.push("--help".to_owned());
            let err = cmd.clone().try_get_matches_from(argv).unwrap_err();
            let help = err.to_string();
            // No English text from clap itself may leak into the French help.
            for english in [
                "Usage:",
                "Options:",
                "Commands:",
                "Arguments:",
                "Print help",
                "default:",
                "<COMMAND>",
            ] {
                assert!(
                    !help.contains(english),
                    "{path:?}: {english} leaked\n{help}"
                );
            }
            assert!(help.contains("Utilisation :"), "{path:?}");
        }
    }

    #[test]
    fn clap_accepts_the_tree() {
        build_command(&Localizer::new(Lang::En).unwrap(), ColorChoice::Never).debug_assert();
    }
}
