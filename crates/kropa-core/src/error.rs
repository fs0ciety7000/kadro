// SPDX-License-Identifier: GPL-3.0-or-later

//! Core error type and its mapping to exit codes.

use std::path::PathBuf;

use crate::ExitCode;

/// Why a file name or a name template was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NameError {
    /// The template syntax is wrong.
    #[error("invalid name template: {0}")]
    Syntax(String),
    /// The template uses a variable that does not exist.
    #[error("unknown variable {{{0}}} in name template")]
    UnknownVariable(String),
    /// A variable has no value in this context.
    #[error("variable {{{0}}} has no value here")]
    MissingVariable(&'static str),
    /// A variable argument (format, width) is invalid.
    #[error("invalid argument for {{{variable}}}: {reason}")]
    BadArgument {
        /// Variable name.
        variable: String,
        /// What is wrong.
        reason: String,
    },
    /// The resulting file name could leave the target folder or is not portable.
    #[error("unsafe file name: {0}")]
    UnsafeName(&'static str),
    /// Every candidate name with a counter is taken.
    #[error("no free file name found")]
    Exhausted,
}

/// Error of `kropa-core`. [`CoreError::exit_code`] gives the documented process exit code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// The configuration file is not valid TOML or has unknown or ill-typed keys.
    #[error("{path}:{line}:{column}: {message}")]
    ConfigParse {
        /// Configuration file.
        path: PathBuf,
        /// 1-based line.
        line: usize,
        /// 1-based column.
        column: usize,
        /// Parser message.
        message: String,
    },
    /// The configuration parses but a value is not acceptable.
    #[error("invalid configuration: {key}: {reason}")]
    ConfigValue {
        /// Offending key.
        key: &'static str,
        /// What is wrong.
        reason: String,
    },
    /// A file name or name template is refused.
    #[error(transparent)]
    Name(#[from] NameError),
    /// No standard folder could be determined (no home directory).
    #[error("cannot determine the {0} folder: no home directory")]
    NoHome(&'static str),
    /// A file-system operation failed.
    #[error("{path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
}

impl CoreError {
    /// Exit code of `docs/cli.md` section 3 for this error.
    ///
    /// A refused name (`--name`, or a template from the CLI) is a usage error; a bad value read from
    /// the configuration is reported as [`CoreError::ConfigValue`] and maps to `config`.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::ConfigParse { .. } | Self::ConfigValue { .. } | Self::NoHome(_) => {
                ExitCode::Config
            }
            Self::Name(_) => ExitCode::Usage,
            Self::Io { .. } => ExitCode::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreError, NameError};
    use crate::ExitCode;

    #[test]
    fn mapping_to_exit_codes() {
        let parse = CoreError::ConfigParse {
            path: "c.toml".into(),
            line: 1,
            column: 2,
            message: "x".into(),
        };
        assert_eq!(parse.exit_code(), ExitCode::Config);
        assert_eq!(parse.to_string(), "c.toml:1:2: x");
        let value = CoreError::ConfigValue {
            key: "k",
            reason: "r".into(),
        };
        assert_eq!(value.exit_code(), ExitCode::Config);
        assert_eq!(CoreError::NoHome("config").exit_code(), ExitCode::Config);
        assert_eq!(
            CoreError::from(NameError::Exhausted).exit_code(),
            ExitCode::Usage
        );
        let io = CoreError::Io {
            path: "p".into(),
            source: std::io::Error::other("boom"),
        };
        assert_eq!(io.exit_code(), ExitCode::Io);
    }
}
