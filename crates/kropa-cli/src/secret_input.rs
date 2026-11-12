// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the value of a secret comes from when a command takes one: a hidden prompt on the
//! terminal, stdin (`--secret-stdin`) or a descriptor (`--secret-fd N`), never an argument
//! (`docs/cli.md` section 1, principle 5; `docs/security/secrets.md` section 3.7).
//!
//! Reading stops at the first `\n` or at the end of the input, takes 4 KiB at most, drops that one
//! `\n`, and never reads past it (one byte at a time, unbuffered: a pipe keeps what follows). The
//! text lives in a [`Zeroizing`] buffer. All of it is safe code: a descriptor is reopened through
//! `/dev/fd/N`, and the terminal is switched to no-echo with `rustix`.
//!
//! Known limits: the descriptor `N` itself stays open until the process ends (closing a raw
//! descriptor needs `unsafe`, which is forbidden outside macOS code), and an interruption while the
//! terminal is hidden leaves it with echo off, as every tool that hides a prompt does.

use std::fs::File;
use std::io::Read;

use zeroize::Zeroizing;

/// Longest secret accepted, in bytes (`docs/cli.md` section 1).
pub const MAX_SECRET_LEN: usize = 4096;

/// Where to read the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFrom {
    /// Standard input.
    Stdin,
    /// This file descriptor.
    Fd(i32),
    /// The terminal, hidden.
    Terminal,
}

/// Why no value could be read. No variant carries anything of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    /// More than [`MAX_SECRET_LEN`] bytes.
    TooLong,
    /// Not UTF-8.
    NotText,
    /// The descriptor or stdin could not be opened or read.
    Unreadable,
    /// No terminal to ask on.
    NoTerminal,
}

/// Reads the value of a secret; injectable for the tests of the commands.
pub trait SecretInput {
    /// The value read from `from`; `prompt` is shown for the terminal only.
    ///
    /// # Errors
    /// [`InputError`] when nothing usable can be read.
    fn read(&self, from: InputFrom, prompt: &str) -> Result<Zeroizing<String>, InputError>;
}

/// The real input: stdin, descriptors and the terminal of the process.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemInput;

/// Reads up to the first `\n` or the end, one byte at a time, at most [`MAX_SECRET_LEN`] bytes.
///
/// # Errors
/// [`InputError::TooLong`], [`InputError::NotText`] or [`InputError::Unreadable`].
pub fn read_line(mut source: impl Read) -> Result<Zeroizing<String>, InputError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_SECRET_LEN + 1));
    let mut one = Zeroizing::new([0_u8; 1]);
    loop {
        match source.read(&mut *one) {
            Ok(0) => break,
            Ok(_) => {
                if one[0] == b'\n' {
                    break;
                }
                bytes.push(one[0]);
                if bytes.len() > MAX_SECRET_LEN {
                    return Err(InputError::TooLong);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(InputError::Unreadable),
        }
    }
    match String::from_utf8(std::mem::take(&mut *bytes)) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(e) => {
            // Wipe what was read before refusing it.
            drop(Zeroizing::new(e.into_bytes()));
            Err(InputError::NotText)
        }
    }
}

impl SecretInput for SystemInput {
    fn read(&self, from: InputFrom, prompt: &str) -> Result<Zeroizing<String>, InputError> {
        match from {
            InputFrom::Stdin => read_line(open_fd(0)?),
            InputFrom::Fd(fd) => read_line(open_fd(fd)?),
            InputFrom::Terminal => read_hidden(prompt),
        }
    }
}

/// Reopens descriptor `fd` as a file.
fn open_fd(fd: i32) -> Result<File, InputError> {
    File::open(format!("/dev/fd/{fd}")).map_err(|_| InputError::Unreadable)
}

#[cfg(unix)]
fn read_hidden(prompt: &str) -> Result<Zeroizing<String>, InputError> {
    use std::io::Write as _;

    use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};

    /// Puts the terminal back as it was, whatever happens.
    struct Restore<'a> {
        tty: &'a File,
        saved: rustix::termios::Termios,
    }
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = tcsetattr(self.tty, OptionalActions::Flush, &self.saved);
        }
    }

    let tty = File::options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| InputError::NoTerminal)?;
    let saved = tcgetattr(&tty).map_err(|_| InputError::NoTerminal)?;
    let mut hidden = saved.clone();
    hidden.local_modes.remove(LocalModes::ECHO);
    tcsetattr(&tty, OptionalActions::Flush, &hidden).map_err(|_| InputError::NoTerminal)?;
    let _restore = Restore { tty: &tty, saved };
    let mut stderr = std::io::stderr();
    let _ = write!(stderr, "{prompt} ");
    let _ = stderr.flush();
    let value = read_line(&tty);
    // The Enter key was not echoed.
    let _ = writeln!(stderr);
    value
}

#[cfg(not(unix))]
fn read_hidden(_prompt: &str) -> Result<Zeroizing<String>, InputError> {
    Err(InputError::NoTerminal)
}

#[cfg(test)]
mod tests {
    use super::{InputError, MAX_SECRET_LEN, SecretInput, SystemInput, read_line};

    #[test]
    fn stops_at_the_first_newline_and_drops_it() {
        let mut input: &[u8] = b"abcd-efgh\nrest\n";
        assert_eq!(&**read_line(&mut input).unwrap(), "abcd-efgh");
        // Nothing past the newline was consumed.
        assert_eq!(input, b"rest\n");
    }

    #[test]
    fn end_of_input_ends_the_value_and_only_one_newline_goes() {
        assert_eq!(&**read_line(&b"abcd-efgh"[..]).unwrap(), "abcd-efgh");
        assert_eq!(&**read_line(&b"abcd\r\n"[..]).unwrap(), "abcd\r");
        assert_eq!(&**read_line(&b"\n"[..]).unwrap(), "");
        assert_eq!(&**read_line(&b""[..]).unwrap(), "");
    }

    #[test]
    fn four_kib_is_the_limit() {
        let mut ok = vec![b'a'; MAX_SECRET_LEN];
        ok.push(b'\n');
        assert_eq!(read_line(&ok[..]).unwrap().len(), MAX_SECRET_LEN);
        let long = vec![b'a'; MAX_SECRET_LEN + 1];
        assert_eq!(read_line(&long[..]).unwrap_err(), InputError::TooLong);
    }

    #[test]
    fn text_must_be_utf8() {
        assert_eq!(
            read_line(&[0xff, 0xfe, b'a', b'b'][..]).unwrap_err(),
            InputError::NotText
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_descriptor_is_read_through_dev_fd() {
        use std::io::Write as _;
        use std::os::fd::AsRawFd as _;

        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"from-a-file\n").unwrap();
        let fd = file.as_raw_fd();
        let read = SystemInput
            .read(super::InputFrom::Fd(fd), "unused")
            .unwrap();
        assert_eq!(&**read, "from-a-file");
        assert_eq!(
            SystemInput
                .read(super::InputFrom::Fd(9999), "unused")
                .unwrap_err(),
            InputError::Unreadable
        );
    }
}
