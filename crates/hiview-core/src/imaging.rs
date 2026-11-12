// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Still-photo imaging (docs/ARCHITECTURE.md §6.6): output resolutions,
//! strided-RGBA conversion, PNG/JPEG encoding and save-file naming.
//!
//! All functions are pure — no GTK/GStreamer — so they run on the photo
//! worker thread and are unit-tested without hardware.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// JPEG quality for saved photos (docs/ARCHITECTURE.md §6.6).
pub const JPEG_QUALITY: u8 = 92;

/// Photo file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhotoFormat {
    /// Lossless PNG (default).
    #[default]
    Png,
    /// JPEG at [`JPEG_QUALITY`].
    Jpeg,
}

impl PhotoFormat {
    /// File extension without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// Saved-photo resolution option (docs/ARCHITECTURE.md §6.6). The two
/// interpolated options are labelled "interpolada" in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhotoResolution {
    /// Sensor native size — 640×480, or 480×640 after a 90° rotation.
    #[default]
    Native,
    /// 2.0× Lanczos3 upscale — 1280×960 (960×1280 rotated).
    #[serde(rename = "1280x960")]
    X2,
    /// 2.5× Lanczos3 upscale — 1600×1200 (1200×1600 rotated).
    #[serde(rename = "1600x1200")]
    X2p5,
}

impl PhotoResolution {
    /// Output pixel size for a source of `(width, height)`.
    #[must_use]
    pub fn output_size(self, src: (u32, u32)) -> (u32, u32) {
        match self {
            Self::Native => src,
            Self::X2 => (src.0 * 2, src.1 * 2),
            Self::X2p5 => (src.0 * 5 / 2, src.1 * 5 / 2),
        }
    }
}

/// Photo file name for a capture instant: `HiView_AAAA-MM-DD_HH-MM-SS.ext`
/// (docs/ARCHITECTURE.md §6.6).
#[must_use]
pub fn photo_filename(at: DateTime<Local>, format: PhotoFormat) -> String {
    media_filename(at, format.extension())
}

/// Video file name for a capture instant: `HiView_AAAA-MM-DD_HH-MM-SS.mp4`
/// (docs/ARCHITECTURE.md §6.7). Same pattern and collision rule as photos.
#[must_use]
pub fn video_filename(at: DateTime<Local>) -> String {
    media_filename(at, "mp4")
}

/// Shared `HiView_<timestamp>.<ext>` naming for captured media.
fn media_filename(at: DateTime<Local>, ext: &str) -> String {
    format!("HiView_{}.{}", at.format("%Y-%m-%d_%H-%M-%S"), ext)
}

/// Returns `dir/filename`, or `…_2`, `…_3`, … on collision (§6.6).
#[must_use]
pub fn unique_photo_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = filename
        .rsplit_once('.')
        .map_or((filename, ""), |(s, e)| (s, e));
    let mut n = 2u32;
    loop {
        let candidate = dir.join(format!("{stem}_{n}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Copies a strided RGBA buffer into a tightly packed [`RgbaImage`].
///
/// `data` must cover `(height - 1) * stride + width * 4` bytes.
pub fn rgba_image(data: &[u8], width: u32, height: u32, stride: usize) -> Result<RgbaImage> {
    let row_bytes = width as usize * 4;
    let required = usize::try_from(height)
        .ok()
        .and_then(|h| h.checked_sub(1))
        .and_then(|h| h.checked_mul(stride))
        .and_then(|off| off.checked_add(row_bytes));
    if required.is_none_or(|need| data.len() < need) {
        return Err(Error::BadFrame {
            width,
            height,
            stride,
            len: data.len(),
        });
    }
    let mut image = RgbaImage::new(width, height);
    for y in 0..height as usize {
        let src = y * stride;
        let dst = y * row_bytes;
        image.as_mut()[dst..dst + row_bytes].copy_from_slice(&data[src..src + row_bytes]);
    }
    Ok(image)
}

/// Converts and upscales a strided RGBA frame to the output resolution
/// (Lanczos3 when interpolated), without encoding.
///
/// # Errors
/// Returns [`Error::BadFrame`] when the buffer is too short.
pub fn render_photo(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    resolution: PhotoResolution,
) -> Result<RgbaImage> {
    let image = rgba_image(data, width, height, stride)?;
    let (out_w, out_h) = resolution.output_size((width, height));
    Ok(if (out_w, out_h) == (width, height) {
        image
    } else {
        image::imageops::resize(&image, out_w, out_h, FilterType::Lanczos3)
    })
}

/// Encodes an image at output resolution to PNG or JPEG.
///
/// # Errors
/// Returns [`Error`] on encoder failure.
pub fn encode_image(image: &RgbaImage, format: PhotoFormat) -> Result<Vec<u8>> {
    let (out_w, out_h) = image.dimensions();
    let mut buf = Vec::new();
    match format {
        PhotoFormat::Png => PngEncoder::new(&mut buf).write_image(
            image.as_raw(),
            out_w,
            out_h,
            ExtendedColorType::Rgba8,
        )?,
        PhotoFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
            JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY).write_image(
                rgb.as_raw(),
                out_w,
                out_h,
                ExtendedColorType::Rgb8,
            )?;
        }
    }
    Ok(buf)
}

/// Encodes a strided RGBA frame to PNG or JPEG, upscaling with Lanczos3
/// when `resolution` is interpolated (§6.6).
pub fn encode_photo(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    resolution: PhotoResolution,
    format: PhotoFormat,
) -> Result<Vec<u8>> {
    encode_image(
        &render_photo(data, width, height, stride, resolution)?,
        format,
    )
}

/// Full save operation for the worker thread: creates the folder, picks a
/// collision-free name, encodes and writes the file. Returns the path.
///
/// `apply` runs on the output-resolution image right before encoding —
/// the app uses it to burn overlays into the photo (§6.6, M6).
// One argument over the lint budget: the frame fields mirror `save_photo`,
// which is already at the limit.
#[allow(clippy::too_many_arguments)]
pub fn save_photo_with(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    resolution: PhotoResolution,
    format: PhotoFormat,
    dir: &Path,
    apply: impl FnOnce(&mut RgbaImage),
) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = unique_photo_path(dir, &photo_filename(Local::now(), format));
    let mut image = render_photo(data, width, height, stride, resolution)?;
    apply(&mut image);
    let encoded = encode_image(&image, format)?;
    fs::write(&path, encoded)?;
    Ok(path)
}

/// Full save operation with no post-processing — see [`save_photo_with`].
pub fn save_photo(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    resolution: PhotoResolution,
    format: PhotoFormat,
    dir: &Path,
) -> Result<PathBuf> {
    save_photo_with(data, width, height, stride, resolution, format, dir, |_| {})
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn filename_uses_timestamp_and_extension() {
        let at = Local
            .with_ymd_and_hms(2026, 11, 11, 14, 30, 5)
            .single()
            .unwrap_or_else(|| panic!("timestamp valid"));
        assert_eq!(
            photo_filename(at, PhotoFormat::Png),
            "HiView_2026-11-11_14-30-05.png"
        );
        assert_eq!(
            photo_filename(at, PhotoFormat::Jpeg),
            "HiView_2026-11-11_14-30-05.jpg"
        );
        assert_eq!(video_filename(at), "HiView_2026-11-11_14-30-05.mp4");
    }

    /// Videos follow the same `_N` collision suffixes as photos (§6.7).
    #[test]
    fn video_names_get_collision_suffixes() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let first = unique_photo_path(dir.path(), "HiView_2026-11-11_14-30-05.mp4");
        fs::write(&first, b"a").unwrap_or_else(|e| panic!("write: {e}"));
        assert_eq!(
            unique_photo_path(dir.path(), "HiView_2026-11-11_14-30-05.mp4")
                .file_name()
                .and_then(|n| n.to_str()),
            Some("HiView_2026-11-11_14-30-05_2.mp4")
        );
    }

    #[test]
    fn collision_suffixes_increment() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let first = unique_photo_path(dir.path(), "HiView_2026-11-11_14-30-05.png");
        assert_eq!(
            first.file_name().and_then(|n| n.to_str()),
            Some("HiView_2026-11-11_14-30-05.png")
        );
        fs::write(&first, b"a").unwrap_or_else(|e| panic!("write: {e}"));
        let second = unique_photo_path(dir.path(), "HiView_2026-11-11_14-30-05.png");
        assert_eq!(
            second.file_name().and_then(|n| n.to_str()),
            Some("HiView_2026-11-11_14-30-05_2.png")
        );
        fs::write(&second, b"b").unwrap_or_else(|e| panic!("write: {e}"));
        assert_eq!(
            unique_photo_path(dir.path(), "HiView_2026-11-11_14-30-05.png")
                .file_name()
                .and_then(|n| n.to_str()),
            Some("HiView_2026-11-11_14-30-05_3.png")
        );
    }

    #[test]
    fn output_sizes_landscape_and_rotated() {
        assert_eq!(PhotoResolution::Native.output_size((640, 480)), (640, 480));
        assert_eq!(PhotoResolution::X2.output_size((640, 480)), (1280, 960));
        assert_eq!(PhotoResolution::X2p5.output_size((640, 480)), (1600, 1200));
        // 90° rotation swaps the frame; every option follows.
        assert_eq!(PhotoResolution::Native.output_size((480, 640)), (480, 640));
        assert_eq!(PhotoResolution::X2.output_size((480, 640)), (960, 1280));
        assert_eq!(PhotoResolution::X2p5.output_size((480, 640)), (1200, 1600));
    }

    #[test]
    fn encode_upscales_to_expected_dimensions() {
        // Flat 4x2 pattern with stride padding to catch stride bugs.
        let mut data = vec![0u8; 2 * 20]; // stride 20, row 16
        for y in 0..2 {
            for x in 0..4 {
                let p = y * 20 + x * 4;
                data[p] = 255;
                data[p + 2] = 128;
                data[p + 3] = 255;
            }
        }
        for (res, (w, h)) in [
            (PhotoResolution::Native, (4, 2)),
            (PhotoResolution::X2, (8, 4)),
            (PhotoResolution::X2p5, (10, 5)),
        ] {
            let png = encode_photo(&data, 4, 2, 20, res, PhotoFormat::Png)
                .unwrap_or_else(|e| panic!("encode: {e}"));
            let decoded = image::load_from_memory(&png).unwrap_or_else(|e| panic!("decode: {e}"));
            assert_eq!((decoded.width(), decoded.height()), (w, h));
        }
    }

    #[test]
    fn encode_rejects_short_buffer() {
        assert!(
            encode_photo(
                &[0u8; 8],
                640,
                480,
                640 * 4,
                PhotoResolution::Native,
                PhotoFormat::Png
            )
            .is_err()
        );
    }

    #[test]
    fn native_png_is_pixel_identical_to_source_frame() {
        // Fixture captured by the microscope itself (M1, tests/fixtures).
        let src = image::open("../../tests/fixtures/leaf_common_object.png")
            .unwrap_or_else(|e| panic!("fixture: {e}"))
            .into_rgba8();
        let (w, h) = src.dimensions();
        // Re-wrap as a "frame" buffer with a padded stride.
        let stride = w as usize * 4 + 8;
        let mut data = vec![0u8; stride * h as usize];
        for y in 0..h as usize {
            let row = y * w as usize * 4;
            data[y * stride..y * stride + w as usize * 4]
                .copy_from_slice(&src.as_raw()[row..row + w as usize * 4]);
        }
        let png = encode_photo(
            &data,
            w,
            h,
            stride,
            PhotoResolution::Native,
            PhotoFormat::Png,
        )
        .unwrap_or_else(|e| panic!("encode: {e}"));
        let decoded = image::load_from_memory(&png)
            .unwrap_or_else(|e| panic!("decode: {e}"))
            .into_rgba8();
        assert_eq!(decoded.as_raw(), src.as_raw());
    }

    #[test]
    fn jpeg_output_is_decodable() {
        let data = vec![128u8; 4 * 4 * 4];
        let jpg = encode_photo(&data, 4, 4, 16, PhotoResolution::X2, PhotoFormat::Jpeg)
            .unwrap_or_else(|e| panic!("encode: {e}"));
        let decoded = image::load_from_memory(&jpg).unwrap_or_else(|e| panic!("decode: {e}"));
        assert_eq!((decoded.width(), decoded.height()), (8, 8));
    }
}
