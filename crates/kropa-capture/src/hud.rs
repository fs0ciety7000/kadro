// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the overlay puts its labels (`DESIGN.md` section 8.1, "Priorité et retournement des HUD"):
//! the area is never covered, and when room is short the dimensions label wins over the loupe,
//! which wins over the help.
//!
//! Everything is in physical pixels local to the active monitor; the sizes of the labels are the
//! logical sizes below times the scale of the monitor.

use crate::selection::{PxRect, Selection};

/// Size of the dimensions label, in logical pixels (`1280 × 720 px · x 424 · y 372`).
pub const DIMENSIONS_SIZE: (u32, u32) = (236, 28);
/// Size of the loupe label: 120 px of pixels, 8 px of margin, and two lines of reading.
pub const LOUPE_SIZE: (u32, u32) = (136, 180);
/// Size of the unfolded help (at most 1080 px wide).
pub const HELP_SIZE: (u32, u32) = (640, 76);
/// Size of the `? Help` pill.
pub const PILL_SIZE: (u32, u32) = (76, 36);
/// Gap between the area and a label next to it.
pub const GAP: u32 = 8;
/// Gap between the pointer and the loupe.
pub const LOUPE_OFFSET: u32 = 16;
/// Margin between the help and the edge of the screen.
pub const EDGE_MARGIN: u32 = 20;

/// What to draw with the help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpLayout {
    /// The unfolded help, at the bottom centre.
    Expanded(PxRect),
    /// The `? Help` pill, in a free corner.
    Pill(PxRect),
    /// No room at all: nothing is drawn.
    Hidden,
}

/// Where the labels go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudLayout {
    /// Dimensions label (only with an area).
    pub dimensions: Option<PxRect>,
    /// The area fills the screen so the label had to go inside it (the only case where a label
    /// covers the area).
    pub dimensions_inside: bool,
    /// Loupe near the pointer, or `None` when every side would cover the area or another label.
    pub magnifier: Option<PxRect>,
    /// Help.
    pub help: HelpLayout,
}

fn scaled(size: (u32, u32), scale: f64) -> (i64, i64) {
    let f = |v: u32| -> i64 {
        #[expect(clippy::cast_possible_truncation, reason = "a few hundred pixels")]
        let px = (f64::from(v) * scale).round() as i64;
        px.max(1)
    };
    (f(size.0), f(size.1))
}

fn rect(left: i64, top: i64, width: i64, height: i64) -> PxRect {
    let px = |v: i64| u32::try_from(v.max(0)).unwrap_or(u32::MAX);
    PxRect {
        x: px(left),
        y: px(top),
        width: px(width),
        height: px(height),
    }
}

/// Places the labels of `selection` on a surface of `viewport` pixels (the active monitor).
#[must_use]
pub fn hud_layout(selection: &Selection, viewport: (u32, u32)) -> HudLayout {
    let scale = selection.monitors()[selection.active()].scale;
    let (vw, vh) = (i64::from(viewport.0), i64::from(viewport.1));
    let gap = scaled((GAP, GAP), scale).0;
    let zone = selection.local_zone();

    // 1. Dimensions: below the area, else above, aligned on its inner edge.
    let (dw, dh) = scaled(DIMENSIONS_SIZE, scale);
    let mut dimensions_inside = false;
    let dimensions = zone.map(|z| {
        let x = i64::from(z.x).min(vw - dw).max(0);
        let below = i64::from(z.bottom()) + gap;
        let above = i64::from(z.y) - gap - dh;
        if below + dh <= vh {
            rect(x, below, dw, dh)
        } else if above >= 0 {
            rect(x, above, dw, dh)
        } else {
            dimensions_inside = true;
            let inner_x = (i64::from(z.x) + gap).min(vw - dw).max(0);
            rect(inner_x, i64::from(z.bottom()) - gap - dh, dw, dh)
        }
    });

    let blockers = |extra: &[PxRect]| -> Vec<PxRect> {
        let mut list: Vec<PxRect> = zone.into_iter().collect();
        list.extend_from_slice(extra);
        list
    };
    let free = |candidate: &PxRect, against: &[PxRect]| -> bool {
        i64::from(candidate.x) + i64::from(candidate.width) <= vw
            && i64::from(candidate.y) + i64::from(candidate.height) <= vh
            && !against.iter().any(|r| r.overlaps(candidate))
    };

    // 2. Loupe: next to the pointer, on the side away from the nearest edge, flipped if needed.
    let (mw, mh) = scaled(LOUPE_SIZE, scale);
    let off = scaled((LOUPE_OFFSET, LOUPE_OFFSET), scale).0;
    let (cx, cy) = {
        let c = selection.cursor();
        (i64::from(c.0), i64::from(c.1))
    };
    let right = cx + off;
    let left = cx - off - mw;
    let below = cy + off;
    let above = cy - off - mh;
    let (px, fx) = if cx < vw - cx {
        (right, left)
    } else {
        (left, right)
    };
    let (py, fy) = if cy < vh - cy {
        (below, above)
    } else {
        (above, below)
    };
    let against = blockers(dimensions.as_slice());
    let magnifier = [(px, py), (fx, py), (px, fy), (fx, fy)]
        .into_iter()
        .filter(|&(x, y)| x >= 0 && y >= 0)
        .map(|(x, y)| rect(x, y, mw, mh))
        .find(|r| free(r, &against));

    // 3. Help: unfolded at the bottom centre, else a pill in the first free corner.
    let against = blockers(
        &dimensions
            .into_iter()
            .chain(magnifier)
            .collect::<Vec<PxRect>>(),
    );
    let margin = scaled((EDGE_MARGIN, EDGE_MARGIN), scale).0;
    let (hw, hh) = scaled(HELP_SIZE, scale);
    let hw = hw.min(vw - 2 * margin).max(1);
    let expanded = rect((vw - hw) / 2, vh - margin - hh, hw, hh);
    let help = if selection.help_expanded() && vh - margin - hh >= 0 && free(&expanded, &against) {
        HelpLayout::Expanded(expanded)
    } else {
        let (pw, ph) = scaled(PILL_SIZE, scale);
        let corners = [
            (vw - margin - pw, vh - margin - ph),
            (margin, vh - margin - ph),
            (vw - margin - pw, margin),
            (margin, margin),
        ];
        corners
            .into_iter()
            .filter(|&(x, y)| x >= 0 && y >= 0)
            .map(|(x, y)| rect(x, y, pw, ph))
            .find(|r| free(r, &against))
            .map_or(HelpLayout::Hidden, HelpLayout::Pill)
    };

    HudLayout {
        dimensions,
        dimensions_inside,
        magnifier,
        help,
    }
}
