// SPDX-License-Identifier: GPL-3.0-or-later

//! A call that cannot be cancelled, under a deadline: it runs on its own thread, and a call that
//! missed the deadline is left to end with the process, which exits right after reporting (the
//! same rule as the Secret Service store of `kropa-store`).

use std::sync::mpsc;
use std::time::Duration;

use crate::item::{Failure, FailureKind};

/// Runs `call` on a thread and waits `limit` for its result.
///
/// # Errors
/// [`FailureKind::Timeout`] when it does not end in time, [`FailureKind::StoreError`] when the
/// thread cannot be started or ended without a result.
pub fn run<T, F>(limit: Duration, call: F) -> Result<T, Failure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Failure> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("kropa-keychain".to_owned())
        .spawn(move || {
            // The caller may have given up; nobody to tell then.
            let _ = tx.send(call());
        });
    if worker.is_err() {
        return Err(Failure::new(
            FailureKind::StoreError,
            "could not start a thread",
        ));
    }
    match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Failure::new(
            FailureKind::Timeout,
            "the Keychain did not answer in time",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Failure::new(
            FailureKind::StoreError,
            "the Keychain call ended without an answer",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::run;
    use crate::item::FailureKind;

    #[test]
    fn a_call_that_ends_gives_its_result() {
        assert_eq!(run(Duration::from_secs(5), || Ok(7)).unwrap(), 7);
    }

    #[test]
    fn a_call_that_never_ends_is_a_timeout() {
        let e = run(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        })
        .unwrap_err();
        assert_eq!(e.kind, FailureKind::Timeout);
    }

    #[test]
    fn a_call_that_panics_is_an_error_not_a_hang() {
        let e = run::<(), _>(Duration::from_secs(5), || panic!("boom")).unwrap_err();
        assert_eq!(e.kind, FailureKind::StoreError);
    }
}
