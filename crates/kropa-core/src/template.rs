// SPDX-License-Identifier: GPL-3.0-or-later

//! File-name templates.
//!
//! A template is literal text with `{variable}` or `{variable:argument}` placeholders; `{{` and `}}`
//! produce a literal brace.
//!
//! | Variable | Value |
//! |---|---|
//! | `{date}` / `{date:FORMAT}` | local date, default `%Y-%m-%d` |
//! | `{time}` / `{time:FORMAT}` | local time, default `%H-%M-%S` |
//! | `{n}` / `{n:WIDTH}` | counter, zero-padded to `WIDTH` digits (1 to 9, default 1) |
//! | `{w}`, `{h}` | image width and height in pixels |
//! | `{kind}` (alias `{mode}`) | capture kind (`screen`, `output`, `area`) |
//! | `{output}` | output (screen) name |
//! | `{host}` | host name |
//!
//! `FORMAT` is a `strftime` pattern (for example `{date:%Y%m%d}`). Substituted values never carry a
//! separator or a control character: they are sanitised, then the whole name is validated by
//! [`validate_file_name`], so a rendered name is always a single, portable path component.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::{Clock, NameError};

const DEFAULT_DATE: &str = "%Y-%m-%d";
const DEFAULT_TIME: &str = "%H-%M-%S";
const MAX_NAME_BYTES: usize = 255;
const MAX_COUNTER_TRIES: u32 = 100_000;
const FORBIDDEN: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Values available to a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameContext {
    /// Capture kind (`{kind}`, `{mode}`).
    pub kind: Option<String>,
    /// Output name (`{output}`).
    pub output: Option<String>,
    /// Host name (`{host}`).
    pub host: Option<String>,
    /// Image width in pixels (`{w}`).
    pub width: Option<u32>,
    /// Image height in pixels (`{h}`).
    pub height: Option<u32>,
    /// Counter (`{n}`), first value tried by [`NameTemplate::render_unique`].
    pub counter: u32,
}

impl Default for NameContext {
    fn default() -> Self {
        Self {
            kind: None,
            output: None,
            host: None,
            width: None,
            height: None,
            counter: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Date(String),
    Time(String),
    Counter(usize),
    Width,
    Height,
    Kind,
    Output,
    Host,
}

/// A parsed, validated name template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameTemplate {
    source: String,
    parts: Vec<Part>,
}

impl NameTemplate {
    /// Parses `source`.
    ///
    /// # Errors
    /// [`NameError`] on a syntax error, an unknown variable or an invalid argument.
    pub fn parse(source: &str) -> Result<Self, NameError> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = source.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    literal.push('}');
                }
                '}' => {
                    return Err(NameError::Syntax(
                        "unmatched '}' (write '}}' for a literal brace)".into(),
                    ));
                }
                '{' => {
                    let mut body = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some('{') | None => {
                                return Err(NameError::Syntax(
                                    "unclosed '{' (write '{{' for a literal brace)".into(),
                                ));
                            }
                            Some(other) => body.push(other),
                        }
                    }
                    if !literal.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(parse_variable(&body)?);
                }
                other => literal.push(other),
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        let template = Self {
            source: source.to_owned(),
            parts,
        };
        template.check_sample()?;
        Ok(template)
    }

    /// Renders with benign sample values so that a template whose literal text can never give a
    /// safe name (`../x`, `a/b`, `CON`) is refused as soon as it is parsed.
    fn check_sample(&self) -> Result<(), NameError> {
        let sample = NameContext {
            kind: Some("screen".into()),
            output: Some("DP-1".into()),
            host: Some("host".into()),
            width: Some(1920),
            height: Some(1080),
            counter: 1,
        };
        let epoch =
            crate::FixedClock::new(jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC));
        self.render(&sample, &epoch, "png").map(|_| ())
    }

    /// The template text as written.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the template contains `{n}`.
    #[must_use]
    pub fn has_counter(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Counter(_)))
    }

    /// Renders the file name `<template>.<ext>`.
    ///
    /// # Errors
    /// [`NameError`] if a variable has no value, or if the result is not a safe file name.
    pub fn render(
        &self,
        ctx: &NameContext,
        clock: &dyn Clock,
        ext: &str,
    ) -> Result<String, NameError> {
        validate_extension(ext)?;
        let now = clock.now();
        let mut name = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => name.push_str(text),
                Part::Date(fmt) | Part::Time(fmt) => {
                    let text = jiff::fmt::strtime::format(fmt.as_str(), &now).map_err(|_| {
                        NameError::Syntax("the date or time format cannot be applied".into())
                    })?;
                    name.push_str(&sanitize(&text));
                }
                Part::Counter(width) => {
                    let _ = write!(name, "{:0width$}", ctx.counter);
                }
                Part::Width => name.push_str(&required(ctx.width, "w")?.to_string()),
                Part::Height => name.push_str(&required(ctx.height, "h")?.to_string()),
                Part::Kind => name.push_str(&sanitize(required(ctx.kind.as_deref(), "kind")?)),
                Part::Output => {
                    name.push_str(&sanitize(required(ctx.output.as_deref(), "output")?));
                }
                Part::Host => name.push_str(&sanitize(required(ctx.host.as_deref(), "host")?)),
            }
        }
        name.push('.');
        name.push_str(ext);
        validate_file_name(&name)?;
        Ok(name)
    }

    /// Renders the target path under `dir`. When the template has `{n}`, the counter is increased
    /// from `ctx.counter` until the path does not exist yet; otherwise the first rendering is returned
    /// and the caller decides what to do if the file exists (`--force`).
    ///
    /// # Errors
    /// [`NameError`] as [`NameTemplate::render`], or [`NameError::Exhausted`].
    pub fn render_unique(
        &self,
        dir: &Path,
        ctx: &NameContext,
        clock: &dyn Clock,
        ext: &str,
    ) -> Result<PathBuf, NameError> {
        let mut ctx = ctx.clone();
        if !self.has_counter() {
            return Ok(dir.join(self.render(&ctx, clock, ext)?));
        }
        for _ in 0..MAX_COUNTER_TRIES {
            let path = dir.join(self.render(&ctx, clock, ext)?);
            if !path.try_exists().unwrap_or(true) {
                return Ok(path);
            }
            ctx.counter = ctx.counter.checked_add(1).ok_or(NameError::Exhausted)?;
        }
        Err(NameError::Exhausted)
    }
}

fn required<T>(value: Option<T>, name: &'static str) -> Result<T, NameError> {
    value.ok_or(NameError::MissingVariable(name))
}

fn parse_variable(body: &str) -> Result<Part, NameError> {
    let (name, arg) = match body.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (body, None),
    };
    let no_arg = |part: Part| match arg {
        None => Ok(part),
        Some(_) => Err(NameError::BadArgument {
            variable: name.to_owned(),
            reason: "this variable takes no argument".into(),
        }),
    };
    match name {
        "date" => Ok(Part::Date(check_format(name, arg.unwrap_or(DEFAULT_DATE))?)),
        "time" => Ok(Part::Time(check_format(name, arg.unwrap_or(DEFAULT_TIME))?)),
        "n" => {
            let width = match arg {
                None => 1,
                Some(a) => a
                    .parse::<usize>()
                    .ok()
                    .filter(|w| (1..=9).contains(w))
                    .ok_or_else(|| NameError::BadArgument {
                        variable: "n".into(),
                        reason: "width must be a number from 1 to 9".into(),
                    })?,
            };
            Ok(Part::Counter(width))
        }
        "w" => no_arg(Part::Width),
        "h" => no_arg(Part::Height),
        "kind" | "mode" => no_arg(Part::Kind),
        "output" => no_arg(Part::Output),
        "host" => no_arg(Part::Host),
        other => Err(NameError::UnknownVariable(other.to_owned())),
    }
}

fn check_format(variable: &str, fmt: &str) -> Result<String, NameError> {
    if fmt.is_empty() {
        return Err(NameError::BadArgument {
            variable: variable.into(),
            reason: "empty format".into(),
        });
    }
    let probe = jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC);
    jiff::fmt::strtime::format(fmt, &probe).map_err(|_| NameError::BadArgument {
        variable: variable.into(),
        reason: "not a valid strftime format".into(),
    })?;
    Ok(fmt.to_owned())
}

/// Replaces separators, reserved characters and control characters of a substituted value by `_`.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn validate_extension(ext: &str) -> Result<(), NameError> {
    if (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(NameError::UnsafeName(
            "extension must be 1 to 8 ASCII letters or digits",
        ))
    }
}

/// Checks that `name` is a single, portable path component: it cannot leave the folder it is joined to.
///
/// Refused: empty names, more than 255 bytes, control characters, `/ \ < > : " | ? *`, any `..`,
/// a leading dot or space, a trailing dot or space, and Windows reserved device names.
///
/// # Errors
/// [`NameError::UnsafeName`] with the reason.
pub fn validate_file_name(name: &str) -> Result<(), NameError> {
    let bad = |why| Err(NameError::UnsafeName(why));
    if name.is_empty() {
        return bad("empty name");
    }
    if name.len() > MAX_NAME_BYTES {
        return bad("name longer than 255 bytes");
    }
    if name.chars().any(char::is_control) {
        return bad("control character");
    }
    if name.contains(FORBIDDEN) {
        return bad("path separator or reserved character");
    }
    if name.contains("..") {
        return bad("'..' sequence");
    }
    if name.starts_with(['.', ' ']) {
        return bad("leading dot or space");
    }
    if name.ends_with(['.', ' ']) || name.contains(" .") {
        return bad("trailing dot or space");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return bad("reserved device name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Component, Path};

    use proptest::prelude::*;

    use super::{NameContext, NameTemplate, validate_file_name};
    use crate::{FixedClock, NameError};

    fn clock() -> FixedClock {
        FixedClock::new("2026-11-11T09:08:07+01:00[+01:00]".parse().unwrap())
    }

    fn full_ctx() -> NameContext {
        NameContext {
            kind: Some("screen".into()),
            output: Some("DP-1".into()),
            host: Some("box".into()),
            width: Some(1920),
            height: Some(1080),
            counter: 7,
        }
    }

    fn render(src: &str) -> Result<String, NameError> {
        NameTemplate::parse(src)?.render(&full_ctx(), &clock(), "png")
    }

    #[test]
    fn renders_variables() {
        assert_eq!(render("{date}_{time}").unwrap(), "2026-11-11_09-08-07.png");
        assert_eq!(render("{date:%Y%m%d}-{host}").unwrap(), "20261111-box.png");
        assert_eq!(
            render("{kind}-{mode}-{output}-{w}x{h}").unwrap(),
            "screen-screen-DP-1-1920x1080.png"
        );
        assert_eq!(render("shot-{n}").unwrap(), "shot-7.png");
        assert_eq!(render("shot-{n:4}").unwrap(), "shot-0007.png");
    }

    #[test]
    fn braces_are_escaped() {
        assert_eq!(render("{{date}}").unwrap(), "{date}.png");
        assert_eq!(render("a}}b{{c").unwrap(), "a}b{c.png");
    }

    #[test]
    fn parse_errors() {
        assert!(matches!(
            NameTemplate::parse("{date"),
            Err(NameError::Syntax(_))
        ));
        assert!(matches!(
            NameTemplate::parse("a}"),
            Err(NameError::Syntax(_))
        ));
        assert!(
            matches!(NameTemplate::parse("{nope}"), Err(NameError::UnknownVariable(v)) if v == "nope")
        );
        assert!(matches!(
            NameTemplate::parse("{n:0}"),
            Err(NameError::BadArgument { .. })
        ));
        assert!(matches!(
            NameTemplate::parse("{n:10}"),
            Err(NameError::BadArgument { .. })
        ));
        assert!(matches!(
            NameTemplate::parse("{w:3}"),
            Err(NameError::BadArgument { .. })
        ));
        assert!(matches!(
            NameTemplate::parse("{date:}"),
            Err(NameError::BadArgument { .. })
        ));
    }

    #[test]
    fn missing_variable_is_explicit() {
        let t = NameTemplate::parse("{host}").unwrap();
        let err = t
            .render(&NameContext::default(), &clock(), "png")
            .unwrap_err();
        assert_eq!(err, NameError::MissingVariable("host"));
    }

    #[test]
    fn substituted_values_are_sanitised() {
        let t = NameTemplate::parse("{host}").unwrap();
        let ctx = NameContext {
            host: Some("a/b\\c\nd".into()),
            ..NameContext::default()
        };
        assert_eq!(t.render(&ctx, &clock(), "png").unwrap(), "a_b_c_d.png");
        let ctx = NameContext {
            host: Some("..".into()),
            ..NameContext::default()
        };
        assert!(t.render(&ctx, &clock(), "png").is_err());
        // A date format cannot smuggle a separator either.
        assert_eq!(render("{date:%Y/%m}").unwrap(), "2026_11.png");
    }

    #[test]
    fn dangerous_templates_are_refused() {
        for src in [
            "../x",
            "a/b",
            "/etc/passwd",
            "a\\b",
            "C:x",
            ".hidden",
            "x\u{0}y",
            "x\ny",
            "CON",
            "nul",
            "a..b",
            "x ",
            "",
        ] {
            assert!(render(src).is_err(), "{src:?} should be refused");
        }
        assert!(render("COM1").is_err());
        assert!(render("lpt9").is_err());
        assert!(
            NameTemplate::parse("a")
                .unwrap()
                .render(&full_ctx(), &clock(), "p/ng")
                .is_err()
        );
        assert!(
            NameTemplate::parse("a")
                .unwrap()
                .render(&full_ctx(), &clock(), "")
                .is_err()
        );
    }

    #[test]
    fn long_names_are_refused() {
        assert!(render(&"a".repeat(300)).is_err());
        assert!(validate_file_name(&"a".repeat(255)).is_ok());
    }

    #[test]
    fn counter_skips_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let t = NameTemplate::parse("s-{n}").unwrap();
        let ctx = NameContext::default();
        std::fs::write(dir.path().join("s-1.png"), b"").unwrap();
        std::fs::write(dir.path().join("s-2.png"), b"").unwrap();
        let path = t.render_unique(dir.path(), &ctx, &clock(), "png").unwrap();
        assert_eq!(path, dir.path().join("s-3.png"));
    }

    #[test]
    fn without_counter_first_rendering_is_returned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s.png"), b"").unwrap();
        let t = NameTemplate::parse("s").unwrap();
        let path = t
            .render_unique(dir.path(), &NameContext::default(), &clock(), "png")
            .unwrap();
        assert_eq!(path, dir.path().join("s.png"));
    }

    fn assert_stays_in(dir: &Path, name: &str) {
        let joined = dir.join(name);
        assert_eq!(joined.parent(), Some(dir));
        let last = joined.components().next_back().unwrap();
        assert!(matches!(last, Component::Normal(_)), "{name:?}");
        assert_eq!(
            joined.components().count(),
            dir.components().count() + 1,
            "{name:?}"
        );
        assert_eq!(joined.file_name().unwrap().to_str(), Some(name));
    }

    proptest! {
        #[test]
        fn accepted_file_names_stay_in_the_folder(name in "\\PC{0,40}") {
            if validate_file_name(&name).is_ok() {
                assert_stays_in(Path::new("/target/dir"), &name);
            }
        }

        #[test]
        fn accepted_file_names_stay_in_the_folder_dangerous_alphabet(name in "[a-z./\\\\:. \u{0}\n~-]{0,12}") {
            if validate_file_name(&name).is_ok() {
                assert_stays_in(Path::new("/target/dir"), &name);
            }
        }

        #[test]
        fn rendered_names_never_escape(
            src in "[a-z{}:%YmdHMS./\\\\ ~-]{0,24}",
            host in "\\PC{0,16}",
            output in "[a-z./\\\\:\u{0}-]{0,8}",
            counter in 0u32..1_000_000,
        ) {
            let ctx = NameContext {
                kind: Some("screen".into()),
                output: Some(output),
                host: Some(host),
                width: Some(1),
                height: Some(1),
                counter,
            };
            if let Ok(t) = NameTemplate::parse(&src)
                && let Ok(name) = t.render(&ctx, &clock(), "png")
            {
                assert_stays_in(Path::new("/target/dir"), &name);
                prop_assert!(validate_file_name(&name).is_ok());
            }
        }

        #[test]
        fn arbitrary_templates_never_panic_nor_escape(src in "\\PC{0,40}") {
            if let Ok(t) = NameTemplate::parse(&src)
                && let Ok(name) = t.render(&NameContext::default(), &clock(), "png")
            {
                assert_stays_in(Path::new("/target/dir"), &name);
            }
        }
    }
}
