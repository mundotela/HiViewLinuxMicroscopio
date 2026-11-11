// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! `GalleryEntry` — the `glib::Object` stored in the gallery's
//! `gio::ListStore`. It carries the file metadata shown on a tile plus
//! the thumbnail state, and keeps a handle to its bound tile widgets so
//! a thumbnail arriving from the worker thread updates the tile in
//! place.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use gtk::glib;
use gtk::glib::subclass::prelude::*;
use gtk::{gdk, prelude::*};

use hiview_core::{MediaItem, MediaKind};

use crate::strings;

/// Everything shown on one tile.
#[derive(Clone)]
pub struct EntryData {
    /// Media file path (or the recording's target path).
    pub path: PathBuf,
    /// Photo or video.
    pub kind: MediaKind,
    /// File size — thumbnail cache key ingredient.
    pub size: u64,
    /// Modification time — sorts the grid.
    pub mtime: SystemTime,
    /// True while this tile represents the in-progress recording.
    pub recording: bool,
    /// Thumbnail texture when loaded.
    pub thumb: Option<gdk::Paintable>,
    /// True when the file could not be decoded.
    pub corrupt: bool,
    /// Video duration in seconds.
    pub duration_secs: Option<u64>,
}

impl Default for EntryData {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            kind: MediaKind::default(),
            size: 0,
            mtime: SystemTime::UNIX_EPOCH,
            recording: false,
            thumb: None,
            corrupt: false,
            duration_secs: None,
        }
    }
}

/// Widgets of the tile an entry is currently bound to. The factory
/// builds one per `bind`; the entry updates it when the thumbnail
/// arrives.
pub(crate) struct TileWidgets {
    /// The tile itself, set as the `gtk::ListItem` child.
    pub root: gtk::Box,
    /// Switches between the loading, thumbnail, warning and recording pages.
    pub stack: gtk::Stack,
    /// Displays the thumbnail texture on the `thumb` page.
    pub picture: gtk::Picture,
    /// Play badge overlaid on video thumbnails.
    pub play_badge: gtk::Image,
    /// Duration pill in the corner of video thumbnails.
    pub duration_badge: gtk::Label,
    /// Date/time (or "Gravando…") line under the thumbnail.
    pub date: gtk::Label,
}

mod imp {
    use super::*;

    /// Shared state of [`super::GalleryEntry`].
    #[derive(Default)]
    pub struct GalleryEntry {
        pub data: RefCell<EntryData>,
        /// Tile currently displaying this entry (one binding at a time).
        pub(crate) tile: RefCell<Option<Rc<TileWidgets>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GalleryEntry {
        const NAME: &'static str = "HiViewGalleryEntry";
        type Type = super::GalleryEntry;
    }

    impl ObjectImpl for GalleryEntry {}
}

glib::wrapper! {
    /// A gallery item: file metadata plus thumbnail state.
    pub struct GalleryEntry(ObjectSubclass<imp::GalleryEntry>);
}

impl GalleryEntry {
    /// Entry for a scanned media file.
    pub fn from_item(item: &MediaItem) -> Self {
        let entry: Self = glib::Object::new();
        {
            let mut data = entry.imp().data.borrow_mut();
            data.path = item.path.clone();
            data.kind = item.kind;
            data.size = item.size;
            data.mtime = item.mtime;
        }
        entry
    }

    /// Synthetic entry for the in-progress recording (§6.8 task): a
    /// "Gravando…" tile that cannot be opened or trashed until the file
    /// is finalized.
    pub fn for_recording(path: &Path) -> Self {
        let entry: Self = glib::Object::new();
        {
            let mut data = entry.imp().data.borrow_mut();
            data.path = path.to_path_buf();
            data.kind = MediaKind::Video;
            data.mtime = SystemTime::now();
            data.recording = true;
        }
        entry
    }

    fn with<R>(&self, f: impl FnOnce(&EntryData) -> R) -> R {
        f(&self.imp().data.borrow())
    }

    /// The media file this entry represents.
    pub fn path(&self) -> PathBuf {
        self.with(|d| d.path.clone())
    }

    /// Whether the entry is a video clip.
    pub fn is_video(&self) -> bool {
        self.with(|d| d.kind == MediaKind::Video)
    }

    /// Whether this is the in-progress recording tile.
    pub fn is_recording(&self) -> bool {
        self.with(|d| d.recording)
    }

    /// Whether the entry already shows a thumbnail.
    pub fn has_thumb(&self) -> bool {
        self.with(|d| d.thumb.is_some() || d.corrupt)
    }

    /// "Open" is allowed for every finalized file — never for the
    /// in-progress recording.
    pub fn can_open(&self) -> bool {
        self.with(|d| !d.recording)
    }

    /// Trashing the file being written would corrupt the recording.
    pub fn can_trash(&self) -> bool {
        self.with(|d| !d.recording)
    }

    /// `dd/mm/aaaa hh:mm` — or "Gravando…" for the recording tile.
    pub fn date_label(&self) -> String {
        self.with(|d| {
            if d.recording {
                crate::strings::RECORDING_TILE.to_string()
            } else {
                hiview_core::date_label(d.mtime)
            }
        })
    }

    /// `M:SS` duration badge text — videos only.
    pub fn duration_label(&self) -> Option<String> {
        self.with(|d| d.duration_secs.map(crate::strings::elapsed_label))
    }

    /// The `MediaItem` needed to (re)generate the thumbnail.
    pub fn media_item(&self) -> MediaItem {
        self.with(|d| MediaItem {
            path: d.path.clone(),
            kind: d.kind,
            size: d.size,
            mtime: d.mtime,
        })
    }

    /// Applies `f` to the data without ever panicking on re-entrant
    /// access — GTK may call back into us while another borrow is
    /// alive, and a `RefCell` panic inside a GTK callback aborts the
    /// process (it cannot unwind).
    fn mutate(&self, f: impl FnOnce(&mut EntryData)) {
        match self.imp().data.try_borrow_mut() {
            Ok(mut data) => f(&mut data),
            Err(_) => tracing::warn!("gallery entry busy; metadata update skipped"),
        }
        self.refresh_tile();
    }

    /// Marks the file as changed on disk: refreshes metadata and resets
    /// the thumbnail so a new one is generated (the cache key changes
    /// with size/mtime).
    pub fn update_item(&self, item: &MediaItem) {
        let item = item.clone();
        self.mutate(move |data| {
            data.kind = item.kind;
            data.size = item.size;
            data.mtime = item.mtime;
            data.thumb = None;
            data.corrupt = false;
        });
    }

    /// The recording finalized into `item`: the tile becomes a normal
    /// video tile and requests a thumbnail.
    pub fn finish_recording(&self, item: &MediaItem) {
        let item = item.clone();
        self.mutate(move |data| {
            data.recording = false;
            data.kind = item.kind;
            data.size = item.size;
            data.mtime = item.mtime;
            data.thumb = None;
            data.corrupt = false;
        });
    }

    /// Stores the generated thumbnail (or the corruption verdict) and
    /// updates the bound tile, if any.
    pub fn set_thumbnail(&self, texture: gdk::Paintable, duration_secs: Option<u64>) {
        self.mutate(move |data| {
            data.thumb = Some(texture);
            data.corrupt = false;
            data.duration_secs = duration_secs;
        });
    }

    /// Shows the warning placeholder — corrupt/unreadable file.
    pub fn set_corrupt(&self) {
        self.mutate(|data| data.corrupt = true);
    }

    /// Called by the factory `bind` signal with the freshly built tile.
    /// If the slot is borrowed — a re-entrant `bind` fired from a GTK
    /// call in [`refresh_tile`](Self::refresh_tile) — the link is
    /// retried on the next idle turn instead of panicking.
    pub(crate) fn set_tile(&self, tile: Rc<TileWidgets>) {
        match self.imp().tile.try_borrow_mut() {
            Ok(mut slot) => *slot = Some(tile),
            Err(_) => {
                let entry = self.clone();
                glib::idle_add_local_once(move || entry.set_tile(tile));
                return;
            }
        }
        self.refresh_tile();
    }

    /// Called by the factory `unbind` signal when the tile is recycled.
    /// A busy slot means a refresh snapshot is in flight; skipping just
    /// leaves a stale `Rc` that the next `bind` replaces — never panic
    /// inside a GTK callback.
    pub(crate) fn clear_tile(&self) {
        match self.imp().tile.try_borrow_mut() {
            Ok(mut slot) => *slot = None,
            Err(_) => tracing::warn!("gallery tile busy; unbind skipped"),
        }
    }

    /// Applies the current entry state to the bound tile.
    ///
    /// IMPORTANT: both `RefCell`s are snapshotted and released BEFORE
    /// any GTK call. Unmapping a stack child while the row is `:active`
    /// makes GTK synthesize `listitem.select`, which re-enters
    /// bind/unbind synchronously — a borrow held across that call
    /// panics, and panics in GTK callbacks abort the process.
    fn refresh_tile(&self) {
        let tile = self
            .imp()
            .tile
            .try_borrow()
            .ok()
            .and_then(|slot| slot.as_ref().cloned());
        let data = self.imp().data.try_borrow().ok().map(|data| data.clone());
        let (Some(tile), Some(data)) = (tile, data) else {
            return;
        };

        let label = if data.recording {
            strings::RECORDING_TILE.to_string()
        } else {
            hiview_core::date_label(data.mtime)
        };
        tile.date.set_label(&label);

        let page = if data.recording {
            "recording"
        } else if data.corrupt {
            "corrupt"
        } else if data.thumb.is_some() {
            "thumb"
        } else {
            "loading"
        };
        // Dedup: every page switch unmaps a child — the exact trigger
        // of the `listitem.select` re-entrancy fixed above.
        if tile.stack.visible_child_name().as_deref() != Some(page) {
            tile.stack.set_visible_child_name(page);
        }
        if let Some(thumb) = &data.thumb {
            tile.picture.set_paintable(Some(thumb));
        }
        // Corrupt files get only the warning icon — no play badge, no
        // duration pill.
        let video = data.kind == MediaKind::Video && !data.recording && !data.corrupt;
        tile.play_badge.set_visible(video);
        match (video, data.duration_secs) {
            (true, Some(secs)) => {
                tile.duration_badge.set_label(&strings::elapsed_label(secs));
                tile.duration_badge.set_visible(true);
            }
            _ => tile.duration_badge.set_visible(false),
        }
    }
}

/// Tile thumbnail box size in pixels (4:3, like the sensor output).
const TILE_WIDTH: i32 = 200;
const TILE_HEIGHT: i32 = 150;

/// Builds one tile widget tree for `entry`: thumbnail stack, badges
/// and date line. Pointer interaction lives on the permanent
/// `GridView` (activation + a single context-menu gesture that
/// hit-tests rows manually) — a gesture or popover parented to a
/// recyclable tile kept a dead grab that swallowed clicks, including
/// the trash dialog's buttons (M5 review).
///
/// Every widget in the tile is also made untargetable: tiles are
/// recycled (unparented) while scrolling, and GTK synthesizes crossing
/// events over whatever widget the pointer last touched — a dead
/// widget chain logs `Gtk-CRITICAL gtk_widget_compute_point`.
pub(crate) fn build_tile() -> TileWidgets {
    /// `set_can_target(false)` for `widget` and every descendant.
    /// Tiles are unparented during recycling — they must never be the
    /// pointer's event target, or GTK synthesizes crossing events over
    /// a dead widget chain (`Gtk-CRITICAL gtk_widget_compute_point`).
    fn untargetable(widget: &gtk::Widget) {
        widget.set_can_target(false);
        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            let next = child_widget.next_sibling();
            untargetable(&child_widget);
            child = next;
        }
    }

    let stack = gtk::Stack::new();
    stack.set_size_request(TILE_WIDTH, TILE_HEIGHT);
    stack.add_css_class("gallery-thumb");

    let loading = gtk::Image::from_icon_name("image-x-generic-symbolic");
    loading.set_pixel_size(48);
    loading.add_css_class("dim-label");
    stack.add_named(&loading, Some("loading"));

    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    // `can-shrink` stays at its default (true): portrait thumbs are
    // downscaled into the fixed 4:3 frame, keeping every tile the same
    // height (M5 review: portrait photos stretched the grid rows).
    stack.add_named(&picture, Some("thumb"));

    let corrupt = gtk::Image::from_icon_name("dialog-warning-symbolic");
    corrupt.set_pixel_size(48);
    corrupt.add_css_class("warning");
    stack.add_named(&corrupt, Some("corrupt"));

    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_size_request(32, 32);
    stack.add_named(&spinner, Some("recording"));

    let play_badge = gtk::Image::from_icon_name("media-playback-start-symbolic");
    play_badge.set_pixel_size(56);
    play_badge.add_css_class("play-badge");
    play_badge.set_halign(gtk::Align::Center);
    play_badge.set_valign(gtk::Align::Center);

    let duration_badge = gtk::Label::new(None);
    duration_badge.add_css_class("thumb-badge");
    duration_badge.set_halign(gtk::Align::End);
    duration_badge.set_valign(gtk::Align::End);
    duration_badge.set_margin_end(6);
    duration_badge.set_margin_bottom(6);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&stack));
    overlay.add_overlay(&play_badge);
    overlay.add_overlay(&duration_badge);

    let date = gtk::Label::new(None);
    date.add_css_class("caption");
    date.set_ellipsize(gtk::pango::EllipsizeMode::End);
    date.set_max_width_chars(18);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("gallery-tile");
    root.append(&overlay);
    root.append(&date);

    untargetable(root.upcast_ref());

    TileWidgets {
        root,
        stack,
        picture,
        play_badge,
        duration_badge,
        date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gio;

    fn item(path: &str) -> MediaItem {
        MediaItem {
            path: PathBuf::from(path),
            kind: MediaKind::Video,
            size: 42,
            mtime: SystemTime::now(),
        }
    }

    /// Required test (task M5): the "Gravando…" tile cannot be opened or
    /// deleted until the recording finalizes — then it becomes a normal
    /// actionable tile.
    #[test]
    fn recording_tile_is_not_actionable_until_finalized() {
        let entry = GalleryEntry::for_recording(Path::new("/tmp/gravando.mp4"));
        assert!(entry.is_recording());
        assert!(!entry.can_open());
        assert!(!entry.can_trash());
        assert_eq!(entry.date_label(), crate::strings::RECORDING_TILE);

        entry.finish_recording(&item("/tmp/gravando.mp4"));
        assert!(!entry.is_recording());
        assert!(entry.can_open());
        assert!(entry.can_trash());
        assert_ne!(entry.date_label(), crate::strings::RECORDING_TILE);
    }

    /// Regression test for the M5-review crash: unmapping a stack child
    /// makes GTK synthesize `listitem.select`, which re-enters
    /// bind/unbind while `refresh_tile` runs. Every accessor must skip
    /// or defer instead of panicking on a borrowed `RefCell` — a panic
    /// inside a GTK callback aborts the process.
    #[test]
    fn accessors_never_panic_on_reentry() {
        let entry = GalleryEntry::from_item(&item("/tmp/a.png"));

        // Tile slot borrowed: `clear_tile` (unbind) must skip, not panic.
        let hold = entry.imp().tile.borrow();
        entry.clear_tile();
        drop(hold);

        // Data borrowed: mutators and `refresh_tile` must skip, not panic.
        let hold = entry.imp().data.borrow();
        entry.set_corrupt();
        entry.finish_recording(&item("/tmp/a.png"));
        entry.refresh_tile();
        drop(hold);

        // Both free again: everything still works.
        entry.set_corrupt();
        assert!(entry.has_thumb());
    }

    /// M5 review regression: the recording tile is keyed by path and
    /// removed by lookup, not by index 0. The old code inserted a tile
    /// on every gallery open during a recording (duplicates) and
    /// removed only `store[0]` on `RecordingEnded`, leaving earlier
    /// tiles as zombie "Gravando…" spinners.
    #[test]
    fn recording_tile_dedup_and_lookup_removal() {
        let store = gio::ListStore::new::<GalleryEntry>();
        let path_a = PathBuf::from("/v/a.mp4");
        let path_b = PathBuf::from("/v/b.mp4");

        let rec_a = GalleryEntry::for_recording(&path_a);
        store.insert(0, &rec_a);
        // Re-emitted `RecordingStarted` for the same path is a no-op
        // (the update() guard is `existing.path() == path`).
        assert_eq!(rec_a.path(), path_a);
        assert_eq!(store.n_items(), 1);

        // A different recording's tile above it displaces it — the
        // removal must find it by lookup, not assume index 0.
        let rec_b = GalleryEntry::for_recording(&path_b);
        store.insert(0, &rec_b);
        assert_eq!(store.find(&rec_a), Some(1));
        if let Some(pos) = store.find(&rec_a) {
            store.remove(pos);
        }
        assert_eq!(store.n_items(), 1);
        assert_eq!(store.find(&rec_b), Some(0));
    }

    /// A changed file (same path, new mtime/size) drops its thumbnail so
    /// a fresh one is requested — stale thumbs are never shown.
    #[test]
    fn update_item_resets_thumbnail_state() {
        let entry = GalleryEntry::from_item(&item("/tmp/a.mp4"));
        entry.set_corrupt();
        assert!(entry.has_thumb());
        entry.update_item(&item("/tmp/a.mp4"));
        assert!(!entry.has_thumb());
        assert!(entry.can_open());
    }
}
