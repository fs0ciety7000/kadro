// SPDX-License-Identifier: GPL-3.0-or-later

//! The result of a selection (a [`Region`] of the captured image), the crop of a PNG to it, and the
//! loupe of the overlay.
//!
//! No system call and no toolkit: the same code serves the GTK and the `AppKit` overlays, and it is
//! tested without a screen.

use std::io::Cursor;

use png::{BitDepth, Decoder, Encoder, Info, Limits, Transformations};
use serde::{Deserialize, Serialize};

use crate::png::{CleanPng, PngError};

/// An area of the captured image, in **pixels of the image** (physical pixels), inside one monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// Index of the monitor the area lies on (in the order of `map_monitors`).
    pub monitor: usize,
    /// Left edge, in pixels of the image.
    pub x: u32,
    /// Top edge, in pixels of the image.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Region {
    /// Whether the area is non-empty and lies inside an image of `image` pixels.
    #[must_use]
    pub fn fits(&self, image: (u32, u32)) -> bool {
        self.width > 0
            && self.height > 0
            && u64::from(self.x) + u64::from(self.width) <= u64::from(image.0)
            && u64::from(self.y) + u64::from(self.height) <= u64::from(image.1)
    }
}

/// Most bytes the decoder may allocate: a 16384 x 16384 RGBA canvas is 1 GiB.
const DECODE_LIMIT: usize = 1 << 30;

/// Cuts `region` out of the PNG `input`.
///
/// The pixels are decoded without any colour conversion (palette, bit depth and transparency are
/// kept) and the area is encoded again, so that the rows outside it never reach the output. Of the
/// metadata only the chunks that decide how the pixels look are kept: `sRGB`, `gAMA`, `cHRM` and
/// `iCCP` (as [`sanitize_png`](crate::sanitize_png) does, which also drops text, EXIF, time and
/// resolution chunks).
///
/// # Errors
/// [`PngError::OutOfBounds`] when the area is empty or leaves the image, [`PngError::Decode`] when
/// the data cannot be decoded.
pub fn crop_png(input: &[u8], region: Region) -> Result<CleanPng, PngError> {
    let mut decoder = Decoder::new_with_limits(
        Cursor::new(input),
        Limits {
            bytes: DECODE_LIMIT,
        },
    );
    decoder.set_transformations(Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|_| PngError::Decode)?;
    let source = reader.info().clone();
    if !region.fits((source.width, source.height)) {
        return Err(PngError::OutOfBounds);
    }
    let size = reader.output_buffer_size().ok_or(PngError::Decode)?;
    let mut pixels = vec![0u8; size];
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(|_| PngError::Decode)?;
    let depth = bit_depth(frame.bit_depth);
    let bits_per_pixel = frame.color_type.samples() * depth;
    let cropped = crop_rows(&pixels, frame.line_size, bits_per_pixel, region);

    let mut info = Info::with_size(region.width, region.height);
    info.color_type = frame.color_type;
    info.bit_depth = frame.bit_depth;
    info.palette.clone_from(&source.palette);
    info.trns.clone_from(&source.trns);
    info.srgb = source.srgb;
    info.source_gamma = source.gama_chunk;
    info.source_chromaticities = source.chrm_chunk;
    info.icc_profile.clone_from(&source.icc_profile);
    let mut out = Vec::new();
    let mut writer = Encoder::with_info(&mut out, info)
        .and_then(Encoder::write_header)
        .map_err(|_| PngError::Decode)?;
    writer
        .write_image_data(&cropped)
        .map_err(|_| PngError::Decode)?;
    writer.finish().map_err(|_| PngError::Decode)?;
    Ok(CleanPng {
        data: out,
        width: region.width,
        height: region.height,
    })
}

fn bit_depth(depth: BitDepth) -> usize {
    match depth {
        BitDepth::One => 1,
        BitDepth::Two => 2,
        BitDepth::Four => 4,
        BitDepth::Eight => 8,
        BitDepth::Sixteen => 16,
    }
}

/// Copies the columns `x..x + width` of rows `y..y + height` of packed rows of `line` bytes.
fn crop_rows(pixels: &[u8], line: usize, bits_per_pixel: usize, region: Region) -> Vec<u8> {
    let (x, y) = (region.x as usize, region.y as usize);
    let (width, height) = (region.width as usize, region.height as usize);
    let out_line = (width * bits_per_pixel).div_ceil(8);
    let mut out = vec![0u8; out_line * height];
    for row in 0..height {
        let source = &pixels[(y + row) * line..(y + row + 1) * line];
        let target = &mut out[row * out_line..(row + 1) * out_line];
        if bits_per_pixel.is_multiple_of(8) {
            let bytes = bits_per_pixel / 8;
            target.copy_from_slice(&source[x * bytes..(x + width) * bytes]);
        } else {
            // 1, 2 or 4 bits per pixel: copy bit by bit, most significant bit first.
            for bit in 0..width * bits_per_pixel {
                let from = x * bits_per_pixel + bit;
                if source[from / 8] >> (7 - from % 8) & 1 == 1 {
                    target[bit / 8] |= 1 << (7 - bit % 8);
                }
            }
        }
    }
    out
}

/// Side of the loupe, in pixels of the image (`DESIGN.md` section 8.1).
pub const LOUPE_SIDE: usize = 15;

/// A read-only view of RGBA8 pixels (4 bytes each, rows without padding), as the overlay holds the
/// frozen image.
#[derive(Debug, Clone, Copy)]
pub struct RgbaView<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
}

impl<'a> RgbaView<'a> {
    /// Wraps `data`; `None` when its length is not `width * height * 4` or the image is empty.
    #[must_use]
    pub fn new(data: &'a [u8], width: u32, height: u32) -> Option<Self> {
        let expected = u64::from(width) * u64::from(height) * 4;
        (width > 0 && height > 0 && u64::try_from(data.len()).ok()? == expected).then_some(Self {
            data,
            width,
            height,
        })
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ]
    }
}

/// What the loupe shows around a pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loupe {
    /// Centre of the loupe, in pixels of the image (the requested point, brought inside the image).
    pub x: u32,
    /// See [`Loupe::x`].
    pub y: u32,
    /// `#RRGGBB` of the centre pixel (the alpha channel is not shown).
    pub hex: String,
    /// The 15 x 15 pixels around the centre, row by row; `None` where the window leaves the image.
    pub cells: Vec<Option<[u8; 4]>>,
}

/// The loupe at `center`: 15 x 15 pixels, the hexadecimal colour and the coordinates of the centre.
/// A centre outside the image is brought to its nearest pixel; near an edge the cells beyond it are
/// `None` (the centre pixel stays at the middle of the window).
#[must_use]
pub fn magnifier(view: &RgbaView<'_>, center: (u32, u32)) -> Loupe {
    let cx = center.0.min(view.width - 1);
    let cy = center.1.min(view.height - 1);
    let half = i64::try_from(LOUPE_SIDE / 2).unwrap_or(7);
    let mut cells = Vec::with_capacity(LOUPE_SIDE * LOUPE_SIDE);
    for row in 0..LOUPE_SIDE {
        for col in 0..LOUPE_SIDE {
            let px = i64::from(cx) + i64::try_from(col).unwrap_or(0) - half;
            let py = i64::from(cy) + i64::try_from(row).unwrap_or(0) - half;
            let cell = match (u32::try_from(px), u32::try_from(py)) {
                (Ok(px), Ok(py)) if px < view.width && py < view.height => Some(view.pixel(px, py)),
                _ => None,
            };
            cells.push(cell);
        }
    }
    let [red, green, blue, _] = view.pixel(cx, cy);
    Loupe {
        x: cx,
        y: cy,
        hex: format!("#{red:02X}{green:02X}{blue:02X}"),
        cells,
    }
}
