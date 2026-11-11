// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Gallery page (docs/ARCHITECTURE.md §6.8): a `adw::NavigationPage`
//! pushed onto the app's `adw::NavigationView` with `Ctrl+G` or the
//! "Galeria" header button. Shows the configured photo folder and
//! `Vídeos/HiView`, newest first, with cached thumbnails generated on a
//! bounded worker queue.

mod entry;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use entry::{GalleryEntry, build_tile};
use hiview_core::{MediaItem, insertion_index, scan_media};
use relm4::adw::prelude::*;
use relm4::gtk::{self, gdk, gio, glib, graphene};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw};

use crate::services::gallery::{ThumbOutcome, ThumbRejected, ThumbService};
use crate::strings;

/// Inputs handled by the gallery component.
#[derive(Debug)]
pub enum Input {
    /// (Re)scan the two media folders — sent every time the page opens
    /// and whenever the configured folders may have changed.
    Refresh {
        /// Configured photo folder (M3 preferences).
        photo_dir: PathBuf,
        /// `Vídeos/HiView`.
        video_dir: PathBuf,
    },
    /// Scan results arrived from the worker.
    ScanDone(Vec<MediaItem>),
    /// The app saved a new file (photo written, video finalized).
    FileSaved(PathBuf),
    /// Stat for a `FileSaved` path arrived.
    ItemReady(Option<MediaItem>),
    /// A recording started: insert the "Gravando…" tile at the top.
    RecordingStarted(PathBuf),
    /// The recording finalized: drop the synthetic tile.
    RecordingEnded,
    /// One-second elapsed label while recording (header indicator).
    RecordElapsed(String),
    /// The thumbnail worker finished one item.
    ThumbDone(ThumbOutcome),
    /// A tile was activated — open it in the system app.
    Activate(u32),
    /// Left-click on a tile (grid-level gesture) — open by path.
    Open(PathBuf),
    /// Right-click on a tile: position the shared context menu.
    ContextMenu {
        /// File the actions resolve by path at click time.
        path: PathBuf,
        /// Pointer position in grid coordinates.
        x: f64,
        /// Pointer position in grid coordinates.
        y: f64,
    },
    /// Context menu: open the file with the system app.
    ContextOpen,
    /// Context menu: reveal the file in its folder.
    ContextShowInFolder,
    /// Context menu: trash the file after confirming.
    ContextTrash,
    /// The confirmation dialog answered "Mover para a lixeira".
    TrashConfirmed(PathBuf),
    /// `trash_future` finished.
    Trashed(PathBuf, bool),
}

/// Outputs the gallery raises for the root component.
#[derive(Debug)]
pub enum Output {
    /// Show a transient toast with this PT-BR message.
    Toast(&'static str),
}

/// Everything [`GalleryModel::new`] needs.
pub struct GalleryInit {
    /// Application window, parent of dialogs and file launchers.
    pub window: adw::ApplicationWindow,
}

/// Live gallery pages — logged on every open as proof that the app
/// keeps exactly one page and pushes/pops it instead of building a
/// new component per open (M5 review).
static GALLERY_INSTANCES: AtomicUsize = AtomicUsize::new(0);

/// The gallery page model.
pub struct GalleryModel {
    window: adw::ApplicationWindow,
    /// Real files, newest first — mirrors the store minus the recording
    /// tile pinned at index 0.
    items: Vec<MediaItem>,
    store: gio::ListStore,
    /// O(1) lookup of entries backing `items`.
    by_path: HashMap<PathBuf, GalleryEntry>,
    /// Synthetic tile for the in-progress recording, always at index 0.
    recording_entry: Option<GalleryEntry>,
    /// Elapsed-time text for the red indicator in the gallery header.
    recording_elapsed: String,
    photo_dir: PathBuf,
    video_dir: PathBuf,
    cache_dir: PathBuf,
    thumbs: ThumbService,
    /// Items the bounded queue could not take yet (§7 backpressure).
    pending: VecDeque<MediaItem>,
    /// Paths with a job in flight (dedupes `pending` and requests).
    requested: HashSet<PathBuf>,
    /// Single context-menu popover, parented to the stable grid — a
    /// popover on a recycled tile kept a dead grab that swallowed every
    /// click, including the trash dialog's buttons (M5 review).
    popover: gtk::Popover,
    /// File the open context menu acts on — resolved at action time.
    context_path: Option<PathBuf>,
    // Widget handles grabbed after `view_output!` (imperative updates).
    gallery_stack: gtk::Stack,
    empty_page: adw::StatusPage,
    grid_page: gtk::ScrolledWindow,
    /// Performance marks for the milestone report (§11.0).
    opened_at: Option<Instant>,
    first_thumb_logged: bool,
    all_thumbs_logged: bool,
}

#[relm4::component(pub)]
impl SimpleComponent for GalleryModel {
    type Init = GalleryInit;
    type Input = Input;
    type Output = Output;

    view! {
        adw::NavigationPage {
            set_title: strings::GALLERY_TITLE,
            set_tag: Some("gallery"),

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_end = &gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 6,
                        set_margin_end: 6,
                        set_css_classes: &["record-indicator"],
                        #[watch]
                        set_visible: model.recording_entry.is_some(),

                        gtk::Image {
                            set_icon_name: Some("media-record-symbolic"),
                        },
                        gtk::Label {
                            #[watch]
                            set_label: &model.recording_elapsed,
                        },
                    },
                },

                #[name = "gallery_stack"]
                #[wrap(Some)]
                set_content = &gtk::Stack {
                    set_transition_type: gtk::StackTransitionType::Crossfade,

                    #[name = "empty_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::GALLERY_EMPTY_ICON),
                        set_title: strings::GALLERY_EMPTY_TITLE,
                        set_description: Some(strings::GALLERY_EMPTY_DESCRIPTION),
                    },

                    #[name = "grid_page"]
                    add_child = &gtk::ScrolledWindow {
                        set_vexpand: true,
                        set_hexpand: true,

                        #[name = "grid_view"]
                        #[wrap(Some)]
                        set_child = &gtk::GridView {
                            // Clicks are handled by the grid-level
                            // GestureClick below: the row widgets are
                            // untargetable by design, so GTK's own
                            // single-click-activate never fires.
                            set_margin_start: 12,
                            set_margin_end: 12,
                            set_margin_top: 12,
                            set_margin_bottom: 12,
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        GALLERY_INSTANCES.fetch_add(1, Ordering::SeqCst);
        let cache_dir = hiview_core::cache_dir();
        let thumbs = ThumbService::new(cache_dir.clone(), {
            let sender = sender.clone();
            move |outcome| sender.input(Input::ThumbDone(outcome))
        });

        let mut model = Self {
            window: init.window,
            items: Vec::new(),
            store: gio::ListStore::new::<GalleryEntry>(),
            by_path: HashMap::new(),
            recording_entry: None,
            recording_elapsed: String::new(),
            photo_dir: PathBuf::new(),
            video_dir: PathBuf::new(),
            cache_dir,
            thumbs,
            pending: VecDeque::new(),
            requested: HashSet::new(),
            popover: gtk::Popover::new(),
            context_path: None,
            gallery_stack: gtk::Stack::new(),
            empty_page: adw::StatusPage::new(),
            grid_page: gtk::ScrolledWindow::new(),
            opened_at: None,
            first_thumb_logged: false,
            all_thumbs_logged: false,
        };
        let widgets = view_output!();
        model.gallery_stack = widgets.gallery_stack.clone();
        model.empty_page = widgets.empty_page.clone();
        model.grid_page = widgets.grid_page.clone();
        model.gallery_stack.set_visible_child(&model.empty_page);
        let grid_view = widgets.grid_view.clone();

        // Grid plumbing: store → selection → factory → view.
        // Tile-root pointer → entry, so the grid-level context menu can
        // map a hit-tested row back to the model (GTK 4.14's
        // `GtkListItemWidget` is private — no `item` accessor to bind).
        let tile_entries: Rc<RefCell<HashMap<usize, GalleryEntry>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let selection =
            gtk::SingleSelection::new(Some(model.store.clone().upcast::<gio::ListModel>()));
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup({
            let grid_view = grid_view.clone();
            move |_, item| {
                if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                    item.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
                }
                strip_item_targets(&grid_view);
            }
        });
        factory.connect_bind({
            let grid_view = grid_view.clone();
            let tile_entries = tile_entries.clone();
            move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let Some(entry) = item
                    .item()
                    .and_then(|object| object.downcast::<GalleryEntry>().ok())
                else {
                    return;
                };
                let tile = build_tile();
                let root_ptr = tile_key(&tile.root);
                item.set_child(Some(&tile.root));
                entry.set_tile(Rc::new(tile));
                tile_entries.borrow_mut().insert(root_ptr, entry);
                strip_item_targets(&grid_view);
            }
        });
        factory.connect_unbind({
            let tile_entries = tile_entries.clone();
            move |_, item| {
                if let Some(child) = item
                    .downcast_ref::<gtk::ListItem>()
                    .and_then(|item| item.child())
                {
                    tile_entries.borrow_mut().remove(&tile_key(&child));
                }
                let entry = item
                    .downcast_ref::<gtk::ListItem>()
                    .and_then(|item| item.item())
                    .and_then(|object| object.downcast::<GalleryEntry>().ok());
                if let Some(entry) = entry {
                    entry.clear_tile();
                }
            }
        });
        grid_view.set_model(Some(&selection));
        grid_view.set_factory(Some(&factory));
        grid_view.connect_activate({
            let sender = sender.clone();
            move |_, position| sender.input(Input::Activate(position))
        });
        strip_item_targets(&grid_view);

        // Primary click opens the file — the row widgets are
        // untargetable by design, so GTK's per-row click gesture (and
        // its single-click-activate) never sees a press; activation is
        // re-implemented here at grid level with the same contract.
        let open = gtk::GestureClick::new();
        open.set_button(gdk::BUTTON_PRIMARY);
        open.connect_released({
            let grid_view = grid_view.clone();
            let tile_entries = tile_entries.clone();
            let sender = sender.clone();
            move |gesture, n_press, x, y| {
                let Some(entry) = entry_at_point(&grid_view, x, y, &tile_entries) else {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                };
                if n_press != 1 || !entry.can_open() {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sender.input(Input::Open(entry.path()));
            }
        });
        grid_view.add_controller(open);

        // One right-click gesture on the permanent grid — never on a
        // tile. Tiles and ListItemWidgets are untargetable (they are
        // unparented when recycled, and crossing events over a dead
        // widget chain log `Gtk-CRITICAL gtk_widget_compute_point`).
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        click.connect_pressed({
            let grid_view = grid_view.clone();
            let tile_entries = tile_entries.clone();
            let sender = sender.clone();
            move |gesture, _, x, y| {
                let Some(entry) = entry_at_point(&grid_view, x, y, &tile_entries) else {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                };
                if entry.is_recording() {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sender.input(Input::ContextMenu {
                    path: entry.path(),
                    x,
                    y,
                });
            }
        });
        grid_view.add_controller(click);

        // One shared context-menu popover for the whole page, parented
        // to the grid (never to a recyclable tile).
        let popover = gtk::Popover::new();
        popover.set_has_arrow(true);
        popover.set_parent(&widgets.grid_view);
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let menu_button = |label: &str, input: fn() -> Input| {
            let button = gtk::Button::with_label(label);
            button.set_has_frame(false);
            let pop = popover.clone();
            let sender = sender.clone();
            button.connect_clicked(move |_| {
                pop.popdown();
                sender.input(input());
            });
            menu.append(&button);
        };
        menu_button(strings::GALLERY_OPEN, || Input::ContextOpen);
        menu_button(strings::GALLERY_SHOW_IN_FOLDER, || {
            Input::ContextShowInFolder
        });
        menu_button(strings::GALLERY_TRASH, || Input::ContextTrash);
        popover.set_child(Some(&menu));
        model.popover = popover;

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            Input::Refresh {
                photo_dir,
                video_dir,
            } => {
                self.photo_dir = photo_dir.clone();
                self.video_dir = video_dir.clone();
                tracing::debug!(
                    "gallery: opened (live pages: {})",
                    GALLERY_INSTANCES.load(Ordering::SeqCst)
                );
                self.opened_at = Some(Instant::now());
                self.first_thumb_logged = false;
                self.all_thumbs_logged = false;
                let worker = sender.clone();
                // Dropping the JoinHandle detaches the task.
                drop(gio::spawn_blocking(move || {
                    worker.input(Input::ScanDone(scan_media(&photo_dir, &video_dir)));
                }));
            }
            Input::ScanDone(items) => {
                if let Some(opened) = self.opened_at {
                    tracing::info!(
                        "gallery: {} items scanned in {:?}",
                        items.len(),
                        opened.elapsed()
                    );
                }
                self.merge_scan(items);
                self.sync_empty();
                self.pump();
            }
            Input::FileSaved(path) => {
                let worker = sender.clone();
                drop(gio::spawn_blocking(move || {
                    worker.input(Input::ItemReady(MediaItem::from_path(&path)));
                }));
            }
            Input::ItemReady(item) => self.on_item_ready(item),
            Input::RecordingStarted(path) => {
                // Keyed by the recording path: at most one synthetic
                // tile per recording — reopening the gallery during a
                // recording re-emits this input and must be a no-op.
                if let Some(existing) = &self.recording_entry {
                    if existing.path() == path {
                        return;
                    }
                    self.remove_recording_tile();
                }
                let entry = GalleryEntry::for_recording(&path);
                self.recording_entry = Some(entry.clone());
                self.store.insert(0, &entry);
                self.recording_elapsed = strings::elapsed_label(0);
                self.sync_empty();
            }
            Input::RecordingEnded => {
                self.remove_recording_tile();
            }
            Input::RecordElapsed(elapsed) => {
                self.recording_elapsed = elapsed;
            }
            Input::ThumbDone(outcome) => self.on_thumb_done(outcome),
            Input::Activate(position) => {
                if let Some(entry) = self.entry_at(position) {
                    self.open_entry(&entry, &sender);
                }
            }
            Input::Open(path) => {
                if let Some(entry) = self.by_path.get(&path).cloned() {
                    self.open_entry(&entry, &sender);
                }
            }
            Input::ContextMenu { path, x, y } => {
                self.context_path = Some(path);
                self.popover
                    .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                self.popover.popup();
            }
            Input::ContextOpen => {
                if let Some(path) = self.context_path.clone()
                    && let Some(entry) = self.by_path.get(&path).cloned()
                {
                    self.open_entry(&entry, &sender);
                }
            }
            Input::ContextShowInFolder => {
                if let Some(path) = self.context_path.clone()
                    && let Some(entry) = self.by_path.get(&path).cloned()
                {
                    self.show_in_folder(&entry, &sender);
                }
            }
            Input::ContextTrash => {
                if let Some(path) = self.context_path.clone() {
                    self.confirm_trash(&path, &sender);
                }
            }
            Input::TrashConfirmed(path) => {
                let worker = sender.clone();
                let target = path.clone();
                glib::MainContext::default().spawn_local(async move {
                    let ok = gio::File::for_path(&target)
                        .trash_future(glib::Priority::DEFAULT)
                        .await
                        .is_ok();
                    worker.input(Input::Trashed(path, ok));
                });
            }
            Input::Trashed(path, ok) => self.on_trashed(path, ok, &sender),
        }
    }
}

impl GalleryModel {
    /// Store index of `items[i]` — the recording tile shifts everything
    /// one position down.
    fn store_pos(&self, item_index: usize) -> u32 {
        (item_index + usize::from(self.recording_entry.is_some())) as u32
    }

    /// Path of the in-progress recording, filtered out of scans.
    fn recording_path(&self) -> Option<PathBuf> {
        self.recording_entry.as_ref().map(|e| e.path())
    }

    /// Removes the synthetic recording tile wherever it sits — removed
    /// by lookup, not by assuming index 0, so a tile displaced by a
    /// concurrent merge can't survive as a zombie "Gravando…".
    fn remove_recording_tile(&mut self) {
        let Some(entry) = self.recording_entry.take() else {
            return;
        };
        if let Some(pos) = self.store.find(&entry) {
            self.store.remove(pos);
        }
        self.sync_empty();
    }

    /// Entry displayed at store `position`.
    fn entry_at(&self, position: u32) -> Option<GalleryEntry> {
        self.store
            .item(position)
            .and_then(|object| object.downcast::<GalleryEntry>().ok())
    }

    /// Switches the stack between the empty status page and the grid.
    fn sync_empty(&self) {
        let child: &gtk::Widget = if self.store.n_items() == 0 {
            self.empty_page.upcast_ref()
        } else {
            self.grid_page.upcast_ref()
        };
        self.gallery_stack.set_visible_child(child);
    }

    /// Merges a fresh scan into the store: drops vanished files, inserts
    /// new ones in order, refreshes changed ones (stale thumbnails are
    /// regenerated because the cache key includes size + mtime).
    fn merge_scan(&mut self, scanned: Vec<MediaItem>) {
        let recording = self.recording_path();
        let scanned: Vec<MediaItem> = scanned
            .into_iter()
            .filter(|i| Some(&i.path) != recording.as_ref())
            .collect();
        let new_paths: HashSet<PathBuf> = scanned.iter().map(|i| i.path.clone()).collect();

        for i in (0..self.items.len()).rev() {
            if !new_paths.contains(&self.items[i].path) {
                let item = self.items.remove(i);
                self.by_path.remove(&item.path);
                self.requested.remove(&item.path);
                self.pending.retain(|p| p.path != item.path);
                self.store.remove(self.store_pos(i));
            }
        }

        for item in scanned {
            if let Some(entry) = self.by_path.get(&item.path).cloned() {
                if let Some(i) = self.items.iter().position(|x| x.path == item.path)
                    && self.items[i] != item
                {
                    self.items[i] = item.clone();
                    entry.update_item(&item);
                    self.requested.remove(&item.path);
                    self.request_thumb(&item);
                }
            } else {
                self.insert_item(item);
            }
        }
    }

    /// Inserts a media item in newest-first order and queues its thumb.
    fn insert_item(&mut self, item: MediaItem) {
        if self.by_path.contains_key(&item.path) {
            return;
        }
        let pos = insertion_index(&self.items, &item);
        self.items.insert(pos, item.clone());
        let entry = GalleryEntry::from_item(&item);
        self.by_path.insert(item.path.clone(), entry.clone());
        self.store.insert(self.store_pos(pos), &entry);
        self.request_thumb(&item);
        self.sync_empty();
    }

    /// A `FileSaved` stat arrived: morph the recording tile into the
    /// finished file when they share a path, otherwise insert.
    fn on_item_ready(&mut self, item: Option<MediaItem>) {
        let Some(item) = item else {
            return;
        };
        if let Some(entry) = &self.recording_entry
            && entry.path() == item.path
        {
            let entry = entry.clone();
            entry.finish_recording(&item);
            self.recording_entry = None;
            self.items.insert(0, item.clone());
            self.by_path.insert(item.path.clone(), entry);
            self.request_thumb(&item);
            return;
        }
        if self.by_path.contains_key(&item.path) {
            if let Some(entry) = self.by_path.get(&item.path).cloned()
                && let Some(i) = self.items.iter().position(|x| x.path == item.path)
                && self.items[i] != item
            {
                self.items[i] = item.clone();
                entry.update_item(&item);
                self.request_thumb(&item);
            }
            return;
        }
        self.insert_item(item);
    }

    /// Queues a thumbnail job or parks it in the backlog when the
    /// bounded queue is full (§7).
    fn request_thumb(&mut self, item: &MediaItem) {
        if self.requested.contains(&item.path) {
            return;
        }
        match self.thumbs.request(item) {
            Ok(()) => {
                self.requested.insert(item.path.clone());
            }
            Err(ThumbRejected::Busy) => self.pending.push_back(item.clone()),
            Err(ThumbRejected::Gone) => {}
        }
    }

    /// Drains the backlog into the bounded queue after a completion.
    fn pump(&mut self) {
        while let Some(item) = self.pending.front().cloned() {
            match self.thumbs.request(&item) {
                Ok(()) => {
                    self.requested.insert(item.path.clone());
                    self.pending.pop_front();
                }
                Err(ThumbRejected::Busy) => break,
                Err(ThumbRejected::Gone) => {
                    self.pending.clear();
                    break;
                }
            }
        }
    }

    /// A thumbnail job finished: update the tile and keep the queue fed.
    fn on_thumb_done(&mut self, outcome: ThumbOutcome) {
        let path = match &outcome {
            ThumbOutcome::Ready { path, .. } | ThumbOutcome::Corrupt { path } => path.clone(),
        };
        self.requested.remove(&path);
        if !self.first_thumb_logged
            && let Some(opened) = self.opened_at
        {
            self.first_thumb_logged = true;
            tracing::info!("gallery: first thumbnail in {:?}", opened.elapsed());
        }
        match outcome {
            ThumbOutcome::Ready {
                path,
                width,
                height,
                rgba,
                duration_secs,
            } => {
                if let Some(entry) = self.by_path.get(&path) {
                    let bytes = glib::Bytes::from_owned(rgba);
                    let texture = gdk::MemoryTexture::new(
                        width as i32,
                        height as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &bytes,
                        width as usize * 4,
                    );
                    entry.set_thumbnail(texture.upcast::<gdk::Paintable>(), duration_secs);
                }
            }
            ThumbOutcome::Corrupt { path } => {
                if let Some(entry) = self.by_path.get(&path) {
                    entry.set_corrupt();
                }
            }
        }
        self.pump();
        if !self.all_thumbs_logged
            && self.requested.is_empty()
            && self.pending.is_empty()
            && let Some(opened) = self.opened_at
        {
            self.all_thumbs_logged = true;
            tracing::info!("gallery: all thumbnails in {:?}", opened.elapsed());
        }
    }

    /// `gtk::FileLauncher` on the file — guarded against the recording
    /// tile, which cannot be opened.
    fn open_entry(&self, entry: &GalleryEntry, sender: &ComponentSender<Self>) {
        if !entry.can_open() {
            return;
        }
        let file = gio::File::for_path(entry.path());
        let window = self.window.clone();
        let output = sender.output_sender().clone();
        gtk::FileLauncher::new(Some(&file)).launch(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| {
                if result.is_err() {
                    output.emit(Output::Toast(strings::OPEN_FAILED));
                }
            },
        );
    }

    /// "Mostrar na pasta" — `FileLauncher::open_containing_folder`.
    fn show_in_folder(&self, entry: &GalleryEntry, sender: &ComponentSender<Self>) {
        if !entry.can_open() {
            return;
        }
        let file = gio::File::for_path(entry.path());
        let window = self.window.clone();
        let output = sender.output_sender().clone();
        gtk::FileLauncher::new(Some(&file)).open_containing_folder(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| {
                if result.is_err() {
                    output.emit(Output::Toast(strings::OPEN_FAILED));
                }
            },
        );
    }

    /// Asks before trashing (`adw::AlertDialog`, destructive action).
    fn confirm_trash(&self, path: &Path, sender: &ComponentSender<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dialog = adw::AlertDialog::new(
            Some(strings::TRASH_CONFIRM_TITLE),
            Some(&strings::trash_confirm_body(&name)),
        );
        dialog.add_response("cancel", strings::CANCEL_BUTTON);
        dialog.add_response("trash", strings::TRASH_CONFIRM_BUTTON);
        dialog.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let worker = sender.clone();
        let path = path.to_path_buf();
        dialog.choose(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |response| {
                if response == "trash" {
                    worker.input(Input::TrashConfirmed(path));
                }
            },
        );
    }

    /// Trash finished: remove the tile and the cached thumbnail.
    fn on_trashed(&mut self, path: PathBuf, ok: bool, sender: &ComponentSender<Self>) {
        if !ok {
            sender
                .output_sender()
                .emit(Output::Toast(strings::TRASH_FAILED));
            return;
        }
        if let Some(i) = self.items.iter().position(|x| x.path == path) {
            self.items.remove(i);
            self.store.remove(self.store_pos(i));
        }
        self.by_path.remove(&path);
        self.requested.remove(&path);
        self.pending.retain(|p| p.path != path);
        self.sync_empty();
        let cache = self.cache_dir.clone();
        drop(gio::spawn_blocking(move || {
            hiview_core::evict_cached_thumbs(&cache, &path);
        }));
        sender.output_sender().emit(Output::Toast(strings::TRASHED));
    }
}

impl Drop for GalleryModel {
    fn drop(&mut self) {
        GALLERY_INSTANCES.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `set_can_target(false)` on every row widget the grid owns.
/// Recycled list-item widgets are unparented on the fly — if one is
/// the pointer's target when that happens, GTK synthesizes crossing
/// events over a dead widget chain and logs `Gtk-CRITICAL
/// gtk_widget_compute_point`. Keeping the pointer on the permanent
/// grid avoids the whole class of failure.
///
/// Only `GtkList*` internals are stripped (GTK 4.14's
/// `GtkListItemWidget`/`GtkListHeaderWidget` are private types with no
/// binding — filtered by GType name). Other grid children, notably
/// the shared context-menu popover, must keep `can_target` or their
/// own buttons stop receiving clicks.
fn strip_item_targets(grid: &gtk::GridView) {
    let mut child = grid.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if widget.type_().name().starts_with("GtkList") {
            widget.set_can_target(false);
        }
        child = next;
    }
}

/// The pure core of [`entry_at_point`]: the first `(bounds, entry)`
/// pair whose bounds contain `point`, scanned in row order.
fn entry_in_bounds(
    point: graphene::Point,
    rows: &[(graphene::Rect, GalleryEntry)],
) -> Option<GalleryEntry> {
    rows.iter()
        .find(|(bounds, _)| bounds.contains_point(&point))
        .map(|(_, entry)| entry.clone())
}

/// Pointer identity of a tile root — the key of the tile-root → entry
/// map (entries are refcounted; keys are not dereferenced).
fn tile_key(widget: &impl IsA<gtk::Widget>) -> usize {
    widget
        .upcast_ref::<gtk::Widget>()
        .upcast_ref::<glib::Object>()
        .as_ptr() as usize
}

/// Entry under the grid point `(x, y)` — a manual hit-test over the
/// visible `ListItemWidget`s, since tiles are not pointer targets.
/// The link back to the model is the tile-root → entry map populated
/// at bind time (GTK 4.14's `GtkListItemWidget` is private, so the
/// tile root — the widget's first child — is the key).
///
/// All GTK calls happen while the map's `RefCell` is free: the widget
/// geometry is collected first, the map is borrowed only afterwards.
fn entry_at_point(
    grid: &gtk::GridView,
    x: f64,
    y: f64,
    tile_entries: &Rc<RefCell<HashMap<usize, GalleryEntry>>>,
) -> Option<GalleryEntry> {
    let point = graphene::Point::new(x as f32, y as f32);
    let mut keys = Vec::new();
    let mut child = grid.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if let Some(bounds) = widget.compute_bounds(grid)
            && let Some(root) = widget.first_child()
        {
            keys.push((bounds, tile_key(&root)));
        }
        child = next;
    }
    let entries = tile_entries.borrow();
    let rows: Vec<(graphene::Rect, GalleryEntry)> = keys
        .into_iter()
        .filter_map(|(bounds, key)| entries.get(&key).cloned().map(|e| (bounds, e)))
        .collect();
    drop(entries);
    entry_in_bounds(point, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_bounds(y: f32) -> graphene::Rect {
        graphene::Rect::new(0.0, y, 200.0, 170.0)
    }

    /// M5 review regression: the grid-level gestures resolve a click
    /// point to the row under it — bounds and entry must line up, and
    /// a miss must not select anything.
    #[test]
    fn hit_test_maps_point_to_entry() {
        let a = GalleryEntry::for_recording(Path::new("/v/a.mp4"));
        let b = GalleryEntry::for_recording(Path::new("/v/b.mp4"));
        let rows = [
            (tile_bounds(0.0), a.clone()),
            (tile_bounds(180.0), b.clone()),
        ];

        assert!(
            entry_in_bounds(graphene::Point::new(100.0, 100.0), &rows)
                .is_some_and(|entry| entry.path() == a.path())
        );
        assert!(
            entry_in_bounds(graphene::Point::new(100.0, 200.0), &rows)
                .is_some_and(|entry| entry.path() == b.path())
        );
        assert!(entry_in_bounds(graphene::Point::new(100.0, 500.0), &rows).is_none());
        assert!(entry_in_bounds(graphene::Point::new(250.0, 100.0), &rows).is_none());
    }
}
