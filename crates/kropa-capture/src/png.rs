// SPDX-License-Identifier: GPL-3.0-or-later

//! PNG structure check and metadata removal.
//!
//! A screenshot handed over by a portal may carry text, EXIF, time or resolution chunks that tell
//! nothing the user asked to share. [`sanitize`] checks the chunk structure and the checksums, then
//! rewrites the file with the chunks that decide how the pixels look (header, palette, image data,
//! transparency, colour space) and nothing else. The pixel data is never decoded nor re-encoded.

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Largest dimension the PNG specification allows.
const MAX_DIMENSION: u32 = i32::MAX as u32;
/// Chunks that are copied as they are: the critical ones, plus those that change the colours.
const KEPT: [&[u8; 4]; 9] = [
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"iCCP",
];
/// Colour chunks added by later revisions of the specification.
const KEPT_LATE: [&[u8; 4]; 3] = [b"cICP", b"mDCV", b"cLLI"];

/// Why a byte string is not a usable PNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PngError {
    /// The 8-byte signature is missing.
    #[error("not a PNG file (bad signature)")]
    Signature,
    /// The data ends in the middle of a chunk, or a length is absurd.
    #[error("truncated PNG data")]
    Truncated,
    /// The first chunk is not a valid `IHDR`.
    #[error("missing or invalid IHDR chunk")]
    Header,
    /// The image has a zero or out-of-range dimension.
    #[error("invalid image dimensions")]
    Dimensions,
    /// A kept chunk fails its CRC-32 check.
    #[error("checksum mismatch in a {0} chunk")]
    Checksum(&'static str),
    /// There is no `IDAT` chunk, or no `IEND` at the end.
    #[error("missing image data or IEND chunk")]
    Incomplete,
}

/// A PNG without metadata chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanPng {
    /// The rewritten file.
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// CRC-32 (ISO 3309, as PNG uses it), table built at compile time.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        #[expect(clippy::cast_possible_truncation, reason = "n < 256")]
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = u32::MAX;
    for part in parts {
        for &byte in *part {
            c = CRC_TABLE[((c ^ u32::from(byte)) & 0xff) as usize] ^ (c >> 8);
        }
    }
    !c
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn kept(kind: &[u8; 4]) -> bool {
    KEPT.contains(&kind) || KEPT_LATE.contains(&kind)
}

fn name(kind: &[u8; 4]) -> &'static str {
    KEPT.iter()
        .chain(KEPT_LATE.iter())
        .find(|k| **k == kind)
        .map_or("unknown", |k| std::str::from_utf8(*k).unwrap_or("unknown"))
}

/// Checks the structure of `input` and returns it without metadata chunks.
///
/// # Errors
/// [`PngError`] when the data is not a complete, well-formed PNG.
pub fn sanitize(input: &[u8]) -> Result<CleanPng, PngError> {
    let mut rest = input.strip_prefix(&SIGNATURE).ok_or(PngError::Signature)?;
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&SIGNATURE);
    let mut dimensions = None;
    let mut has_data = false;
    let mut ended = false;
    while !rest.is_empty() {
        if ended {
            // Bytes after IEND are not part of the image (and may hide anything).
            break;
        }
        if rest.len() < 12 {
            return Err(PngError::Truncated);
        }
        let length = usize::try_from(be32(&rest[..4])).map_err(|_| PngError::Truncated)?;
        let kind: [u8; 4] = [rest[4], rest[5], rest[6], rest[7]];
        let end = length.checked_add(12).ok_or(PngError::Truncated)?;
        if rest.len() < end {
            return Err(PngError::Truncated);
        }
        let (chunk, tail) = rest.split_at(end);
        let data = &chunk[8..8 + length];
        if dimensions.is_none() {
            if &kind != b"IHDR" || length != 13 {
                return Err(PngError::Header);
            }
            let (w, h) = (be32(&data[..4]), be32(&data[4..8]));
            if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION {
                return Err(PngError::Dimensions);
            }
            dimensions = Some((w, h));
        }
        if kept(&kind) {
            if be32(&chunk[8 + length..]) != crc32(&[&kind, data]) {
                return Err(PngError::Checksum(name(&kind)));
            }
            out.extend_from_slice(chunk);
        }
        has_data |= &kind == b"IDAT";
        ended = &kind == b"IEND";
        rest = tail;
    }
    let (width, height) = dimensions.ok_or(PngError::Header)?;
    if !has_data || !ended {
        return Err(PngError::Incomplete);
    }
    Ok(CleanPng {
        data: out,
        width,
        height,
    })
}

/// Builds a PNG from chunks, with correct checksums (tests).
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn build_png(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    for (kind, data) in chunks {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(*kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&[*kind, data]).to_be_bytes());
    }
    out
}

/// A minimal valid PNG of `width` x `height` made of the given extra chunks (tests).
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn sample_png(width: u32, height: u32, extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // 8 bits, greyscale, deflate, no filter, no interlace.
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    // Not real pixels: the sanitizer never decodes the image data.
    let idat: &[u8] = &[0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01];
    let mut chunks: Vec<(&[u8; 4], &[u8])> = vec![(b"IHDR", &ihdr)];
    chunks.extend_from_slice(extra);
    chunks.push((b"IDAT", idat));
    chunks.push((b"IEND", &[]));
    build_png(&chunks)
}

#[cfg(test)]
mod tests {
    use super::{PngError, build_png, crc32, sample_png, sanitize};

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xcbf4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xcbf4_3926);
    }

    #[test]
    fn metadata_chunks_are_dropped_and_colour_chunks_kept() {
        let input = sample_png(
            640,
            480,
            &[
                (b"tEXt", b"Software\0gnome-shell"),
                (b"gAMA", &[0, 1, 0x86, 0xa0]),
                (b"eXIf", b"MM\0*"),
                (b"tIME", &[7, 234, 11, 11, 14, 3, 22]),
                (b"pHYs", &[0, 0, 0x0e, 0xc4, 0, 0, 0x0e, 0xc4, 1]),
                (b"zTXt", b"Comment\0\0xx"),
            ],
        );
        let clean = sanitize(&input).unwrap();
        assert_eq!((clean.width, clean.height), (640, 480));
        let expected = sample_png(640, 480, &[(b"gAMA", &[0, 1, 0x86, 0xa0])]);
        assert_eq!(clean.data, expected);
        assert!(clean.data.len() < input.len());
    }

    #[test]
    fn a_clean_file_is_returned_unchanged() {
        let input = sample_png(2, 3, &[]);
        assert_eq!(sanitize(&input).unwrap().data, input);
    }

    #[test]
    fn bytes_after_iend_are_dropped() {
        let mut input = sample_png(2, 2, &[]);
        let clean_len = input.len();
        input.extend_from_slice(b"hidden payload");
        assert_eq!(sanitize(&input).unwrap().data.len(), clean_len);
    }

    #[test]
    fn rejects_what_is_not_a_png() {
        assert_eq!(sanitize(b"GIF89a....").unwrap_err(), PngError::Signature);
        assert_eq!(sanitize(&[]).unwrap_err(), PngError::Signature);
    }

    #[test]
    fn rejects_truncated_data() {
        let input = sample_png(4, 4, &[]);
        for cut in [9, 20, input.len() - 5] {
            assert_eq!(
                sanitize(&input[..cut]).unwrap_err(),
                PngError::Truncated,
                "{cut}"
            );
        }
    }

    #[test]
    fn rejects_a_missing_iend() {
        let input = sample_png(4, 4, &[]);
        // Drop the whole IEND chunk (12 bytes).
        assert_eq!(
            sanitize(&input[..input.len() - 12]).unwrap_err(),
            PngError::Incomplete
        );
    }

    #[test]
    fn rejects_a_corrupt_kept_chunk() {
        let mut input = sample_png(4, 4, &[]);
        // Flip a byte of the IHDR data (offset 8 + 8 = 16 is the width).
        input[19] ^= 0x01;
        assert_eq!(sanitize(&input).unwrap_err(), PngError::Checksum("IHDR"));
    }

    #[test]
    fn rejects_a_bad_header() {
        let no_ihdr = build_png(&[(b"IDAT", &[1, 2, 3]), (b"IEND", &[])]);
        assert_eq!(sanitize(&no_ihdr).unwrap_err(), PngError::Header);
        assert_eq!(
            sanitize(&sample_png(0, 10, &[])).unwrap_err(),
            PngError::Dimensions
        );
        assert_eq!(
            sanitize(&sample_png(u32::MAX, 10, &[])).unwrap_err(),
            PngError::Dimensions
        );
    }

    #[test]
    fn rejects_an_absurd_chunk_length() {
        let mut input = sample_png(4, 4, &[]);
        // Length of the IHDR chunk set to 4 GiB - 1.
        input[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(sanitize(&input).unwrap_err(), PngError::Truncated);
    }
}
