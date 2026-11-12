// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Pure domain logic for HiView: configuration, calibration, view transforms,
//! frame processing, file naming and the UVC status packet parser.
//!
//! This crate must not depend on GTK or GStreamer (docs/ARCHITECTURE.md §5).

mod adjust;
mod config;
mod devices;
mod error;
mod gallery;
mod image_adjust;
mod imaging;
mod measure;
mod orientation;
mod overlay;
mod view;

pub use adjust::{
    AdjustKind, BRIGHTNESS_RANGE, CONTRAST_RANGE, GAMMA_RANGE, ImageAdjust, SATURATION_RANGE,
    SHARPEN_RANGE, digits_of, range_of, step_of,
};
pub use config::{AppConfig, ConfigLoad, PhotoConfig, config_path, load_config, save_config};
pub use devices::{KNOWN_MICROSCOPES, KnownMicroscope, is_known_microscope};
pub use error::{Error, Result};
pub use gallery::{
    MediaItem, MediaKind, THUMB_MAX_EDGE, cache_dir, cache_files, date_label, decode_png,
    evict_cached_thumbs, fit_thumb, insertion_index, media_kind_for, photo_thumbnail,
    read_duration_sidecar, scan_media, thumb_from_rgba, write_duration_sidecar, write_thumb_png,
};
pub use image_adjust::AdjustPlan;
pub use imaging::{
    JPEG_QUALITY, PhotoFormat, PhotoResolution, encode_image, encode_photo, photo_filename,
    render_photo, rgba_image, save_photo, save_photo_with, unique_photo_path, video_filename,
};
pub use measure::{
    CalibrationProfile, DraftView, MeasureGesture, MeasureSession, MeasureShape, MeasureTool,
    Measurement, OrientationOp, angle_deg, distance, format_angle_deg, format_length_px, map_point,
    mapped_size,
};
pub use orientation::{Orientation, ViewOrientation};
pub use overlay::{
    AppMode, CROSSHAIR_CIRCLE_FRACTION, CROSSHAIR_OPACITY, CROSSHAIR_TICK_LEN_PX, CrosshairConfig,
    CrosshairStyle, DEFAULT_GRID_OPACITY, DEFAULT_GRID_SPACING_PX, DEFAULT_TICK_SPACING_PX,
    GridConfig, GridSpacing, MAX_GRID_SPACING_MM, MAX_GRID_SPACING_PX, MIN_EFFECTIVE_SPACING_PX,
    MIN_GRID_SPACING_MM, MIN_GRID_SPACING_PX, MeasureOverlay, OverlayColor, OverlayState,
    grid_line_positions, image_center, overlays_for,
};
pub use view::{MAX_SCALE, MIN_SCALE, NEAREST_FILTER_SCALE, Point, Size, ViewMode, ViewTransform};
