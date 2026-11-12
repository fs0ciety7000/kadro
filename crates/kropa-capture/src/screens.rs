// SPDX-License-Identifier: GPL-3.0-or-later

//! Screens as the backends see them: how to pick one by name or number, and how to place several
//! on one canvas.
//!
//! This is plain geometry with no system call, so that it is tested on every platform even though
//! only the macOS backend needs it today (the Linux portal cannot isolate a screen). Coordinates
//! are in points in the global space of the desktop (origin at the top-left of the primary screen,
//! y pointing down); sizes of images are in pixels.

/// Largest number of pixels of one composed image (a 16384 x 16384 canvas is 1 GiB of RGBA).
pub const MAX_CANVAS_PIXELS: u64 = 1 << 28;
/// Largest side of a composed image, in pixels.
pub const MAX_CANVAS_SIDE: u32 = 65_535;

/// One screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// Number the system gives the screen (`CGDirectDisplayID` on macOS).
    pub id: u32,
    /// Name shown to the user (`Built-in Retina Display`), when the system tells it.
    pub name: Option<String>,
    /// Left edge, in points.
    pub x: f64,
    /// Top edge, in points.
    pub y: f64,
    /// Width, in points.
    pub width: f64,
    /// Height, in points.
    pub height: f64,
    /// Pixels per point (2.0 on a Retina screen).
    pub scale: f64,
    /// Whether this is the primary screen (the one with the menu bar).
    pub primary: bool,
}

impl Screen {
    /// The name to show: the system name, else the number.
    #[must_use]
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.id.to_string())
    }
}

/// Why no screen was selected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectError {
    /// The system reports no screen at all.
    #[error("no screen is connected")]
    NoScreen,
    /// Nothing matches the requested name or number.
    #[error("no screen is named or numbered {wanted:?}")]
    NotFound {
        /// What was asked.
        wanted: String,
    },
    /// Several screens share the requested name.
    #[error("{} screens are named {wanted:?} (numbers {})", .ids.len(), join(.ids))]
    Ambiguous {
        /// What was asked.
        wanted: String,
        /// Numbers of the screens that match, to use instead.
        ids: Vec<u32>,
    },
}

fn join(ids: &[u32]) -> String {
    ids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Picks the screen a request designates.
///
/// Without a name: the primary screen. With one: the screen whose number is `wanted`, else the one
/// whose name equals `wanted` ignoring case and surrounding spaces. Two screens of the same model
/// share their name: that is [`SelectError::Ambiguous`], never a silent choice.
///
/// # Errors
/// [`SelectError`] when no screen, or more than one, matches.
pub fn select<'a>(screens: &'a [Screen], wanted: Option<&str>) -> Result<&'a Screen, SelectError> {
    let Some(wanted) = wanted else {
        return screens
            .iter()
            .find(|s| s.primary)
            .or_else(|| screens.first())
            .ok_or(SelectError::NoScreen);
    };
    let wanted = wanted.trim();
    if let Ok(id) = wanted.parse::<u32>()
        && let Some(screen) = screens.iter().find(|s| s.id == id)
    {
        return Ok(screen);
    }
    let lower = wanted.to_lowercase();
    let matches: Vec<&Screen> = screens
        .iter()
        .filter(|s| s.name.as_deref().is_some_and(|n| n.to_lowercase() == lower))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(SelectError::NotFound {
            wanted: wanted.to_owned(),
        }),
        many => Err(SelectError::Ambiguous {
            wanted: wanted.to_owned(),
            ids: many.iter().map(|s| s.id).collect(),
        }),
    }
}

/// Where one screen goes on the canvas, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Number of the screen.
    pub id: u32,
    /// Left edge on the canvas.
    pub x: u32,
    /// Top edge on the canvas (y down).
    pub y: u32,
    /// Width on the canvas.
    pub width: u32,
    /// Height on the canvas.
    pub height: u32,
}

/// All screens on one canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Pixels per point of the canvas: the finest screen, so that no screen loses detail.
    pub scale: f64,
    /// One placement per screen, in the order of the input.
    pub placements: Vec<Placement>,
}

/// Why the screens cannot be put on one canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// There is no screen.
    #[error("no screen is connected")]
    Empty,
    /// A screen has no usable size.
    #[error("a screen reports an empty or invalid size")]
    InvalidScreen,
    /// The canvas would exceed [`MAX_CANVAS_PIXELS`] or [`MAX_CANVAS_SIDE`].
    #[error("the screens together are too large for one image")]
    TooLarge,
}

fn pixels(value: f64) -> Option<u32> {
    let rounded = value.round();
    if rounded.is_finite() && (0.0..=f64::from(MAX_CANVAS_SIDE)).contains(&rounded) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to be an integer between 0 and 65535"
        )]
        Some(rounded as u32)
    } else {
        None
    }
}

/// Places `screens` on one canvas, keeping their relative positions.
///
/// The canvas is as fine as the finest screen; a coarser screen is stretched to its rectangle by
/// the caller. Gaps between screens stay empty (transparent or black, the caller decides).
///
/// # Errors
/// [`LayoutError`] when there is no screen, one has no size, or the canvas would be too large.
pub fn layout(screens: &[Screen]) -> Result<Layout, LayoutError> {
    if screens.is_empty() {
        return Err(LayoutError::Empty);
    }
    let valid = |s: &Screen| {
        [s.x, s.y, s.width, s.height, s.scale]
            .iter()
            .all(|v| v.is_finite())
            && s.width > 0.0
            && s.height > 0.0
            && s.scale > 0.0
    };
    if !screens.iter().all(valid) {
        return Err(LayoutError::InvalidScreen);
    }
    let scale = screens.iter().map(|s| s.scale).fold(f64::MIN, f64::max);
    let left = screens.iter().map(|s| s.x).fold(f64::MAX, f64::min);
    let top = screens.iter().map(|s| s.y).fold(f64::MAX, f64::min);
    let mut placements = Vec::with_capacity(screens.len());
    let (mut width, mut height) = (0u32, 0u32);
    for s in screens {
        let place = Placement {
            id: s.id,
            x: pixels((s.x - left) * scale).ok_or(LayoutError::TooLarge)?,
            y: pixels((s.y - top) * scale).ok_or(LayoutError::TooLarge)?,
            width: pixels(s.width * scale)
                .filter(|w| *w > 0)
                .ok_or(LayoutError::TooLarge)?,
            height: pixels(s.height * scale)
                .filter(|h| *h > 0)
                .ok_or(LayoutError::TooLarge)?,
        };
        width = width.max(place.x.saturating_add(place.width));
        height = height.max(place.y.saturating_add(place.height));
        placements.push(place);
    }
    if width > MAX_CANVAS_SIDE
        || height > MAX_CANVAS_SIDE
        || u64::from(width) * u64::from(height) > MAX_CANVAS_PIXELS
    {
        return Err(LayoutError::TooLarge);
    }
    Ok(Layout {
        width,
        height,
        scale,
        placements,
    })
}

#[cfg(test)]
mod tests {
    use super::{Layout, LayoutError, Placement, Screen, SelectError, layout, select};

    fn screen(id: u32, name: Option<&str>, x: f64, y: f64, w: f64, h: f64, scale: f64) -> Screen {
        Screen {
            id,
            name: name.map(str::to_owned),
            x,
            y,
            width: w,
            height: h,
            scale,
            primary: x == 0.0 && y == 0.0,
        }
    }

    fn two() -> Vec<Screen> {
        vec![
            screen(
                1,
                Some("Built-in Retina Display"),
                0.0,
                0.0,
                1512.0,
                982.0,
                2.0,
            ),
            screen(7, Some("DELL U2720Q"), 1512.0, -200.0, 2560.0, 1440.0, 1.0),
        ]
    }

    #[test]
    fn without_a_name_the_primary_screen_is_chosen() {
        let screens = two();
        assert_eq!(select(&screens, None).unwrap().id, 1);
    }

    #[test]
    fn without_a_primary_flag_the_first_screen_is_chosen() {
        let mut screens = two();
        screens[0].primary = false;
        assert_eq!(select(&screens, None).unwrap().id, 1);
    }

    #[test]
    fn no_screen_at_all_is_reported() {
        assert_eq!(select(&[], None), Err(SelectError::NoScreen));
    }

    #[test]
    fn a_number_selects_a_screen() {
        let screens = two();
        assert_eq!(select(&screens, Some("7")).unwrap().id, 7);
    }

    #[test]
    fn a_name_selects_a_screen_ignoring_case_and_spaces() {
        let screens = two();
        assert_eq!(select(&screens, Some("  dell u2720q ")).unwrap().id, 7);
    }

    #[test]
    fn a_number_wins_over_a_name_that_looks_like_it() {
        let mut screens = two();
        screens[0].name = Some("7".into());
        assert_eq!(select(&screens, Some("7")).unwrap().id, 7);
    }

    #[test]
    fn a_numeric_name_still_works_when_no_screen_has_that_number() {
        let mut screens = two();
        screens[0].name = Some("27".into());
        assert_eq!(select(&screens, Some("27")).unwrap().id, 1);
    }

    #[test]
    fn an_unknown_name_is_not_found() {
        let screens = two();
        assert_eq!(
            select(&screens, Some("DP-1")),
            Err(SelectError::NotFound {
                wanted: "DP-1".into()
            })
        );
    }

    #[test]
    fn identical_names_are_ambiguous_and_the_numbers_are_listed() {
        let mut screens = two();
        screens[1].name = Some("Built-in Retina Display".into());
        let err = select(&screens, Some("built-in retina display")).unwrap_err();
        assert_eq!(
            err,
            SelectError::Ambiguous {
                wanted: "built-in retina display".into(),
                ids: vec![1, 7]
            }
        );
        assert!(err.to_string().contains("1, 7"), "{err}");
    }

    #[test]
    fn a_screen_without_a_name_is_reached_by_number_only() {
        let screens = vec![screen(3, None, 0.0, 0.0, 100.0, 100.0, 1.0)];
        assert_eq!(select(&screens, Some("3")).unwrap().label(), "3");
        assert!(select(&screens, Some("")).is_err());
    }

    #[test]
    fn one_screen_is_its_own_canvas() {
        let l = layout(&two()[..1]).unwrap();
        assert_eq!((l.width, l.height), (3024, 1964));
        assert_eq!(l.scale, 2.0);
        assert_eq!(
            l.placements,
            [Placement {
                id: 1,
                x: 0,
                y: 0,
                width: 3024,
                height: 1964
            }]
        );
    }

    #[test]
    fn mixed_scales_use_the_finest_and_keep_positions() {
        let l = layout(&two()).unwrap();
        assert_eq!(l.scale, 2.0);
        // The external screen sticks out 200 points above the built-in one.
        assert_eq!(
            l.placements[0],
            Placement {
                id: 1,
                x: 0,
                y: 400,
                width: 3024,
                height: 1964
            }
        );
        assert_eq!(
            l.placements[1],
            Placement {
                id: 7,
                x: 3024,
                y: 0,
                width: 5120,
                height: 2880
            }
        );
        assert_eq!((l.width, l.height), (8144, 2880));
    }

    #[test]
    fn every_placement_lies_inside_the_canvas() {
        let screens = vec![
            screen(1, None, -1920.0, 120.0, 1920.0, 1080.0, 1.0),
            screen(2, None, 0.0, 0.0, 1440.0, 900.0, 2.0),
            screen(3, None, 1440.0, -700.5, 1280.0, 720.25, 1.5),
        ];
        let Layout {
            width,
            height,
            placements,
            ..
        } = layout(&screens).unwrap();
        for p in placements {
            assert!(p.x + p.width <= width, "{p:?}");
            assert!(p.y + p.height <= height, "{p:?}");
        }
    }

    #[test]
    fn nothing_to_place_is_an_error() {
        assert_eq!(layout(&[]), Err(LayoutError::Empty));
    }

    #[test]
    fn an_empty_or_invalid_screen_is_an_error() {
        let flat = screen(1, None, 0.0, 0.0, 0.0, 100.0, 1.0);
        assert_eq!(layout(&[flat]), Err(LayoutError::InvalidScreen));
        let nan = screen(1, None, f64::NAN, 0.0, 100.0, 100.0, 1.0);
        assert_eq!(layout(&[nan]), Err(LayoutError::InvalidScreen));
        let no_scale = screen(1, None, 0.0, 0.0, 100.0, 100.0, 0.0);
        assert_eq!(layout(&[no_scale]), Err(LayoutError::InvalidScreen));
    }

    #[test]
    fn an_absurd_canvas_is_refused_instead_of_allocated() {
        let huge = screen(1, None, 0.0, 0.0, 40_000.0, 40_000.0, 1.0);
        assert_eq!(layout(&[huge]), Err(LayoutError::TooLarge));
        let wide = screen(1, None, 0.0, 0.0, 100_000.0, 10.0, 1.0);
        assert_eq!(layout(&[wide]), Err(LayoutError::TooLarge));
    }
}
