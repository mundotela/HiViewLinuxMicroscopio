// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! PNG `tEXt` metadata for saved photos (docs/ARCHITECTURE.md §6.6,
//! M8): the active calibration, the mirror/rotation transforms and the
//! app version travel inside the file so a photo is self-describing.
//!
//! The `image` crate's PNG encoder writes no text chunks, so they are
//! spliced in after encoding — pure byte manipulation, CRC included,
//! no extra dependencies. JPEG carries no metadata (out of scope).

/// The PNG 8-byte signature.
const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// CRC-32 (ISO 3309, PNG polynomial) — table-free bitwise version: the
/// few bytes a tEXt chunk holds make table setup costlier than the
/// loop itself.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & u32::wrapping_neg(crc & 1));
        }
    }
    !crc
}

/// Builds one `tEXt` chunk (`length | "tEXt" | keyword\0text | CRC`).
/// Keywords are restricted to printable Latin-1 without a leading or
/// trailing space per spec; non-conforming keys are skipped by the
/// caller instead of written.
fn text_chunk(key: &str, value: &str) -> Option<Vec<u8>> {
    let key = key.trim();
    if key.is_empty() || key.len() > 79 || !key.is_ascii() {
        return None;
    }
    let mut payload = Vec::with_capacity(key.len() + 1 + value.len());
    payload.extend_from_slice(key.as_bytes());
    payload.push(0);
    payload.extend_from_slice(value.as_bytes());
    let mut chunk = Vec::with_capacity(payload.len() + 12);
    chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    let mut crc_input = Vec::with_capacity(payload.len() + 4);
    crc_input.extend_from_slice(b"tEXt");
    crc_input.extend_from_slice(&payload);
    chunk.extend_from_slice(&payload);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    Some(chunk)
}

/// Splices `entries` into a PNG byte stream as `tEXt` chunks right
/// after `IHDR` — the position every reader scans. Non-PNG input
/// (e.g. a JPEG buffer) and malformed streams are returned untouched.
#[must_use]
pub fn inject_png_text(png: &[u8], entries: &[(String, String)]) -> Vec<u8> {
    const IHDR: &[u8; 4] = b"IHDR";
    // Signature + IHDR length field + "IHDR" + 13-byte data + CRC.
    let ihdr_end = PNG_SIG.len() + 4 + 4 + 13 + 4;
    if entries.is_empty()
        || png.len() < ihdr_end
        || !png.starts_with(PNG_SIG)
        || &png[PNG_SIG.len() + 4..PNG_SIG.len() + 8] != IHDR
    {
        return png.to_vec();
    }
    let mut out = Vec::with_capacity(png.len() + entries.len() * 96);
    out.extend_from_slice(&png[..ihdr_end]);
    for (key, value) in entries {
        if let Some(chunk) = text_chunk(key, value) {
            out.extend_from_slice(&chunk);
        }
    }
    out.extend_from_slice(&png[ihdr_end..]);
    out
}

/// Reads every `tEXt` chunk back out of a PNG stream — used by tests
/// to round-trip [`inject_png_text`] and by future code that wants the
/// provenance of a saved photo.
#[must_use]
pub fn extract_png_text(png: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !png.starts_with(PNG_SIG) {
        return out;
    }
    let mut pos = PNG_SIG.len();
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        let kind = &png[pos + 4..pos + 8];
        let data_end = pos + 8 + len;
        if data_end + 4 > png.len() {
            break;
        }
        if kind == b"tEXt" {
            let data = &png[pos + 8..data_end];
            if let Some(nul) = data.iter().position(|b| *b == 0) {
                let key = String::from_utf8_lossy(&data[..nul]).into_owned();
                let value = String::from_utf8_lossy(&data[nul + 1..]).into_owned();
                out.push((key, value));
            }
        }
        pos = data_end + 4;
        if kind == b"IEND" {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn minimal_png() -> Vec<u8> {
        // 1x1 RGBA PNG, produced by the real encoder.
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]));
        let mut buf = Vec::new();
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap_or_else(|e| panic!("encode: {e}"));
        buf
    }

    #[test]
    fn text_round_trip_preserves_pairs_and_image() {
        let png = minimal_png();
        let entries = vec![
            ("Software".to_string(), "HiView 0.8.0".to_string()),
            ("Calibration".to_string(), "Anel máximo".to_string()),
            ("Calibration px/mm".to_string(), "89.000".to_string()),
        ];
        let tagged = inject_png_text(&png, &entries);
        assert_eq!(extract_png_text(&tagged), entries);
        // The image still decodes identically — chunks are valid PNG.
        let a = image::load_from_memory(&png).unwrap_or_else(|e| panic!("decode: {e}"));
        let b = image::load_from_memory(&tagged).unwrap_or_else(|e| panic!("decode: {e}"));
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn non_png_and_empty_inputs_pass_through() {
        assert_eq!(inject_png_text(b"jpeg-bytes", &[]), b"jpeg-bytes");
        let png = minimal_png();
        assert_eq!(inject_png_text(&png, &[]), png);
        // Truncated streams are not corrupted further.
        let cut = &png[..png.len() - 4];
        assert_eq!(inject_png_text(cut, &[]), cut);
    }

    #[test]
    fn invalid_keywords_are_skipped() {
        let png = minimal_png();
        let tagged = inject_png_text(
            &png,
            &[
                (String::new(), "v".into()),
                ("good".into(), "v".into()),
                ("k".repeat(90), "v".into()),
            ],
        );
        assert_eq!(
            extract_png_text(&tagged),
            vec![("good".to_string(), "v".to_string())]
        );
    }
}
