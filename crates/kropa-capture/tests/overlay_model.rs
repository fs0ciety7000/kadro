// SPDX-License-Identifier: GPL-3.0-or-later

//! The overlay model without a screen: monitor mapping, keyboard and pointer, HUD positions, loupe
//! and crop (lot 2.2).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // test sizes are small

use kropa_capture::selection::{
    ANNOUNCE_AT_EDGE, ANNOUNCE_MONITOR_CHANGED, HINT_DRAG_OR_SELECT_ALL, HINT_LAST_AREA_MISMATCH,
    HINT_NO_LAST_AREA, HINT_OTHER_MONITOR, Point,
};
use kropa_capture::{
    Input, Key, MapError, MonitorGeom, Outcome, PngError, RectF, Region, RgbaView, Selection,
    StoredArea, crop_png, hud_layout, magnifier, map_monitors,
};
use proptest::prelude::*;

fn geom(id: &str, x: f64, y: f64, w: f64, h: f64, scale: f64) -> MonitorGeom {
    MonitorGeom {
        id: id.into(),
        logical: RectF {
            x,
            y,
            width: w,
            height: h,
        },
        scale,
    }
}

fn single(width: f64, height: f64, scale: f64) -> Selection {
    let image = (
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    );
    let monitors = map_monitors(&[geom("DP-1", 0.0, 0.0, width, height, scale)], image).unwrap();
    Selection::new(monitors, 0, None)
}

fn dual(last: Option<StoredArea>) -> Selection {
    let monitors = map_monitors(
        &[
            geom("DP-1", 0.0, 0.0, 1280.0, 720.0, 1.0),
            geom("HDMI-1", 1280.0, 0.0, 1920.0, 1080.0, 1.0),
        ],
        (3200, 1080),
    )
    .unwrap();
    Selection::new(monitors, 0, last)
}

fn key(key: Key, shift: bool, alt: bool, primary: bool) -> Input {
    Input::Key {
        key,
        shift,
        alt,
        primary,
    }
}

fn plain(k: Key) -> Input {
    key(k, false, false, false)
}

fn at(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn drag(sel: &mut Selection, from: (f64, f64), to: (f64, f64)) {
    sel.handle(Input::Down(at(from.0, from.1)));
    sel.handle(Input::Move(at(to.0, to.1)));
    sel.handle(Input::Up(at(to.0, to.1)));
}

// ---------------------------------------------------------------- map_monitors

#[test]
fn maps_a_single_monitor_at_100_and_200_percent() {
    let one = map_monitors(&[geom("DP-1", 0.0, 0.0, 1280.0, 720.0, 1.0)], (1280, 720)).unwrap();
    assert_eq!((one[0].px.width, one[0].px.height), (1280, 720));
    let two = map_monitors(&[geom("eDP-1", 0.0, 0.0, 1280.0, 720.0, 2.0)], (2560, 1440)).unwrap();
    assert_eq!((two[0].px.width, two[0].px.height), (2560, 1440));
}

#[test]
fn maps_two_monitors_side_by_side_with_an_offset_origin() {
    let monitors = map_monitors(
        &[
            geom("A", -1920.0, 100.0, 1920.0, 1080.0, 1.0),
            geom("B", 0.0, 0.0, 1280.0, 720.0, 1.0),
        ],
        (3200, 1180),
    )
    .unwrap();
    assert_eq!((monitors[0].px.x, monitors[0].px.y), (0, 100));
    assert_eq!((monitors[1].px.x, monitors[1].px.y), (1920, 0));
}

#[test]
fn fractional_scale_rounds_by_at_most_one_pixel() {
    // 1536 x 864 logical at 1.25 is 1920 x 1080.
    assert!(map_monitors(&[geom("A", 0.0, 0.0, 1536.0, 864.0, 1.25)], (1920, 1080)).is_ok());
    assert!(map_monitors(&[geom("A", 0.0, 0.0, 1536.0, 864.0, 1.25)], (1921, 1080)).is_ok());
}

#[test]
fn an_inconsistent_layout_is_an_error_never_a_guess() {
    let cases: Vec<(Vec<MonitorGeom>, (u32, u32))> = vec![
        (vec![], (100, 100)),
        // The image is twice the size the geometry says.
        (vec![geom("A", 0.0, 0.0, 1280.0, 720.0, 1.0)], (2560, 1440)),
        // Mixed scales.
        (
            vec![
                geom("A", 0.0, 0.0, 1280.0, 720.0, 1.0),
                geom("B", 1280.0, 0.0, 1280.0, 720.0, 2.0),
            ],
            (3840, 1440),
        ),
        // Overlap (a mirrored pair).
        (
            vec![
                geom("A", 0.0, 0.0, 1280.0, 720.0, 1.0),
                geom("B", 0.0, 0.0, 1280.0, 720.0, 1.0),
            ],
            (1280, 720),
        ),
        (vec![geom("A", 0.0, 0.0, 0.0, 720.0, 1.0)], (1, 720)),
        (vec![geom("A", 0.0, 0.0, 10.0, 10.0, f64::NAN)], (10, 10)),
        (vec![geom("A", 0.0, 0.0, 10.0, 10.0, 1.0)], (0, 10)),
    ];
    for (geoms, image) in cases {
        let error = map_monitors(&geoms, image).unwrap_err();
        assert!(matches!(error, MapError::LayoutMismatch(_)), "{geoms:?}");
        assert_eq!(error.reason(), "layout_mismatch");
        assert_eq!(error.exit_code(), 10);
    }
}

// ------------------------------------------------------------------- keyboard

#[test]
fn enter_without_an_area_is_a_hint_then_ctrl_a_accepts_the_screen() {
    let mut sel = single(1280.0, 720.0, 1.0);
    assert_eq!(
        sel.handle(plain(Key::Enter)),
        Outcome::Hint(HINT_DRAG_OR_SELECT_ALL)
    );
    sel.handle(key(Key::A, false, false, true));
    let full = Region {
        monitor: 0,
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };
    assert_eq!(sel.handle(plain(Key::Enter)), Outcome::Accepted(full));
    assert_eq!(sel.handle(plain(Key::Escape)), Outcome::Cancelled);
}

#[test]
fn a_without_the_primary_key_does_nothing() {
    let mut sel = single(100.0, 100.0, 1.0);
    sel.handle(plain(Key::A));
    assert!(sel.region().is_none());
}

#[test]
fn arrows_move_the_crosshair_then_the_area_by_1_or_10_physical_pixels() {
    let mut sel = single(1280.0, 720.0, 2.0);
    let start = sel.cursor();
    sel.handle(plain(Key::Right));
    sel.handle(key(Key::Down, true, false, false));
    assert_eq!(sel.cursor(), (start.0 + 1, start.1 + 10));

    drag(&mut sel, (100.0, 100.0), (300.0, 200.0));
    let before = sel.region().unwrap();
    assert_eq!((before.width, before.height), (200, 100));
    sel.handle(plain(Key::Left));
    sel.handle(key(Key::Up, true, false, false));
    let after = sel.region().unwrap();
    assert_eq!((after.x, after.y), (before.x - 1, before.y - 10));
    assert_eq!((after.width, after.height), (200, 100));
}

#[test]
fn alt_arrows_resize_from_the_amber_handle() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (100.0, 100.0), (300.0, 200.0));
    sel.handle(key(Key::Right, false, true, false));
    sel.handle(key(Key::Down, true, true, false));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (100, 100, 201, 110));
    // Never below one pixel, and shrinking is not the screen edge.
    for _ in 0..30 {
        assert_eq!(
            sel.handle(key(Key::Left, true, true, false)),
            Outcome::Redraw
        );
    }
    assert_eq!(sel.region().unwrap().width, 1);
}

#[test]
fn pushing_the_area_against_the_edge_announces_it_and_keeps_it_inside() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (5.0, 10.0), (105.0, 60.0));
    let outcome = sel.handle(key(Key::Left, true, false, false));
    assert_eq!(outcome, Outcome::Announce(ANNOUNCE_AT_EDGE));
    assert_eq!(sel.region().unwrap().x, 0);
    // Growing past the right edge is announced too.
    sel.handle(key(Key::A, false, false, true));
    assert_eq!(
        sel.handle(key(Key::Right, false, true, false)),
        Outcome::Announce(ANNOUNCE_AT_EDGE)
    );
    assert_eq!(sel.region().unwrap().width, 1280);
}

#[test]
fn tab_is_absent_with_one_monitor_and_cycles_with_several() {
    let mut one = single(1280.0, 720.0, 1.0);
    assert_eq!(one.handle(plain(Key::Tab)), Outcome::Redraw);
    assert_eq!(one.active(), 0);

    let mut two = dual(None);
    two.handle(key(Key::A, false, false, true));
    assert_eq!(
        two.handle(plain(Key::Tab)),
        Outcome::Announce(ANNOUNCE_MONITOR_CHANGED)
    );
    assert_eq!(two.active(), 1);
    assert!(two.region().is_none(), "the area is limited to one screen");
    two.handle(key(Key::Tab, true, false, false));
    assert_eq!(two.active(), 0);
    two.handle(key(Key::Tab, true, false, false));
    assert_eq!(two.active(), 1);
}

#[test]
fn r_takes_the_last_area_only_on_an_image_of_the_same_size() {
    let region = Region {
        monitor: 1,
        x: 1500,
        y: 100,
        width: 400,
        height: 300,
    };
    let mut sel = dual(Some(StoredArea::new((3200, 1080), region)));
    assert_eq!(sel.handle(plain(Key::R)), Outcome::Redraw);
    assert_eq!(sel.active(), 1);
    assert_eq!(sel.region(), Some(region));

    let mut other = dual(Some(StoredArea::new((2560, 1440), region)));
    assert_eq!(
        other.handle(plain(Key::R)),
        Outcome::Hint(HINT_LAST_AREA_MISMATCH)
    );
    assert!(other.region().is_none());

    // A region that straddles two monitors (a stale file) is refused as well.
    let straddling = Region {
        monitor: 0,
        x: 1200,
        y: 0,
        width: 200,
        height: 100,
    };
    let mut bad = dual(Some(StoredArea::new((3200, 1080), straddling)));
    assert_eq!(
        bad.handle(plain(Key::R)),
        Outcome::Hint(HINT_LAST_AREA_MISMATCH)
    );

    let mut none = dual(None);
    assert_eq!(none.handle(plain(Key::R)), Outcome::Hint(HINT_NO_LAST_AREA));
}

#[test]
fn help_key_toggles_the_help() {
    let mut sel = single(100.0, 100.0, 1.0);
    assert!(sel.help_expanded());
    sel.handle(plain(Key::Help));
    assert!(!sel.help_expanded());
}

// -------------------------------------------------------------------- pointer

#[test]
fn dragging_draws_moves_and_resizes_the_area() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (400.0, 300.0), (200.0, 100.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (200, 100, 200, 200));

    // Inside: move.
    drag(&mut sel, (300.0, 200.0), (350.0, 230.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (250, 130, 200, 200));

    // The amber corner (bottom right) resizes.
    drag(&mut sel, (450.0, 330.0), (500.0, 400.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (250, 130, 250, 270));

    // A middle handle moves one edge only.
    drag(&mut sel, (375.0, 130.0), (375.0, 150.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (250, 150, 250, 250));
}

#[test]
fn a_click_without_movement_leaves_no_area_and_a_drag_off_screen_stops_at_the_edge() {
    let mut sel = single(1280.0, 720.0, 1.0);
    sel.handle(Input::Down(at(500.0, 500.0)));
    sel.handle(Input::Up(at(500.0, 500.0)));
    assert!(sel.region().is_none());
    drag(&mut sel, (1000.0, 600.0), (5000.0, 5000.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x + r.width, r.y + r.height), (1280, 720));
}

#[test]
fn a_press_on_another_monitor_is_a_hint_and_changes_nothing() {
    let mut sel = dual(None);
    assert_eq!(
        sel.handle(Input::Down(at(2000.0, 500.0))),
        Outcome::Hint(HINT_OTHER_MONITOR)
    );
    assert!(!sel.is_dragging());
    assert!(sel.region().is_none());
}

#[test]
fn a_small_area_has_no_middle_handles_and_corners_grab_outwards() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (100.0, 100.0), (130.0, 130.0));
    assert!(sel.is_small());
    let grabs = sel.handle_grabs();
    assert_eq!(grabs.len(), 4);
    let nw = grabs
        .iter()
        .find(|g| g.handle == kropa_capture::selection::Handle::NorthWest)
        .unwrap();
    assert_eq!((nw.left, nw.top), (76, 76));
    // Pressing just outside the corner resizes instead of starting a new area.
    drag(&mut sel, (90.0, 90.0), (80.0, 80.0));
    let r = sel.region().unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (80, 80, 50, 50));
}

// ------------------------------------------------------------------------ HUD

fn layout_text(sel: &Selection, viewport: (u32, u32)) -> String {
    format!("{:#?}", hud_layout(sel, viewport))
}

#[test]
fn hud_positions_with_room() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (400.0, 200.0), (800.0, 400.0));
    insta::assert_snapshot!("hud_with_room", layout_text(&sel, (1280, 720)));
}

#[test]
fn hud_flips_the_dimensions_above_when_the_area_touches_the_bottom() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (400.0, 400.0), (800.0, 720.0));
    insta::assert_snapshot!("hud_dimensions_above", layout_text(&sel, (1280, 720)));
}

#[test]
fn hud_puts_the_dimensions_inside_only_when_the_area_fills_the_screen() {
    let mut sel = single(1280.0, 720.0, 1.0);
    sel.handle(key(Key::A, false, false, true));
    let layout = hud_layout(&sel, (1280, 720));
    assert!(layout.dimensions_inside);
    insta::assert_snapshot!("hud_full_screen", format!("{layout:#?}"));
}

#[test]
fn hud_folds_the_help_into_a_pill_when_it_would_cover_the_area() {
    let mut sel = single(1280.0, 720.0, 1.0);
    drag(&mut sel, (100.0, 300.0), (1200.0, 700.0));
    insta::assert_snapshot!("hud_help_pill", layout_text(&sel, (1280, 720)));
}

#[test]
fn hud_at_200_percent_scales_the_labels() {
    let mut sel = single(1280.0, 720.0, 2.0);
    drag(&mut sel, (800.0, 400.0), (1600.0, 800.0));
    insta::assert_snapshot!("hud_200_percent", layout_text(&sel, (2560, 1440)));
}

#[test]
fn hud_without_an_area_has_no_dimensions() {
    let sel = single(1280.0, 720.0, 1.0);
    let layout = hud_layout(&sel, (1280, 720));
    assert!(layout.dimensions.is_none());
    insta::assert_snapshot!("hud_no_area", format!("{layout:#?}"));
}

proptest! {
    #[test]
    fn hud_never_covers_the_area_and_stays_in_the_viewport(
        x0 in 0.0f64..1280.0, y0 in 0.0f64..720.0, x1 in 0.0f64..1280.0, y1 in 0.0f64..720.0,
        scale in prop::sample::select(vec![1.0f64, 2.0]),
    ) {
        let mut sel = single(1280.0, 720.0, scale);
        drag(&mut sel, (x0 * scale, y0 * scale), (x1 * scale, y1 * scale));
        let viewport = ((1280.0 * scale) as u32, (720.0 * scale) as u32);
        let layout = hud_layout(&sel, viewport);
        let inside = |r: &kropa_capture::PxRect| r.x + r.width <= viewport.0 && r.y + r.height <= viewport.1;
        if let Some(zone) = sel.local_zone() {
            if let Some(d) = layout.dimensions {
                prop_assert!(layout.dimensions_inside || !d.overlaps(&zone));
                prop_assert!(inside(&d));
            }
            if let Some(m) = layout.magnifier {
                prop_assert!(!m.overlaps(&zone));
                prop_assert!(inside(&m));
            }
            match layout.help {
                kropa_capture::HelpLayout::Expanded(r) | kropa_capture::HelpLayout::Pill(r) => {
                    prop_assert!(!r.overlaps(&zone));
                    prop_assert!(inside(&r));
                }
                kropa_capture::HelpLayout::Hidden => {}
            }
        }
    }
}

// ------------------------------------------------------------ model invariants

#[derive(Debug, Clone)]
enum Step {
    Down(f64, f64),
    Move(f64, f64),
    Up(f64, f64),
    Key(u8, bool, bool, bool),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (-100.0f64..3400.0, -100.0f64..1200.0).prop_map(|(x, y)| Step::Down(x, y)),
        (-100.0f64..3400.0, -100.0f64..1200.0).prop_map(|(x, y)| Step::Move(x, y)),
        (-100.0f64..3400.0, -100.0f64..1200.0).prop_map(|(x, y)| Step::Up(x, y)),
        (0u8..10, any::<bool>(), any::<bool>(), any::<bool>())
            .prop_map(|(k, s, a, p)| Step::Key(k, s, a, p)),
    ]
}

fn to_input(step: &Step) -> Input {
    const KEYS: [Key; 10] = [
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::A,
        Key::R,
        Key::Tab,
        Key::Enter,
        Key::Help,
        Key::Escape,
    ];
    match *step {
        Step::Down(x, y) => Input::Down(at(x, y)),
        Step::Move(x, y) => Input::Move(at(x, y)),
        Step::Up(x, y) => Input::Up(at(x, y)),
        Step::Key(k, shift, alt, primary) => key(KEYS[usize::from(k)], shift, alt, primary),
    }
}

proptest! {
    #[test]
    fn the_area_always_stays_inside_one_monitor(steps in prop::collection::vec(step(), 0..60)) {
        let last = StoredArea::new((3200, 1080), Region { monitor: 1, x: 1500, y: 10, width: 300, height: 200 });
        let mut sel = dual(Some(last));
        for s in &steps {
            let outcome = sel.handle(to_input(s));
            let monitor = sel.monitors()[sel.active()].px;
            if let Some(r) = sel.region() {
                prop_assert!(r.width >= 1 && r.height >= 1);
                prop_assert!(r.x >= monitor.x && r.y >= monitor.y);
                prop_assert!(u64::from(r.x) + u64::from(r.width) <= u64::from(monitor.x + monitor.width));
                prop_assert!(u64::from(r.y) + u64::from(r.height) <= u64::from(monitor.y + monitor.height));
                prop_assert_eq!(r.monitor, sel.active());
                let zone = sel.local_zone().unwrap();
                prop_assert!(zone.x + zone.width <= monitor.width && zone.y + zone.height <= monitor.height);
            }
            let cursor = sel.cursor();
            prop_assert!(cursor.0 < monitor.width && cursor.1 < monitor.height);
            if let Outcome::Accepted(r) = outcome {
                prop_assert!(r.fits(sel.image_size()));
            }
        }
    }

    #[test]
    fn mapped_monitors_never_leave_the_image(
        w in 100.0f64..4000.0, h in 100.0f64..3000.0, scale in prop::sample::select(vec![1.0f64, 1.25, 1.5, 2.0, 3.0]),
    ) {
        let image = ((w * scale).round() as u32, (h * scale).round() as u32);
        let monitors = map_monitors(&[geom("A", 0.0, 0.0, w, h, scale)], image).unwrap();
        for m in monitors {
            prop_assert!(m.px.x + m.px.width <= image.0 && m.px.y + m.px.height <= image.1);
        }
    }
}

// --------------------------------------------------------------------- loupe

fn gradient(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
        }
    }
    data
}

#[test]
fn the_loupe_reads_15_by_15_pixels_and_the_hex_of_the_centre() {
    let data = gradient(40, 30);
    let view = RgbaView::new(&data, 40, 30).unwrap();
    let loupe = magnifier(&view, (20, 15));
    assert_eq!((loupe.x, loupe.y), (20, 15));
    assert_eq!(loupe.hex, "#140F23");
    assert_eq!(loupe.cells.len(), 225);
    assert_eq!(loupe.cells[7 * 15 + 7], Some([20, 15, 35, 255]));
    assert_eq!(loupe.cells[0], Some([13, 8, 21, 255]));
    assert!(loupe.cells.iter().all(Option::is_some));
}

#[test]
fn the_loupe_marks_cells_beyond_the_edges_and_clamps_a_centre_outside() {
    let data = gradient(40, 30);
    let view = RgbaView::new(&data, 40, 30).unwrap();
    let corner = magnifier(&view, (0, 0));
    let present: Vec<usize> = (0..225).filter(|&i| corner.cells[i].is_some()).collect();
    assert_eq!(present.len(), 8 * 8);
    assert_eq!(corner.cells[7 * 15 + 7], Some([0, 0, 0, 255]));
    assert_eq!(corner.hex, "#000000");

    let far = magnifier(&view, (500, 500));
    assert_eq!((far.x, far.y), (39, 29));
    assert_eq!(far.cells.iter().flatten().count(), 8 * 8);
    insta::assert_debug_snapshot!(
        "loupe_edge_cells",
        corner
            .cells
            .chunks(15)
            .map(|row| row
                .iter()
                .map(|c| if c.is_some() { '#' } else { '.' })
                .collect::<String>())
            .collect::<Vec<_>>()
    );
}

#[test]
fn rgba_view_rejects_a_wrong_length() {
    assert!(RgbaView::new(&[0; 7], 1, 2).is_none());
    assert!(RgbaView::new(&[], 0, 0).is_none());
}

// ----------------------------------------------------------------------- crop

mod crop {
    use super::*;
    use png::{BitDepth, ColorType, Decoder, Encoder, SrgbRenderingIntent, Transformations};

    struct Fixture {
        bytes: Vec<u8>,
        pixels: Vec<u8>,
        line: usize,
        bits_per_pixel: usize,
    }

    fn encode(
        width: u32,
        height: u32,
        color: ColorType,
        depth: BitDepth,
        pixels: &[u8],
        configure: impl FnOnce(&mut Encoder<'_, &mut Vec<u8>>),
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        configure(&mut encoder);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        writer.finish().unwrap();
        out
    }

    /// RGBA8 with a pattern that tells every pixel apart, plus metadata that must not survive.
    fn rgba(width: u32, height: u32) -> Fixture {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    (x * 7 + y) as u8,
                    (y * 5 + x) as u8,
                    (x ^ y) as u8,
                    if (x + y) % 5 == 0 { 128 } else { 255 },
                ]);
            }
        }
        let bytes = encode(
            width,
            height,
            ColorType::Rgba,
            BitDepth::Eight,
            &pixels,
            |e| {
                e.set_source_srgb(SrgbRenderingIntent::Perceptual);
                e.set_source_gamma(png::ScaledFloat::new(0.45455));
                e.set_source_chromaticities(png::SourceChromaticities::new(
                    (0.3127, 0.3290),
                    (0.64, 0.33),
                    (0.30, 0.60),
                    (0.15, 0.06),
                ));
                e.set_pixel_dims(Some(png::PixelDimensions {
                    xppu: 2835,
                    yppu: 2835,
                    unit: png::Unit::Meter,
                }));
            },
        );
        Fixture {
            bytes,
            pixels,
            line: width as usize * 4,
            bits_per_pixel: 32,
        }
    }

    fn decode(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>, png::Info<'static>) {
        let mut decoder = Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(Transformations::IDENTITY);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info, buf, reader.info().clone())
    }

    fn chunks(bytes: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = &bytes[8..];
        while rest.len() >= 12 {
            let len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            names.push(String::from_utf8_lossy(&rest[4..8]).into_owned());
            rest = &rest[12 + len..];
        }
        names
    }

    fn expected(fixture: &Fixture, region: Region) -> Vec<u8> {
        let bytes = fixture.bits_per_pixel / 8;
        let mut out = Vec::new();
        for row in region.y..region.y + region.height {
            let start = row as usize * fixture.line + region.x as usize * bytes;
            out.extend_from_slice(&fixture.pixels[start..start + region.width as usize * bytes]);
        }
        out
    }

    fn region(x: u32, y: u32, width: u32, height: u32) -> Region {
        Region {
            monitor: 0,
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn crops_byte_for_byte_at_100_percent() {
        let fixture = rgba(200, 120);
        let r = region(17, 9, 101, 63);
        let cut = crop_png(&fixture.bytes, r).unwrap();
        assert_eq!((cut.width, cut.height), (101, 63));
        let (info, pixels, _) = decode(&cut.data);
        assert_eq!((info.width, info.height), (101, 63));
        assert_eq!(pixels, expected(&fixture, r));
    }

    #[test]
    fn crops_byte_for_byte_at_200_percent() {
        // The same 200 x 120 logical screen at twice the density: 400 x 240 pixels.
        let fixture = rgba(400, 240);
        for r in [
            region(34, 18, 202, 126),
            region(0, 0, 400, 240),
            region(399, 239, 1, 1),
        ] {
            let cut = crop_png(&fixture.bytes, r).unwrap();
            let (_, pixels, _) = decode(&cut.data);
            assert_eq!(pixels, expected(&fixture, r), "{r:?}");
        }
    }

    #[test]
    fn keeps_the_colour_chunks_and_drops_everything_else() {
        let fixture = rgba(64, 64);
        let with_text = {
            // Re-encode with text and a time chunk the crop must not carry over.
            let mut out = Vec::new();
            let mut encoder = Encoder::new(&mut out, 64, 64);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);
            encoder
                .add_text_chunk("Author".into(), "somebody".into())
                .unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&fixture.pixels).unwrap();
            writer.finish().unwrap();
            out
        };
        assert!(chunks(&with_text).contains(&"tEXt".to_string()));
        let cut = crop_png(&with_text, region(1, 1, 10, 10)).unwrap();
        let names = chunks(&cut.data);
        assert!(names.contains(&"sRGB".to_string()), "{names:?}");
        assert!(!names.contains(&"tEXt".to_string()), "{names:?}");
        assert!(!names.contains(&"pHYs".to_string()), "{names:?}");

        // The fixture with gAMA + cHRM (and no sRGB) keeps both.
        let plain = encode(
            8,
            8,
            ColorType::Rgb,
            BitDepth::Eight,
            &[7; 8 * 8 * 3],
            |e| {
                e.set_source_gamma(png::ScaledFloat::new(0.5));
                e.set_source_chromaticities(png::SourceChromaticities::new(
                    (0.31, 0.33),
                    (0.64, 0.33),
                    (0.30, 0.60),
                    (0.15, 0.06),
                ));
            },
        );
        let names = chunks(&crop_png(&plain, region(0, 0, 4, 4)).unwrap().data);
        assert!(names.contains(&"gAMA".to_string()), "{names:?}");
        assert!(names.contains(&"cHRM".to_string()), "{names:?}");
        assert!(!names.contains(&"sRGB".to_string()), "{names:?}");
    }

    #[test]
    fn keeps_an_embedded_profile() {
        let fixture = rgba(16, 16);
        let mut out = Vec::new();
        let mut info = png::Info::with_size(16, 16);
        info.color_type = ColorType::Rgba;
        info.bit_depth = BitDepth::Eight;
        info.icc_profile = Some(std::borrow::Cow::Owned(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        let mut writer = Encoder::with_info(&mut out, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer
            .write_image_data(&fixture.pixels[..16 * 16 * 4])
            .unwrap();
        writer.finish().unwrap();
        let cut = crop_png(&out, region(2, 2, 4, 4)).unwrap();
        assert!(chunks(&cut.data).contains(&"iCCP".to_string()));
        let (_, _, info) = decode(&cut.data);
        assert_eq!(
            info.icc_profile.as_deref(),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
        );
    }

    #[test]
    fn crops_sixteen_bit_and_packed_and_palette_images() {
        // 16-bit greyscale + alpha.
        let mut sixteen = Vec::new();
        for i in 0..(20 * 10) {
            sixteen.extend_from_slice(&[(i >> 8) as u8, i as u8, 0xAB, (i * 3) as u8]);
        }
        let bytes = encode(
            20,
            10,
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &sixteen,
            |_| {},
        );
        let fixture = Fixture {
            bytes,
            pixels: sixteen,
            line: 20 * 4,
            bits_per_pixel: 32,
        };
        let r = region(3, 2, 9, 5);
        let (_, pixels, _) = decode(&crop_png(&fixture.bytes, r).unwrap().data);
        assert_eq!(pixels, expected(&fixture, r));

        // 2-bit greyscale: 4 pixels per byte, rows of 10 pixels padded to 3 bytes.
        let mut packed = Vec::new();
        let sample = |x: u32, y: u32| ((x * 3 + y) % 4) as u8;
        for y in 0..6u32 {
            for chunk in (0..10u32).collect::<Vec<_>>().chunks(4) {
                let mut byte = 0u8;
                for (i, x) in chunk.iter().enumerate() {
                    byte |= sample(*x, y) << (6 - 2 * i);
                }
                packed.push(byte);
            }
        }
        let bytes = encode(10, 6, ColorType::Grayscale, BitDepth::Two, &packed, |_| {});
        let cut = crop_png(&bytes, region(3, 1, 5, 4)).unwrap();
        let (info, out, _) = decode(&cut.data);
        assert_eq!(info.bit_depth, BitDepth::Two);
        for row in 0..4u32 {
            for col in 0..5u32 {
                let at_ = row as usize * info.line_size + (col as usize * 2) / 8;
                let value = (out[at_] >> (6 - (col as usize * 2) % 8)) & 3;
                assert_eq!(value, sample(3 + col, 1 + row), "({col},{row})");
            }
        }

        // 4-bit palette with transparency: the palette and tRNS survive.
        let indices: Vec<u8> = (0..8).flat_map(|_| [0x01u8, 0x23, 0x45, 0x67]).collect();
        let palette: Vec<u8> = (0..8u8).flat_map(|i| [i * 30, 255 - i * 30, i]).collect();
        let bytes = encode(8, 8, ColorType::Indexed, BitDepth::Four, &indices, |e| {
            e.set_palette(palette.clone());
            e.set_trns(vec![255, 128]);
        });
        let cut = crop_png(&bytes, region(2, 2, 4, 3)).unwrap();
        let (info, out, meta) = decode(&cut.data);
        assert_eq!(meta.palette.as_deref(), Some(palette.as_slice()));
        assert_eq!(meta.trns.as_deref(), Some(&[255u8, 128][..]));
        assert_eq!(&out[..info.line_size], &[0x23, 0x45]);
    }

    #[test]
    fn refuses_a_region_outside_the_image_and_bad_data() {
        let fixture = rgba(10, 10);
        for r in [
            region(0, 0, 0, 5),
            region(0, 0, 5, 0),
            region(6, 0, 5, 5),
            region(0, 6, 5, 5),
            region(u32::MAX, 0, 2, 2),
        ] {
            assert_eq!(
                crop_png(&fixture.bytes, r).unwrap_err(),
                PngError::OutOfBounds
            );
        }
        assert_eq!(
            crop_png(b"not a png", region(0, 0, 1, 1)).unwrap_err(),
            PngError::Decode
        );
        assert_eq!(
            crop_png(
                &fixture.bytes[..fixture.bytes.len() / 2],
                region(0, 0, 1, 1)
            )
            .unwrap_err(),
            PngError::Decode
        );
    }

    proptest! {
        #[test]
        fn any_region_inside_the_image_crops_exactly(
            x in 0u32..60, y in 0u32..40, w in 1u32..60, h in 1u32..40,
        ) {
            let fixture = rgba(60, 40);
            let r = region(x, y, w.min(60 - x), h.min(40 - y));
            let cut = crop_png(&fixture.bytes, r).unwrap();
            let (_, pixels, _) = decode(&cut.data);
            prop_assert_eq!(pixels, expected(&fixture, r));
        }
    }
}
