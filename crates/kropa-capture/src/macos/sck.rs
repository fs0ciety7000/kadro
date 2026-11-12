// SPDX-License-Identifier: GPL-3.0-or-later

//! `ScreenCaptureKit` calls, made blocking.
//!
//! `ScreenCaptureKit` answers through completion handlers that run on a queue of its own. Each
//! function here starts the request, parks the calling thread on a channel until the handler has
//! sent its result (or a timeout expired), and returns plain Rust values: no future and no
//! Objective-C object other than the image leaves this module.

use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
    SCWindow,
};

use super::system;
use crate::apple::{SckError, classify_sck_error};
use crate::{CaptureError, Denial, Failure};

/// How long `ScreenCaptureKit` gets to answer one request. A missing permission answers at once; a
/// handler that never fires would otherwise hang the command forever. It is long because macOS 15
/// may hold the answer behind its own dialog (the periodic "allow screen recording" question) until
/// the user clicks.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// An error object of `ScreenCaptureKit`, reduced to what is reported.
#[derive(Debug)]
struct ErrorInfo {
    domain: String,
    code: isize,
    description: String,
}

impl ErrorInfo {
    fn read(error: *mut NSError) -> Self {
        // SAFETY: ScreenCaptureKit passes either null or a valid `NSError` that stays alive for
        // the duration of the handler, which is when this runs.
        match unsafe { error.as_ref() } {
            Some(e) => Self {
                domain: e.domain().to_string(),
                code: e.code(),
                description: e.localizedDescription().to_string(),
            },
            None => Self {
                domain: String::new(),
                code: 0,
                description: "no result and no error object".to_owned(),
            },
        }
    }

    fn into_capture_error(self, what: &str) -> CaptureError {
        match classify_sck_error(&self.domain, self.code) {
            SckError::UserDeclined => {
                if system::screen_recording_granted() {
                    CaptureError::PermissionDenied(Denial::ScreenRecordingStale)
                } else {
                    CaptureError::PermissionDenied(Denial::ScreenRecordingDenied)
                }
            }
            SckError::Other => CaptureError::failed(
                Failure::ScreenCaptureKitError,
                format!(
                    "{what}: {} (domain {}, code {})",
                    self.description, self.domain, self.code
                ),
            ),
        }
    }
}

/// Carries a value from the thread of a completion handler to the thread that waits for it.
struct Handoff<T>(T);

// SAFETY: the handler creates the value, sends it and never touches it again; the waiting thread
// is the only one that uses it afterwards. The objects handed over this way (`SCShareableContent`,
// a `CGImage`) are immutable once delivered, and reference counting is atomic, so moving them to
// another thread is sound.
unsafe impl<T> Send for Handoff<T> {}

type Answer<T> = Result<Handoff<T>, ErrorInfo>;

fn wait<T>(rx: &mpsc::Receiver<Answer<T>>, what: &str) -> Result<T, CaptureError> {
    match rx.recv_timeout(ANSWER_TIMEOUT) {
        Ok(Ok(Handoff(value))) => Ok(value),
        Ok(Err(info)) => Err(info.into_capture_error(what)),
        Err(_) => Err(CaptureError::failed(
            Failure::ScreenCaptureKitNoResponse,
            format!("{what}: no answer within {} s", ANSWER_TIMEOUT.as_secs()),
        )),
    }
}

/// The displays `ScreenCaptureKit` can capture. Needs the Screen Recording permission.
fn shareable_content() -> Result<Retained<SCShareableContent>, CaptureError> {
    let (tx, rx) = mpsc::channel::<Answer<Retained<SCShareableContent>>>();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: `content` is null or a valid object that is not owned by the handler; `retain`
            // takes our own reference before the handler returns.
            let answer = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(Handoff(content)),
                None => Err(ErrorInfo::read(error)),
            };
            // The receiver is gone when the wait timed out: nobody is interested any more.
            let _ = tx.send(answer);
        },
    );
    // SAFETY: `handler` has the signature the method documents, and it is copied by the callee, so
    // it may be dropped as soon as the call returns.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };
    wait(&rx, "listing the displays")
}

fn find_display(
    content: &SCShareableContent,
    id: u32,
) -> Result<Retained<SCDisplay>, CaptureError> {
    // SAFETY: plain getters of objects we own, called on the thread that owns them.
    let displays = unsafe { content.displays() };
    displays
        .iter()
        // SAFETY: as above.
        .find(|d| unsafe { d.displayID() } == id)
        .ok_or_else(|| {
            CaptureError::failed(
                Failure::OutputNotFound,
                format!("display {id} is not offered by ScreenCaptureKit"),
            )
        })
}

/// Takes a picture of one display, at its native resolution and without the mouse pointer.
///
/// # Errors
/// A [`CaptureError`]: a refusal when the permission is missing or stale, a failure otherwise.
pub(super) fn capture_display(id: u32) -> Result<CFRetained<CGImage>, CaptureError> {
    let content = shareable_content()?;
    let display = find_display(&content, id)?;
    let no_windows: Retained<NSArray<SCWindow>> = NSArray::new();
    // SAFETY: `alloc` returns a fresh object that `initWithDisplay:excludingWindows:` initializes;
    // both arguments are valid objects. Getters and setters below act on objects we own.
    let (filter, config) = unsafe {
        let filter = SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &no_windows,
        );
        let config = SCStreamConfiguration::new();
        // The size in pixels is the size in points times the pixel density of the display: a
        // Retina screen is captured at 2x, not blown up from points.
        let scale = f64::from(filter.pointPixelScale());
        let rect = filter.contentRect();
        config.setWidth(to_pixels(rect.size.width * scale));
        config.setHeight(to_pixels(rect.size.height * scale));
        config.setShowsCursor(false);
        (filter, config)
    };

    let (tx, rx) = mpsc::channel::<Answer<CFRetained<CGImage>>>();
    let handler = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        // SAFETY: `image` is null or a valid `CGImage` the handler does not own; `retain` takes a
        // reference of our own.
        let answer = match NonNull::new(image).map(|p| unsafe { CFRetained::retain(p) }) {
            Some(image) => Ok(Handoff(image)),
            None => Err(ErrorInfo::read(error)),
        };
        let _ = tx.send(answer);
    });
    // SAFETY: valid filter and configuration, and a handler of the documented signature (copied by
    // the callee).
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            &filter,
            &config,
            Some(&handler),
        );
    }
    wait(&rx, "taking the screenshot")
}

fn to_pixels(value: f64) -> usize {
    let rounded = value.round();
    if rounded.is_finite() && rounded >= 1.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked: a finite number of at least 1"
        )]
        {
            rounded as usize
        }
    } else {
        1
    }
}
