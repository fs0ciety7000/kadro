// SPDX-License-Identifier: GPL-3.0-or-later

//! The screens that are connected: geometry from CoreGraphics (no permission needed), names from
//! AppKit.

use std::collections::HashMap;

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayMode,
    CGGetActiveDisplayList, CGMainDisplayID,
};
use objc2_foundation::{NSNumber, NSString};

use crate::screens::Screen;

/// More than this many screens at once do not exist on a Mac.
const MAX_DISPLAYS: u32 = 32;

fn active_ids() -> Vec<CGDirectDisplayID> {
    let mut ids = [0 as CGDirectDisplayID; MAX_DISPLAYS as usize];
    let mut count = 0u32;
    // SAFETY: `ids` has room for `MAX_DISPLAYS` entries, which is the maximum passed; `count` is a
    // valid pointer; both live until the call returns.
    let status = unsafe { CGGetActiveDisplayList(MAX_DISPLAYS, ids.as_mut_ptr(), &raw mut count) };
    if status.0 != 0 {
        return Vec::new();
    }
    ids.iter()
        .take(usize::try_from(count).unwrap_or(0).min(ids.len()))
        .copied()
        .collect()
}

/// Display names by screen number, from `NSScreen`.
///
/// AppKit wants the main thread. The command line runs there; on any other thread (unit tests) the
/// names are simply unknown and screens are designated by number.
fn names() -> HashMap<CGDirectDisplayID, String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return HashMap::new();
    };
    let key = NSString::from_str("NSScreenNumber");
    let mut out = HashMap::new();
    for screen in NSScreen::screens(mtm).iter() {
        let description = screen.deviceDescription();
        let Some(value) = description.objectForKey(&key) else {
            continue;
        };
        // SAFETY: the value of "NSScreenNumber" is documented to be an NSNumber; it is only
        // used as one after `downcast` has checked its class.
        let Ok(number) = value.downcast::<NSNumber>() else {
            continue;
        };
        out.insert(number.unsignedIntValue(), screen.localizedName().to_string());
    }
    out
}

fn scale_of(id: CGDirectDisplayID) -> f64 {
    let mode = CGDisplayCopyDisplayMode(id);
    let points = CGDisplayMode::width(mode.as_deref());
    let pixels = CGDisplayMode::pixel_width(mode.as_deref());
    if points == 0 || pixels == 0 {
        return 1.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "pixel counts of a screen are far below 2^52"
    )]
    {
        pixels as f64 / points as f64
    }
}

/// The connected screens, primary first in the order the system gives them.
pub(super) fn connected() -> Vec<Screen> {
    let main = CGMainDisplayID();
    let names = names();
    active_ids()
        .into_iter()
        .map(|id| {
            let bounds = CGDisplayBounds(id);
            Screen {
                id,
                name: names.get(&id).cloned(),
                x: bounds.origin.x,
                y: bounds.origin.y,
                width: bounds.size.width,
                height: bounds.size.height,
                scale: scale_of(id),
                primary: id == main,
            }
        })
        .collect()
}
