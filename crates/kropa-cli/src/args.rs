// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed command line of `docs/cli.md` section 2 (clap derive).
//!
//! Doc comments here are only a developer aid: every string the user sees (about, help, value names)
//! is replaced by a Fluent message in [`crate::command`]. Value parsers return an [`ArgError`], a
//! Fluent message id, so that even validation errors are localised.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use clap::{ArgAction, ArgGroup, Args, Parser, Subcommand, ValueEnum};
use kropa_core::{NameError, NameTemplate};
use kropa_i18n::Lang;

/// Longest delay accepted by `--delay`, in seconds.
pub const MAX_DELAY_SECS: f64 = 60.0;
/// Longest pre-signed URL lifetime accepted by `--presign`.
pub const MAX_PRESIGN: Duration = Duration::from_hours(7 * 24);

/// A refused argument value. `id` is the Fluent message that explains why; `detail` fills `$detail`.
#[derive(Debug, Clone)]
pub struct ArgError {
    /// Fluent message id.
    pub id: &'static str,
    /// Value for the `$detail` placeable (may be empty).
    pub detail: String,
}

impl ArgError {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            detail: String::new(),
        }
    }
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Only used when a message cannot be localised; the real text comes from Fluent.
        write!(f, "{}", self.id)
    }
}

impl std::error::Error for ArgError {}

/// Whole command line.
#[derive(Debug, Parser)]
#[command(
    name = "kropa",
    version,
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Options valid for every command.
    #[command(flatten)]
    pub global: GlobalArgs,
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Global options (`docs/cli.md` section 1).
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Machine-readable output.
    #[arg(long, global = true)]
    pub json: bool,
    /// Language of messages.
    #[arg(long, global = true, value_name = "LANG", value_parser = parse_lang)]
    pub lang: Option<String>,
    /// Fewer messages on stderr.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    /// More messages on stderr.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,
    /// Never prompt.
    #[arg(long, global = true)]
    pub no_input: bool,
    /// Colour of stderr output.
    #[arg(long, global = true, value_enum, default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,
    /// Configuration folder.
    #[arg(long, global = true, value_name = "DIR", value_parser = parse_path)]
    pub config: Option<PathBuf>,
    /// Descriptor holding the vault passphrase.
    #[arg(long, global = true, value_name = "N", value_parser = parse_fd)]
    pub password_fd: Option<i32>,
}

/// Values of `--color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Colour on a terminal, unless `NO_COLOR` is set.
    Auto,
    /// Always.
    Always,
    /// Never.
    Never,
}

/// Values of `--copy` for commands that may copy an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CopyTarget {
    /// The URL.
    Url,
    /// A Markdown link.
    Markdown,
    /// An HTML snippet.
    Html,
    /// The image itself.
    Image,
    /// Nothing.
    None,
}

/// Values of `--copy` for `upload`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LinkCopy {
    /// The URL.
    Url,
    /// A Markdown link.
    Markdown,
    /// An HTML snippet.
    Html,
    /// Nothing.
    None,
}

/// Destination types (`dest add --type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DestKind {
    /// Zipline server.
    Zipline,
    /// S3-compatible storage.
    S3,
    /// SFTP server.
    Sftp,
    /// Custom HTTP API.
    Http,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Take a screenshot.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Capture {
        /// What to capture.
        #[command(subcommand)]
        mode: CaptureCommand,
    },
    /// Upload an existing file.
    Upload(UploadArgs),
    /// Manage destinations.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Dest {
        /// Action.
        #[command(subcommand)]
        action: DestCommand,
    },
    /// Manage secrets.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Secrets {
        /// Action.
        #[command(subcommand)]
        action: SecretsCommand,
    },
    /// Manage the upload queue.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Queue {
        /// Action.
        #[command(subcommand)]
        action: QueueCommand,
    },
    /// Browse the history.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    History {
        /// Action.
        #[command(subcommand)]
        action: HistoryCommand,
    },
    /// Open the editor on an image.
    Edit(EditArgs),
    /// Report the system and its capabilities.
    Doctor,
    /// Print version information.
    Version(VersionArgs),
}

/// `capture` modes.
#[derive(Debug, Subcommand)]
pub enum CaptureCommand {
    /// All screens.
    Screen(CaptureOpts),
    /// One screen.
    Output {
        /// Connector name; the primary screen when absent.
        #[arg(value_name = "NAME", value_parser = parse_connector)]
        connector: Option<String>,
        /// Common options.
        #[command(flatten)]
        opts: CaptureOpts,
    },
    /// A selected area.
    Area {
        /// Reuse the last area.
        #[arg(long)]
        last: bool,
        /// Common options.
        #[command(flatten)]
        opts: CaptureOpts,
    },
}

/// Options common to the `capture` modes.
#[derive(Debug, Clone, Args)]
pub struct CaptureOpts {
    /// Wait before capturing.
    #[arg(long, value_name = "SEC", value_parser = parse_delay)]
    pub delay: Option<f64>,
    /// Destination folder or file.
    #[arg(long, value_name = "PATH", value_parser = parse_path)]
    pub output: Option<PathBuf>,
    /// File-name template.
    #[arg(long, value_name = "TEMPLATE", value_parser = parse_template)]
    pub name: Option<String>,
    /// Upload and copy options.
    #[command(flatten)]
    pub send: SendOpts,
    /// Open the editor first.
    #[arg(long)]
    pub edit: bool,
    /// Value of an import variable.
    #[arg(long = "var", id = "var", value_name = "NAME=VALUE", value_parser = parse_var)]
    pub vars: Vec<(String, String)>,
    /// Overwrite an existing file.
    #[arg(long)]
    pub force: bool,
}

/// `--upload`, `--no-upload` and `--copy`, shared by `capture` and `edit`.
#[derive(Debug, Clone, Args)]
pub struct SendOpts {
    /// Upload, to the given profile or the default one.
    #[arg(
        long,
        num_args = 0..=1,
        require_equals = true,
        value_name = "PROFILE",
        value_parser = parse_profile,
        conflicts_with = "no_upload"
    )]
    pub upload: Option<Option<String>>,
    /// Never upload.
    #[arg(long)]
    pub no_upload: bool,
    /// What goes to the clipboard.
    #[arg(long, value_enum, value_name = "WHAT")]
    pub copy: Option<CopyTarget>,
}

/// `upload`.
#[derive(Debug, Args)]
pub struct UploadArgs {
    /// File to upload.
    #[arg(value_name = "FILE", value_parser = parse_path)]
    pub file: PathBuf,
    /// Destination profile.
    #[arg(long, value_name = "PROFILE", value_parser = parse_profile)]
    pub to: Option<String>,
    /// What goes to the clipboard.
    #[arg(long, value_enum, value_name = "WHAT")]
    pub copy: Option<LinkCopy>,
    /// Value of an import variable.
    #[arg(long = "var", id = "var", value_name = "NAME=VALUE", value_parser = parse_var)]
    pub vars: Vec<(String, String)>,
    /// Answer yes to confirmations.
    #[arg(long)]
    pub yes: bool,
}

/// `edit`.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Image to edit.
    #[arg(value_name = "FILE", value_parser = parse_path)]
    pub file: PathBuf,
    /// Upload and copy options.
    #[command(flatten)]
    pub send: SendOpts,
}

/// `version`.
#[derive(Debug, Args)]
pub struct VersionArgs {
    /// Ask GitHub Releases for a newer version.
    #[arg(long)]
    pub check: bool,
}

/// `dest` actions.
#[derive(Debug, Subcommand)]
pub enum DestCommand {
    /// List destinations.
    List,
    /// Show one destination.
    Show {
        /// Profile identifier.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        id: String,
    },
    /// Add a destination.
    Add(Box<DestAddArgs>),
    /// Edit a destination.
    Edit {
        /// Profile identifier.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        id: String,
        /// New base URL.
        #[arg(long, value_name = "URL", value_parser = parse_url)]
        url: Option<String>,
        /// Set a profile key.
        #[arg(long = "set", id = "set", value_name = "KEY=VALUE", value_parser = parse_set)]
        sets: Vec<(String, String)>,
    },
    /// Test a destination.
    Test {
        /// Profile identifier.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        id: String,
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
    },
    /// Remove a destination.
    Remove {
        /// Profile identifier.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        id: String,
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
    },
    /// Choose the default destination.
    #[command(group(ArgGroup::new("target").required(true).args(["id", "none"])))]
    SetDefault {
        /// Profile identifier.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        id: Option<String>,
        /// Have no default destination.
        #[arg(long)]
        none: bool,
    },
    /// Import a `.sxcu` or TOML profile.
    Import {
        /// File to import.
        #[arg(value_name = "FILE", value_parser = parse_path)]
        file: PathBuf,
        /// Identifier for the imported profile.
        #[arg(long = "id", id = "as_id", value_name = "ID", value_parser = parse_profile_required)]
        as_id: Option<String>,
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
        /// Host the file must target.
        #[arg(long, value_name = "HOST", value_parser = parse_host)]
        expect_host: Option<String>,
    },
    /// Export profiles as TOML.
    Export {
        /// Profiles to export; all when empty.
        #[arg(value_name = "ID", value_parser = parse_profile_required)]
        ids: Vec<String>,
    },
}

/// `dest add`.
#[derive(Debug, Args)]
pub struct DestAddArgs {
    /// Profile identifier.
    #[arg(value_name = "ID", value_parser = parse_profile_required)]
    pub id: String,
    /// Destination type.
    #[arg(
        long = "type",
        id = "kind",
        value_enum,
        required = true,
        value_name = "TYPE"
    )]
    pub kind: DestKind,
    /// Base URL.
    #[arg(long, value_name = "URL", value_parser = parse_url)]
    pub url: Option<String>,
    /// Make it the default destination.
    #[arg(long = "default", id = "make_default")]
    pub make_default: bool,
    /// Type-specific options.
    #[command(flatten)]
    pub type_opts: TypeOpts,
    /// Where the secret comes from.
    #[command(flatten)]
    pub secret: SecretSource,
}

/// Options that only make sense for some destination types.
#[derive(Debug, Clone, Args)]
pub struct TypeOpts {
    /// S3 bucket.
    #[arg(long, value_name = "BUCKET")]
    pub bucket: Option<String>,
    /// S3 region.
    #[arg(long, value_name = "REGION")]
    pub region: Option<String>,
    /// S3 endpoint URL.
    #[arg(long, value_name = "URL", value_parser = parse_url)]
    pub endpoint: Option<String>,
    /// S3 path-style addressing.
    #[arg(long)]
    pub path_style: bool,
    /// S3 key prefix.
    #[arg(long, value_name = "PREFIX")]
    pub prefix: Option<String>,
    /// Public base URL of the bucket.
    #[arg(long, value_name = "URL", value_parser = parse_url)]
    pub public_base_url: Option<String>,
    /// Lifetime of pre-signed URLs.
    #[arg(long, value_name = "DURATION", value_parser = parse_presign)]
    pub presign: Option<Duration>,
    /// SFTP host.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// SFTP port.
    #[arg(long, value_name = "PORT", value_parser = parse_port)]
    pub port: Option<u16>,
    /// SFTP user.
    #[arg(long, value_name = "USER")]
    pub user: Option<String>,
    /// SFTP remote folder.
    #[arg(long, value_name = "DIR")]
    pub remote_dir: Option<String>,
    /// Public base URL of the SFTP folder.
    #[arg(long, value_name = "URL", value_parser = parse_url)]
    pub public_url: Option<String>,
}

/// How a secret is provided when there is no terminal.
#[derive(Debug, Clone, Args)]
pub struct SecretSource {
    /// Read the secret on stdin.
    #[arg(long, conflicts_with = "secret_fd")]
    pub secret_stdin: bool,
    /// Read the secret on a descriptor.
    #[arg(long, value_name = "N", value_parser = parse_fd)]
    pub secret_fd: Option<i32>,
}

/// `secrets` actions.
#[derive(Debug, Subcommand)]
pub enum SecretsCommand {
    /// Show the secret store and missing secrets.
    Status,
    /// Set a secret.
    Set {
        /// Profile identifier.
        #[arg(value_name = "PROFILE", value_parser = parse_profile_required)]
        profile: String,
        /// Secret name.
        #[arg(value_name = "NAME", value_parser = parse_secret_name)]
        secret: String,
        /// Where the secret comes from.
        #[command(flatten)]
        source: SecretSource,
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
    },
    /// Remove a secret.
    Remove {
        /// Profile identifier.
        #[arg(value_name = "PROFILE", value_parser = parse_profile_required)]
        profile: String,
        /// Secret name.
        #[arg(value_name = "NAME", value_parser = parse_secret_name)]
        secret: String,
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
    },
    /// Authorize a secret for another origin or place.
    #[command(group(ArgGroup::new("what").required(true).args(["origin", "placement"])))]
    Allow {
        /// Profile identifier.
        #[arg(value_name = "PROFILE", value_parser = parse_profile_required)]
        profile: String,
        /// Secret name.
        #[arg(value_name = "NAME", value_parser = parse_secret_name)]
        secret: String,
        /// Origin the secret may also be sent to.
        #[arg(long, value_name = "URL", value_parser = parse_url)]
        origin: Option<String>,
        /// Place the secret may also be put in.
        #[arg(long, value_name = "PLACE", value_parser = parse_placement)]
        placement: Option<String>,
        /// Has no effect: this confirmation is never given without a terminal.
        #[arg(long, hide = true)]
        yes: bool,
    },
    /// Unlock the vault.
    Unlock,
    /// Lock the vault.
    Lock,
}

/// `queue` actions.
#[derive(Debug, Subcommand)]
pub enum QueueCommand {
    /// List jobs.
    List {
        /// Include finished jobs.
        #[arg(long)]
        all: bool,
    },
    /// Retry one job or all failed ones.
    #[command(group(ArgGroup::new("target").required(true).args(["job", "failed"])))]
    Retry {
        /// Job identifier.
        #[arg(value_name = "ID", value_parser = parse_job)]
        job: Option<String>,
        /// Retry every failed job.
        #[arg(long)]
        failed: bool,
    },
    /// Cancel a job.
    Cancel {
        /// Job identifier.
        #[arg(value_name = "ID", value_parser = parse_job)]
        job: String,
    },
    /// Remove finished jobs.
    Clear {
        /// Answer yes to confirmations.
        #[arg(long)]
        yes: bool,
    },
}

/// `history` actions.
#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    /// List captures and uploads.
    List {
        /// Text to search for.
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Maximum number of lines.
        #[arg(long, value_name = "N", value_parser = parse_limit)]
        limit: Option<u32>,
        /// Only entries from this date.
        #[arg(long, value_name = "DATE", value_parser = parse_since)]
        since: Option<String>,
    },
}

// ---------------------------------------------------------------------------------------------
// Value parsers
// ---------------------------------------------------------------------------------------------

fn parse_lang(value: &str) -> Result<String, ArgError> {
    Lang::parse(value)
        .map(|l| l.code().to_owned())
        .map_err(|_| ArgError::new("err-lang"))
}

fn parse_path(value: &str) -> Result<PathBuf, ArgError> {
    if value.is_empty() {
        Err(ArgError::new("err-path"))
    } else {
        Ok(PathBuf::from(value))
    }
}

fn is_identifier(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A profile name, or empty (`--upload=` is the default profile).
fn parse_profile(value: &str) -> Result<String, ArgError> {
    if value.is_empty() || is_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-profile"))
    }
}

fn parse_profile_required(value: &str) -> Result<String, ArgError> {
    if is_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-profile"))
    }
}

fn parse_secret_name(value: &str) -> Result<String, ArgError> {
    if is_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-secret-name"))
    }
}

/// A place in the notation of `docs/cli.md`; what it means is checked when it is used, and a host or a
/// path is refused there (code 11), so that the value is never repeated in a usage error.
fn parse_placement(value: &str) -> Result<String, ArgError> {
    if !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-placement"))
    }
}

fn parse_job(value: &str) -> Result<String, ArgError> {
    if is_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-job"))
    }
}

fn parse_connector(value: &str) -> Result<String, ArgError> {
    if !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control) {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-connector"))
    }
}

/// `0` to [`MAX_DELAY_SECS`] seconds, decimals accepted.
fn parse_delay(value: &str) -> Result<f64, ArgError> {
    let seconds: f64 = value.parse().map_err(|_| ArgError::new("err-delay"))?;
    if seconds.is_finite() && (0.0..=MAX_DELAY_SECS).contains(&seconds) {
        Ok(seconds)
    } else {
        Err(ArgError::new("err-delay"))
    }
}

fn parse_template(value: &str) -> Result<String, ArgError> {
    // The user's text is never quoted back (a variable name, a format): only fixed texts and the
    // name of a known variable.
    NameTemplate::parse(value).map_err(|e| match e {
        NameError::UnknownVariable(_) => ArgError::new("err-template-variable"),
        NameError::BadArgument { variable, .. } => ArgError {
            id: "err-template-argument",
            detail: variable,
        },
        other => ArgError {
            id: "err-template",
            detail: other.to_string(),
        },
    })?;
    Ok(value.to_owned())
}

fn parse_var(value: &str) -> Result<(String, String), ArgError> {
    let (name, val) = value.split_once('=').ok_or(ArgError::new("err-var"))?;
    let valid = name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if valid {
        Ok((name.to_owned(), val.to_owned()))
    } else {
        Err(ArgError::new("err-var"))
    }
}

fn parse_set(value: &str) -> Result<(String, String), ArgError> {
    let (key, val) = value.split_once('=').ok_or(ArgError::new("err-set"))?;
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if valid {
        Ok((key.to_owned(), val.to_owned()))
    } else {
        Err(ArgError::new("err-set"))
    }
}

/// A host name, optionally with a port: no scheme, no path, no user.
fn parse_host(value: &str) -> Result<String, ArgError> {
    let ok = !value.is_empty()
        && value.len() <= 260
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']')
        })
        && !value.starts_with(['.', '-']);
    if ok {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-host"))
    }
}

/// A descriptor number: 0 (stdin) or 3 and above; 1 and 2 are the output streams.
fn parse_fd(value: &str) -> Result<i32, ArgError> {
    match value.parse::<i32>() {
        Ok(fd) if fd == 0 || fd >= 3 => Ok(fd),
        _ => Err(ArgError::new("err-fd")),
    }
}

fn parse_port(value: &str) -> Result<u16, ArgError> {
    match value.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(ArgError::new("err-port")),
    }
}

fn parse_limit(value: &str) -> Result<u32, ArgError> {
    match value.parse::<u32>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(ArgError::new("err-limit")),
    }
}

/// `YYYY-MM-DD` or an RFC 3339 timestamp; returned unchanged.
fn parse_since(value: &str) -> Result<String, ArgError> {
    let ok = value.parse::<jiff::civil::Date>().is_ok() || value.parse::<jiff::Timestamp>().is_ok();
    if ok {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-since"))
    }
}

/// An `http://` or `https://` URL with a host and no whitespace. Whether HTTPS is required depends
/// on the secret and is checked when the destination is used.
fn parse_url(value: &str) -> Result<String, ArgError> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let host_ok = rest.is_some_and(|r| {
        !r.starts_with(['/', '?', '#']) && !r.is_empty() && !r.chars().any(char::is_whitespace)
    });
    if host_ok {
        Ok(value.to_owned())
    } else {
        Err(ArgError::new("err-url"))
    }
}

/// `30s`, `15m`, `12h` or `7d`, at most [`MAX_PRESIGN`].
fn parse_presign(value: &str) -> Result<Duration, ArgError> {
    let bad = || ArgError::new("err-presign");
    let unit = value.chars().last().ok_or_else(bad)?;
    let factor: u64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return Err(bad()),
    };
    let digits = &value[..value.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let count: u64 = digits.parse().map_err(|_| bad())?;
    let secs = count.checked_mul(factor).ok_or_else(bad)?;
    let duration = Duration::from_secs(secs);
    if secs == 0 || duration > MAX_PRESIGN {
        return Err(bad());
    }
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        parse_delay, parse_fd, parse_host, parse_limit, parse_port, parse_presign, parse_profile,
        parse_set, parse_since, parse_url, parse_var,
    };

    #[test]
    fn delay_is_bounded() {
        assert_eq!(parse_delay("0").unwrap().to_bits(), 0.0_f64.to_bits());
        assert!((parse_delay("2.5").unwrap() - 2.5).abs() < f64::EPSILON);
        assert!(parse_delay("60").is_ok());
        for bad in ["60.1", "-1", "abc", "", "NaN", "inf", "1e9"] {
            assert!(parse_delay(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn profiles() {
        assert_eq!(parse_profile("").unwrap(), "");
        assert_eq!(parse_profile("s3-prod_1").unwrap(), "s3-prod_1");
        assert!(parse_profile("a b").is_err());
        assert!(parse_profile("../x").is_err());
        assert!(parse_profile(&"a".repeat(65)).is_err());
    }

    #[test]
    fn key_values() {
        assert_eq!(parse_var("A_1=x=y").unwrap(), ("A_1".into(), "x=y".into()));
        assert!(parse_var("1A=x").is_err());
        assert!(parse_var("novalue").is_err());
        assert!(parse_var("=x").is_err());
        assert_eq!(parse_set("a.b-c=1").unwrap().0, "a.b-c");
        assert!(parse_set("a b=1").is_err());
    }

    #[test]
    fn descriptors_and_numbers() {
        assert_eq!(parse_fd("3").unwrap(), 3);
        assert_eq!(parse_fd("0").unwrap(), 0);
        assert!(parse_fd("1").is_err());
        assert!(parse_fd("2").is_err());
        assert!(parse_fd("-1").is_err());
        assert!(parse_port("0").is_err());
        assert_eq!(parse_port("22").unwrap(), 22);
        assert!(parse_limit("0").is_err());
        assert_eq!(parse_limit("5").unwrap(), 5);
    }

    #[test]
    fn presign_is_at_most_seven_days() {
        assert_eq!(parse_presign("30m").unwrap(), Duration::from_mins(30));
        assert_eq!(parse_presign("7d").unwrap(), Duration::from_hours(168));
        for bad in ["8d", "169h", "0s", "d", "10", "1.5h", "-1h", ""] {
            assert!(parse_presign(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hosts() {
        for ok in [
            "z.example.org",
            "z.example.org:8443",
            "[::1]:80",
            "localhost",
        ] {
            assert!(parse_host(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "https://z.example.org",
            "z.example.org/x",
            "u@z.example.org",
            "a b",
            "-x",
        ] {
            assert!(parse_host(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn urls_and_dates() {
        assert!(parse_url("https://z.example.org/x").is_ok());
        assert!(parse_url("http://localhost:3000").is_ok());
        for bad in [
            "ftp://x",
            "https://",
            "https:///x",
            "example.org",
            "https://a b",
        ] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
        assert!(parse_since("2026-11-11").is_ok());
        assert!(parse_since("2026-11-11T14:03:22+01:00").is_ok());
        assert!(parse_since("yesterday").is_err());
    }
}
