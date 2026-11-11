// SPDX-License-Identifier: GPL-3.0-or-later

//! Localisation of Kropa, based on Fluent.
//!
//! Resources are embedded (`include_str!`) for French and English. The language is negotiated from
//! `--lang`, then the `language` key of the configuration, then `LC_ALL`, `LC_MESSAGES` and `LANG`
//! (first one that is set and not empty, as POSIX does), falling back to English.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

const EN_FTL: &str = include_str!("../i18n/en.ftl");
const FR_FTL: &str = include_str!("../i18n/fr.ftl");

/// Environment variables consulted for the language, in order of precedence.
pub const ENV_VARS: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];

/// A supported language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English (fallback).
    #[default]
    En,
    /// French.
    Fr,
}

impl Lang {
    /// All supported languages.
    pub const ALL: [Self; 2] = [Self::En, Self::Fr];

    /// Two-letter code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Fr => "fr",
        }
    }

    fn langid(self) -> LanguageIdentifier {
        self.code().parse().expect("static language code")
    }

    fn source(self) -> &'static str {
        match self {
            Self::En => EN_FTL,
            Self::Fr => FR_FTL,
        }
    }

    /// Parses an explicit choice (`--lang`, `language` in the config): `fr`, `en`, also `fr-FR`,
    /// `fr_FR.UTF-8`, case-insensitive. Unlike [`negotiate`], an unsupported language is an error.
    ///
    /// # Errors
    /// [`I18nError::UnsupportedLanguage`].
    pub fn parse(value: &str) -> Result<Self, I18nError> {
        from_locale(value)
            .filter(|_| !is_c_locale(value))
            .ok_or_else(|| I18nError::UnsupportedLanguage(value.to_owned()))
    }
}

/// Error of `kropa-i18n`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum I18nError {
    /// The requested language is not available.
    #[error("unsupported language {0:?} (available: fr, en)")]
    UnsupportedLanguage(String),
    /// An embedded resource does not parse (a bug, caught by the tests).
    #[error("invalid embedded {lang} resource")]
    BadResource {
        /// Language of the resource.
        lang: &'static str,
    },
}

fn is_c_locale(value: &str) -> bool {
    matches!(value, "C" | "POSIX") || value.starts_with("C.")
}

/// Maps a POSIX locale (`fr_FR.UTF-8`, `en_US@euro`, `fr-CA`) to a supported language.
fn from_locale(value: &str) -> Option<Lang> {
    let tag = value.split(['.', '@']).next()?.replace('_', "-");
    let requested: LanguageIdentifier = tag.parse().ok()?;
    // Only two languages: match on the primary language subtag (fr-CA -> fr).
    Lang::ALL
        .into_iter()
        .find(|l| l.code() == requested.language.as_str())
}

/// Negotiates the language: `cli` (value of `--lang`) wins when it is supported, then `config`
/// (the `language` key of the configuration) when it is supported, then the first of `LC_ALL`,
/// `LC_MESSAGES`, `LANG` that is set and not empty, read through `env`. The result is English when
/// nothing matches, including for the `C` and `POSIX` locales. An unsupported explicit choice is
/// skipped here; reporting it is up to whoever parses the option.
#[must_use]
pub fn negotiate(
    cli: Option<&str>,
    config: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Lang {
    if let Some(lang) = cli
        .into_iter()
        .chain(config)
        .find_map(|v| Lang::parse(v).ok())
    {
        return lang;
    }
    ENV_VARS
        .iter()
        .find_map(|name| env(name).filter(|v| !v.is_empty()))
        .and_then(|v| {
            if is_c_locale(&v) {
                None
            } else {
                from_locale(&v)
            }
        })
        .unwrap_or_default()
}

/// Looks messages up in the chosen language, falling back to English, then to the message id.
pub struct Localizer {
    lang: Lang,
    primary: FluentBundle<FluentResource>,
    fallback: Option<FluentBundle<FluentResource>>,
}

impl std::fmt::Debug for Localizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Localizer")
            .field("lang", &self.lang)
            .finish_non_exhaustive()
    }
}

fn bundle(lang: Lang) -> Result<FluentBundle<FluentResource>, I18nError> {
    let resource = FluentResource::try_new(lang.source().to_owned())
        .map_err(|_| I18nError::BadResource { lang: lang.code() })?;
    let mut bundle = FluentBundle::new_concurrent(vec![lang.langid()]);
    // Terminal output: no Unicode isolation marks around placeables.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .map_err(|_| I18nError::BadResource { lang: lang.code() })?;
    Ok(bundle)
}

impl Localizer {
    /// Builds a localizer for `lang`.
    ///
    /// # Errors
    /// [`I18nError::BadResource`] if an embedded resource is invalid.
    pub fn new(lang: Lang) -> Result<Self, I18nError> {
        let fallback = if lang == Lang::En {
            None
        } else {
            Some(bundle(Lang::En)?)
        };
        Ok(Self {
            lang,
            primary: bundle(lang)?,
            fallback,
        })
    }

    /// The language in use.
    #[must_use]
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Message `id` without arguments.
    #[must_use]
    pub fn message(&self, id: &str) -> String {
        self.message_with(id, None)
    }

    /// Message `id` with text arguments (`$name` placeables). An unknown id gives the id itself.
    #[must_use]
    pub fn message_args(&self, id: &str, args: &[(&str, &str)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (name, value) in args {
            fluent_args.set(*name, *value);
        }
        self.message_with(id, Some(&fluent_args))
    }

    /// Message `id` with arguments. An unknown id gives the id itself, never a panic.
    ///
    /// A placeable without a value (a call that forgets an argument, a message that uses one the
    /// caller does not know) is a bug: debug builds and tests panic on it, release builds show the
    /// text as Fluent formats it, `{$name}` in place of the value.
    #[must_use]
    pub fn message_with(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        std::iter::once(&self.primary)
            .chain(self.fallback.as_ref())
            .find_map(|b| {
                let pattern = b.get_message(id)?.value()?;
                let mut errors = Vec::new();
                let text = b.format_pattern(pattern, args, &mut errors).into_owned();
                debug_assert!(errors.is_empty(), "message {id}: {errors:?}");
                Some(text)
            })
            .unwrap_or_else(|| id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fluent_bundle::FluentResource;

    use super::{EN_FTL, FR_FTL, I18nError, Lang, Localizer, negotiate};

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    fn ids(source: &str) -> BTreeSet<String> {
        // Also checks that the resource parses.
        FluentResource::try_new(source.to_owned()).expect("valid ftl");
        source
            .lines()
            .filter_map(|l| l.split_once(" = ").map(|(id, _)| id))
            .filter(|id| {
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn locales_define_the_same_messages() {
        assert_eq!(ids(EN_FTL), ids(FR_FTL));
        assert!(!ids(EN_FTL).is_empty());
    }

    /// `$name` placeables of every message, by message id.
    fn variables(source: &str) -> std::collections::BTreeMap<String, BTreeSet<String>> {
        let mut map = std::collections::BTreeMap::new();
        let mut current: Option<String> = None;
        for line in source.lines() {
            let head = line.split_once(" = ").map(|(id, _)| id).filter(|id| {
                !line.starts_with([' ', '#'])
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            });
            if let Some(id) = head {
                current = Some(id.to_owned());
                map.entry(id.to_owned()).or_insert_with(BTreeSet::new);
            } else if !line.starts_with(' ') {
                current = None;
            }
            let Some(id) = &current else { continue };
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if !name.is_empty() {
                    map.get_mut(id).unwrap().insert(name);
                }
            }
        }
        map
    }

    #[test]
    fn locales_use_the_same_variables() {
        let (en, fr) = (variables(EN_FTL), variables(FR_FTL));
        assert!(en.values().any(|v| !v.is_empty()));
        for (id, vars) in &en {
            assert_eq!(Some(vars), fr.get(id), "variables of {id} differ");
        }
    }

    #[test]
    #[should_panic(expected = "error-unknown-argument")]
    fn a_missing_argument_is_a_bug_that_tests_catch() {
        let en = Localizer::new(Lang::En).unwrap();
        let _ = en.message("error-unknown-argument");
    }

    #[test]
    fn cli_wins_over_environment() {
        assert_eq!(
            negotiate(Some("fr"), None, &env(&[("LANG", "en_US.UTF-8")])),
            Lang::Fr
        );
        assert_eq!(
            negotiate(Some("EN"), None, &env(&[("LC_ALL", "fr_FR.UTF-8")])),
            Lang::En
        );
    }

    #[test]
    fn environment_precedence() {
        let all = [
            ("LC_ALL", "fr_FR.UTF-8"),
            ("LC_MESSAGES", "en_US.UTF-8"),
            ("LANG", "en_US.UTF-8"),
        ];
        assert_eq!(negotiate(None, None, &env(&all)), Lang::Fr);
        let msg = [("LC_MESSAGES", "fr_BE.UTF-8"), ("LANG", "en_US.UTF-8")];
        assert_eq!(negotiate(None, None, &env(&msg)), Lang::Fr);
        assert_eq!(negotiate(None, None, &env(&[("LANG", "fr_CA@euro")])), Lang::Fr);
        // An empty LC_ALL is ignored.
        assert_eq!(
            negotiate(None, None, &env(&[("LC_ALL", ""), ("LANG", "fr_FR")])),
            Lang::Fr
        );
        // The first set variable decides even if it is not supported.
        assert_eq!(
            negotiate(None, None, &env(&[("LC_ALL", "de_DE.UTF-8"), ("LANG", "fr_FR")])),
            Lang::En
        );
    }

    #[test]
    fn configuration_sits_between_the_option_and_the_environment() {
        let fr_env = env(&[("LANG", "fr_FR.UTF-8")]);
        // The option wins over the configuration, which wins over the environment.
        assert_eq!(negotiate(Some("en"), Some("fr"), &fr_env), Lang::En);
        assert_eq!(negotiate(None, Some("en"), &fr_env), Lang::En);
        assert_eq!(negotiate(None, Some("FR-ca"), &env(&[("LANG", "en_US")])), Lang::Fr);
        assert_eq!(negotiate(None, None, &fr_env), Lang::Fr);
        // A language that is not supported is skipped, at each level.
        assert_eq!(negotiate(Some("de"), Some("en"), &fr_env), Lang::En);
        assert_eq!(negotiate(None, Some("de"), &fr_env), Lang::Fr);
        assert_eq!(negotiate(None, Some("C"), &env(&[])), Lang::En);
    }

    #[test]
    fn fallback_is_english() {
        assert_eq!(negotiate(None, None, &env(&[])), Lang::En);
        assert_eq!(negotiate(None, None, &env(&[("LANG", "C")])), Lang::En);
        assert_eq!(negotiate(None, None, &env(&[("LANG", "POSIX")])), Lang::En);
        assert_eq!(negotiate(None, None, &env(&[("LANG", "ja_JP.UTF-8")])), Lang::En);
        assert_eq!(negotiate(None, None, &env(&[("LANG", "%%garbage")])), Lang::En);
        // An unsupported --lang falls through to the environment.
        assert_eq!(negotiate(Some("de"), None, &env(&[("LANG", "fr_FR")])), Lang::Fr);
    }

    #[test]
    fn explicit_parse() {
        assert_eq!(Lang::parse("fr").unwrap(), Lang::Fr);
        assert_eq!(Lang::parse("fr-FR").unwrap(), Lang::Fr);
        assert_eq!(Lang::parse("EN").unwrap(), Lang::En);
        assert!(matches!(
            Lang::parse("de"),
            Err(I18nError::UnsupportedLanguage(_))
        ));
        assert!(Lang::parse("C").is_err());
        assert!(Lang::parse("").is_err());
    }

    #[test]
    fn messages_differ_by_language() {
        let en = Localizer::new(Lang::En).unwrap();
        let fr = Localizer::new(Lang::Fr).unwrap();
        assert_eq!(en.lang(), Lang::En);
        assert_ne!(en.message("cli-about"), fr.message("cli-about"));
        assert_eq!(
            fr.message("cli-about"),
            "Capturer, annoter et envoyer des captures d'écran."
        );
        assert_eq!(fr.message("cli-usage-heading"), "Utilisation :");
    }

    #[test]
    fn text_arguments_are_substituted() {
        let en = Localizer::new(Lang::En).unwrap();
        let fr = Localizer::new(Lang::Fr).unwrap();
        let args = [("arg", "--x")];
        assert_eq!(
            en.message_args("error-unknown-argument", &args),
            "unexpected argument '--x'"
        );
        assert_eq!(
            fr.message_args("error-unknown-argument", &args),
            "argument inattendu '--x'"
        );
    }

    #[test]
    fn unknown_id_returns_the_id() {
        let fr = Localizer::new(Lang::Fr).unwrap();
        assert_eq!(fr.message("does-not-exist"), "does-not-exist");
    }
}
