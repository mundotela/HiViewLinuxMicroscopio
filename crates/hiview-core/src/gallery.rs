// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Gallery domain logic (docs/ARCHITECTURE.md §6.8): scanning the two
//! HiView media folders, ordering items newest-first and the on-disk
//! thumbnail cache at `$XDG_CACHE_HOME/hiview/thumbs/` (§6.12).
//!
//! Everything here is pure Rust plus the `image` crate — no GTK, no
//! GStreamer — so it is fully unit-testable and safe to call from a
//! worker thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbaImage, codecs::png::PngEncoder};

use crate::error::Result;

/// Longest edge of a generated thumbnail, in pixels.
pub const THUMB_MAX_EDGE: u32 = 256;

/// What a media file shown in the gallery is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaKind {
    /// Still photo (`png`, `jpg`, `jpeg`).
    #[default]
    Photo,
    /// Recorded video (`mp4`).
    Video,
}

/// One file shown in the gallery grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// Absolute path of the media file.
    pub path: PathBuf,
    /// Photo or video, decided by extension.
    pub kind: MediaKind,
    /// File size in bytes — part of the thumbnail cache key.
    pub size: u64,
    /// Modification time — sorts the grid and keys the cache.
    pub mtime: SystemTime,
}

impl MediaItem {
    /// Stats `path`; `None` when the file is missing or its extension is
    /// not a known media type.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let kind = media_kind_for(path)?;
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        Some(Self {
            path: path.to_path_buf(),
            kind,
            size: meta.len(),
            mtime: meta.modified().unwrap_or(UNIX_EPOCH),
        })
    }

    /// `dd/mm/aaaa hh:mm` label shown under the thumbnail.
    #[must_use]
    pub fn date_label(&self) -> String {
        date_label(self.mtime)
    }
}

/// `dd/mm/aaaa hh:mm` label for a file mtime.
#[must_use]
pub fn date_label(mtime: SystemTime) -> String {
    DateTime::<Local>::from(mtime)
        .format("%d/%m/%Y %H:%M")
        .to_string()
}

/// Media kind for `path` by extension (case-insensitive).
#[must_use]
pub fn media_kind_for(path: &Path) -> Option<MediaKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" => Some(MediaKind::Photo),
        "mp4" => Some(MediaKind::Video),
        _ => None,
    }
}

/// Lists both media folders merged, newest first (§6.8). Any known media
/// extension is accepted in either folder — the configured photo folder
/// may hold videos and vice versa. Missing folders scan as empty.
#[must_use]
pub fn scan_media(photo_dir: &Path, video_dir: &Path) -> Vec<MediaItem> {
    let mut items = Vec::new();
    for dir in [photo_dir, video_dir] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(item) = MediaItem::from_path(&entry.path()) {
                    items.push(item);
                }
            }
        }
    }
    items.sort_by(newest_first);
    items
}

/// Ordering for the grid: newest mtime first; file names (which embed a
/// timestamp) break ties descending for determinism.
fn newest_first(a: &MediaItem, b: &MediaItem) -> std::cmp::Ordering {
    b.mtime
        .cmp(&a.mtime)
        .then_with(|| b.path.file_name().cmp(&a.path.file_name()))
}

/// Index at which `item` must be inserted to keep `items` in
/// newest-first order (assumes `items` is already sorted).
#[must_use]
pub fn insertion_index(items: &[MediaItem], item: &MediaItem) -> usize {
    items
        .iter()
        .position(|existing| newest_first(item, existing).is_lt())
        .unwrap_or(items.len())
}

/// Thumbnail cache directory: `$XDG_CACHE_HOME/hiview/thumbs/`
/// (`~/.cache/hiview/thumbs/` without the variable) — §6.12.
#[must_use]
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || {
                std::env::var_os("HOME").map_or_else(
                    || PathBuf::from(".cache"),
                    |home| PathBuf::from(home).join(".cache"),
                )
            },
            PathBuf::from,
        );
    base.join("hiview").join("thumbs")
}

/// FNV-1a 64-bit fingerprint of the path bytes — deterministic across
/// runs (unlike `RandomState`), which is what makes the on-disk cache
/// stable between sessions.
fn path_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn mtime_millis(mtime: SystemTime) -> u128 {
    mtime
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn cache_stem(path: &Path) -> String {
    format!("{:016x}", path_hash(path))
}

/// Cache file names for one media file: `<hash>_<size>_<mtime>.png`
/// plus the `<…>.dur` sidecar holding a video's duration in seconds.
/// Any change of size or mtime produces a different key, so stale
/// thumbnails are regenerated (and cleaned by [`write_thumb_png`]).
#[must_use]
pub fn cache_files(dir: &Path, path: &Path, size: u64, mtime: SystemTime) -> (PathBuf, PathBuf) {
    let stem = format!("{}_{:x}_{:x}", cache_stem(path), size, mtime_millis(mtime));
    (
        dir.join(format!("{stem}.png")),
        dir.join(format!("{stem}.dur")),
    )
}

/// Deletes every cached thumbnail and sidecar belonging to `path`,
/// whichever size/mtime key they were written under. Returns how many
/// files were removed. Used after trashing a file (§6.8).
pub fn evict_cached_thumbs(dir: &Path, path: &Path) -> usize {
    let prefix = format!("{}_", cache_stem(path));
    let mut removed = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with(&prefix) && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Writes `seconds` into the `.dur` sidecar next to `png`.
///
/// # Errors
/// Returns [`crate::error::Error`] on I/O failure.
pub fn write_duration_sidecar(png: &Path, seconds: u64) -> Result<()> {
    fs::write(png.with_extension("dur"), seconds.to_string())?;
    Ok(())
}

/// Reads the `.dur` sidecar of `png`; `None` when absent or unparsable.
#[must_use]
pub fn read_duration_sidecar(png: &Path) -> Option<u64> {
    fs::read_to_string(png.with_extension("dur"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Encodes `rgba` as PNG and stores it under the cache key of
/// (`path`, `size`, `mtime`), after evicting that path's stale entries.
/// Returns the cache file written.
///
/// # Errors
/// Returns [`crate::error::Error`] on encode or I/O failure.
pub fn write_thumb_png(
    dir: &Path,
    path: &Path,
    size: u64,
    mtime: SystemTime,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    evict_cached_thumbs(dir, path);
    let (png, _) = cache_files(dir, path, size, mtime);
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded).write_image(rgba, width, height, ExtendedColorType::Rgba8)?;
    fs::write(&png, encoded)?;
    Ok(png)
}

/// Decodes a cached PNG back to `(width, height, rgba)`.
///
/// # Errors
/// Returns [`crate::error::Error`] when the file is unreadable or not a
/// decodable image.
pub fn decode_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let image = image::open(path)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

/// Downscales an image so its longest edge is at most
/// [`THUMB_MAX_EDGE`]. Already-small images pass through unchanged.
#[must_use]
pub fn fit_thumb(image: RgbaImage) -> RgbaImage {
    let (w, h) = image.dimensions();
    if w <= THUMB_MAX_EDGE && h <= THUMB_MAX_EDGE {
        return image;
    }
    let scale = f64::from(THUMB_MAX_EDGE) / f64::from(w.max(h));
    let new_w = (f64::from(w) * scale).round().max(1.0) as u32;
    let new_h = (f64::from(h) * scale).round().max(1.0) as u32;
    image::imageops::resize(&image, new_w, new_h, FilterType::Triangle)
}

/// Wraps tightly packed RGBA8 pixels and fits them into a thumbnail;
/// `None` when the buffer does not match the declared geometry.
#[must_use]
pub fn thumb_from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<RgbaImage> {
    RgbaImage::from_raw(width, height, rgba).map(fit_thumb)
}

/// Loads a photo file and returns its thumbnail. Corrupt or unreadable
/// files produce an error — the UI maps that to the placeholder tile.
///
/// # Errors
/// Returns [`crate::error::Error`] when the file cannot be decoded.
pub fn photo_thumbnail(path: &Path) -> Result<RgbaImage> {
    Ok(fit_thumb(image::open(path)?.into_rgba8()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn png_bytes() -> Vec<u8> {
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&[200u8, 30, 30, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap_or_else(|e| panic!("png: {e}"));
        encoded
    }

    fn write_media(dir: &Path, name: &str, mtime: SystemTime) -> PathBuf {
        fs::create_dir_all(dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
        let path = dir.join(name);
        fs::write(&path, png_bytes()).unwrap_or_else(|e| panic!("write: {e}"));
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("open: {e}"))
            .set_modified(mtime)
            .unwrap_or_else(|e| panic!("mtime: {e}"));
        path
    }

    #[test]
    fn scan_merges_both_folders_newest_first() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let photos = dir.path().join("fotos");
        let videos = dir.path().join("videos");
        let now = SystemTime::now();
        let old = write_media(&photos, "HiView_old.png", now - Duration::from_secs(300));
        let mid = write_media(&videos, "HiView_mid.mp4", now - Duration::from_secs(200));
        let new = write_media(&photos, "HiView_new.jpg", now - Duration::from_secs(100));
        // Non-media files are ignored.
        fs::write(photos.join("notas.txt"), b"oi").unwrap_or_else(|e| panic!("txt: {e}"));

        let items = scan_media(&photos, &videos);
        let paths: Vec<_> = items.iter().map(|i| i.path.clone()).collect();
        assert_eq!(paths, vec![new, mid, old]);
        assert_eq!(items[0].kind, MediaKind::Photo);
        assert_eq!(items[1].kind, MediaKind::Video);
    }

    /// The configured photo folder is a parameter, not a constant — this
    /// proves a custom folder is honored (M3 preference).
    #[test]
    fn scan_uses_the_configured_photo_folder() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let configured = dir.path().join("pasta-personalizada");
        let default = dir.path().join("padrao");
        let now = SystemTime::now();
        let found = write_media(&configured, "a.png", now);
        let ignored = write_media(&default, "b.png", now);

        let items = scan_media(&configured, &dir.path().join("sem-videos"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, found);
        assert!(!items.iter().any(|i| i.path == ignored));
    }

    #[test]
    fn missing_folders_scan_as_empty() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let items = scan_media(&dir.path().join("nada"), &dir.path().join("nada2"));
        assert!(items.is_empty());
    }

    #[test]
    fn insertion_index_keeps_newest_first() {
        let now = SystemTime::now();
        let item = |name: &str, secs_ago: u64| MediaItem {
            path: PathBuf::from(name),
            kind: MediaKind::Photo,
            size: 1,
            mtime: now - Duration::from_secs(secs_ago),
        };
        let mut items = vec![item("new.png", 10), item("old.png", 100)];
        let idx = insertion_index(&items, &item("mid.png", 50));
        items.insert(idx, item("mid.png", 50));
        assert_eq!(items[1].path, PathBuf::from("mid.png"));
        assert_eq!(insertion_index(&items, &item("first.png", 0)), 0);
        assert_eq!(insertion_index(&items, &item("last.png", 1000)), 3);
    }

    /// Cache key: identical path+size+mtime hits; a changed mtime or size
    /// misses (stale entry gets regenerated).
    #[test]
    fn cache_key_invalidates_on_size_or_mtime_change() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = PathBuf::from("/fotos/x.png");
        let t0 = SystemTime::now();
        let (a, _) = cache_files(dir.path(), &path, 100, t0);
        let (b, _) = cache_files(dir.path(), &path, 100, t0);
        assert_eq!(a, b, "same inputs must hit the same key");
        let (c, _) = cache_files(dir.path(), &path, 101, t0);
        assert_ne!(a, c, "size change must miss");
        let (d, _) = cache_files(dir.path(), &path, 100, t0 + Duration::from_secs(1));
        assert_ne!(a, d, "mtime change must miss");
        // Different file, same metadata: different path hash.
        let (e, _) = cache_files(dir.path(), &PathBuf::from("/fotos/y.png"), 100, t0);
        assert_ne!(a, e, "path must be part of the key");
    }

    #[test]
    fn thumb_write_then_decode_round_trips_and_evicts_stale() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let cache = dir.path().join("thumbs");
        let src = PathBuf::from("/fotos/x.png");
        let t0 = SystemTime::now();
        let first = write_thumb_png(&cache, &src, 10, t0, 1, 1, &[1, 2, 3, 255])
            .unwrap_or_else(|e| panic!("write: {e}"));
        // Same path with newer mtime: stale entry is removed on write.
        let second = write_thumb_png(
            &cache,
            &src,
            10,
            t0 + Duration::from_secs(5),
            1,
            1,
            &[4, 5, 6, 255],
        )
        .unwrap_or_else(|e| panic!("write2: {e}"));
        assert!(!first.exists(), "stale entry was not evicted");
        let (w, h, rgba) = decode_png(&second).unwrap_or_else(|e| panic!("decode: {e}"));
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba, vec![4, 5, 6, 255]);
    }

    #[test]
    fn duration_sidecar_round_trips() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let png = dir.path().join("k.png");
        write_duration_sidecar(&png, 95).unwrap_or_else(|e| panic!("dur: {e}"));
        assert_eq!(read_duration_sidecar(&png), Some(95));
        assert_eq!(read_duration_sidecar(&dir.path().join("x.png")), None);
    }

    #[test]
    fn evict_removes_all_versions_of_a_path() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let src = PathBuf::from("/fotos/x.png");
        for offset in 0..3 {
            write_thumb_png(
                dir.path(),
                &src,
                10,
                SystemTime::now() + Duration::from_secs(offset),
                1,
                1,
                &[0, 0, 0, 255],
            )
            .unwrap_or_else(|e| panic!("write: {e}"));
        }
        // Three writes evicted two stale files; one remains — evict removes it.
        let removed = evict_cached_thumbs(dir.path(), &src);
        assert_eq!(removed, 1);
        assert!(
            fs::read_dir(dir.path())
                .unwrap_or_else(|e| panic!("ls: {e}"))
                .next()
                .is_none()
        );
    }

    #[test]
    fn photo_thumbnail_scales_and_corrupt_file_errors() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let big = dir.path().join("big.png");
        RgbaImage::from_pixel(640, 480, image::Rgba([9, 9, 9, 255]))
            .save(&big)
            .unwrap_or_else(|e| panic!("save: {e}"));
        let thumb = photo_thumbnail(&big).unwrap_or_else(|e| panic!("thumb: {e}"));
        assert_eq!(thumb.dimensions(), (THUMB_MAX_EDGE, 192));

        let corrupt = dir.path().join("lixo.png");
        fs::write(&corrupt, b"isto nao e uma imagem").unwrap_or_else(|e| panic!("w: {e}"));
        assert!(photo_thumbnail(&corrupt).is_err());
    }

    #[test]
    fn fit_thumb_keeps_small_images() {
        let img = RgbaImage::from_pixel(10, 20, image::Rgba([0, 0, 0, 255]));
        assert_eq!(fit_thumb(img).dimensions(), (10, 20));
    }
}
