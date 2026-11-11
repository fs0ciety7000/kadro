// SPDX-License-Identifier: GPL-3.0-or-later

//! Templates of the destination files (`docs/uploads.md` section 2.3), in the syntax of ShareX:
//! `{function:arg|arg}`.
//!
//! Request side: `{filename}`, `{input}` (the file name too), `{base64:text}`, `{secret:name}`
//! (Kropa's own) and `{var:name}` (a variable answered with `--var`). Response side: `{response}`,
//! `{responseurl}`, `{header:name}`, `{json:path}`, `{json:input|path}` and `{regex:pattern|group}`.
//!
//! A brace that does not open one of these functions is plain text, which keeps JSON and XML body
//! templates readable. The functions of ShareX that Kropa does not provide (`{xml:}`, `{random:}`,
//! `{select:}`, `{inputbox:}`, `{outputbox:}`...) are refused with their name, never ignored.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::error::UploadError;

/// Functions of ShareX that Kropa does not implement: a destination that uses one is refused.
pub const UNSUPPORTED_FUNCTIONS: [&str; 7] = [
    "xml",
    "random",
    "select",
    "inputbox",
    "outputbox",
    "prompt",
    "cmd",
];

/// Where a template is used: which functions it may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Header, query, field or body of the request.
    Request,
    /// `url`, `thumbnail_url`, `deletion_url`, `error` of the response.
    Response,
}

/// A function of the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Function {
    /// `{filename}` and `{input}`.
    Filename,
    /// `{secret:name}`.
    Secret,
    /// `{var:name}`.
    Var,
    /// `{base64:text}`.
    Base64,
    /// `{response}`.
    Response,
    /// `{responseurl}`.
    ResponseUrl,
    /// `{header:name}`.
    Header,
    /// `{json:path}`.
    Json,
    /// `{regex:pattern|group}`.
    Regex,
}

impl Function {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "filename" | "input" => Self::Filename,
            "secret" => Self::Secret,
            "var" => Self::Var,
            "base64" => Self::Base64,
            "response" => Self::Response,
            "responseurl" => Self::ResponseUrl,
            "header" => Self::Header,
            "json" => Self::Json,
            "regex" => Self::Regex,
            _ => return None,
        })
    }

    fn allowed(self, side: Side) -> bool {
        match self {
            Self::Filename | Self::Var => true,
            Self::Secret | Self::Base64 => side == Side::Request,
            Self::Response | Self::ResponseUrl | Self::Header | Self::Json | Self::Regex => {
                side == Side::Response
            }
        }
    }
}

/// Which capture group of a `{regex:}` is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    /// By number (`0` is the whole match).
    Index(usize),
    /// By name.
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Call(Call),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    function: Function,
    /// Arguments, each a template (split on top-level `|`). Empty for `{regex:}`.
    args: Vec<Template>,
    /// `{regex:}`: the pattern and the group.
    regex: Option<(String, Group)>,
}

/// A parsed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    nodes: Vec<Node>,
    source: String,
}

/// What the evaluator of a template provides.
pub trait Env {
    /// The value of a function other than `{base64:}`; `args` are already evaluated.
    ///
    /// # Errors
    /// Whatever the environment refuses (an unknown secret, a missing variable...).
    fn call(
        &self,
        function: Function,
        args: &[String],
        regex: Option<&(String, Group)>,
    ) -> Result<String, UploadError>;
}

impl Template {
    /// The text the template was parsed from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Parses `source` for `side`.
    ///
    /// # Errors
    /// [`UploadError::Config`] on an unbalanced function, a function that is not allowed on this
    /// side or one that Kropa does not provide.
    pub fn parse(source: &str, side: Side) -> Result<Self, UploadError> {
        parse_nodes(source, side).map(|nodes| Self {
            nodes,
            source: source.to_owned(),
        })
    }

    /// Evaluates the template.
    ///
    /// # Errors
    /// What `env` refuses.
    pub fn render(&self, env: &dyn Env) -> Result<String, UploadError> {
        let mut out = String::new();
        for node in &self.nodes {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Call(call) => {
                    let args = call
                        .args
                        .iter()
                        .map(|a| a.render(env))
                        .collect::<Result<Vec<_>, _>>()?;
                    if call.function == Function::Base64 {
                        out.push_str(&STANDARD.encode(args.concat()));
                    } else {
                        out.push_str(&env.call(call.function, &args, call.regex.as_ref())?);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Every call of `function`, with its first argument in clear when it is a literal.
    #[must_use]
    pub fn literal_args(&self, function: Function) -> Vec<String> {
        let mut found = Vec::new();
        self.walk(&mut |call| {
            if call.function == function {
                if let Some(first) = call.args.first() {
                    found.push(first.source.clone());
                } else if let Some((pattern, _)) = &call.regex {
                    found.push(pattern.clone());
                }
            }
        });
        found
    }

    /// Names of the secrets the template refers to.
    #[must_use]
    pub fn secrets(&self) -> Vec<String> {
        self.literal_args(Function::Secret)
    }

    /// Names of the variables the template refers to.
    #[must_use]
    pub fn vars(&self) -> Vec<String> {
        self.literal_args(Function::Var)
    }

    /// `{regex:}` patterns of the template.
    #[must_use]
    pub fn regexes(&self) -> Vec<String> {
        self.literal_args(Function::Regex)
    }

    /// `{json:}` paths of the template (the last argument).
    #[must_use]
    pub fn json_paths(&self) -> Vec<String> {
        let mut found = Vec::new();
        self.walk(&mut |call| {
            if call.function == Function::Json
                && let Some(last) = call.args.last()
            {
                found.push(last.source.clone());
            }
        });
        found
    }

    /// Names of the headers read by `{header:}`.
    #[must_use]
    pub fn header_reads(&self) -> Vec<String> {
        self.literal_args(Function::Header)
    }

    /// Whether the template has no function at all.
    #[must_use]
    pub fn is_literal(&self) -> bool {
        self.nodes.iter().all(|n| matches!(n, Node::Text(_)))
    }

    /// Whether every node is a function call and nothing else but blanks (a "pure template" such
    /// as `{filename}`).
    #[must_use]
    pub fn is_pure_call(&self) -> bool {
        !self.nodes.is_empty()
            && self.nodes.iter().all(|n| match n {
                Node::Call(_) => true,
                Node::Text(t) => t.trim().is_empty(),
            })
    }

    fn walk(&self, f: &mut dyn FnMut(&Call)) {
        for node in &self.nodes {
            if let Node::Call(call) = node {
                f(call);
                for arg in &call.args {
                    arg.walk(f);
                }
            }
        }
    }
}

fn syntax(detail: impl Into<String>) -> UploadError {
    UploadError::config(detail)
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

/// Index of the `}` that closes the function whose `{` is at `open`, or `None`. A backslash
/// protects the next character (`\{` in a regular expression).
fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Splits on `|` outside braces and not escaped.
fn split_args(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&text[start..]);
    parts
}

fn parse_regex(arg: &str) -> Result<(String, Group), UploadError> {
    let (pattern, group) = match arg.rsplit_once('|') {
        Some((head, tail))
            if !head.is_empty()
                && !tail.is_empty()
                && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
        {
            let group = tail
                .parse::<usize>()
                .map_or_else(|_| Group::Name(tail.to_owned()), Group::Index);
            (head, Some(group))
        }
        _ => (arg, None),
    };
    if pattern.is_empty() {
        return Err(syntax("{regex:} needs a pattern"));
    }
    // Without a group, the first capture group when the pattern has one, else the whole match.
    let group = group.unwrap_or_else(|| {
        if pattern.contains('(') && !pattern.contains("(?:") {
            Group::Index(1)
        } else {
            Group::Index(0)
        }
    });
    Ok((pattern.to_owned(), group))
}

fn parse_nodes(source: &str, side: Side) -> Result<Vec<Node>, UploadError> {
    let bytes = source.as_bytes();
    let mut nodes = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            let ch = source[i..].chars().next().unwrap_or('\u{fffd}');
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let name_end = bytes[i + 1..]
            .iter()
            .position(|b| !is_name_char(*b))
            .map_or(bytes.len(), |p| i + 1 + p);
        let name = &source[i + 1..name_end];
        let delimiter = bytes.get(name_end).copied();
        let function = Function::from_name(name);
        let known = function.is_some() || UNSUPPORTED_FUNCTIONS.contains(&name);
        if name.is_empty() || !known || !matches!(delimiter, Some(b':' | b'}')) {
            text.push('{');
            i += 1;
            continue;
        }
        if UNSUPPORTED_FUNCTIONS.contains(&name) {
            return Err(syntax(format!(
                "{{{name}}} is not supported by Kropa (docs/uploads.md section 2.4)"
            )));
        }
        let function = function.ok_or_else(|| syntax("unknown function"))?;
        if !function.allowed(side) {
            return Err(syntax(format!(
                "{{{name}}} cannot be used {}",
                match side {
                    Side::Request => "in a request",
                    Side::Response => "in a response template",
                }
            )));
        }
        let close =
            matching_brace(bytes, i).ok_or_else(|| syntax(format!("{{{name}}}: missing '}}'")))?;
        let inner = if delimiter == Some(b':') {
            &source[name_end + 1..close]
        } else {
            ""
        };
        let call = build_call(name, function, inner, delimiter == Some(b':'), side)?;
        if !text.is_empty() {
            nodes.push(Node::Text(std::mem::take(&mut text)));
        }
        nodes.push(Node::Call(call));
        i = close + 1;
    }
    if !text.is_empty() {
        nodes.push(Node::Text(text));
    }
    Ok(nodes)
}

fn build_call(
    name: &str,
    function: Function,
    inner: &str,
    has_args: bool,
    side: Side,
) -> Result<Call, UploadError> {
    let arity = |min: usize, max: usize, n: usize| -> Result<(), UploadError> {
        if (min..=max).contains(&n) {
            Ok(())
        } else {
            Err(syntax(format!("{{{name}}} takes {min} to {max} arguments")))
        }
    };
    if function == Function::Regex {
        if !has_args {
            return Err(syntax("{regex:} needs a pattern"));
        }
        let regex = parse_regex(inner)?;
        return Ok(Call {
            function,
            args: Vec::new(),
            regex: Some(regex),
        });
    }
    let parts: Vec<&str> = if has_args {
        split_args(inner)
    } else {
        Vec::new()
    };
    match function {
        Function::Filename | Function::Response | Function::ResponseUrl => {
            arity(0, 0, parts.len())?;
        }
        Function::Secret | Function::Var | Function::Header | Function::Base64 => {
            arity(1, 1, parts.len())?;
        }
        Function::Json => arity(1, 2, parts.len())?,
        Function::Regex => {}
    }
    // The name of a secret, a variable or a header, and the path of a `{json:}`, are literal text:
    // a function inside them would let a response choose the secret that is read.
    let literal = matches!(
        function,
        Function::Secret | Function::Var | Function::Header
    );
    let args = parts
        .iter()
        .enumerate()
        .map(|(n, p)| {
            if literal || (function == Function::Json && n + 1 == parts.len()) {
                Ok(Template {
                    nodes: vec![Node::Text((*p).to_owned())],
                    source: (*p).to_owned(),
                })
            } else {
                Template::parse(p, side)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Call {
        function,
        args,
        regex: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Env, Function, Group, Side, Template};
    use crate::error::UploadError;

    struct Fixed;

    impl Env for Fixed {
        fn call(
            &self,
            function: Function,
            args: &[String],
            regex: Option<&(String, Group)>,
        ) -> Result<String, UploadError> {
            Ok(match function {
                Function::Filename => "shot.png".into(),
                Function::Secret => format!("<secret {}>", args[0]),
                Function::Var => format!("<var {}>", args[0]),
                Function::Json => format!("<json {}>", args.join("|")),
                Function::Regex => format!("<regex {:?}>", regex.unwrap()),
                Function::Header => format!("<header {}>", args[0]),
                Function::Response => "<body>".into(),
                Function::ResponseUrl => "<url>".into(),
                Function::Base64 => unreachable!("handled by the template"),
            })
        }
    }

    fn request(s: &str) -> String {
        Template::parse(s, Side::Request)
            .unwrap()
            .render(&Fixed)
            .unwrap()
    }

    fn response(s: &str) -> String {
        Template::parse(s, Side::Response)
            .unwrap()
            .render(&Fixed)
            .unwrap()
    }

    #[test]
    fn plain_text_and_unknown_braces_pass_through() {
        assert_eq!(request("hello"), "hello");
        assert_eq!(
            request(r#"{"a": "{filename}", "n": {"b": 1}}"#),
            r#"{"a": "shot.png", "n": {"b": 1}}"#
        );
        assert_eq!(request("{nope}{}"), "{nope}{}");
        assert_eq!(request("{filename"), "{filename");
    }

    #[test]
    fn calls_nest() {
        assert_eq!(request("Bearer {secret:token}"), "Bearer <secret token>");
        assert_eq!(
            request("{base64:user:{secret:pw}}"),
            "dXNlcjo8c2VjcmV0IHB3Pg=="
        );
        assert_eq!(request("{input}-{filename}"), "shot.png-shot.png");
        assert_eq!(request("{var:title}"), "<var title>");
    }

    #[test]
    fn response_functions() {
        assert_eq!(
            response("https://x/del/{json:$.data.token}"),
            "https://x/del/<json $.data.token>"
        );
        assert_eq!(response("{json:{header:x}|$.a}"), "<json <header x>|$.a>");
        assert_eq!(response("{responseurl}"), "<url>");
        assert_eq!(response("{response}"), "<body>");
    }

    #[test]
    fn regex_group_rules() {
        let get = |s: &str| {
            let t = Template::parse(s, Side::Response).unwrap();
            t.regexes()
        };
        assert_eq!(get("{regex:id=(\\d+)}"), ["id=(\\d+)"]);
        assert_eq!(get("{regex:a|b|1}"), ["a|b"]);
        assert_eq!(get("{regex:a{2,3}|0}"), ["a{2,3}"]);
        assert!(response("{regex:id=(\\d+)}").contains("Index(1)"));
        assert!(response("{regex:id=\\d+}").contains("Index(0)"));
        assert!(response("{regex:(?P<x>a)|x}").contains("Name(\"x\")"));
    }

    #[test]
    fn side_rules_and_unsupported_functions() {
        for (s, side) in [
            ("{secret:t}", Side::Response),
            ("{base64:x}", Side::Response),
            ("{json:$.a}", Side::Request),
            ("{responseurl}", Side::Request),
            ("{xml:/a}", Side::Response),
            ("{random:a|b}", Side::Request),
            ("{inputbox:t|d}", Side::Request),
            ("{select:a|b}", Side::Request),
            ("{json:$.a", Side::Response),
            ("{secret}", Side::Request),
            ("{filename:x}", Side::Request),
            ("{regex}", Side::Response),
        ] {
            assert!(
                matches!(Template::parse(s, side), Err(UploadError::Config { .. })),
                "{s}"
            );
        }
    }

    #[test]
    fn introspection() {
        let t = Template::parse("{base64:{secret:a}:{secret:b}} {var:v}", Side::Request).unwrap();
        assert_eq!(t.secrets(), ["a", "b"]);
        assert_eq!(t.vars(), ["v"]);
        assert!(!t.is_literal());
        assert!(
            Template::parse("plain", Side::Request)
                .unwrap()
                .is_literal()
        );
        assert!(
            Template::parse("{filename}", Side::Request)
                .unwrap()
                .is_pure_call()
        );
        assert!(
            !Template::parse("x{filename}", Side::Request)
                .unwrap()
                .is_pure_call()
        );
        let r = Template::parse("{json:$.a.b} {header:Location}", Side::Response).unwrap();
        assert_eq!(r.json_paths(), ["$.a.b"]);
        assert_eq!(r.header_reads(), ["Location"]);
    }

    #[test]
    fn a_secret_name_cannot_be_computed() {
        // The name is literal text: a nested call stays text and is not a secret reference.
        let t = Template::parse("{secret:{var:x}}", Side::Request);
        assert!(t.is_err() || t.unwrap().secrets() == ["{var:x}"]);
    }
}
