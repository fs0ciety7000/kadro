// SPDX-License-Identifier: GPL-3.0-or-later

//! Pictures: several displays on one canvas, and PNG encoding.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{CFData, CFMutableData, CFRetained, CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage, CGInterpolationQuality,
    kCGColorSpaceSRGB,
};
use objc2_image_io::CGImageDestination;

use crate::screens::{Layout, Placement};

/// Alpha last, premultiplied, 8 bits per component, default byte order: `kCGImageAlphaPremultipliedLast`.
const RGBA_PREMULTIPLIED: u32 = 1;

// `CGBitmapContextCreate` is the one CoreGraphics function this module needs that the generated
// bindings leave out.
unsafe extern "C-unwind" {
    // SAFETY (declaration): matches `CGBitmapContext.h`; CoreGraphics is linked by
    // `objc2-core-graphics`.
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Option<&CGColorSpace>,
        bitmap_info: u32,
    ) -> Option<NonNull<CGContext>>;
}

/// Draws each picture on a canvas of the size of `layout`, in its rectangle.
///
/// The canvas is sRGB with an alpha channel: the gaps between displays of different heights stay
/// transparent. A picture of a coarser display is stretched to its rectangle.
pub(super) fn compose(
    layout: &Layout,
    frames: &[(Placement, CFRetained<CGImage>)],
) -> Result<CFRetained<CGImage>, String> {
    // SAFETY: `kCGColorSpaceSRGB` is a constant of CoreGraphics, valid for the life of the process.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or("the sRGB color space is not available")?;
    let width = usize::try_from(layout.width).map_err(|e| e.to_string())?;
    let height = usize::try_from(layout.height).map_err(|e| e.to_string())?;
    // SAFETY: a null buffer asks CoreGraphics to allocate and own it; `bytes_per_row` 0 lets it
    // choose; 8 bits per component with premultiplied alpha is a supported combination for an RGB
    // color space. The returned context is owned by us (the function name has "Create").
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            Some(&space),
            RGBA_PREMULTIPLIED,
        )
    }
    .map(|ptr| {
        // SAFETY: see above: +1 reference that `from_raw` takes over.
        unsafe { CFRetained::from_raw(ptr) }
    })
    .ok_or("the canvas could not be allocated")?;
    CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
    for (place, image) in frames {
        // CoreGraphics counts y upwards from the bottom of the canvas; the layout counts down.
        let y = layout.height - place.y - place.height;
        let rect = CGRect::new(
            CGPoint::new(f64::from(place.x), f64::from(y)),
            CGSize::new(f64::from(place.width), f64::from(place.height)),
        );
        CGContext::draw_image(Some(&context), rect, Some(image));
    }
    CGBitmapContextCreateImage(Some(&context)).ok_or_else(|| "the canvas has no image".to_owned())
}

/// Encodes a picture as PNG with `ImageIO`, with the color profile of the display.
///
/// `ImageIO` may add its own chunks (resolution, text); the caller passes the bytes through
/// [`crate::sanitize_png`], which keeps only what the pixels need.
pub(super) fn encode_png(image: &CGImage) -> Result<Vec<u8>, String> {
    let data = CFMutableData::new(None, 0).ok_or("no memory for the image")?;
    let kind = CFString::from_static_str("public.png");
    // SAFETY: `data` and `kind` are valid objects that outlive the destination; one image is
    // announced and exactly one is added; no options dictionary is passed (null is allowed).
    let destination = unsafe { CGImageDestination::with_data(&data, &kind, 1, None) }
        .ok_or("ImageIO has no PNG encoder")?;
    // SAFETY: valid image, no properties.
    unsafe { destination.add_image(image, None) };
    // SAFETY: called once, after the single image was added.
    if !unsafe { destination.finalize() } {
        return Err("ImageIO could not finish the PNG".to_owned());
    }
    let bytes = CFData::to_vec(&data);
    if bytes.is_empty() {
        return Err("ImageIO produced no data".to_owned());
    }
    Ok(bytes)
}
