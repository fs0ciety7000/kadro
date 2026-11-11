// SPDX-License-Identifier: GPL-3.0-or-later

//! Localisation of Kropa, based on Fluent.
//!
//! Resources are embedded (`include_str!`) for French and English. The language is negotiated from
//! `--lang`, then `LC_ALL`, `LC_MESSAGES` and `LANG` (first one that is set and not empty, as POSIX
//! does), falling back to English.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
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
    let available: Vec<LanguageIdentifier> = Lang::ALL.iter().map(|l| l.langid()).collect();
    let chosen = negotiate_languages(&[requested], &available, None, NegotiationStrategy::Filtering);
    let first = chosen.first()?;
    Lang::ALL.into_iter().find(|l| l.langid() == **first)
}

/// Negotiates the language: `cli` (value of `--lang`) wins when it is supported, then the first of
/// `LC_ALL`, `LC_MESSAGES`, `LANG` that is set and not empty, read through `env`. The result is
/// English when nothing matches, including for the `C` and `POSIX` locales.
#[must_use]
pub fn negotiate(cli: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Lang {
    if let Some(lang) = cli.and_then(|v| Lang::parse(v).ok()) {
        return lang;
    }
    ENV_VARS
        .iter()
        .find_map(|name| env(name).filter(|v| !v.is_empty()))
        .and_then(|v| if is_c_locale(&v) { None } else { from_locale(&v) })
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
        f.debug_struct("Localizer").field("lang", &self.lang).finish_non_exhaustive()
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
        let fallback = if lang == Lang::En { None } else { Some(bundle(Lang::En)?) };
        Ok(Self { lang, primary: bundle(lang)?, fallback })
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

    /// Message `id` with arguments. An unknown id gives the id itself, never a panic.
    #[must_use]
    pub fn message_with(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        std::iter::once(&self.primary)
            .chain(self.fallback.as_ref())
            .find_map(|b| {
                let pattern = b.get_message(id)?.value()?;
                let mut errors = Vec::new();
                Some(b.format_pattern(pattern, args, &mut errors).into_owned())
            })
            .unwrap_or_else(|| id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fluent_bundle::FluentResource;

    use super::{FR_FTL, EN_FTL, I18nError, Lang, Localizer, negotiate};

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    fn ids(source: &str) -> BTreeSet<String> {
        let res = FluentResource::try_new(source.to_owned()).expect("valid ftl");
        res.entries()
            .filter_map(|e| match e {
                fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn locales_define_the_same_messages() {
        assert_eq!(ids(EN_FTL), ids(FR_FTL));
        assert!(!ids(EN_FTL).is_empty());
    }

    #[test]
    fn cli_wins_over_environment() {
        assert_eq!(negotiate(Some("fr"), &env(&[("LANG", "en_US.UTF-8")])), Lang::Fr);
        assert_eq!(negotiate(Some("EN"), &env(&[("LC_ALL", "fr_FR.UTF-8")])), Lang::En);
    }

    #[test]
    fn environment_precedence() {
        let all = [("LC_ALL", "fr_FR.UTF-8"), ("LC_MESSAGES", "en_US.UTF-8"), ("LANG", "en_US.UTF-8")];
        assert_eq!(negotiate(None, &env(&all)), Lang::Fr);
        let msg = [("LC_MESSAGES", "fr_BE.UTF-8"), ("LANG", "en_US.UTF-8")];
        assert_eq!(negotiate(None, &env(&msg)), Lang::Fr);
        assert_eq!(negotiate(None, &env(&[("LANG", "fr_CA@euro")])), Lang::Fr);
        // An empty LC_ALL is ignored.
        assert_eq!(negotiate(None, &env(&[("LC_ALL", ""), ("LANG", "fr_FR")])), Lang::Fr);
        // The first set variable decides even if it is not supported.
        assert_eq!(negotiate(None, &env(&[("LC_ALL", "de_DE.UTF-8"), ("LANG", "fr_FR")])), Lang::En);
    }

    #[test]
    fn fallback_is_english() {
        assert_eq!(negotiate(None, &env(&[])), Lang::En);
        assert_eq!(negotiate(None, &env(&[("LANG", "C")])), Lang::En);
        assert_eq!(negotiate(None, &env(&[("LANG", "POSIX")])), Lang::En);
        assert_eq!(negotiate(None, &env(&[("LANG", "ja_JP.UTF-8")])), Lang::En);
        assert_eq!(negotiate(None, &env(&[("LANG", "%%garbage")])), Lang::En);
        // An unsupported --lang falls through to the environment.
        assert_eq!(negotiate(Some("de"), &env(&[("LANG", "fr_FR")])), Lang::Fr);
    }

    #[test]
    fn explicit_parse() {
        assert_eq!(Lang::parse("fr").unwrap(), Lang::Fr);
        assert_eq!(Lang::parse("fr-FR").unwrap(), Lang::Fr);
        assert_eq!(Lang::parse("EN").unwrap(), Lang::En);
        assert!(matches!(Lang::parse("de"), Err(I18nError::UnsupportedLanguage(_))));
        assert!(Lang::parse("C").is_err());
        assert!(Lang::parse("").is_err());
    }

    #[test]
    fn messages_differ_by_language() {
        let en = Localizer::new(Lang::En).unwrap();
        let fr = Localizer::new(Lang::Fr).unwrap();
        assert_eq!(en.lang(), Lang::En);
        assert_ne!(en.message("cli-about"), fr.message("cli-about"));
        assert_eq!(fr.message("cli-about"), "Capturer, annoter et envoyer des captures d'écran.");
        assert_eq!(fr.message("cli-usage-heading"), "Utilisation :");
    }

    #[test]
    fn unknown_id_returns_the_id() {
        let fr = Localizer::new(Lang::Fr).unwrap();
        assert_eq!(fr.message("does-not-exist"), "does-not-exist");
    }
}
