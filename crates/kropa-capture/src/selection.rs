// SPDX-License-Identifier: GPL-3.0-or-later

//! The model of the selection overlay, without a screen (`DESIGN.md` section 8.1): which monitor
//! shows which part of the captured image, and what the pointer and the keyboard do to the area.
//!
//! The toolkit (GTK or `AppKit`) feeds [`Input`] to [`Selection::handle`] and draws what the
//! [`Outcome`] and the accessors say. Every coordinate here is in **physical pixels of the captured
//! image**, so that what the user sees in the dimensions label is the size of the PNG. The area is
//! always inside one monitor, whatever the input.
//!
//! The editor key `E` does not exist yet (lot 3).

use serde::{Deserialize, Serialize};

use crate::last_area::StoredArea;
use crate::region::Region;

/// The pointer pressed outside the active monitor: `Tab` selects on another one.
pub const HINT_OTHER_MONITOR: &str = "other-monitor";
/// `Enter` without an area: "Drag, or Ctrl+A".
pub const HINT_DRAG_OR_SELECT_ALL: &str = "drag-or-select-all";
/// `R` with no remembered area.
pub const HINT_NO_LAST_AREA: &str = "no-last-area";
/// `R` with an area taken on an image of another size (or that no longer fits a monitor).
pub const HINT_LAST_AREA_MISMATCH: &str = "last-area-mismatch";
/// The area was pushed against the edge of its monitor ("The area cannot leave this screen").
pub const ANNOUNCE_AT_EDGE: &str = "area-at-edge";
/// `Tab` moved to another monitor.
pub const ANNOUNCE_MONITOR_CHANGED: &str = "monitor-changed";

/// Pointer grab zone, in logical pixels (`DESIGN.md`: 24 px at least).
pub const GRAB_LOGICAL: f64 = 24.0;
/// Below this side (logical pixels) an area counts as small.
pub const SMALL_LOGICAL: f64 = 48.0;

/// A rectangle in logical units (points), as a toolkit reports a monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// A monitor as the toolkit reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorGeom {
    /// Connector name (`DP-1`) or number.
    pub id: String,
    /// Position and size in the logical space of the desktop.
    pub logical: RectF,
    /// Physical pixels per logical unit.
    pub scale: f64,
}

/// A rectangle of whole pixels of the captured image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PxRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl PxRect {
    /// Right edge (excluded).
    #[must_use]
    pub fn right(&self) -> u32 {
        self.x + self.width
    }

    /// Bottom edge (excluded).
    #[must_use]
    pub fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Whether the two rectangles share a pixel.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// A monitor placed on the captured image.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// Connector name or number.
    pub id: String,
    /// Position and size in logical units.
    pub logical: RectF,
    /// Physical pixels per logical unit.
    pub scale: f64,
    /// The part of the image this monitor shows.
    pub px: PxRect,
}

/// Why the monitors cannot be matched to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MapError {
    /// The geometry reported and the size of the image do not agree; nothing is guessed.
    #[error("the monitor layout does not match the captured image ({0})")]
    LayoutMismatch(&'static str),
}

impl MapError {
    /// Stable reason (`layout_mismatch`).
    #[must_use]
    pub fn reason(&self) -> &'static str {
        "layout_mismatch"
    }

    /// Exit code of the command line (`docs/cli.md`).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        10
    }
}

/// Places the monitors on an image of `image` pixels.
///
/// Only one scale for all monitors is understood (with mixed scales the size of the image depends
/// on how the compositor composed it): a different scale, a size that is not the extent of the
/// monitors times the scale (one pixel of rounding is allowed), overlapping monitors or a bad
/// number are all [`MapError::LayoutMismatch`].
///
/// # Errors
/// [`MapError::LayoutMismatch`].
pub fn map_monitors(monitors: &[MonitorGeom], image: (u32, u32)) -> Result<Vec<Monitor>, MapError> {
    const TOLERANCE: f64 = 1.0;
    let mismatch = MapError::LayoutMismatch;
    let first = monitors.first().ok_or(mismatch("no monitor"))?;
    let scale = first.scale;
    if image.0 == 0 || image.1 == 0 {
        return Err(mismatch("empty image"));
    }
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for m in monitors {
        let r = m.logical;
        let finite = [r.x, r.y, r.width, r.height, m.scale]
            .iter()
            .all(|v| v.is_finite());
        if !finite || r.width <= 0.0 || r.height <= 0.0 || m.scale <= 0.0 {
            return Err(mismatch("invalid geometry"));
        }
        if (m.scale - scale).abs() > 1e-6 {
            return Err(mismatch("mixed scales"));
        }
        left = left.min(r.x);
        top = top.min(r.y);
        right = right.max(r.x + r.width);
        bottom = bottom.max(r.y + r.height);
    }
    let expected = ((right - left) * scale, (bottom - top) * scale);
    if (expected.0 - f64::from(image.0)).abs() > TOLERANCE
        || (expected.1 - f64::from(image.1)).abs() > TOLERANCE
    {
        return Err(mismatch("image size differs from the monitors extent"));
    }
    let edge = |v: f64, max: u32| -> u32 {
        let px = (v * scale).round().clamp(0.0, f64::from(max));
        // Clamped to 0..=max just above, so the conversion is exact.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let px = px as u32;
        px
    };
    let mut placed: Vec<Monitor> = Vec::with_capacity(monitors.len());
    for m in monitors {
        let r = m.logical;
        let x0 = edge(r.x - left, image.0);
        let y0 = edge(r.y - top, image.1);
        let x1 = edge(r.x + r.width - left, image.0);
        let y1 = edge(r.y + r.height - top, image.1);
        if x1 <= x0 || y1 <= y0 {
            return Err(mismatch("empty monitor"));
        }
        let px = PxRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        };
        if placed.iter().any(|p| p.px.overlaps(&px)) {
            return Err(mismatch("overlapping monitors"));
        }
        placed.push(Monitor {
            id: m.id.clone(),
            logical: r,
            scale,
            px,
        });
    }
    Ok(placed)
}

/// A point in pixels of the captured image (the toolkit multiplies logical coordinates by the scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
}

/// Keys the overlay reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// `A` (with `primary`: select the whole active monitor).
    A,
    /// `R`: take the last area again.
    R,
    /// `Tab` (`shift`: backwards).
    Tab,
    /// `Enter`: accept.
    Enter,
    /// `Escape`: cancel.
    Escape,
    /// `?`: show or hide the help.
    Help,
}

/// What the toolkit tells the model.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Input {
    /// The pointer entered a surface of the overlay.
    PointerEnter,
    /// Button pressed.
    Down(Point),
    /// Pointer moved (button held or not).
    Move(Point),
    /// Button released.
    Up(Point),
    /// A key. `primary` is Ctrl on Linux and Cmd on macOS; `alt` is Alt or Option.
    Key {
        /// Which key.
        key: Key,
        /// Shift held.
        shift: bool,
        /// Alt / Option held.
        alt: bool,
        /// Ctrl (Linux) / Cmd (macOS) held.
        primary: bool,
    },
}

/// What the toolkit does after an [`Input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Draw again (also returned for an input with no effect: drawing is cheap).
    Redraw,
    /// The user validated this area.
    Accepted(Region),
    /// The user cancelled (exit code 3).
    Cancelled,
    /// Show this hint in the help line (identifier of a Fluent message).
    Hint(&'static str),
    /// Say this to the screen reader and show it for a moment (identifier of a Fluent message).
    Announce(&'static str),
}

/// A handle of the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Top left corner.
    NorthWest,
    /// Top edge.
    North,
    /// Top right corner.
    NorthEast,
    /// Right edge.
    East,
    /// Bottom right corner: the amber handle, the one `Alt` and the arrows resize.
    SouthEast,
    /// Bottom edge.
    South,
    /// Bottom left corner.
    SouthWest,
    /// Left edge.
    West,
}

/// In hit-test order: the amber corner first, then the other corners, then the middles.
const HANDLES: [Handle; 8] = [
    Handle::SouthEast,
    Handle::NorthWest,
    Handle::NorthEast,
    Handle::SouthWest,
    Handle::North,
    Handle::East,
    Handle::South,
    Handle::West,
];

impl Handle {
    fn is_corner(self) -> bool {
        matches!(
            self,
            Self::NorthWest | Self::NorthEast | Self::SouthEast | Self::SouthWest
        )
    }

    /// Side this handle sits on, per axis: -1 (left, top), 0 (middle), 1 (right, bottom).
    fn sides(self) -> (i64, i64) {
        match self {
            Self::NorthWest => (-1, -1),
            Self::North => (0, -1),
            Self::NorthEast => (1, -1),
            Self::East => (1, 0),
            Self::SouthEast => (1, 1),
            Self::South => (0, 1),
            Self::SouthWest => (-1, 1),
            Self::West => (-1, 0),
        }
    }
}

/// Pointer grab zone of a handle, in pixels local to the active monitor, right and bottom excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grab {
    /// Which handle.
    pub handle: Handle,
    /// Left edge.
    pub left: i64,
    /// Top edge.
    pub top: i64,
    /// Right edge (excluded).
    pub right: i64,
    /// Bottom edge (excluded).
    pub bottom: i64,
}

/// The area, in pixels local to the active monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Zone {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

/// What happens to one axis during a drag.
#[derive(Debug, Clone, Copy)]
enum Axis {
    /// Untouched: the two edges stay.
    Keep(i64, i64),
    /// One edge is fixed here; the other follows the pointer.
    Pull(i64),
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    /// Resizing (or drawing: `fresh`, where a click without movement leaves no area).
    Resize { x: Axis, y: Axis, fresh: bool },
    /// Moving: offset of the pointer from the corner of the area.
    Move { dx: i64, dy: i64 },
}

/// The state of the overlay: monitors, active monitor, area, drag, help.
#[derive(Debug, Clone)]
pub struct Selection {
    monitors: Vec<Monitor>,
    active: usize,
    last: Option<StoredArea>,
    image: (u32, u32),
    zone: Option<Zone>,
    drag: Option<Drag>,
    cursor: (i64, i64),
    help_expanded: bool,
}

fn to_i64(v: u32) -> i64 {
    i64::from(v)
}

fn to_u32(v: i64) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(u32::MAX)
}

impl Selection {
    /// A selection on `monitors` (from [`map_monitors`], at least one), `active` first.
    /// `last` is the area `R` takes again. An `active` out of range becomes the first monitor.
    ///
    /// # Panics
    /// When `monitors` is empty.
    #[must_use]
    pub fn new(monitors: Vec<Monitor>, active: usize, last: Option<StoredArea>) -> Self {
        assert!(!monitors.is_empty(), "a selection needs a monitor");
        let image = monitors.iter().fold((0, 0), |acc, m| {
            (acc.0.max(m.px.right()), acc.1.max(m.px.bottom()))
        });
        let active = if active < monitors.len() { active } else { 0 };
        let mut selection = Self {
            monitors,
            active,
            last,
            image,
            zone: None,
            drag: None,
            cursor: (0, 0),
            help_expanded: true,
        };
        selection.center_cursor();
        selection
    }

    fn center_cursor(&mut self) {
        let (w, h) = self.size();
        self.cursor = (w / 2, h / 2);
    }

    fn monitor_px(&self) -> PxRect {
        self.monitors[self.active].px
    }

    fn size(&self) -> (i64, i64) {
        let m = self.monitor_px();
        (to_i64(m.width), to_i64(m.height))
    }

    /// The monitors, in the order `Region::monitor` refers to.
    #[must_use]
    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// Index of the monitor the selection is on.
    #[must_use]
    pub fn active(&self) -> usize {
        self.active
    }

    /// Size of the captured image.
    #[must_use]
    pub fn image_size(&self) -> (u32, u32) {
        self.image
    }

    /// The area, in pixels of the image.
    #[must_use]
    pub fn region(&self) -> Option<Region> {
        let z = self.zone?;
        let m = self.monitor_px();
        Some(Region {
            monitor: self.active,
            x: m.x + to_u32(z.x),
            y: m.y + to_u32(z.y),
            width: to_u32(z.w),
            height: to_u32(z.h),
        })
    }

    /// The area in pixels local to the active monitor (what the surface of the monitor draws).
    #[must_use]
    pub fn local_zone(&self) -> Option<PxRect> {
        let z = self.zone?;
        Some(PxRect {
            x: to_u32(z.x),
            y: to_u32(z.y),
            width: to_u32(z.w),
            height: to_u32(z.h),
        })
    }

    /// The crosshair, in pixels local to the active monitor.
    #[must_use]
    pub fn cursor(&self) -> (u32, u32) {
        (to_u32(self.cursor.0), to_u32(self.cursor.1))
    }

    /// The crosshair, in pixels of the image.
    #[must_use]
    pub fn cursor_in_image(&self) -> (u32, u32) {
        let m = self.monitor_px();
        (m.x + to_u32(self.cursor.0), m.y + to_u32(self.cursor.1))
    }

    /// Whether a drag is under way.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether the full help is shown (else only the `? Help` pill).
    #[must_use]
    pub fn help_expanded(&self) -> bool {
        self.help_expanded
    }

    /// Shows the full help or the pill (the toolkit unfolds it for the first three uses).
    pub fn set_help_expanded(&mut self, expanded: bool) {
        self.help_expanded = expanded;
    }

    /// Whether the area is small: a side under 48 logical pixels.
    #[must_use]
    pub fn is_small(&self) -> bool {
        self.zone.is_some_and(|z| {
            let limit = SMALL_LOGICAL * self.monitors[self.active].scale;
            #[expect(
                clippy::cast_precision_loss,
                reason = "pixel counts are far below 2^52"
            )]
            let side = z.w.min(z.h) as f64;
            side < limit
        })
    }

    fn grab_size(&self) -> i64 {
        #[expect(clippy::cast_possible_truncation, reason = "a few dozen pixels")]
        let g = (GRAB_LOGICAL * self.monitors[self.active].scale).round() as i64;
        g.max(1)
    }

    /// Pointer grab zones of the handles, in pixels local to the active monitor. Empty without an
    /// area. Mid-edge handles disappear on a small area, and the grab zones of the corners then
    /// extend outwards (the inside stays the move zone).
    #[must_use]
    pub fn handle_grabs(&self) -> Vec<Grab> {
        let Some(z) = self.zone else {
            return Vec::new();
        };
        let g = self.grab_size();
        let small = self.is_small();
        HANDLES
            .iter()
            .filter(|h| !small || h.is_corner())
            .map(|&handle| {
                let (sx, sy) = handle.sides();
                let cx = z.x + (sx + 1) * z.w / 2;
                let cy = z.y + (sy + 1) * z.h / 2;
                let span = |c: i64, side: i64| -> (i64, i64) {
                    if small {
                        if side < 0 { (c - g, c + 1) } else { (c, c + g) }
                    } else {
                        (c - g / 2, c - g / 2 + g)
                    }
                };
                let (left, right) = span(cx, sx);
                let (top, bottom) = span(cy, sy);
                Grab {
                    handle,
                    left,
                    top,
                    right,
                    bottom,
                }
            })
            .collect()
    }

    fn hit_handle(&self, px: i64, py: i64) -> Option<Handle> {
        self.handle_grabs()
            .into_iter()
            .find(|g| px >= g.left && px < g.right && py >= g.top && py < g.bottom)
            .map(|g| g.handle)
    }

    /// Feeds one input and says what to do.
    pub fn handle(&mut self, input: Input) -> Outcome {
        match input {
            Input::PointerEnter => Outcome::Redraw,
            Input::Down(p) => self.down(p),
            Input::Move(p) => self.pointer_move(p),
            Input::Up(_) => {
                self.drag = None;
                Outcome::Redraw
            }
            Input::Key {
                key,
                shift,
                alt,
                primary,
            } => self.on_key(key, shift, alt, primary),
        }
    }

    /// The monitor containing `p`, if any.
    fn monitor_at(&self, p: Point) -> Option<usize> {
        self.monitors.iter().position(|m| {
            p.x >= f64::from(m.px.x)
                && p.x < f64::from(m.px.right())
                && p.y >= f64::from(m.px.y)
                && p.y < f64::from(m.px.bottom())
        })
    }

    /// Pixel under `p` local to the active monitor, brought inside it.
    fn local_pixel(&self, p: Point) -> (i64, i64) {
        let m = self.monitor_px();
        let (w, h) = self.size();
        let conv = |v: f64, origin: u32, max: i64| -> i64 {
            if !v.is_finite() {
                return 0;
            }
            #[expect(clippy::cast_possible_truncation, reason = "clamped just after")]
            let local = (v - f64::from(origin)).floor() as i64;
            local.clamp(0, max - 1)
        };
        (conv(p.x, m.x, w), conv(p.y, m.y, h))
    }

    /// Edge position under `p` local to the active monitor, in `0..=size`.
    fn local_edge(&self, p: Point) -> (i64, i64) {
        let m = self.monitor_px();
        let (w, h) = self.size();
        let conv = |v: f64, origin: u32, max: i64| -> i64 {
            if !v.is_finite() {
                return 0;
            }
            #[expect(clippy::cast_possible_truncation, reason = "clamped just after")]
            let local = (v - f64::from(origin)).round() as i64;
            local.clamp(0, max)
        };
        (conv(p.x, m.x, w), conv(p.y, m.y, h))
    }

    fn down(&mut self, p: Point) -> Outcome {
        if self.monitor_at(p) != Some(self.active) {
            return Outcome::Hint(HINT_OTHER_MONITOR);
        }
        let (px, py) = self.local_pixel(p);
        self.cursor = (px, py);
        let (ex, ey) = self.local_edge(p);
        if let Some(z) = self.zone {
            if let Some(handle) = self.hit_handle(px, py) {
                let (sx, sy) = handle.sides();
                let axis = |side: i64, lo: i64, hi: i64| match side {
                    -1 => Axis::Pull(hi),
                    1 => Axis::Pull(lo),
                    _ => Axis::Keep(lo, hi),
                };
                self.drag = Some(Drag::Resize {
                    x: axis(sx, z.x, z.x + z.w),
                    y: axis(sy, z.y, z.y + z.h),
                    fresh: false,
                });
                return Outcome::Redraw;
            }
            if px >= z.x && px < z.x + z.w && py >= z.y && py < z.y + z.h {
                self.drag = Some(Drag::Move {
                    dx: px - z.x,
                    dy: py - z.y,
                });
                return Outcome::Redraw;
            }
        }
        self.zone = None;
        self.drag = Some(Drag::Resize {
            x: Axis::Pull(ex),
            y: Axis::Pull(ey),
            fresh: true,
        });
        Outcome::Redraw
    }

    fn pointer_move(&mut self, p: Point) -> Outcome {
        let Some(drag) = self.drag else {
            if self.monitor_at(p) == Some(self.active) {
                self.cursor = self.local_pixel(p);
            }
            return Outcome::Redraw;
        };
        // During a drag the pointer may leave the monitor: the area stops at its edge.
        self.cursor = self.local_pixel(p);
        let (ex, ey) = self.local_edge(p);
        let (w, h) = self.size();
        match drag {
            Drag::Move { dx, dy } => {
                if let Some(z) = self.zone.as_mut() {
                    z.x = (self.cursor.0 - dx).clamp(0, w - z.w);
                    z.y = (self.cursor.1 - dy).clamp(0, h - z.h);
                }
            }
            Drag::Resize { x, y, fresh } => {
                let span = |axis: Axis, edge: i64, max: i64| -> (i64, i64) {
                    match axis {
                        Axis::Keep(lo, hi) => (lo, hi),
                        Axis::Pull(fixed) => {
                            let (mut lo, mut hi) = (fixed.min(edge), fixed.max(edge));
                            if lo == hi && !fresh {
                                // An existing area never shrinks to nothing.
                                if hi < max {
                                    hi += 1;
                                } else {
                                    lo -= 1;
                                }
                            }
                            (lo, hi)
                        }
                    }
                };
                let (x0, x1) = span(x, ex, w);
                let (y0, y1) = span(y, ey, h);
                self.zone = (x1 > x0 && y1 > y0).then_some(Zone {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                });
            }
        }
        Outcome::Redraw
    }

    fn on_key(&mut self, key: Key, shift: bool, alt: bool, primary: bool) -> Outcome {
        let step = if shift { 10 } else { 1 };
        match key {
            Key::Left => self.arrow(-step, 0, alt),
            Key::Right => self.arrow(step, 0, alt),
            Key::Up => self.arrow(0, -step, alt),
            Key::Down => self.arrow(0, step, alt),
            Key::A if primary => {
                let (w, h) = self.size();
                self.drag = None;
                self.zone = Some(Zone { x: 0, y: 0, w, h });
                Outcome::Redraw
            }
            Key::A => Outcome::Redraw,
            Key::R => self.take_last(),
            Key::Tab => self.next_monitor(shift),
            Key::Enter => match self.region() {
                Some(region) => Outcome::Accepted(region),
                None => Outcome::Hint(HINT_DRAG_OR_SELECT_ALL),
            },
            Key::Escape => Outcome::Cancelled,
            Key::Help => {
                self.help_expanded = !self.help_expanded;
                Outcome::Redraw
            }
        }
    }

    fn arrow(&mut self, dx: i64, dy: i64, alt: bool) -> Outcome {
        let (w, h) = self.size();
        let Some(z) = self.zone else {
            self.cursor = (
                (self.cursor.0 + dx).clamp(0, w - 1),
                (self.cursor.1 + dy).clamp(0, h - 1),
            );
            return Outcome::Redraw;
        };
        self.drag = None;
        let blocked = if alt {
            // Resize from the amber handle: the bottom right corner moves. Shrinking to the
            // minimum of one pixel is not "the screen edge", only growing past it is.
            let nw = (z.w + dx).clamp(1, w - z.x);
            let nh = (z.h + dy).clamp(1, h - z.y);
            self.zone = Some(Zone { w: nw, h: nh, ..z });
            z.w + dx > w - z.x || z.h + dy > h - z.y
        } else {
            let nx = (z.x + dx).clamp(0, w - z.w);
            let ny = (z.y + dy).clamp(0, h - z.h);
            self.zone = Some(Zone { x: nx, y: ny, ..z });
            self.cursor = (
                (self.cursor.0 + nx - z.x).clamp(0, w - 1),
                (self.cursor.1 + ny - z.y).clamp(0, h - 1),
            );
            nx != z.x + dx || ny != z.y + dy
        };
        if blocked {
            Outcome::Announce(ANNOUNCE_AT_EDGE)
        } else {
            Outcome::Redraw
        }
    }

    fn take_last(&mut self) -> Outcome {
        let Some(last) = self.last else {
            return Outcome::Hint(HINT_NO_LAST_AREA);
        };
        let region = last.region;
        let fits = last.image == self.image
            && self
                .monitors
                .get(region.monitor)
                .is_some_and(|m| contains(&m.px, &region));
        if !fits {
            return Outcome::Hint(HINT_LAST_AREA_MISMATCH);
        }
        let m = self.monitors[region.monitor].px;
        self.active = region.monitor;
        self.drag = None;
        self.zone = Some(Zone {
            x: to_i64(region.x - m.x),
            y: to_i64(region.y - m.y),
            w: to_i64(region.width),
            h: to_i64(region.height),
        });
        Outcome::Redraw
    }

    fn next_monitor(&mut self, backwards: bool) -> Outcome {
        let n = self.monitors.len();
        if n < 2 {
            // No Tab with one monitor.
            return Outcome::Redraw;
        }
        self.active = if backwards {
            (self.active + n - 1) % n
        } else {
            (self.active + 1) % n
        };
        self.zone = None;
        self.drag = None;
        self.center_cursor();
        Outcome::Announce(ANNOUNCE_MONITOR_CHANGED)
    }
}

fn contains(outer: &PxRect, r: &Region) -> bool {
    r.width > 0
        && r.height > 0
        && r.x >= outer.x
        && r.y >= outer.y
        && u64::from(r.x) + u64::from(r.width) <= u64::from(outer.right())
        && u64::from(r.y) + u64::from(r.height) <= u64::from(outer.bottom())
}
