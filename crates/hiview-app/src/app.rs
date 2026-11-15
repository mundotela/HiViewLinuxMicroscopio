// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Root application component (docs/CONVENTIONS.md §4).

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hiview_button::{ButtonEdge, ButtonEvent, SourceKind, SourceState, SourceUpdate, pick_source};
use hiview_capture::{
    AudioSource, CameraDescriptor, CameraEvent, CameraState, RecordDenied, RecordEvent, RecordStop,
    Recorder, RecorderOptions, check_disk_space,
};
use hiview_core::{
    AdjustKind, AppConfig, AppMode, ButtonTracker, CalibrationProfile, CrosshairStyle, GridSpacing,
    ImageAdjust, MeasureCommit, MeasureGesture, MeasureSet, MeasureTool, NOISE_FRAME_OPTIONS,
    Orientation, OverlayColor, OverlayState, PhotoFormat, PhotoResolution, Point, PressOutcome,
    ReleaseOutcome, ViewOrientation, config_path, digits_of, load_config, overlays_for, range_of,
    save_config, step_of,
};
use relm4::actions::{RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::gtk::{self, gio, glib};
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    adw,
};

use crate::cli::Args;
use crate::components::gallery::{
    GalleryInit, GalleryModel, Input as GalleryInput, Output as GalleryOutput,
};
use crate::measure_ui::MeasurePanel;
use crate::services::camera::CameraService;
use crate::services::noise::{NoiseCancel, NoiseCapture, NoiseOutcome};
use crate::services::photo::{PhotoOutcome, PhotoService};
use crate::services::recorder::{ActiveRecording, StopReason, filesystem_free_bytes};
use crate::strings;
use crate::widgets::viewport::Viewport;
use std::path::PathBuf;

/// Stylesheet for the FPS badge, freeze indicator and floating toolbar,
/// installed once at startup.
pub const GLOBAL_CSS: &str = ".fps-indicator {
    background-color: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    padding: 4px 10px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
}
.frozen-view {
    border: 4px solid #e5a50a;
}
.frozen-badge {
    background-color: rgba(0, 0, 0, 0.65);
    color: #f6d32d;
    padding: 6px 14px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
}
.floating-toolbar {
    background-color: rgba(0, 0, 0, 0.6);
    border-radius: 24px;
    padding: 4px;
    margin-bottom: 16px;
}
.shutter-flash {
    background-color: #ffffff;
    opacity: 0;
    transition: opacity 350ms ease-out;
}
.shutter-flash.on {
    opacity: 0.55;
    transition: none;
}
.noise-progress {
    background-color: rgba(0, 0, 0, 0.7);
    color: #ffffff;
    padding: 14px 18px;
    border-radius: 12px;
    font-weight: bold;
}
.shutter-button {
    padding: 12px;
    margin: 0 8px;
}
.shutter-button.recording {
    background-color: #e01b24;
    color: #ffffff;
}
.record-indicator {
    background-color: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    padding: 4px 10px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
}
.record-indicator image {
    color: #e01b24;
}
.gallery-tile {
    margin: 6px;
}
.gallery-thumb {
    background-color: rgba(0, 0, 0, 0.55);
    border-radius: 8px;
}
.play-badge {
    color: rgba(255, 255, 255, 0.92);
}
.thumb-badge {
    background-color: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 0.8em;
    font-weight: bold;
}";

/// Menu target value meaning "pick automatically".
const AUTO_TARGET: &str = "auto";

relm4::new_action_group!(WindowActionGroup, "win");
relm4::new_stateful_action!(
    SelectCameraAction,
    WindowActionGroup,
    "select-camera",
    String,
    String
);
relm4::new_stateless_action!(ZoomInAction, WindowActionGroup, "zoom-in");
relm4::new_stateless_action!(ZoomOutAction, WindowActionGroup, "zoom-out");
relm4::new_stateless_action!(FitViewAction, WindowActionGroup, "fit-view");
relm4::new_stateless_action!(Zoom100Action, WindowActionGroup, "zoom-100");
relm4::new_stateless_action!(MirrorHAction, WindowActionGroup, "mirror-h");
relm4::new_stateless_action!(MirrorVAction, WindowActionGroup, "mirror-v");
relm4::new_stateless_action!(RotateCcwAction, WindowActionGroup, "rotate-ccw");
relm4::new_stateless_action!(RotateCwAction, WindowActionGroup, "rotate-cw");
relm4::new_stateless_action!(FreezeAction, WindowActionGroup, "freeze");
relm4::new_stateless_action!(FullscreenAction, WindowActionGroup, "fullscreen");
relm4::new_stateless_action!(EscapeAction, WindowActionGroup, "escape");
relm4::new_stateless_action!(TakePhotoAction, WindowActionGroup, "take-photo");
relm4::new_stateless_action!(RecordAction, WindowActionGroup, "record");
relm4::new_stateless_action!(ToggleMicAction, WindowActionGroup, "toggle-mic");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(GalleryAction, WindowActionGroup, "gallery");
relm4::new_stateless_action!(ToggleModeAction, WindowActionGroup, "toggle-mode");
relm4::new_stateless_action!(ToggleGridAction, WindowActionGroup, "toggle-grid");
relm4::new_stateless_action!(ToggleCrossAction, WindowActionGroup, "toggle-crosshair");
relm4::new_stateless_action!(MeasureRulerAction, WindowActionGroup, "measure-ruler");
relm4::new_stateless_action!(MeasureAngleAction, WindowActionGroup, "measure-angle");
relm4::new_stateless_action!(MeasureCircleAction, WindowActionGroup, "measure-circle");
relm4::new_stateless_action!(ShortcutsAction, WindowActionGroup, "shortcuts");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(TourAction, WindowActionGroup, "tour");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");

use crate::shortcuts::shortcut_action;

/// Which capture the floating shutter button performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutterMode {
    /// The shutter button takes a photo.
    Photo,
    /// The shutter button starts/stops a recording (`R` always does).
    Video,
}

/// Inputs handled by the root component.
#[derive(Debug)]
pub enum Input {
    /// A camera event forwarded from the monitor/bus threads.
    Camera(CameraEvent),
    /// At least one new frame is pending in the frame bus.
    FrameReady,
    /// The user picked an entry in the camera menu.
    SelectCamera(String),
    /// "Usar" action of the microscope-connected toast (M12).
    UseMicroscope,
    /// Zoom in around the widget center (`+`).
    ZoomIn,
    /// Zoom out around the widget center (`-`).
    ZoomOut,
    /// Fit the frame to the window (`0`).
    FitView,
    /// Show the frame at native size (`1`).
    Zoom100,
    /// Toggle horizontal mirroring (`H`).
    MirrorH,
    /// Toggle vertical mirroring (`V`).
    MirrorV,
    /// Rotate 90° counterclockwise (`[`).
    RotateCcw,
    /// Rotate 90° clockwise (`]`).
    RotateCw,
    /// Freeze/unfreeze the displayed frame (`F`); the stream keeps running.
    ToggleFreeze,
    /// Toggle fullscreen (`F11`).
    ToggleFullscreen,
    /// Leave fullscreen (`Esc`).
    Escape,
    /// Take a photo (`Espaço`).
    TakePhoto,
    /// The floating shutter button was pressed; dispatches by mode.
    Shutter,
    /// Either mode toggle fired; the active one decides the mode.
    ShutterModeToggled,
    /// Start/stop a recording (`R` or the video-mode shutter).
    ToggleRecord,
    /// `Recorder::start` finished on the worker thread; the flag is
    /// whether the user had asked for an audio track (drives the
    /// "microphone unavailable" toast when the fallback recorded
    /// video-only).
    RecordStarted(Result<Recorder, String>, bool),
    /// One-second tick while recording (elapsed + dropped counters).
    RecordTick,
    /// Fast tick that refreshes the microphone level meter (~8 Hz).
    MicLevelTick,
    /// The record pipeline reported a fatal error mid-recording.
    RecordFailed(String),
    /// The microphone died mid-recording — finalize the file with
    /// both tracks (M10.5 minimum-accepted behavior).
    RecordAudioLost,
    /// `Recorder::stop` finished; carries the finalize outcome.
    RecordStopped(RecordStop),
    /// The window close button was pressed.
    CloseRequested,
    /// A photo save job finished on the worker thread.
    PhotoDone(PhotoOutcome),
    /// Open the preferences dialog (`Ctrl+,`).
    OpenPreferences,
    /// Open the keyboard shortcuts window (`Ctrl+?` or menu).
    OpenShortcuts,
    /// Open the About dialog (menu).
    OpenAbout,
    /// Open the welcome tour (menu; also auto-shown on first run).
    OpenTour,
    /// The tour was closed by any path — persist `tour_seen`.
    TourFinished,
    /// Photo format combo changed (0 = PNG, 1 = JPEG).
    SetPhotoFormat(u32),
    /// Resolution combo changed (0 = native, 1 = 1280x960, 2 = 1600x1200).
    SetPhotoResolution(u32),
    /// Noise-reduction combo changed (index into `NOISE_FRAME_OPTIONS`).
    SetNoiseReduction(u32),
    /// A noise-reduction capture reported progress/done/cancelled (M10).
    NoiseUpdate(NoiseOutcome),
    /// A new photo folder was picked in the preferences dialog.
    SetPhotoFolder(PathBuf),
    /// The toolbar microphone toggle was clicked (`Shift+R`).
    MicToggled,
    /// The "Gravar áudio do microfone" preferences switch changed.
    SetRecordAudio(bool),
    /// The microphone preferences combo changed — `None` is the
    /// system default input (M10.5).
    SetMicrophone(Option<String>),
    /// Open the gallery page (`Ctrl+G` or the header button).
    ToggleGallery,
    /// The gallery page was popped off the navigation stack.
    GalleryClosed,
    /// The gallery asks for a toast on the shared overlay.
    GalleryToast(&'static str),
    /// `M`: toggle Explorar/Bancada.
    ToggleMode,
    /// Either mode toggle in the header changed; read the active one.
    ModeToggled,
    /// `G`: toggle the grid (Bancada only).
    ToggleGrid,
    /// `C`: toggle the crosshair (Bancada only).
    ToggleCrosshair,
    /// Side panel grid switch.
    SetGridEnabled(bool),
    /// Side panel grid spacing, in image pixels.
    SetGridSpacing(f64),
    /// Side panel grid color (index into `OverlayColor::PRESETS`).
    SetGridColor(u32),
    /// Side panel grid opacity, `0.0..=1.0`.
    SetGridOpacity(f64),
    /// Side panel crosshair switch.
    SetCrosshairEnabled(bool),
    /// Side panel crosshair style (index into `CrosshairStyle` order).
    SetCrosshairStyle(u32),
    /// Side panel crosshair color (index into `OverlayColor::PRESETS`).
    SetCrosshairColor(u32),
    /// Preferences "Gravar sobreposições na foto" switch.
    SetBurnOverlays(bool),
    /// An image-adjustment slider moved (§6.11); coalesced into
    /// `ApplyAdjust` so a drag never floods the pipeline.
    SetAdjust(AdjustKind, f64),
    /// Debounced apply of the current adjustments to the pipeline and
    /// the config file.
    ApplyAdjust,
    /// "Restaurar padrões" — all five adjustments back to neutral.
    ResetAdjust,
    /// A measurement gesture from the viewport (image coords, §6.9).
    Measure(MeasureGesture),
    /// `L`/`A`/`O`: arm a tool (same key again disarms it).
    ArmMeasure(MeasureTool),
    /// The side-panel tool combo changed (index into `TOOL_NAMES`).
    ArmMeasureIndex(u32),
    /// Per-item delete in the measurement list.
    DeleteMeasurement(u64),
    /// "Limpar medições" — drop every committed measurement.
    ClearMeasurements,
    /// "Nova calibração…" — arms the reference-line tool.
    StartCalibration,
    /// The wizard committed a profile: name + real millimetres for
    /// the line stored in `pending_calibration`.
    CommitCalibration { name: String, mm: f64 },
    /// Active-profile combo changed (0 = no calibration).
    SelectCalibration(u32),
    /// "Renomear…" clicked — opens the rename dialog.
    RequestRenameCalibration,
    /// The rename dialog committed a new name.
    CalibrationRenamed(String),
    /// "Excluir…" clicked — opens the destructive confirmation.
    RequestDeleteCalibration,
    /// Profile deletion confirmed.
    DeleteCalibration,
    /// A Snap-button transport update: an edge or a connectivity
    /// change (§6.10).
    ButtonUpdate(SourceUpdate),
    /// The long-press timer fired while the button is held. Carries
    /// the EDGE-domain deadline captured at press time — the timer
    /// runs on the arrival clock but `fire_long` must be fed a
    /// timestamp in the same domain as `pressed_at` (usbmon stamps
    /// CLOCK_REALTIME, evdev CLOCK_MONOTONIC; never comparable to
    /// `glib::monotonic_time`).
    ButtonLongDeadline(u64),
    /// Preferences "Toque longo (ms)" spin value changed.
    SetButtonLongPress(f64),
}

/// Root component of the application.
pub struct AppModel {
    camera: CameraService,
    state: CameraState,
    /// Mirror/rotation toggles, forwarded to the pipeline's `videoflip`.
    orientation: ViewOrientation,
    /// View-only freeze: the pipeline keeps streaming underneath.
    frozen: bool,
    /// Window handle for fullscreen toggling.
    window: adw::ApplicationWindow,
    // Widget handles stored for imperative updates (frame upload, page
    // switching). All UI *state* lives in the fields above.
    content_stack: gtk::Stack,
    searching_page: adw::StatusPage,
    disconnected_page: adw::StatusPage,
    error_page: adw::StatusPage,
    video_page: gtk::Overlay,
    viewport: Viewport,
    menu_model: gio::Menu,
    /// Camera-list signature the current menu model was built from.
    /// `None` until the menu is first built, so an empty camera list
    /// still produces a menu.
    menu_signature: Option<Vec<String>>,

    /// MenuButton handle, updated imperatively only on real changes.
    menu_button: gtk::MenuButton,
    /// The microscope-connected toast was already shown for the
    /// current microscope presence — it fires once per transition
    /// (M12) and re-arms when the device leaves.
    microscope_toast_shown: bool,
    /// Frame size of the active camera, refreshed on `FrameReady` —
    /// filters the photo-resolution choices (M12).
    last_frame_size: Option<(u32, u32)>,

    /// Whether the FPS badge is enabled (debug builds or `--debug-overlay`).
    show_fps: bool,
    fps_label: String,
    frame_times: VecDeque<Instant>,
    /// Persistent preferences (docs/ARCHITECTURE.md section 6.12).
    config: AppConfig,
    /// Last `active_px_per_mm` seen at a camera state change — the
    /// calibration can only resolve while a pipeline runs (M12), so a
    /// state transition may activate/clear it just like a profile
    /// selection does.
    state_ppm: Option<f64>,
    /// Saver-worker channel: `persist_config` posts the latest config;
    /// the single worker drains to the newest value and writes it
    /// atomically — a burst of changes can never spawn a thread storm
    /// or race concurrent writers on the same file (v1.1.1).
    config_saver: std::sync::mpsc::Sender<AppConfig>,
    /// True when the config file was invalid and reset; a toast warns once.
    config_recovered: bool,
    /// Background photo encode/write worker (section 7).
    photos: PhotoService,
    toast_overlay: adw::ToastOverlay,
    /// The toast currently on screen; dismissed before showing a new
    /// one so a persistent toast can never hide the queue behind it.
    /// The displayed toast so a new one can replace it (crate::toast).
    toasts: crate::toast::ToastTracker<adw::Toast>,
    shutter_flash: gtk::Box,
    /// Folder row of the open preferences dialog, to refresh its subtitle.
    pref_folder_row: Option<adw::ActionRow>,
    /// Video-mode toggle handle: its `active` state selects the mode.
    mode_video_btn: gtk::ToggleButton,
    /// Which capture the shutter button performs (Foto/Video toggle).
    shutter_mode: ShutterMode,
    /// Recording in flight, if any.
    recording: Option<ActiveRecording>,
    /// A recorder start/stop is running on a worker thread (section 7).
    record_transition: bool,
    /// Why the in-flight finalize was triggered (drives the toast).
    stop_reason: Option<StopReason>,
    /// The window close was requested while recording/finalizing.
    quit_pending: bool,
    /// Whether an H.264 encoder exists on this system.
    rec_supported: bool,
    /// `--debug-overlay`: also shows the dropped-frame counter.
    debug_overlay: bool,
    /// Elapsed recording time shown in the red indicator.
    record_elapsed: String,
    /// Latest microphone peak 0..1 feeding the level meter (M10.5).
    mic_level: f32,
    /// Dropped-frame counter (debug overlay only).
    drops_label: String,
    /// Navigation stack: live page plus the pushed gallery page (§6.8).
    nav_view: adw::NavigationView,
    /// Gallery page component, kept alive so reopening is instant.
    gallery: Controller<GalleryModel>,
    /// Whether the gallery page is currently pushed.
    gallery_open: bool,
    /// Explorar/Bancada mode (§1), persisted in `config.mode`.
    mode: AppMode,
    /// The Bancada toggle in the header; its `active` selects the mode.
    mode_bench_btn: gtk::ToggleButton,
    /// Split view that hosts the Bancada side panel (§6.9).
    split_view: adw::OverlaySplitView,
    bench_sidebar: adw::ToolbarView,
    /// Current software image adjustments (§6.11) — what the sliders
    /// show and the pipeline applies.
    adjust: ImageAdjust,
    /// Config key of the active source's adjustments (`"VID:PID"`,
    /// `"test"`, `"file"`); `None` while no pipeline runs.
    adjust_key: Option<String>,
    /// Coalescing flag for slider drags: one pipeline apply per 80 ms.
    adjust_pending: Rc<Cell<bool>>,
    /// Every slider showing an adjustment — the Bancada panel rows plus
    /// the Explorar popover twins — kept for value sync.
    adjust_sliders: Vec<(AdjustKind, gtk::Scale)>,
    /// Containers disabled while no source streams (the Bancada group
    /// and the popover content): adjustments are meaningless without a
    /// device to persist them under.
    adjust_controls: Vec<gtk::Widget>,
    /// Committed measurements plus the armed tool's session (§6.9).
    /// Runtime state — never persisted.
    measure: MeasureSet,
    /// The reference line awaiting the calibration wizard's answer.
    pending_calibration: Option<(Point, Point)>,
    /// The tool armed before "Nova calibração…" — restored when the
    /// reference line commits and the wizard opens (§6.9).
    calibrate_restore: Option<MeasureTool>,
    /// The Medição panel widgets, built in `init` (list + dialogs are
    /// imperative — see `measure_ui.rs`).
    measure_panel: Option<MeasurePanel>,
    /// Input sink handed to the imperative Medição widgets and
    /// dialogs — `Rc` so contract tests inject a fake (the same
    /// pattern as the popover rows).
    measure_sink: Rc<dyn Fn(Input)>,
    /// The preferences page hosting the sidebar groups — the Medição
    /// group is added imperatively like the adjust rows.
    bench_prefs: adw::PreferencesPage,
    /// Grid spacing row: its unit (px/mm), digits and range follow the
    /// active calibration, so it is reconfigured imperatively.
    grid_spacing_row: adw::SpinRow,
    /// Snap-button edge interpreter: debounce + short/long split
    /// (§6.10). The long threshold mirrors `config.button`.
    button_tracker: ButtonTracker,
    /// Whether the button source was picked and started (once per app
    /// run — the helper reconnects by itself, and the button can only
    /// fire while the microscope streams).
    button_started: bool,
    /// Live transport status for the preferences row: `Some(kind)` =
    /// connected via that kind, `None` = unavailable.
    button_status: Option<SourceKind>,
    /// Status row of the open preferences dialog — updated live when
    /// the transport connects/drops.
    pref_button_status: Option<adw::ActionRow>,
    /// Noise-reduction capture in flight (M10): `Some` blocks new
    /// shots and rotation, `None` means the photo path is a single
    /// frame. Dropping the handle cancels the worker.
    noise_capture: Option<NoiseCapture>,
    /// `got / total` fraction shown by the progress overlay.
    noise_progress: f64,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = Args;
    type Input = Input;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some(strings::APP_TITLE),
            set_default_width: 960,
            set_default_height: 720,

            #[name = "toast_overlay"]
            #[wrap(Some)]
            set_content = &adw::ToastOverlay {
                #[name = "nav_view"]
                #[wrap(Some)]
                set_child = &adw::NavigationView {
                    add = &adw::NavigationPage {
                        set_title: strings::APP_TITLE,

                        #[wrap(Some)]
                        set_child = &adw::ToolbarView {
                            add_top_bar = &adw::HeaderBar {
                                // adw::ToggleGroup is libadwaita 1.7+;
                                // the system has 1.5, so two linked
                                // toggle buttons do the same job.
                                pack_start = &gtk::Box {
                                    set_css_classes: &["linked"],

                                    #[name = "mode_explore_btn"]
                                    gtk::ToggleButton {
                                        set_label: strings::MODE_EXPLORE_LABEL,
                                        set_tooltip_text: Some(strings::MODE_TOGGLE_TOOLTIP),
                                        set_focusable: false,
                                        #[watch]
                                        set_active: model.mode == AppMode::Explore,
                                        connect_toggled => Input::ModeToggled,
                                    },
                                    #[name = "mode_bench_btn"]
                                    gtk::ToggleButton {
                                        set_label: strings::MODE_BENCH_LABEL,
                                        set_tooltip_text: Some(strings::MODE_TOGGLE_TOOLTIP),
                                        set_focusable: false,
                                        set_group: Some(&mode_explore_btn),
                                        #[watch]
                                        set_active: model.mode == AppMode::Bench,
                                        connect_toggled => Input::ModeToggled,
                                    },
                                },

                                pack_end = &gtk::Button {
                                    set_label: strings::GALLERY_BUTTON_LABEL,
                                    set_tooltip_text: Some(strings::GALLERY_BUTTON_TOOLTIP),
                                    connect_clicked => Input::ToggleGallery,
                                },
                                #[name = "menu_button"]
                                pack_end = &gtk::MenuButton {
                                    set_icon_name: "open-menu-symbolic",
                                    set_tooltip_text: Some(strings::CAMERA_MENU_TOOLTIP),
                                    set_visible: true,
                                    set_menu_model: Some(&model.menu_model),
                                },
                            },

                            // Permanent while a calibration is active
                            // (§6.9): ring/height invalidate the scale.
                            add_top_bar = &adw::Banner {
                                set_title: strings::CALIBRATION_BANNER,
                                #[watch]
                                set_revealed: model.mode == AppMode::Bench
                                    && model.active_calibration().is_some(),
                            },

                            #[name = "split_view"]
                            #[wrap(Some)]
                            set_content = &adw::OverlaySplitView {
                                #[watch]
                                set_show_sidebar: model.mode == AppMode::Bench,

                                #[name = "bench_sidebar"]
                                #[wrap(Some)]
                                set_sidebar = &adw::ToolbarView {
                                    add_top_bar = &adw::HeaderBar {
                                        #[wrap(Some)]
                                        set_title_widget = &adw::WindowTitle {
                                            set_title: strings::BENCH_PANEL_TITLE,
                                        },
                                    },

                                    #[name = "bench_prefs"]
                                    #[wrap(Some)]
                                    set_content = &adw::PreferencesPage {
                                        add = &adw::PreferencesGroup {
                                            set_title: strings::GRID_GROUP_TITLE,

                                            adw::SwitchRow {
                                                set_title: strings::GRID_SHOW_LABEL,
                                                #[watch]
                                                set_active: model.config.overlays.grid.enabled,
                                                connect_active_notify[sender] => move |row| {
                                                    sender.input(Input::SetGridEnabled(row.is_active()));
                                                },
                                            },

                                            #[name = "grid_spacing"]
                                            adw::SpinRow {
                                                // The unit (px/mm),
                                                // digits and range
                                                // follow the active
                                                // calibration —
                                                // `sync_grid_unit`
                                                // reconfigures the row.
                                                #[watch]
                                                set_title: model.grid_spacing_label(),
                                                #[watch]
                                                set_value: model.grid_spacing_display(),
                                                connect_value_notify[sender] => move |row| {
                                                    sender.input(Input::SetGridSpacing(row.value()));
                                                },
                                            },

                                            #[name = "grid_color_row"]
                                            adw::ComboRow {
                                                set_title: strings::GRID_COLOR_LABEL,
                                                set_model: Some(&gtk::StringList::new(&strings::COLOR_NAMES)),
                                                #[watch]
                                                set_selected: model.config.overlays.grid.color.preset_index(),
                                                connect_selected_notify[sender] => move |row| {
                                                    sender.input(Input::SetGridColor(row.selected()));
                                                },
                                            },

                                            #[name = "grid_opacity_row"]
                                            adw::SpinRow {
                                                set_title: strings::GRID_OPACITY_LABEL,
                                                set_adjustment: Some(&gtk::Adjustment::new(
                                                    model.config.overlays.grid.opacity * 100.0,
                                                    10.0, 100.0, 5.0, 10.0, 0.0,
                                                )),
                                                set_digits: 0,
                                                #[watch]
                                                set_value: model.config.overlays.grid.opacity * 100.0,
                                                connect_value_notify[sender] => move |row| {
                                                    sender.input(Input::SetGridOpacity(row.value() / 100.0));
                                                },
                                            },
                                        },

                                        add = &adw::PreferencesGroup {
                                            set_title: strings::CROSS_GROUP_TITLE,

                                            adw::SwitchRow {
                                                set_title: strings::CROSS_SHOW_LABEL,
                                                #[watch]
                                                set_active: model.config.overlays.crosshair.enabled,
                                                connect_active_notify[sender] => move |row| {
                                                    sender.input(Input::SetCrosshairEnabled(row.is_active()));
                                                },
                                            },

                                            #[name = "cross_style_row"]
                                            adw::ComboRow {
                                                set_title: strings::CROSS_STYLE_LABEL,
                                                set_model: Some(&gtk::StringList::new(&strings::STYLE_NAMES)),
                                                #[watch]
                                                set_selected: model.config.overlays.crosshair.style as u32,
                                                connect_selected_notify[sender] => move |row| {
                                                    sender.input(Input::SetCrosshairStyle(row.selected()));
                                                },
                                            },

                                            #[name = "cross_color_row"]
                                            adw::ComboRow {
                                                set_title: strings::CROSS_COLOR_LABEL,
                                                set_model: Some(&gtk::StringList::new(&strings::COLOR_NAMES)),
                                                #[watch]
                                                set_selected: model.config.overlays.crosshair.color.preset_index(),
                                                connect_selected_notify[sender] => move |row| {
                                                    sender.input(Input::SetCrosshairColor(row.selected()));
                                                },
                                            },
                                        },

                                        // The five sliders and the
                                        // "Restaurar padrões" row are
                                        // appended imperatively in
                                        // `build_adjust_ui` — they share
                                        // one factory with the Explorar
                                        // popover (§6.11).
                                        #[name = "adjust_group"]
                                        add = &adw::PreferencesGroup {
                                            set_title: strings::ADJUST_GROUP_TITLE,
                                        },
                                    },
                                },

                                #[name = "content_stack"]
                                #[wrap(Some)]
                                set_content = &gtk::Stack {

                    #[name = "searching_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::SEARCHING_ICON),
                        set_title: strings::SEARCHING_TITLE,
                        set_description: Some(strings::SEARCHING_DESCRIPTION),
                    },


                    #[name = "disconnected_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::DISCONNECTED_ICON),
                        set_title: strings::DISCONNECTED_TITLE,
                        set_description: Some(strings::DISCONNECTED_DESCRIPTION),
                    },

                    #[name = "error_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::ERROR_ICON),
                        set_title: strings::ERROR_TITLE,
                        set_description: Some(strings::ERROR_DESCRIPTION),
                    },

                    #[name = "video_page"]
                    add_child = &gtk::Overlay {

                        #[name = "viewport"]
                        #[wrap(Some)]
                        set_child = &Viewport {
                        },

                        #[name = "shutter_flash"]
                        add_overlay = &gtk::Box {
                            set_css_classes: &["shutter-flash"],
                            set_can_target: false,
                        },

                        add_overlay = &gtk::Label {
                            set_halign: gtk::Align::End,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["fps-indicator"],
                            #[watch]
                            set_visible: model.show_fps
                                && model.state == CameraState::Streaming,
                            #[watch]
                            set_label: &model.fps_label,
                        },

                        add_overlay = &gtk::Label {
                            set_label: strings::FROZEN_BADGE,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["frozen-badge"],
                            #[watch]
                            set_visible: model.frozen,
                        },

                        // M10: centered "hold still" notice + progress
                        // bar while frames are averaged.
                        add_overlay = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_css_classes: &["noise-progress"],
                            set_can_target: false,
                            #[watch]
                            set_visible: model.noise_capture.is_some(),

                            gtk::Label {
                                set_label: strings::NOISE_HOLD_STILL,
                            },
                            gtk::ProgressBar {
                                set_width_request: 240,
                                #[watch]
                                set_fraction: model.noise_progress,
                            },
                        },

                        add_overlay = &gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 6,
                            set_halign: gtk::Align::Start,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["record-indicator"],
                            #[watch]
                            set_visible: model.recording.is_some(),

                            gtk::Image {
                                set_icon_name: Some("media-record-symbolic"),
                            },
                            gtk::Image {
                                set_icon_name: Some("audio-input-microphone-symbolic"),
                                #[watch]
                                set_visible: model.recording_has_audio(),
                            },
                            gtk::LevelBar {
                                #[watch]
                                set_visible: model.recording_has_audio(),
                                #[watch]
                                set_value: model.mic_level as f64,
                                set_width_request: 48,
                                set_valign: gtk::Align::Center,
                            },
                            gtk::Label {
                                #[watch]
                                set_label: &model.record_elapsed,
                            },
                            gtk::Label {
                                #[watch]
                                set_visible: model.debug_overlay,
                                #[watch]
                                set_label: &model.drops_label,
                            },
                        },

                        add_overlay = &gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::End,
                            set_css_classes: &["floating-toolbar"],

                            gtk::Button {
                                set_icon_name: "zoom-out-symbolic",
                                set_tooltip_text: Some(strings::ZOOM_OUT_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::ZoomOut,
                            },
                            gtk::Button {
                                set_icon_name: "zoom-in-symbolic",
                                set_tooltip_text: Some(strings::ZOOM_IN_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::ZoomIn,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Button {
                                set_icon_name: "object-flip-horizontal-symbolic",
                                set_tooltip_text: Some(strings::MIRROR_H_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::MirrorH,
                            },
                            gtk::Button {
                                set_icon_name: "object-flip-vertical-symbolic",
                                set_tooltip_text: Some(strings::MIRROR_V_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::MirrorV,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_css_classes: &["linked"],
                                #[name = "mode_photo_btn"]
                                gtk::ToggleButton {
                                    set_icon_name: "camera-photo-symbolic",
                                    set_tooltip_text: Some(strings::MODE_PHOTO_TOOLTIP),
                                    set_focusable: false,
                                    set_active: true,
                                    connect_toggled => Input::ShutterModeToggled,
                                },
                                #[name = "mode_video_btn"]
                                gtk::ToggleButton {
                                    set_icon_name: "camera-video-symbolic",
                                    set_focusable: false,
                                    set_group: Some(&mode_photo_btn),
                                    #[watch]
                                    set_sensitive: model.rec_supported,
                                    // Tooltips are set ONCE, here at
                                    // construction — never #[watch]:
                                    // every update_view would re-call
                                    // set_tooltip_text →
                                    // trigger_tooltip_query → pick
                                    // under the pointer → the GTK
                                    // 4.14 pick-recursion freeze
                                    // (v1.1.2 hotfix).
                                    set_tooltip_text: Some(model.video_mode_tooltip()),
                                    connect_toggled => Input::ShutterModeToggled,
                                },
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Button {
                                #[watch]
                                set_icon_name: model.shutter_icon(),
                                // Static tooltip, set once — the
                                // pick-recursion rule above.
                                set_tooltip_text: Some(strings::SHUTTER_MODE_TOOLTIP),
                                #[watch]
                                set_css_classes: model.shutter_css(),
                                set_focusable: false,
                                connect_clicked => Input::Shutter,
                            },
                            // M10.5: microphone toggle mirroring the
                            // "Gravar áudio" preference; a track cannot
                            // be added mid-file, so it is disabled
                            // while a recording is active.
                            gtk::ToggleButton {
                                #[watch]
                                set_icon_name: model.mic_icon(),
                                set_tooltip_text: Some(strings::MIC_TOGGLE_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                #[watch]
                                set_active: model.config.video.record_audio,
                                #[watch]
                                set_sensitive: model.recording.is_none(),
                                connect_clicked => Input::MicToggled,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Button {
                                set_icon_name: "object-rotate-left-symbolic",
                                set_tooltip_text: Some(strings::ROTATE_CCW_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::RotateCcw,
                                #[watch]
                                set_sensitive: model.recording.is_none(),
                            },
                            gtk::Button {
                                set_icon_name: "object-rotate-right-symbolic",
                                set_tooltip_text: Some(strings::ROTATE_CW_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::RotateCw,
                                #[watch]
                                set_sensitive: model.recording.is_none(),
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::ToggleButton {
                                set_icon_name: "weather-snow-symbolic",
                                set_tooltip_text: Some(strings::FREEZE_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                connect_clicked => Input::ToggleFreeze,
                                #[watch]
                                set_sensitive: model.recording.is_none(),
                                #[watch]
                                set_active: model.frozen,
                            },
                            // Explorar-only: a small popover with just
                            // brightness and contrast keeps the mode
                            // clean (§6.11); Bancada uses the side panel.
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                                #[watch]
                                set_visible: model.mode == AppMode::Explore,
                            },
                            #[name = "adjust_button"]
                            gtk::MenuButton {
                                set_icon_name: "display-brightness-symbolic",
                                set_tooltip_text: Some(strings::ADJUST_POPOVER_TOOLTIP),
                                set_has_frame: false,
                                set_focusable: false,
                                #[watch]
                                set_visible: model.mode == AppMode::Explore,

                                // The popover is attached at template
                                // build — a post-init `set_popover`
                                // proved fragile (it opened empty once
                                // after a hotplug). Rows are appended
                                // imperatively in `build_adjust_ui`.
                                #[wrap(Some)]
                                set_popover = &gtk::Popover {
                                    #[name = "adjust_pop_box"]
                                    #[wrap(Some)]
                                    set_child = &gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 6,
                                        set_margin_top: 6,
                                        set_margin_bottom: 6,
                                        set_width_request: 280,
                                    },
                                },
                            },
                        },
                    },
                },
                },
                },
                },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        if init.debug_overlay
            && let Some(settings) = gtk::Settings::default()
            && settings.has_property("gtk-debug-overlay")
        {
            settings.set_property("gtk-debug-overlay", true);
        }

        let on_event: Arc<dyn Fn(CameraEvent) + Send + Sync> = {
            let input = sender.input_sender().clone();
            Arc::new(move |event| {
                let _ = input.send(Input::Camera(event));
            })
        };

        let cfg_path = config_path();
        let config_outcome = load_config(&cfg_path);
        let config_recovered = config_outcome.was_recovered();
        let config = config_outcome.into_config();
        let config_mode = config.mode;
        let button_long_press = config.button.long_press_ms;

        let photos = PhotoService::new({
            let input = sender.input_sender().clone();
            move |outcome| {
                let _ = input.send(Input::PhotoDone(outcome));
            }
        });

        // The gallery launches before `view_output!` builds the
        // viewport, so it receives a cell the app fills afterwards —
        // its context popover returns keyboard focus to it (§7).
        let gallery_focus: crate::focus::Slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let gallery = GalleryModel::builder()
            .launch(GalleryInit {
                window: root.clone(),
                focus_slot: gallery_focus.clone(),
            })
            .forward(sender.input_sender(), |out| match out {
                GalleryOutput::Toast(text) => Input::GalleryToast(text),
            });

        let camera = CameraService::new(&init.source, on_event);
        camera.frame_bus().set_ui_notify(Box::new({
            let input = sender.input_sender().clone();
            move || {
                let _ = input.send(Input::FrameReady);
            }
        }));

        let mut model = Self {
            state: camera.state(),
            camera,
            orientation: ViewOrientation::default(),
            frozen: false,
            window: root.clone(),
            content_stack: gtk::Stack::new(),
            searching_page: adw::StatusPage::new(),
            disconnected_page: adw::StatusPage::new(),
            error_page: adw::StatusPage::new(),
            video_page: gtk::Overlay::new(),
            viewport: Viewport::new(),
            menu_model: gio::Menu::new(),
            menu_signature: None,
            microscope_toast_shown: false,
            last_frame_size: None,
            menu_button: gtk::MenuButton::new(),
            show_fps: cfg!(debug_assertions) || init.debug_overlay,
            fps_label: String::new(),
            frame_times: VecDeque::new(),
            config,
            state_ppm: None,
            config_saver: Self::spawn_config_saver(cfg_path),
            config_recovered,
            photos,
            toast_overlay: adw::ToastOverlay::new(),
            toasts: crate::toast::ToastTracker::default(),
            shutter_flash: gtk::Box::new(gtk::Orientation::Vertical, 0),
            pref_folder_row: None,
            mode_video_btn: gtk::ToggleButton::new(),
            shutter_mode: ShutterMode::Photo,
            recording: None,
            record_transition: false,
            stop_reason: None,
            quit_pending: false,
            rec_supported: hiview_capture::available_encoder().is_some(),
            debug_overlay: init.debug_overlay,
            record_elapsed: String::new(),
            mic_level: 0.0,
            drops_label: strings::drops_label(0),
            nav_view: adw::NavigationView::new(),
            gallery,
            gallery_open: false,
            mode: config_mode,
            mode_bench_btn: gtk::ToggleButton::new(),
            split_view: adw::OverlaySplitView::new(),
            bench_sidebar: adw::ToolbarView::new(),
            adjust: ImageAdjust::default(),
            adjust_key: None,
            adjust_pending: Rc::new(Cell::new(false)),
            adjust_sliders: Vec::new(),
            adjust_controls: Vec::new(),
            measure: MeasureSet::default(),
            pending_calibration: None,
            calibrate_restore: None,
            measure_panel: None,
            measure_sink: Rc::new(|_| {}),
            bench_prefs: adw::PreferencesPage::new(),
            grid_spacing_row: adw::SpinRow::with_range(0.0, 1.0, 1.0),
            button_tracker: ButtonTracker::new(50, button_long_press),
            button_started: false,
            button_status: None,
            pref_button_status: None,
            noise_capture: None,
            noise_progress: 0.0,
        };
        model.sync_menu();

        let widgets = view_output!();
        // No panel control may take the wheel — see
        // `measure_ui::swallow_scroll` (the "profile lost" regression
        // extended to every ComboRow/SpinRow by Henrique's rule).
        crate::measure_ui::swallow_scroll(&widgets.grid_color_row);
        crate::measure_ui::swallow_scroll(&widgets.cross_style_row);
        crate::measure_ui::swallow_scroll(&widgets.cross_color_row);
        crate::measure_ui::swallow_scroll(&widgets.grid_spacing);
        crate::measure_ui::swallow_scroll(&widgets.grid_opacity_row);
        model.grab_widgets(&widgets);
        // The gallery popover joins the shared focus-return contract
        // now that the real viewport exists (§7).
        *gallery_focus.borrow_mut() = Some(widgets.viewport.clone().upcast());
        // The slider rows (Bancada group + Explorar popover) are built
        // imperatively — they share one factory (§6.11).
        model.build_adjust_ui(&widgets, &sender);
        model.maybe_apply_stored_adjust();
        // The Medição group is imperative too (dynamic list, dialogs).
        // Its widgets need an `Rc` input sink — the popover pattern.
        let measure_sink: Rc<dyn Fn(Input)> = Rc::new({
            let sender = sender.clone();
            move |input| sender.input(input)
        });
        let (measure_group, measure_panel) = crate::measure_ui::build(&measure_sink);
        widgets.bench_prefs.add(&measure_group);
        model.measure_panel = Some(measure_panel);
        model.measure_sink = measure_sink;
        // Measurement gestures arrive as image coordinates from the
        // viewport's gesture controllers (§6.9).
        widgets.viewport.set_measure_handler({
            let sender = sender.clone();
            move |g| sender.input(Input::Measure(g))
        });
        model.sync_grid_unit();
        model.sync_measure_ui();
        model.nav_view = widgets.nav_view.clone();
        model.nav_view.connect_popped({
            let sender = sender.clone();
            let gallery_page = model.gallery.widget().clone();
            move |_, page| {
                if page == &gallery_page {
                    sender.input(Input::GalleryClosed);
                }
            }
        });
        // Debug aid (M4 orphan-overlay bug): log the actual widget tree
        // so a toast targeting an off-screen overlay is visible in logs.
        dump_widget_tree(&root, 0);
        if model.config_recovered {
            model.show_toast(adw::Toast::new(strings::CONFIG_RECOVERED));
        }

        let mut group = RelmActionGroup::<WindowActionGroup>::new();
        group.add_action(
            RelmAction::<SelectCameraAction>::new_stateful_with_target_value(
                &model.camera.selection_target(),
                {
                    let sender = sender.clone();
                    move |_, state, value: String| {
                        sender.input(Input::SelectCamera(value.clone()));
                        *state = value;
                    }
                },
            ),
        );
        macro_rules! view_action {
            ($action:ident, $input:expr) => {
                group.add_action(RelmAction::<$action>::new_stateless({
                    let sender = sender.clone();
                    move |_| sender.input($input)
                }));
            };
        }
        view_action!(ZoomInAction, Input::ZoomIn);
        view_action!(ZoomOutAction, Input::ZoomOut);
        view_action!(FitViewAction, Input::FitView);
        view_action!(Zoom100Action, Input::Zoom100);
        view_action!(MirrorHAction, Input::MirrorH);
        view_action!(MirrorVAction, Input::MirrorV);
        view_action!(RotateCcwAction, Input::RotateCcw);
        view_action!(RotateCwAction, Input::RotateCw);
        view_action!(FreezeAction, Input::ToggleFreeze);
        view_action!(FullscreenAction, Input::ToggleFullscreen);
        view_action!(EscapeAction, Input::Escape);
        view_action!(TakePhotoAction, Input::TakePhoto);
        view_action!(RecordAction, Input::ToggleRecord);
        view_action!(ToggleMicAction, Input::MicToggled);
        view_action!(PreferencesAction, Input::OpenPreferences);
        view_action!(GalleryAction, Input::ToggleGallery);
        view_action!(ToggleModeAction, Input::ToggleMode);
        view_action!(ToggleGridAction, Input::ToggleGrid);
        view_action!(ToggleCrossAction, Input::ToggleCrosshair);
        view_action!(MeasureRulerAction, Input::ArmMeasure(MeasureTool::Ruler));
        view_action!(MeasureAngleAction, Input::ArmMeasure(MeasureTool::Angle));
        view_action!(MeasureCircleAction, Input::ArmMeasure(MeasureTool::Circle));
        view_action!(ShortcutsAction, Input::OpenShortcuts);
        view_action!(AboutAction, Input::OpenAbout);
        view_action!(TourAction, Input::OpenTour);
        view_action!(QuitAction, Input::CloseRequested);

        // The window must finalize a running recording (EOS sequence)
        // before it may close — see Input::CloseRequested.
        root.connect_close_request({
            let sender = sender.clone();
            move |_| {
                sender.input(Input::CloseRequested);
                glib::Propagation::Stop
            }
        });
        group.register_for_widget(root.clone());

        // Ctrl+C and logout send SIGINT/SIGTERM; both run the same
        // finalize-then-quit path as the window close request. glib's
        // Rust bindings don't expose `g_unix_signal_add`, hence
        // `signal-hook` (see report §6).
        match signal_hook::iterator::Signals::new([
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
        ]) {
            Ok(mut signals) => {
                let sender = sender.clone();
                std::thread::spawn(move || {
                    if signals.forever().next().is_some() {
                        sender.input(Input::CloseRequested);
                    }
                });
            }
            Err(err) => tracing::warn!("cannot install signal handlers: {err}"),
        }

        // Shortcuts (docs/CONVENTIONS.md §13). NOT
        // `set_accelerators_for_action`: app-level accelerators fire
        // while a dialog is open and eat keystrokes destined for an
        // Entry (M8 round-2 finding). A capture-phase key controller
        // on the window runs `focus::shortcuts_allowed` first — never
        // with a dialog up, never with a text input focused.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let window = root.clone();
            move |_, keyval, _keycode, state| {
                if !crate::focus::shortcuts_allowed(&window) {
                    return glib::Propagation::Proceed;
                }
                match shortcut_action(keyval, state) {
                    Some(name) => {
                        let _ = gtk::prelude::WidgetExt::activate_action(&window, name, None);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                }
            }
        });
        root.add_controller(keys);

        // First run only (M11): the welcome tour presents itself once;
        // any close path sets `tour_seen` and it stays reachable from
        // the ☰ menu.
        if !model.config.ui.tour_seen {
            model.open_tour(&sender);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            Input::Camera(event) => {
                let microscope_arrived =
                    matches!(&event, CameraEvent::Connected(d) if d.is_microscope);
                let microscope_left =
                    matches!(&event, CameraEvent::Disconnected(d) if d.is_microscope);
                if microscope_left {
                    self.microscope_toast_shown = false;
                }
                self.camera.on_event(event);
                self.on_state_change();
                self.ensure_button_source(&sender);
                // M12: while another camera streams, a hot-plugged
                // microscope is OFFERED, never auto-selected. Once
                // per transition, and never during a recording.
                if microscope_arrived
                    && self.state == CameraState::Streaming
                    && !self.camera.active_is_microscope()
                    && self.recording.is_none()
                    && !self.microscope_toast_shown
                    && self.camera.microscope_id().is_some()
                {
                    self.microscope_toast_shown = true;
                    let toast = adw::Toast::new(strings::MICROSCOPE_CONNECTED_TOAST);
                    toast.set_button_label(Some(strings::MICROSCOPE_USE_ACTION));
                    let sender2 = sender.clone();
                    toast.connect_button_clicked(move |_| {
                        sender2.input(Input::UseMicroscope);
                    });
                    self.show_toast(toast);
                }
                if matches!(self.state, CameraState::Disconnected | CameraState::Error) {
                    // No frames will arrive — stop the average and
                    // explain (M10 cancel rule).
                    self.cancel_noise_capture();
                    if let Some(active) = self.recording.take() {
                        self.begin_finalize(active, StopReason::Disconnected, &sender);
                    }
                }
            }
            Input::SelectCamera(target) => {
                // Switching mid-recording would orphan the recorder's
                // frame source (M12) — refuse and explain.
                if self.recording.is_some() {
                    self.show_toast(adw::Toast::new(strings::SWITCH_WHILE_RECORDING));
                } else {
                    self.camera.select(&target);
                    self.on_state_change();
                    self.ensure_button_source(&sender);
                }
            }
            Input::UseMicroscope => {
                if let Some(id) = self.camera.microscope_id() {
                    sender.input(Input::SelectCamera(id));
                }
            }
            Input::FrameReady => {
                if let Some(frame) = self.camera.frame_bus().take_latest_for_ui() {
                    self.last_frame_size = Some((frame.width, frame.height));
                    self.viewport.set_frame(frame);
                    if self.show_fps {
                        self.tick_fps();
                    }
                }
            }
            Input::ZoomIn => self.viewport.zoom_in(),
            Input::ZoomOut => self.viewport.zoom_out(),
            Input::FitView => self.viewport.fit(),
            Input::Zoom100 => self.viewport.zoom_100(),
            Input::MirrorH => {
                let from = self.orientation.method();
                self.orientation.toggle_mirror_h();
                self.on_orientation_changed(from);
            }
            Input::MirrorV => {
                let from = self.orientation.method();
                self.orientation.toggle_mirror_v();
                self.on_orientation_changed(from);
            }
            // Rotation is blocked while recording (a video cannot
            // change resolution mid-file, §6.4) and during a noise
            // capture (M10: the frames must share one geometry).
            Input::RotateCcw if self.recording.is_none() && self.noise_capture.is_none() => {
                let from = self.orientation.method();
                self.orientation.rotate_ccw();
                self.on_orientation_changed(from);
            }
            Input::RotateCw if self.recording.is_none() && self.noise_capture.is_none() => {
                let from = self.orientation.method();
                self.orientation.rotate_cw();
                self.on_orientation_changed(from);
            }
            Input::RotateCcw | Input::RotateCw => {
                tracing::debug!("rotate ignored while recording or averaging");
            }
            Input::ToggleFreeze => {
                if self.state == CameraState::Streaming && self.recording.is_none() {
                    self.set_frozen(!self.frozen);
                }
            }
            Input::ToggleFullscreen => {
                if self.window.is_fullscreen() {
                    self.window.unfullscreen();
                } else {
                    self.window.fullscreen();
                }
            }
            Input::Escape => {
                // `Esc` cancels a measurement in progress before it
                // touches fullscreen (§6.9).
                if self.measure.cancel() {
                    self.measure_changed();
                } else if self.window.is_fullscreen() {
                    self.window.unfullscreen();
                }
            }
            Input::TakePhoto => self.take_photo(&sender),
            Input::Shutter => match self.shutter_mode {
                ShutterMode::Photo => self.take_photo(&sender),
                ShutterMode::Video => self.toggle_record(&sender),
            },
            Input::ShutterModeToggled => {
                self.shutter_mode = if self.mode_video_btn.is_active() {
                    ShutterMode::Video
                } else {
                    ShutterMode::Photo
                };
            }
            Input::ToggleRecord => self.toggle_record(&sender),
            Input::RecordStarted(result, want_audio) => {
                self.on_record_started(result, want_audio, &sender);
            }
            Input::RecordTick => {
                if let Some(active) = &self.recording {
                    self.record_elapsed =
                        strings::elapsed_label(active.started.elapsed().as_secs());
                    self.drops_label = strings::drops_label(
                        active.recorder.dropped_frames() + active.recorder.rejected_frames(),
                    );
                    self.gallery
                        .emit(GalleryInput::RecordElapsed(self.record_elapsed.clone()));
                    Self::schedule_record_tick(&sender);
                }
            }
            Input::MicLevelTick => {
                if let Some(active) = &self.recording
                    && let Some(level) = active.recorder.audio_level()
                {
                    self.mic_level = level;
                    Self::schedule_mic_tick(&sender);
                }
            }
            Input::RecordFailed(detail) => {
                tracing::error!("record pipeline reported an error: {detail}");
                if let Some(active) = self.recording.take() {
                    self.begin_finalize(active, StopReason::Failed, &sender);
                }
            }
            Input::RecordAudioLost => {
                // M10.5 minimum path: the muxer already got EOS on the
                // audio pad — finalize the file now with a toast.
                tracing::warn!("microphone lost mid-recording; finalizing");
                self.show_toast(adw::Toast::new(strings::MIC_LOST));
                if let Some(active) = self.recording.take() {
                    self.begin_finalize(active, StopReason::AudioLost, &sender);
                }
            }
            Input::RecordStopped(stop) => self.on_record_stopped(stop),
            Input::CloseRequested => {
                if let Some(active) = self.recording.take() {
                    self.quit_pending = true;
                    self.begin_finalize(active, StopReason::Close, &sender);
                } else if self.record_transition {
                    self.quit_pending = true;
                } else {
                    self.window.destroy();
                }
            }
            Input::PhotoDone(outcome) => self.on_photo_done(outcome),
            Input::OpenPreferences => self.open_preferences(&sender),
            Input::OpenShortcuts => self.open_shortcuts(),
            Input::OpenAbout => self.open_about(),
            Input::OpenTour => self.open_tour(&sender),
            Input::TourFinished => {
                if !self.config.ui.tour_seen {
                    self.config.ui.tour_seen = true;
                    self.persist_config();
                }
            }
            Input::SetPhotoFormat(sel) => {
                self.config.photo.format = match sel {
                    1 => PhotoFormat::Jpeg,
                    _ => PhotoFormat::Png,
                };
                self.persist_config();
            }
            Input::SetPhotoResolution(sel) => {
                let native = self.last_frame_size.unwrap_or((640, 480));
                let available = PhotoResolution::available_for(native);
                self.config.photo.resolution = available
                    .get(sel as usize)
                    .copied()
                    .unwrap_or(PhotoResolution::Native);
                self.persist_config();
            }
            Input::SetNoiseReduction(sel) => {
                let frames = NOISE_FRAME_OPTIONS.get(sel as usize).copied().unwrap_or(1);
                if self.config.photo.noise_reduction_frames != frames {
                    self.config.photo.noise_reduction_frames = frames;
                    self.persist_config();
                }
            }
            Input::NoiseUpdate(outcome) => self.on_noise_update(outcome),
            Input::SetPhotoFolder(path) => {
                self.config.photo.folder = Some(path);
                if let Some(row) = &self.pref_folder_row {
                    row.set_subtitle(&self.photo_dir().display().to_string());
                }
                self.persist_config();
                self.refresh_gallery();
            }
            Input::ToggleGallery => self.open_gallery(),
            Input::GalleryClosed => {
                self.gallery_open = false;
            }
            Input::GalleryToast(text) => self.show_toast(adw::Toast::new(text)),
            Input::ToggleMode => self.set_mode(self.mode.toggled()),
            Input::ModeToggled => {
                let mode = if self.mode_bench_btn.is_active() {
                    AppMode::Bench
                } else {
                    AppMode::Explore
                };
                self.set_mode(mode);
            }
            Input::ToggleGrid => {
                if self.bench_only() {
                    self.config.overlays.grid.enabled = !self.config.overlays.grid.enabled;
                    self.overlays_changed();
                }
            }
            Input::ToggleCrosshair => {
                if self.bench_only() {
                    self.config.overlays.crosshair.enabled =
                        !self.config.overlays.crosshair.enabled;
                    self.overlays_changed();
                }
            }
            Input::SetGridEnabled(enabled) => {
                if self.config.overlays.grid.enabled != enabled {
                    self.config.overlays.grid.enabled = enabled;
                    self.overlays_changed();
                }
            }
            Input::SetGridSpacing(value) => {
                // The row's unit follows the STORED variant — the
                // `rekeyed` conversions keep it aligned with the
                // calibration state (§6.9), so a mm value can never
                // land in a px-ranged row and get rewritten by a
                // clamp notify.
                let spacing = if self.grid_spacing_is_mm() {
                    GridSpacing::Millimeters(value.clamp(
                        hiview_core::MIN_GRID_SPACING_MM,
                        hiview_core::MAX_GRID_SPACING_MM,
                    ))
                } else {
                    GridSpacing::Pixels(value.clamp(
                        hiview_core::MIN_GRID_SPACING_PX,
                        hiview_core::MAX_GRID_SPACING_PX,
                    ))
                };
                if self.config.overlays.grid.spacing != spacing {
                    self.config.overlays.grid.spacing = spacing;
                    self.overlays_changed();
                }
            }
            Input::SetGridColor(index) => {
                if let Some(&color) = OverlayColor::PRESETS.get(index as usize)
                    && self.config.overlays.grid.color != color
                {
                    self.config.overlays.grid.color = color;
                    self.overlays_changed();
                }
            }
            Input::SetGridOpacity(opacity) => {
                let opacity = opacity.clamp(0.0, 1.0);
                if self.config.overlays.grid.opacity != opacity {
                    self.config.overlays.grid.opacity = opacity;
                    self.overlays_changed();
                }
            }
            Input::SetCrosshairEnabled(enabled) => {
                if self.config.overlays.crosshair.enabled != enabled {
                    self.config.overlays.crosshair.enabled = enabled;
                    self.overlays_changed();
                }
            }
            Input::SetCrosshairStyle(index) => {
                let style = match index {
                    1 => CrosshairStyle::Circle,
                    2 => CrosshairStyle::Ticks,
                    _ => CrosshairStyle::Cross,
                };
                if self.config.overlays.crosshair.style != style {
                    self.config.overlays.crosshair.style = style;
                    self.overlays_changed();
                }
            }
            Input::SetCrosshairColor(index) => {
                if let Some(&color) = OverlayColor::PRESETS.get(index as usize)
                    && self.config.overlays.crosshair.color != color
                {
                    self.config.overlays.crosshair.color = color;
                    self.overlays_changed();
                }
            }
            Input::SetBurnOverlays(burn) => {
                if self.config.photo.burn_overlays != burn {
                    self.config.photo.burn_overlays = burn;
                    self.persist_config();
                }
            }
            Input::MicToggled => {
                if self.recording.is_some() {
                    // A track cannot be added mid-file — the button is
                    // already insensitive, this guards the shortcut.
                    return;
                }
                self.set_record_audio(!self.config.video.record_audio);
            }
            Input::SetRecordAudio(on) => self.set_record_audio(on),
            Input::SetMicrophone(device) => {
                if self.config.video.microphone != device {
                    self.config.video.microphone = device;
                    self.persist_config();
                }
            }
            Input::SetAdjust(kind, value) => {
                self.adjust.set(kind, value);
                self.sync_adjust_sliders();
                self.schedule_adjust_apply(&sender);
            }
            Input::ApplyAdjust => self.apply_adjust(),
            Input::ResetAdjust => {
                self.adjust = ImageAdjust::default();
                self.sync_adjust_sliders();
                self.apply_adjust();
            }
            Input::Measure(gesture) => {
                match self.measure.gesture(gesture) {
                    MeasureCommit::CalibrationLine { a, b } => {
                        self.open_calibration_wizard(a, b);
                    }
                    MeasureCommit::Shape(_) | MeasureCommit::Nothing => {}
                }
                // Hover/drag drafts live in the overlay state — every
                // gesture refreshes the drawing.
                self.measure_changed();
            }
            Input::ArmMeasure(tool) => {
                if self.bench_only() {
                    // Same key again disarms the tool.
                    let next = (self.measure.armed_tool() != Some(tool)).then_some(tool);
                    self.arm_measure(next);
                }
            }
            Input::ArmMeasureIndex(index) => {
                if self.mode == AppMode::Bench {
                    self.arm_measure(match index {
                        1 => Some(MeasureTool::Ruler),
                        2 => Some(MeasureTool::Angle),
                        3 => Some(MeasureTool::Circle),
                        _ => None,
                    });
                }
            }
            Input::DeleteMeasurement(id) => {
                if self.measure.remove(id) {
                    self.measure_changed();
                }
            }
            Input::ClearMeasurements => {
                if !self.measure.items.is_empty() {
                    self.measure.clear();
                    self.measure_changed();
                }
            }
            Input::StartCalibration => {
                if self.bench_only() {
                    if self.state == CameraState::Streaming {
                        // Remember the armed tool — the wizard restores
                        // it once the reference line commits (§6.9).
                        if self.measure.armed_tool() != Some(MeasureTool::Calibrate) {
                            self.calibrate_restore = self.measure.armed_tool();
                        }
                        self.arm_measure(Some(MeasureTool::Calibrate));
                    } else {
                        self.show_toast(adw::Toast::new(strings::MEASURE_NO_FRAME));
                    }
                }
            }
            Input::CommitCalibration { name, mm } => {
                let device = self.camera.device_key();
                if let (Some((a, b)), Some(device)) = (self.pending_calibration.take(), device) {
                    let old_ppm = self.active_px_per_mm();
                    if let Some(profile) =
                        CalibrationProfile::from_line(a, b, mm, name.clone(), device)
                    {
                        self.config.upsert_profile(profile);
                        self.calibration_changed(old_ppm);
                        tracing::info!("calibration profile created: {name}");
                    }
                }
            }
            Input::SelectCalibration(index) => {
                // The combo lists only the ACTIVE DEVICE's profiles
                // (M12) — the index maps into `profiles_for`.
                let Some(device) = self.camera.device_key() else {
                    return;
                };
                // Ignore out-of-range selections — GTK can notify with
                // `INVALID_LIST_POSITION` (u32::MAX) while the combo's
                // model splices; treating it as "Nenhuma" would wipe
                // the active profile (M8 round-2 finding).
                if index as usize > self.config.profiles_for(&device).count() {
                    return;
                }
                let old_ppm = self.active_px_per_mm();
                let name = index.checked_sub(1).and_then(|i| {
                    self.config
                        .profiles_for(&device)
                        .nth(i as usize)
                        .map(|p| p.name.clone())
                });
                if self
                    .config
                    .active_calibrations
                    .get(&device)
                    .map(String::as_str)
                    != name.as_deref()
                {
                    self.config.set_active_profile(&device, name.as_deref());
                    self.calibration_changed(old_ppm);
                }
            }
            Input::RequestRenameCalibration => {
                if let Some(active) = self.active_calibration() {
                    let current = active.name.clone();
                    let others: Vec<String> = self
                        .config
                        .calibration
                        .iter()
                        .filter(|p| p.device == active.device)
                        .map(|p| p.name.clone())
                        .collect();
                    crate::measure_dialogs::open_rename_dialog(
                        &self.window,
                        &self.viewport,
                        &current,
                        others,
                        &self.measure_sink,
                    );
                }
            }
            Input::CalibrationRenamed(name) => {
                let trimmed = name.trim();
                let device = self.camera.device_key();
                if let (Some(active), Some(device)) = (self.active_calibration(), device)
                    && !trimmed.is_empty()
                    && !self
                        .config
                        .calibration
                        .iter()
                        .any(|p| p.device == device && p.name == trimmed && p.name != active.name)
                {
                    let old = active.name.clone();
                    if let Some(p) = self
                        .config
                        .calibration
                        .iter_mut()
                        .find(|p| p.device == device && p.name == old)
                    {
                        p.name = trimmed.to_string();
                    }
                    self.config.set_active_profile(&device, Some(trimmed));
                    self.persist_config();
                    self.sync_measure_ui();
                }
            }
            Input::RequestDeleteCalibration => {
                if let Some(active) = self.active_calibration() {
                    crate::measure_dialogs::confirm_delete_profile(
                        &self.window,
                        &active.name.clone(),
                        &self.measure_sink,
                    );
                }
            }
            Input::DeleteCalibration => {
                let old_ppm = self.active_px_per_mm();
                if let Some(device) = self.camera.device_key()
                    && let Some(name) = self.config.active_calibrations.get(&device).cloned()
                {
                    self.config.remove_profile(&device, &name);
                    self.calibration_changed(old_ppm);
                    tracing::info!("calibration profile deleted: {name}");
                }
            }
            Input::ButtonUpdate(update) => match update {
                SourceUpdate::State(kind, SourceState::Connected) => {
                    // Toast on TRANSITIONS only: the first connect
                    // (None → kind) counts, repeats do not — a source
                    // may re-report while recovering from a drop.
                    if self.button_status != Some(kind) {
                        self.button_status = Some(kind);
                        self.sync_button_status_row();
                        self.show_toast(adw::Toast::new(&strings::button_connected_toast(kind)));
                    }
                }
                SourceUpdate::State(_, SourceState::Disconnected) => {
                    self.button_tracker.reset();
                    if self.button_status.is_some() {
                        self.button_status = None;
                        self.sync_button_status_row();
                        self.show_toast(adw::Toast::new(strings::BUTTON_DISCONNECTED_TOAST));
                    }
                }
                SourceUpdate::Event(edge) => self.on_button_edge(edge, &sender),
            },
            Input::ButtonLongDeadline(deadline) => {
                // `deadline` is in the edge's kernel clock domain — the
                // stale-timer case is safe: a newer press re-bases
                // `pressed_at`, so an old deadline never fires early.
                if self.button_tracker.fire_long(deadline) {
                    self.toggle_record(&sender);
                }
            }
            Input::SetButtonLongPress(ms) => {
                self.config.button.long_press_ms = ms as u64;
                self.button_tracker.set_long_press_ms(ms as u64);
                self.persist_config();
            }
        }
    }
}

impl AppModel {
    /// Mirrors the widget handles the component needs to drive imperatively.
    fn grab_widgets(&mut self, widgets: &<Self as SimpleComponent>::Widgets) {
        self.content_stack = widgets.content_stack.clone();
        self.searching_page = widgets.searching_page.clone();
        self.disconnected_page = widgets.disconnected_page.clone();
        self.error_page = widgets.error_page.clone();
        self.video_page = widgets.video_page.clone();
        self.viewport = widgets.viewport.clone();
        self.menu_button = widgets.menu_button.clone();
        self.mode_video_btn = widgets.mode_video_btn.clone();
        // Without these binds the fields keep the `::new()` instances
        // created in init — off-screen orphans: every toast went to an
        // overlay that is not in the widget tree (toasts never visible).
        self.toast_overlay = widgets.toast_overlay.clone();
        self.shutter_flash = widgets.shutter_flash.clone();
        self.mode_bench_btn = widgets.mode_bench_btn.clone();
        self.split_view = widgets.split_view.clone();
        self.bench_sidebar = widgets.bench_sidebar.clone();
        self.bench_prefs = widgets.bench_prefs.clone();
        self.grid_spacing_row = widgets.grid_spacing.clone();
        // Push the persisted overlay/mode state into the viewport — the
        // placeholder overlays stay `None` (hidden) until this runs.
        self.apply_overlays();
        self.install_focus_return();
        // No camera event fires when nothing is plugged — without this
        // the stack would sit on the placeholder page and text
        // forever (the M12 alternatives hint never reached).
        self.on_state_change();
    }

    /// Searching-page description (pure, unit-tested): the base
    /// text gains the menu hint only while another camera exists.
    fn searching_description(alternatives: bool) -> String {
        if alternatives {
            format!(
                "{} {}",
                strings::SEARCHING_DESCRIPTION,
                strings::SEARCHING_OTHER_CAMERA_HINT
            )
        } else {
            strings::SEARCHING_DESCRIPTION.to_string()
        }
    }

    /// Syncs derived UI after a camera state change.
    fn on_state_change(&mut self) {
        self.state = self.camera.state();
        let page: &gtk::Widget = match self.state {
            CameraState::Searching | CameraState::Starting => {
                // M12: with other cameras available the searching
                // page also points to the Câmera menu; with none it
                // keeps the plain "connect it" text.
                self.searching_page
                    .set_description(Some(&Self::searching_description(
                        self.camera.offers_alternatives(),
                    )));
                self.searching_page.upcast_ref()
            }
            CameraState::Disconnected => self.disconnected_page.upcast_ref(),
            CameraState::Error => {
                if self.camera.discovery_error().is_some() {
                    self.error_page.set_title(strings::DISCOVERY_ERROR_TITLE);
                    self.error_page
                        .set_description(Some(strings::DISCOVERY_ERROR_DESCRIPTION));
                } else {
                    self.error_page.set_title(strings::ERROR_TITLE);
                    self.error_page
                        .set_description(Some(strings::ERROR_DESCRIPTION));
                }
                self.error_page.upcast_ref()
            }

            CameraState::Streaming => self.video_page.upcast_ref(),
        };
        self.content_stack.set_visible_child(page);
        tracing::debug!(
            "camera state {:?}: alternatives={}, searching_desc={:?}",
            self.state,
            self.camera.offers_alternatives(),
            self.searching_page.description(),
        );
        self.maybe_apply_stored_adjust();
        // M12 regression fix: `active_px_per_mm` needs a running
        // pipeline, so the calibration state can flip on transitions
        // (camera switch, stream start) — run the SAME path a profile
        // selection would: re-key px↔mm spacings, refresh the combo
        // and the spacing row, persist.
        let new_ppm = self.active_px_per_mm();
        if new_ppm != self.state_ppm {
            let old_ppm = self.state_ppm;
            self.state_ppm = new_ppm;
            self.calibration_changed(old_ppm);
        }
        self.sync_grid_unit();
        if self.state != CameraState::Streaming {
            self.viewport.reset();
            self.set_frozen(false);
            self.frame_times.clear();
            self.fps_label.clear();
        }
        self.sync_menu();
    }

    /// Starts the Snap-button transport once (§6.10). The pick happens
    /// on the first streaming transition because the evdev probe needs
    /// the camera's `ID_PATH`; the helper ignores it. After that the
    /// source never restarts — the helper reconnects to a restarted
    /// daemon by itself, and the button can only fire while the
    /// microscope streams anyway.
    fn ensure_button_source(&mut self, sender: &ComponentSender<Self>) {
        if self.button_started || self.state != CameraState::Streaming {
            return;
        }
        let usb_path = self.camera.active_usb_path();
        let (tx, rx) = std::sync::mpsc::channel::<SourceUpdate>();
        if let Err(err) = pick_source(usb_path.as_deref()).start(tx) {
            tracing::warn!("snap button source failed to start: {err}");
            return;
        }
        self.button_started = true;
        let input = sender.input_sender().clone();
        if let Err(err) = std::thread::Builder::new()
            .name("hiview-button-relay".into())
            .spawn(move || {
                for update in rx {
                    if input.send(Input::ButtonUpdate(update)).is_err() {
                        return;
                    }
                }
            })
        {
            tracing::warn!("snap button relay thread: {err}");
        }
    }

    /// A Snap edge arrived — interpret it via the tracker (§6:10).
    /// `edge.ts_ms` is the kernel's capture clock — CLOCK_REALTIME for
    /// the usbmon daemon, CLOCK_MONOTONIC for evdev — so a
    /// PRESSED/RELEASED pair delivered in the same batch still
    /// measures the real hold duration (same-domain delta). The
    /// long-press timer, however, is scheduled by ARRIVAL time:
    /// `edge.ts` and `glib::monotonic_time` are different domains and
    /// must never be subtracted.
    fn on_button_edge(&mut self, edge: ButtonEdge, sender: &ComponentSender<Self>) {
        match edge.event {
            ButtonEvent::Pressed => {
                if self.button_tracker.press(edge.ts_ms()) == PressOutcome::Accepted
                    && let Some(deadline) = self.button_tracker.long_deadline()
                {
                    // Arrival-clock scheduling: the timeout fires
                    // `long_press_ms` after the edge reached the app.
                    // `deadline` rides along so `fire_long` can
                    // re-check the hold in the edge's own domain.
                    let wait = Duration::from_millis(self.config.button.long_press_ms.max(1));
                    let sender = sender.clone();
                    glib::timeout_add_local_once(wait, move || {
                        sender.input(Input::ButtonLongDeadline(deadline));
                    });
                }
            }
            ButtonEvent::Released => {
                if self.button_tracker.release(edge.ts_ms()) == ReleaseOutcome::Short {
                    self.take_photo(sender);
                }
            }
        }
    }

    /// Pushes the live transport status into an open preferences row.
    fn sync_button_status_row(&self) {
        if let Some(row) = &self.pref_button_status {
            row.set_subtitle(strings::button_status(self.button_status));
        }
    }

    /// Forwards the current mirror/rotation to the capture pipeline.
    fn apply_orientation(&mut self) {
        if self.state == CameraState::Streaming {
            self.camera.set_orientation(self.orientation);
        }
    }

    /// The orientation was toggled: stored measurement points are
    /// remapped into the new displayed frame (`from` is the method
    /// before the toggle) so they stay glued to the measured feature.
    fn on_orientation_changed(&mut self, from: Orientation) {
        let to = self.orientation.method();
        if to != from {
            self.measure.remap(from, to, self.viewport.frame_size());
            self.apply_overlays();
        }
        self.apply_orientation();
    }

    /// The active calibration's pixels-per-millimetre FOR THE ACTIVE
    /// DEVICE, or `None` — measurements and grid/tick spacings fall
    /// back to pixels. A different camera's profile never applies
    /// (§6.9/M12).
    fn active_px_per_mm(&self) -> Option<f64> {
        self.active_calibration().map(|p| p.px_per_mm)
    }

    /// The calibration profile active for the device currently in
    /// use, keyed by the same VID:PID / stable-id rule as the image
    /// adjustments. `None` while no pipeline runs or when the device
    /// has no calibration ("Sem calibração para esta câmera").
    fn active_calibration(&self) -> Option<&CalibrationProfile> {
        let key = self.camera.device_key()?;
        self.config.active_profile_for(&key)
    }

    /// The effective overlay state: the persisted grid/crosshair plus
    /// the runtime measurement layer converted by the calibration.
    fn current_overlays(&self) -> Option<OverlayState> {
        let mut state = self.config.overlays.clone();
        state.measure = self.measure.overlay(self.active_px_per_mm());
        overlays_for(self.mode, &state)
    }

    /// Measurement state changed (gesture, arm, delete, cancel): push
    /// the overlay snapshot and resync the side-panel list.
    fn measure_changed(&mut self) {
        self.apply_overlays();
        self.sync_measure_ui();
    }

    /// Arms/disarms the measurement tool: while armed the viewport
    /// routes primary clicks/drags to the session and pans on the
    /// middle button (§6.9). Arming also drops a pending wizard line.
    fn arm_measure(&mut self, tool: Option<MeasureTool>) {
        if tool != Some(MeasureTool::Calibrate) {
            // An explicit tool choice discards a pending wizard line
            // and the saved "restore" tool — the user moved on.
            self.pending_calibration = None;
            self.calibrate_restore = None;
        }
        self.measure.arm(tool);
        self.viewport.set_measure_tool(tool);
        self.measure_changed();
    }

    /// Pushes measurement + calibration state into the side panel.
    fn sync_measure_ui(&mut self) {
        if let Some(panel) = self.measure_panel.as_mut() {
            // No pipeline → no device: an empty key matches no
            // profile, so the combo shows only "Nenhuma".
            let device = self.camera.device_key().unwrap_or_default();
            crate::measure_ui::sync(
                panel,
                &self.measure,
                &self.config,
                &device,
                &self.measure_sink,
            );
        }
    }

    /// The active calibration changed (select/create/delete): rekey
    /// grid and tick spacings to the new unit, persist, refresh.
    fn calibration_changed(&mut self, old_ppm: Option<f64>) {
        let new_ppm = self.active_px_per_mm();
        self.config.overlays.grid.spacing =
            self.config.overlays.grid.spacing.rekeyed(old_ppm, new_ppm);
        self.config.overlays.crosshair.tick_spacing = self
            .config
            .overlays
            .crosshair
            .tick_spacing
            .rekeyed(old_ppm, new_ppm);
        self.persist_config();
        self.apply_overlays();
        self.sync_grid_unit();
        self.sync_measure_ui();
    }

    /// The reference line was drawn: disarm the calibration tool —
    /// restoring whatever tool was armed before (or none) — remember
    /// the line, then open the wizard. The disarm MUST come first:
    /// `arm_measure` clears `pending_calibration` when the next tool
    /// isn't `Calibrate`.
    fn open_calibration_wizard(&mut self, a: Point, b: Point) {
        let restore = self.calibrate_restore.take();
        self.arm_measure(restore);
        self.pending_calibration = Some((a, b));
        let existing: Vec<String> = self
            .config
            .calibration
            .iter()
            .map(|p| p.name.clone())
            .collect();
        crate::measure_dialogs::open_calibration_dialog(
            &self.window,
            &self.viewport,
            hiview_core::distance(a, b),
            existing,
            strings::calibration_default_name(self.config.calibration.len() + 1),
            &self.measure_sink,
        );
    }

    /// Grid spacing row title — the unit follows the calibration (§6.9).
    fn grid_spacing_label(&self) -> &'static str {
        if self.grid_spacing_is_mm() {
            strings::GRID_SPACING_MM_LABEL
        } else {
            strings::GRID_SPACING_PX_LABEL
        }
    }

    /// Whether the stored grid spacing is expressed in millimetres —
    /// the unit the spacing row edits. `calibration_changed` re-keys
    /// the stored variant whenever the active calibration flips, so
    /// this is true exactly while a calibration applies (§6.9).
    fn grid_spacing_is_mm(&self) -> bool {
        matches!(
            self.config.overlays.grid.spacing,
            GridSpacing::Millimeters(_)
        )
    }

    /// The value the grid spacing row shows, in its current unit.
    fn grid_spacing_display(&self) -> f64 {
        match self.config.overlays.grid.spacing {
            GridSpacing::Pixels(px) => px,
            GridSpacing::Millimeters(mm) => mm,
        }
    }

    /// Reconfigures the grid spacing row for its current unit — the
    /// adjustment's range and digits can't come from a `#[watch]`.
    fn sync_grid_unit(&self) {
        let (value, lo, hi, step, digits) = if self.grid_spacing_is_mm() {
            (
                self.grid_spacing_display(),
                hiview_core::MIN_GRID_SPACING_MM,
                hiview_core::MAX_GRID_SPACING_MM,
                0.1,
                2,
            )
        } else {
            (
                self.grid_spacing_display(),
                hiview_core::MIN_GRID_SPACING_PX,
                hiview_core::MAX_GRID_SPACING_PX,
                5.0,
                0,
            )
        };
        self.grid_spacing_row
            .set_adjustment(Some(&gtk::Adjustment::new(
                value,
                lo,
                hi,
                step,
                step * 5.0,
                0.0,
            )));
        self.grid_spacing_row.set_digits(digits);
    }

    /// PNG `tEXt` provenance for the next photo (§6.6, M8): app
    /// version, mirror/rotation, the active calibration and — since
    /// M10 — `noise_reduction_frames` (1 on single-frame shots, N on
    /// averaged ones). JPEG files carry no metadata (out of scope).
    fn photo_png_text(&self, noise_frames: u32) -> Vec<(String, String)> {
        let mirror = match (self.orientation.mirror_h(), self.orientation.mirror_v()) {
            (true, true) => "H+V",
            (true, false) => "H",
            (false, true) => "V",
            (false, false) => "none",
        };
        let mut meta = vec![
            (
                "Software".into(),
                format!("HiView {}", env!("CARGO_PKG_VERSION")),
            ),
            ("Mirror".into(), mirror.to_string()),
            (
                "Rotation".into(),
                format!("{} deg CW", 90 * u32::from(self.orientation.quarters_cw())),
            ),
        ];
        if let Some(p) = self.active_calibration() {
            meta.push(("Calibration".into(), p.name.clone()));
            meta.push(("Calibration px/mm".into(), format!("{:.3}", p.px_per_mm)));
        }
        meta.push(("noise_reduction_frames".into(), noise_frames.to_string()));
        meta
    }

    /// Toggles the view-only freeze and its visual indicator.
    fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.viewport.set_frozen(frozen);
        if frozen {
            self.video_page.add_css_class("frozen-view");
        } else {
            self.video_page.remove_css_class("frozen-view");
        }
    }

    /// Menu entry (label, action target) for one camera (pure, unit-tested):
    /// the microscope keeps its fixed label, other cameras show the
    /// friendly product name with the node appended only when names
    /// collide; the target is always the stable id, never the /dev node.
    fn camera_menu_entry(
        camera: &CameraDescriptor,
        cameras: &[CameraDescriptor],
        active_node: Option<&std::path::Path>,
    ) -> (String, String) {
        let mut label = if camera.is_microscope {
            strings::MICROSCOPE_LABEL.to_string()
        } else if cameras
            .iter()
            .filter(|c| c.display_name() == camera.display_name())
            .count()
            > 1
        {
            format!("{} ({})", camera.display_name(), camera.dev_node.display())
        } else {
            camera.display_name().to_string()
        };
        // The entry the pipeline is using gets a check mark (M12).
        if active_node.is_some_and(|node| node == camera.dev_node.as_path()) {
            label = format!("✓ {label}");
        }
        (label, camera.stable_id())
    }

    /// Camera-list signature used to detect when the menu model must be
    /// rebuilt (pure, unit-tested). Frame updates never change it.
    fn camera_menu_signature(
        cameras: &[CameraDescriptor],
        active_node: Option<&std::path::Path>,
    ) -> Vec<String> {
        // The active node rides the signature so a camera SWITCH
        // rebuilds the ✓ marks even when the list itself is unchanged.
        let active = active_node.map(|p| p.display().to_string());
        std::iter::once(format!("active:{}", active.unwrap_or_default()))
            .chain(cameras.iter().map(|c| {
                format!(
                    "{}{}{}{}",
                    c.stable_id(),
                    c.display_name(),
                    c.dev_node.display(),
                    c.is_microscope
                )
            }))
            .collect()
    }

    /// Whether the camera list changed since the menu was last built
    /// (pure, unit-tested). `None` means the menu was never built.
    fn menu_needs_rebuild(
        previous: &Option<Vec<String>>,
        cameras: &[CameraDescriptor],
        active_node: Option<&std::path::Path>,
    ) -> bool {
        previous.as_ref() != Some(&Self::camera_menu_signature(cameras, active_node))
    }

    /// Rebuilds the menu model only when the camera list actually
    /// changed: re-setting the model on every update (e.g. the 30 fps
    /// frame updates) closes the open popover.
    fn sync_menu(&mut self) {
        let active_node = self.camera.active_dev_node();
        if !Self::menu_needs_rebuild(
            &self.menu_signature,
            self.camera.cameras(),
            active_node.as_deref(),
        ) {
            return;
        }
        self.menu_signature = Some(Self::camera_menu_signature(
            self.camera.cameras(),
            active_node.as_deref(),
        ));
        let menu = gio::Menu::new();
        if self.camera.has_camera_menu() {
            let cameras_menu = gio::Menu::new();
            cameras_menu.append_item(
                &RelmAction::<SelectCameraAction>::to_menu_item_with_target_value(
                    strings::CAMERA_AUTO_LABEL,
                    &AUTO_TARGET.to_string(),
                ),
            );
            for camera in self.camera.cameras() {
                let (label, target) =
                    Self::camera_menu_entry(camera, self.camera.cameras(), active_node.as_deref());
                cameras_menu.append_item(
                    &RelmAction::<SelectCameraAction>::to_menu_item_with_target_value(
                        &label, &target,
                    ),
                );
            }
            menu.append_submenu(Some(strings::CAMERA_SUBMENU_LABEL), &cameras_menu);
        }
        menu.append_item(&RelmAction::<PreferencesAction>::to_menu_item(
            strings::PREFERENCES_LABEL,
        ));
        menu.append_item(&RelmAction::<ShortcutsAction>::to_menu_item(
            strings::SHORTCUTS_LABEL,
        ));
        menu.append_item(&RelmAction::<TourAction>::to_menu_item(
            strings::TOUR_MENU_LABEL,
        ));
        menu.append_item(&RelmAction::<AboutAction>::to_menu_item(
            strings::ABOUT_LABEL,
        ));
        menu.append_item(&RelmAction::<QuitAction>::to_menu_item(strings::QUIT_LABEL));
        self.menu_model = menu.clone();
        self.menu_button.set_menu_model(Some(&menu));
        self.menu_button.set_visible(true);
    }

    /// Sliding-window FPS counter over the last second of rendered frames.
    fn tick_fps(&mut self) {
        let now = Instant::now();
        self.frame_times.push_back(now);
        while self
            .frame_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.frame_times.pop_front();
        }
        let label = format!("{} fps", self.frame_times.len());
        if label != self.fps_label {
            tracing::debug!("rendered: {label}");
            self.fps_label = label;
        }
    }

    /// Directory photos are saved into (section 6.6): the configured
    /// folder or the localized `Pictures/HiView` default.
    fn photo_dir(&self) -> PathBuf {
        self.config.photo.folder.clone().unwrap_or_else(|| {
            glib::user_special_dir(glib::UserDirectory::Pictures).map_or_else(
                || PathBuf::from("HiView"),
                |pictures| pictures.join("HiView"),
            )
        })
    }

    /// Directory videos are saved into (section 6.7): `Videos/HiView`.
    fn video_dir(&self) -> PathBuf {
        glib::user_special_dir(glib::UserDirectory::Videos)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("HiView")
    }

    /// Pushes the gallery page over the live view (F13/F14, §6.8). The
    /// camera pipeline is untouched, so returning has no reconnect
    /// delay.
    fn open_gallery(&mut self) {
        if self.gallery_open {
            return;
        }
        self.gallery_open = true;
        // Refresh also triggers a rescan inside the gallery component.
        self.gallery.emit(GalleryInput::Refresh {
            photo_dir: self.photo_dir(),
            video_dir: self.video_dir(),
        });
        // If a recording is already in flight the gallery must show the
        // indicator and the "Gravando…" tile right away.
        if let Some(active) = &self.recording {
            self.gallery.emit(GalleryInput::RecordingStarted(
                active.recorder.path().to_path_buf(),
            ));
            self.gallery
                .emit(GalleryInput::RecordElapsed(self.record_elapsed.clone()));
        }
        self.nav_view.push(self.gallery.widget());
    }

    /// Points the gallery at the current media folders and rescans.
    fn refresh_gallery(&self) {
        self.gallery.emit(GalleryInput::Refresh {
            photo_dir: self.photo_dir(),
            video_dir: self.video_dir(),
        });
    }

    /// Shows a toast, dismissing whatever is displayed first. Without
    /// this, `adw::ToastOverlay` queues the new toast behind the current
    /// one — and a persistent (`timeout` = 0) toast would hide every
    /// later toast forever, which is exactly what swallowed the
    /// recording-end toasts in M4 review.
    fn show_toast(&mut self, toast: adw::Toast) {
        // A persistent toast hides every toast queued behind it —
        // replace the displayed one instead of queueing.
        self.toasts.replace(toast.clone());
        // Diagnostic for manual testing: distinguishes "never shown"
        // from "shown but expired before the user looked".
        let shown = Instant::now();
        toast.connect_dismissed(move |t| {
            tracing::debug!(
                "toast dismissed: {:?} after {:?}",
                t.title(),
                shown.elapsed()
            );
        });
        self.toast_overlay.add_toast(toast);
    }

    /// Brief white flash over the video when a photo is taken.
    fn flash_shutter(&self) {
        self.shutter_flash.add_css_class("on");
        let flash = self.shutter_flash.clone();
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            flash.remove_css_class("on");
        });
    }

    /// Switches Explorar/Bancada (header toggle or `M`), persists the
    /// choice and applies the visible consequences: the side panel only
    /// exists in Bancada and overlays are only drawn there.
    fn set_mode(&mut self, mode: AppMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.config.mode = mode;
        // Explorar stays clean: disarm any tool and drop a pending
        // wizard line — committed measurements reappear on return.
        if mode == AppMode::Explore {
            self.measure.arm(None);
            self.pending_calibration = None;
            self.viewport.set_measure_tool(None);
        }
        self.persist_config();
        self.apply_overlays();
        tracing::info!("mode switched to {mode:?}");
    }

    /// Pushes the effective overlay state into the viewport:
    /// `overlays_for` hides everything in Explorar and when all overlays
    /// are off; switching back to Bancada restores the saved settings.
    fn apply_overlays(&self) {
        self.viewport.set_overlays(self.current_overlays());
    }

    /// Any overlay setting changed: persist and redraw.
    fn overlays_changed(&self) {
        self.persist_config();
        self.apply_overlays();
    }

    /// Whether a Bancada-only tool may run. In Explorar the overlay
    /// shortcuts show a toast instead of silently toggling hidden state
    /// (docs/CONVENTIONS.md §13 — same shortcuts, explained behavior).
    fn bench_only(&mut self) -> bool {
        if self.mode == AppMode::Bench {
            true
        } else {
            self.show_toast(adw::Toast::new(strings::BENCH_ONLY));
            false
        }
    }

    /// Takes a photo from the frozen/latest frame (section 6.6) —
    /// or starts a noise-reduction capture (M10) when the preference
    /// is above 1 and the view is live. Works while recording —
    /// photos are still allowed (M4). When "Gravar sobreposições na
    /// foto" is on, the visible overlays are burned in (frozen-frame
    /// photos included — `photo_frame` already picks the frozen one).
    fn take_photo(&mut self, sender: &ComponentSender<Self>) {
        if self.state != CameraState::Streaming {
            return;
        }
        // One capture at a time (M10): shots fired mid-average are
        // ignored, never queued.
        if self.noise_capture.is_some() {
            tracing::debug!("noise-reduction capture running; shot ignored");
            return;
        }
        let frames = self.config.photo.noise_frames();
        if frames > 1 && !self.frozen {
            self.start_noise_capture(frames, sender);
            return;
        }
        // Single-frame path: frozen frame or reduction off (a frozen
        // photo can never be averaged — metadata records `=1`).
        if let Some(frame) = self.viewport.photo_frame() {
            self.queue_photo(&frame, 1);
        }
    }

    /// Feeds the photo worker with `frame` and `noise_frames` for the
    /// `noise_reduction_frames` PNG tEXt key (1 on single-frame shots).
    fn queue_photo(&mut self, frame: &hiview_capture::Frame, noise_frames: u32) {
        // Clamp the stored resolution to what the ACTIVE frame can
        // produce (M12): a 1600×1200 choice left over from the
        // microscope falls back to native on a 720p webcam.
        let mut prefs = self.config.photo.clone();
        if !PhotoResolution::available_for((frame.width, frame.height)).contains(&prefs.resolution)
        {
            prefs.resolution = PhotoResolution::Native;
        }
        let overlays = if self.config.photo.burn_overlays {
            // Burn the committed measurements — never a
            // half-finished draft (§6.9).
            self.current_overlays().map(|mut s| {
                s.measure.draft = None;
                s
            })
        } else {
            None
        };
        if self
            .photos
            .take(
                frame,
                &prefs,
                self.photo_dir(),
                overlays,
                self.photo_png_text(noise_frames),
            )
            .is_ok()
        {
            self.flash_shutter();
        } else {
            self.show_toast(adw::Toast::new(strings::PHOTO_BUSY));
        }
    }

    /// M10: subscribes a full FrameBus consumer and lets the noise
    /// worker average `frames` consecutive frames. Everything after —
    /// progress overlay, cancel paths, the actual save — arrives via
    /// `Input::NoiseUpdate`.
    fn start_noise_capture(&mut self, frames: u32, sender: &ComponentSender<Self>) {
        let subscription = self.camera.frame_bus().subscribe();
        let input = sender.input_sender().clone();
        self.noise_progress = 0.0;
        self.noise_capture = Some(crate::services::noise::capture(
            subscription,
            frames,
            move |outcome| {
                let _ = input.send(Input::NoiseUpdate(outcome));
            },
        ));
        tracing::info!("noise reduction: averaging {frames} frames");
    }

    /// Worker reports from the noise capture: progress → bar, Done →
    /// the normal photo path (resolution/format/overlays/metadata),
    /// Cancelled → the matching toast. A stale report after an
    /// app-side cancel is dropped.
    fn on_noise_update(&mut self, outcome: NoiseOutcome) {
        if self.noise_capture.is_none() {
            tracing::debug!("noise update ignored: no capture running");
            return;
        }
        match outcome {
            NoiseOutcome::Progress { got, total } => {
                self.noise_progress = f64::from(got) / f64::from(total.max(1));
            }
            NoiseOutcome::Done { frame, frames } => {
                self.noise_capture = None;
                self.noise_progress = 0.0;
                self.queue_photo(&frame, frames);
            }
            NoiseOutcome::Cancelled(reason) => {
                self.noise_capture = None;
                self.noise_progress = 0.0;
                self.show_toast(adw::Toast::new(strings::noise_cancelled(reason)));
            }
        }
    }

    /// Stops a running capture because the camera went away: drops
    /// the handle (worker exits on its flag) and explains. Idempotent.
    fn cancel_noise_capture(&mut self) {
        if self.noise_capture.take().is_some() {
            self.noise_progress = 0.0;
            self.show_toast(adw::Toast::new(strings::noise_cancelled(
                NoiseCancel::Disconnected,
            )));
        }
    }

    /// `R` or the video-mode shutter: start or stop a recording (section
    /// 6.7). Both transitions run on a worker thread — starting builds a
    /// pipeline and touches the filesystem, stopping waits for EOS.
    fn toggle_record(&mut self, sender: &ComponentSender<Self>) {
        if self.record_transition {
            return;
        }
        if let Some(active) = self.recording.take() {
            self.begin_finalize(active, StopReason::User, sender);
            return;
        }
        if self.state != CameraState::Streaming {
            return;
        }
        if !self.rec_supported {
            self.show_toast(adw::Toast::new(strings::REC_UNSUPPORTED));
            return;
        }
        // Probe the XDG Videos root: the same filesystem as HiView/, and
        // it exists even before the folder does.
        let videos_root = glib::user_special_dir(glib::UserDirectory::Videos)
            .unwrap_or_else(|| PathBuf::from("."));
        match check_disk_space(&videos_root, filesystem_free_bytes) {
            Ok(()) => {}
            Err(RecordDenied::LowDiskSpace { free_mb }) => {
                tracing::warn!("recording refused: {free_mb} MB free");
                self.show_toast(adw::Toast::new(strings::REC_LOW_DISK));
                return;
            }
            Err(RecordDenied::FreeSpaceUnknown) => {
                tracing::warn!("recording refused: free space unknown");
                self.show_toast(adw::Toast::new(strings::REC_LOW_DISK));
                return;
            }
        }
        let Some(frame) = self.viewport.photo_frame() else {
            return;
        };
        let dir = videos_root.join("HiView");
        let subscription = self.camera.frame_bus().subscribe();
        // M10.5: the mic is only opened now, inside the record
        // pipeline — never for preview or monitoring. The shared clock
        // puts video and audio on one timeline (§6.7 A/V sync).
        let want_audio = self.config.video.record_audio;
        let options = RecorderOptions {
            audio: want_audio.then(|| AudioSource::Pulse {
                device: self.config.video.microphone.clone(),
            }),
            clock_ref: self.camera.clock_ref(),
        };
        self.record_transition = true;
        let worker = sender.clone();
        let reporter = sender.clone();
        std::thread::spawn(move || {
            let result = Recorder::start(
                subscription,
                frame.width,
                frame.height,
                30,
                1,
                &dir,
                options,
                move |event| match event {
                    RecordEvent::Error(detail) => reporter.input(Input::RecordFailed(detail)),
                    RecordEvent::AudioLost => reporter.input(Input::RecordAudioLost),
                },
            )
            .map_err(|e| e.to_string());
            worker.input(Input::RecordStarted(result, want_audio));
        });
    }

    /// Kicks the 1 Hz elapsed/dropped tick that reschedules itself while
    /// a recording is active.
    fn schedule_record_tick(sender: &ComponentSender<Self>) {
        let sender = sender.clone();
        glib::timeout_add_seconds_local_once(1, move || sender.input(Input::RecordTick));
    }

    /// Kicks the ~8 Hz tick refreshing the microphone level meter
    /// (M10.5). The `level` element posts at 100 ms; the UI simply
    /// reads the latest coalesced value, so nothing is logged per
    /// message (CONVENTIONS §7: toasts only on state changes).
    fn schedule_mic_tick(sender: &ComponentSender<Self>) {
        let sender = sender.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            sender.input(Input::MicLevelTick);
        });
    }

    /// Finalizes a recording off the UI thread (section 7): `stop()` can
    /// wait up to 5 s for EOS on the bus.
    fn begin_finalize(
        &mut self,
        active: ActiveRecording,
        reason: StopReason,
        sender: &ComponentSender<Self>,
    ) {
        self.record_transition = true;
        self.stop_reason = Some(reason);
        self.record_elapsed.clear();
        self.mic_level = 0.0;
        self.drops_label.clear();
        let worker = sender.clone();
        std::thread::spawn(move || {
            let stop = active.recorder.stop();
            worker.input(Input::RecordStopped(stop));
        });
    }

    /// A `Recorder::start` came back from the worker thread.
    fn on_record_started(
        &mut self,
        result: Result<Recorder, String>,
        want_audio: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.record_transition = false;
        match result {
            Ok(recorder) if self.quit_pending => {
                // The window close raced the worker start: finalize at
                // once instead of leaking a live recorder.
                self.begin_finalize(
                    ActiveRecording {
                        recorder,
                        started: Instant::now(),
                    },
                    StopReason::Close,
                    sender,
                );
                return;
            }
            Ok(recorder) => {
                // M10.5: recording is never blocked by the mic — when
                // the audio track could not be built the file simply
                // has no sound, and the user is told once.
                if want_audio && !recorder.has_audio() {
                    self.show_toast(adw::Toast::new(strings::MIC_UNAVAILABLE));
                }
                let has_audio = recorder.has_audio();
                tracing::info!("recording started (audio={has_audio})");
                self.gallery.emit(GalleryInput::RecordingStarted(
                    recorder.path().to_path_buf(),
                ));
                self.recording = Some(ActiveRecording {
                    recorder,
                    started: Instant::now(),
                });
                self.record_elapsed = strings::elapsed_label(0);
                self.mic_level = 0.0;
                self.gallery
                    .emit(GalleryInput::RecordElapsed(self.record_elapsed.clone()));
                Self::schedule_record_tick(sender);
                if has_audio {
                    Self::schedule_mic_tick(sender);
                }
            }
            Err(err) => {
                tracing::error!("recording failed to start: {err}");
                self.show_toast(adw::Toast::new(strings::VIDEO_SAVE_FAILED));
            }
        }
        if self.quit_pending {
            self.window.destroy();
        }
    }

    /// A `Recorder::stop` came back: report the outcome and quit if the
    /// finalize was triggered by the window closing.
    fn on_record_stopped(&mut self, stop: RecordStop) {
        self.record_transition = false;
        // FileSaved first so the "Gravando…" tile morphs into the
        // finished video in place; RecordingEnded is then a no-op.
        if stop.error.is_none() {
            self.gallery
                .emit(GalleryInput::FileSaved(stop.path.clone()));
        }
        self.gallery.emit(GalleryInput::RecordingEnded);
        let reason = self.stop_reason.take();
        match stop.error {
            Some(err) => {
                tracing::error!("recording finalize failed: {err}");
                // Without the EOS sequence the MP4 lacks `moov` — corrupt.
                if let Err(e) = std::fs::remove_file(&stop.path) {
                    tracing::warn!("could not remove broken video: {e}");
                }
                self.show_toast(adw::Toast::new(strings::VIDEO_SAVE_FAILED));
            }
            None if reason == Some(StopReason::Disconnected) => {
                tracing::info!(
                    "recording finalized after disconnect: {}",
                    stop.path.display()
                );
                // Persistent: the user must actually see this — the
                // disconnected StatusPage is up, so a 5 s toast would
                // expire unwatched.
                let toast = adw::Toast::new(strings::REC_ENDED_DISCONNECTED);
                toast.set_timeout(0);
                self.show_toast(toast);
            }
            None => {
                tracing::info!(
                    "video saved: {} ({} dropped, {} rejected)",
                    stop.path.display(),
                    stop.dropped_frames,
                    stop.rejected_frames
                );
                let toast = adw::Toast::new(strings::VIDEO_SAVED);
                toast.set_button_label(Some(strings::OPEN_BUTTON));
                // Persistent: the "Abrir" action must still be there
                // when the user comes back to the window.
                toast.set_timeout(0);
                let window = self.window.clone();
                toast.connect_button_clicked(move |_| {
                    let file = gio::File::for_path(&stop.path);
                    gtk::FileLauncher::new(Some(&file)).launch(
                        Some(&window),
                        gio::Cancellable::NONE,
                        |_| {},
                    );
                });
                self.show_toast(toast);
            }
        }
        if self.quit_pending {
            self.window.destroy();
        }
    }

    /// Tooltip for the video-mode toggle: explains when recording is
    /// unsupported on this system (no H.264 encoder).
    fn video_mode_tooltip(&self) -> &'static str {
        if self.rec_supported {
            strings::MODE_VIDEO_TOOLTIP
        } else {
            strings::MODE_VIDEO_UNAVAILABLE_TOOLTIP
        }
    }

    /// Icon shown on the shutter button for the current mode/state.
    fn shutter_icon(&self) -> &'static str {
        match (self.shutter_mode, self.recording.is_some()) {
            (_, true) => "media-playback-stop-symbolic",
            (ShutterMode::Video, false) => "media-record-symbolic",
            (ShutterMode::Photo, false) => "camera-photo-symbolic",
        }
    }

    /// CSS classes for the shutter button (`.recording` paints it red).
    fn shutter_css(&self) -> &'static [&'static str] {
        if self.recording.is_some() {
            &["circular", "shutter-button", "recording"]
        } else {
            &["circular", "shutter-button"]
        }
    }

    /// Reports the outcome of a background photo save (section 6.6).
    fn on_photo_done(&mut self, outcome: PhotoOutcome) {
        match outcome {
            PhotoOutcome::Saved(path) => {
                tracing::info!("photo saved: {}", path.display());
                self.gallery.emit(GalleryInput::FileSaved(path.clone()));
                let toast = adw::Toast::new(strings::PHOTO_SAVED);
                toast.set_button_label(Some(strings::OPEN_BUTTON));
                toast.set_timeout(6);
                let window = self.window.clone();
                toast.connect_button_clicked(move |_| {
                    let file = gio::File::for_path(&path);
                    gtk::FileLauncher::new(Some(&file)).launch(
                        Some(&window),
                        gio::Cancellable::NONE,
                        |_| {},
                    );
                });
                self.show_toast(toast);
            }
            PhotoOutcome::Failed(reason) => {
                tracing::error!("photo save failed: {reason}");
                self.show_toast(adw::Toast::new(strings::PHOTO_SAVE_FAILED));
            }
        }
    }

    /// Persists the "Gravar áudio do microfone" flag shared by the
    /// preferences switch, the toolbar toggle and `Shift+R` (M10.5).
    fn set_record_audio(&mut self, on: bool) {
        if self.config.video.record_audio != on {
            self.config.video.record_audio = on;
            self.persist_config();
        }
    }

    /// Toolbar microphone toggle icon — active input vs. disabled.
    fn mic_icon(&self) -> &'static str {
        if self.config.video.record_audio {
            "audio-input-microphone-symbolic"
        } else {
            "microphone-disabled-symbolic"
        }
    }

    /// Whether the in-flight recording actually carries an audio track
    /// (drives the mic icon and the level meter in the indicator).
    fn recording_has_audio(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(|active| active.recorder.has_audio())
    }

    /// Writes the current config off the UI thread (section 7).
    fn persist_config(&self) {
        let _ = self.config_saver.send(self.config.clone());
    }

    /// One background thread owns `config.toml` writes: it drains the
    /// channel to the LATEST posted config and persists that — bursts
    /// coalesce to a single atomic write instead of N racing threads.
    fn spawn_config_saver(path: PathBuf) -> std::sync::mpsc::Sender<AppConfig> {
        let (tx, rx) = std::sync::mpsc::channel::<AppConfig>();
        std::thread::Builder::new()
            .name("hiview-config-saver".into())
            .spawn(move || {
                while let Ok(mut config) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        config = newer;
                    }
                    if let Err(e) = save_config(&config, &path) {
                        tracing::error!("could not save config: {e}");
                    }
                }
            })
            .unwrap_or_else(|e| panic!("spawn config saver thread: {e}"));
        tx
    }

    /// Label of one adjustment slider (`strings::ADJUST_NAMES` follows
    /// `ImageAdjust::KINDS` order).
    fn adjust_label(kind: AdjustKind) -> &'static str {
        let index = ImageAdjust::KINDS
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(0);
        strings::ADJUST_NAMES[index]
    }

    /// One label + `gtk::Scale` row wired to `Input::SetAdjust`. The
    /// returned scale is stored for value sync — Bancada rows and the
    /// Explorar popover twins share the same model value (§6.11).
    fn adjust_row(kind: AdjustKind, sink: &Rc<dyn Fn(Input)>) -> (gtk::Widget, gtk::Scale) {
        let (min, max) = range_of(kind);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step_of(kind));
        // The wheel must scroll the panel, never the value (§13 rule).
        crate::measure_ui::swallow_scroll(&scale);
        scale.set_value(ImageAdjust::default().get(kind));
        scale.set_draw_value(true);
        scale.set_value_pos(gtk::PositionType::Right);
        scale.set_digits(digits_of(kind));
        if kind == AdjustKind::Sharpen {
            scale.set_format_value_func(|_, v| format!("{v:.0}%"));
        }
        scale.set_hexpand(true);
        scale.connect_value_changed({
            let sink = sink.clone();
            move |scale| sink(Input::SetAdjust(kind, scale.value()))
        });
        let label = gtk::Label::builder()
            .label(Self::adjust_label(kind))
            .xalign(0.0)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(9)
            .margin_bottom(9)
            .margin_start(12)
            .margin_end(12)
            .build();
        row.append(&label);
        row.append(&scale);
        (row.upcast(), scale)
    }

    /// Fills the Bancada "Imagem" group (five sliders + "Restaurar
    /// padrões") and builds the Explorar popover (brightness and
    /// contrast only — that mode stays visually clean, §6.11). Runs
    /// once in `init`, after `grab_widgets`.
    fn build_adjust_ui(
        &mut self,
        widgets: &<Self as SimpleComponent>::Widgets,
        sender: &ComponentSender<Self>,
    ) {
        let sink: Rc<dyn Fn(Input)> = {
            let sender = sender.clone();
            Rc::new(move |input| sender.input(input))
        };
        for kind in ImageAdjust::KINDS {
            let (row, scale) = Self::adjust_row(kind, &sink);
            widgets.adjust_group.add(&row);
            self.adjust_sliders.push((kind, scale));
        }
        let reset = adw::ActionRow::builder()
            .title(strings::ADJUST_RESET_LABEL)
            .activatable(true)
            .build();
        reset.add_suffix(&gtk::Image::from_icon_name("edit-undo-symbolic"));
        reset.connect_activated({
            let sender = sender.clone();
            move |_| sender.input(Input::ResetAdjust)
        });
        widgets.adjust_group.add(&reset);
        self.adjust_controls
            .push(widgets.adjust_group.clone().upcast());

        // The popover and its box are declared in view! — attach
        // happens at template build — so only the rows are
        // appended here (§6.11: Brilho + Contraste only).
        Self::build_popover_rows(&widgets.adjust_pop_box, &sink, &mut self.adjust_sliders);
        if let Some(popover) = widgets.adjust_button.popover() {
            crate::focus::watch_popover(&popover, &self.viewport);
        }
        self.adjust_controls
            .push(widgets.adjust_pop_box.clone().upcast());
    }

    /// Exactly the two Explorar controls (§6.11): Brilho and
    /// Contraste, each one a twin of its Bancada row — same
    /// `Input::SetAdjust` binding, same `adjust_sliders` entry.
    const EXPLORE_ADJUST_KINDS: [AdjustKind; 2] = [AdjustKind::Brightness, AdjustKind::Contrast];

    fn build_popover_rows(
        pop_box: &gtk::Box,
        sink: &Rc<dyn Fn(Input)>,
        scales: &mut Vec<(AdjustKind, gtk::Scale)>,
    ) {
        for kind in Self::EXPLORE_ADJUST_KINDS {
            let (row, scale) = Self::adjust_row(kind, sink);
            pop_box.append(&row);
            scales.push((kind, scale));
        }
    }

    /// Re-applies the stored adjustments whenever the active source
    /// changes: app start, camera switch and hotplug reconnect all
    /// re-key the running pipeline (`None` disables the controls —
    /// nothing to persist under while no source streams, §6.12).
    fn maybe_apply_stored_adjust(&mut self) {
        let key = self.camera.adjust_key();
        if key == self.adjust_key {
            return;
        }
        if let Some(key) = &key {
            self.adjust = self.config.adjust_for(key);
            self.camera.set_adjust(self.adjust);
        } else {
            self.adjust = ImageAdjust::default();
        }
        self.adjust_key = key;
        self.sync_adjust_sliders();
        let active = self.adjust_key.is_some();
        for widget in &self.adjust_controls {
            widget.set_sensitive(active);
        }
    }

    /// Every slider mirrors `self.adjust` — covers the Bancada/Explorar
    /// twins, "Restaurar padrões" and stored-value re-application.
    /// (`set_value` on the twin re-emits one `SetAdjust` that converges
    /// on the same value.)
    fn sync_scales(adjust: &ImageAdjust, scales: &[(AdjustKind, gtk::Scale)]) {
        for (kind, scale) in scales {
            let value = adjust.get(*kind);
            if (scale.value() - value).abs() > f64::EPSILON {
                scale.set_value(value);
            }
        }
    }

    fn sync_adjust_sliders(&self) {
        Self::sync_scales(&self.adjust, &self.adjust_sliders);
    }

    /// Slider drags emit `value-changed` per pixel; the actual
    /// GStreamer property write is coalesced to one per 80 ms so a drag
    /// never floods the pipeline (§6.11).
    fn schedule_adjust_apply(&self, sender: &ComponentSender<Self>) {
        if self.adjust_pending.replace(true) {
            return;
        }
        let pending = self.adjust_pending.clone();
        let sender = sender.clone();
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            pending.set(false);
            sender.input(Input::ApplyAdjust);
        });
    }

    /// Pushes the current adjustments to the pipeline — live property
    /// writes, no restart — and persists them under the active
    /// source's key (§6.12).
    fn apply_adjust(&mut self) {
        self.camera.set_adjust(self.adjust);
        if let Some(key) = self.adjust_key.clone() {
            self.config.set_adjust_for(&key, self.adjust);
            self.persist_config();
        }
    }

    /// Wires the shared focus-return helper (crate::focus): the window
    /// watchers, and a `closed` hook on every dialog/popover the app
    /// owns so single-key accelerators never stay dead (CONVENTIONS §7).
    fn install_focus_return(&self) {
        crate::focus::install(
            &self.window,
            &self.viewport,
            self.bench_sidebar.upcast_ref(),
        );
        // The camera-menu popover is created lazily by the MenuButton.
        if let Some(popover) = self.menu_button.popover() {
            crate::focus::watch_popover(&popover, &self.viewport);
        } else {
            let viewport = self.viewport.clone();
            self.menu_button
                .connect_notify_local(Some("popover"), move |button, _| {
                    if let Some(popover) = button.popover() {
                        crate::focus::watch_popover(&popover, &viewport);
                    }
                });
        }
    }

    /// Builds and presents the preferences dialog (Fotos page).
    fn open_preferences(&mut self, sender: &ComponentSender<Self>) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title(strings::PREFERENCES_LABEL);
        let page = adw::PreferencesPage::new();
        page.set_title(strings::PREFS_PHOTOS_PAGE);
        page.set_icon_name(Some("camera-photo-symbolic"));
        let group = adw::PreferencesGroup::new();
        group.set_title(strings::PREFS_PHOTOS_PAGE);

        let format_row = adw::ComboRow::new();
        crate::measure_ui::swallow_scroll(&format_row);
        format_row.set_title(strings::PREFS_FORMAT_LABEL);
        format_row.set_model(Some(&gtk::StringList::new(&["PNG", "JPEG"])));
        format_row.set_selected(match self.config.photo.format {
            PhotoFormat::Png => 0,
            PhotoFormat::Jpeg => 1,
        });
        format_row.connect_selected_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetPhotoFormat(row.selected()))
        });
        group.add(&format_row);

        let res_row = adw::ComboRow::new();
        crate::measure_ui::swallow_scroll(&res_row);
        res_row.set_title(strings::PREFS_RESOLUTION_LABEL);
        // Only the resolutions the ACTIVE camera can produce (M12):
        // interpolated sizes that would exceed 1600×1200 are hidden.
        let native = self.last_frame_size.unwrap_or((640, 480));
        let available = PhotoResolution::available_for(native);
        let labels: Vec<String> = available
            .iter()
            .map(|r| strings::resolution_label(*r, native))
            .collect();
        res_row.set_model(Some(&gtk::StringList::new(
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        res_row.set_selected(
            available
                .iter()
                .position(|r| *r == self.config.photo.resolution)
                .unwrap_or(0) as u32,
        );
        res_row.connect_selected_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetPhotoResolution(row.selected()))
        });
        group.add(&res_row);

        // M10: "Redução de ruído" — off / 4 / 8 / 16 averaged frames.
        let noise_row = adw::ComboRow::new();
        crate::measure_ui::swallow_scroll(&noise_row);
        noise_row.set_title(strings::PREFS_NOISE_LABEL);
        noise_row.set_model(Some(&gtk::StringList::new(&strings::NOISE_CHOICES)));
        noise_row.set_selected(
            NOISE_FRAME_OPTIONS
                .iter()
                .position(|&n| n == self.config.photo.noise_frames() as u8)
                .unwrap_or(0) as u32,
        );
        noise_row.connect_selected_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetNoiseReduction(row.selected()))
        });
        group.add(&noise_row);

        let folder_row = adw::ActionRow::new();
        folder_row.set_title(strings::PREFS_FOLDER_LABEL);
        folder_row.set_subtitle(&self.photo_dir().display().to_string());
        let pick = gtk::Button::with_label(strings::PREFS_FOLDER_BUTTON);
        pick.set_valign(gtk::Align::Center);
        pick.connect_clicked({
            let sender = sender.clone();
            let window = self.window.clone();
            move |_| {
                gtk::FileDialog::new().select_folder(Some(&window), gio::Cancellable::NONE, {
                    let sender = sender.clone();
                    move |res| {
                        if let Ok(file) = res
                            && let Some(path) = file.path()
                        {
                            sender.input(Input::SetPhotoFolder(path));
                        }
                    }
                });
            }
        });
        folder_row.add_suffix(&pick);
        folder_row.set_activatable_widget(Some(&pick));
        group.add(&folder_row);
        self.pref_folder_row = Some(folder_row);

        let burn_row = adw::SwitchRow::new();
        burn_row.set_title(strings::PREFS_BURN_LABEL);
        burn_row.set_active(self.config.photo.burn_overlays);
        burn_row.connect_active_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetBurnOverlays(row.is_active()))
        });
        group.add(&burn_row);

        page.add(&group);
        dialog.add(&page);

        // "Vídeo" page (M10.5): microphone recording opt-in plus the
        // source combo. The mic itself is only opened while a
        // recording runs — never for preview or monitoring.
        let vpage = adw::PreferencesPage::new();
        vpage.set_title(strings::PREFS_VIDEO_PAGE);
        vpage.set_icon_name(Some("camera-video-symbolic"));
        let vgroup = adw::PreferencesGroup::new();
        vgroup.set_title(strings::PREFS_VIDEO_PAGE);

        let audio_row = adw::SwitchRow::new();
        audio_row.set_title(strings::PREFS_RECORD_AUDIO_LABEL);
        audio_row.set_active(self.config.video.record_audio);
        audio_row.connect_active_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetRecordAudio(row.is_active()))
        });
        vgroup.add(&audio_row);

        let mics = hiview_capture::audio_sources();
        let mut labels = vec![strings::MIC_DEFAULT_LABEL.to_string()];
        labels.extend(mics.iter().map(|m| m.display_name.clone()));
        let mic_row = adw::ComboRow::new();
        crate::measure_ui::swallow_scroll(&mic_row);
        mic_row.set_title(strings::PREFS_MIC_LABEL);
        let model = gtk::StringList::new(&[]);
        for label in &labels {
            model.append(label);
        }
        mic_row.set_model(Some(&model));
        let selected = self
            .config
            .video
            .microphone
            .as_ref()
            .and_then(|name| mics.iter().position(|m| &m.device_name == name))
            .map(|pos| pos as u32 + 1)
            .unwrap_or(0);
        mic_row.set_selected(selected);
        mic_row.connect_selected_notify({
            let sender = sender.clone();
            move |row| {
                let index = row.selected();
                let device = if index == 0 {
                    None
                } else {
                    mics.get(index as usize - 1).map(|m| m.device_name.clone())
                };
                sender.input(Input::SetMicrophone(device));
            }
        });
        vgroup.add(&mic_row);

        vpage.add(&vgroup);
        dialog.add(&vpage);

        // "Botão físico" page (§6.10): live transport status plus the
        // long-press threshold that maps to `ButtonTracker`.
        let bpage = adw::PreferencesPage::new();
        bpage.set_title(strings::PREFS_BUTTON_PAGE);
        bpage.set_icon_name(Some("input-keyboard-symbolic"));
        let bgroup = adw::PreferencesGroup::new();
        bgroup.set_title(strings::PREFS_BUTTON_PAGE);
        bgroup.set_description(Some(strings::BUTTON_GROUP_DESC));

        let status_row = adw::ActionRow::new();
        status_row.set_title(strings::BUTTON_STATUS_LABEL);
        status_row.set_subtitle(strings::button_status(self.button_status));
        bgroup.add(&status_row);
        self.pref_button_status = Some(status_row);

        let long_row = adw::SpinRow::with_range(300.0, 2_000.0, 50.0);
        crate::measure_ui::swallow_scroll(&long_row);
        long_row.set_title(strings::BUTTON_LONG_PRESS_LABEL);
        long_row.set_value(self.config.button.long_press_ms as f64);
        long_row.connect_value_notify({
            let sender = sender.clone();
            move |row| sender.input(Input::SetButtonLongPress(row.value()))
        });
        bgroup.add(&long_row);

        bpage.add(&bgroup);
        dialog.add(&bpage);

        crate::focus::watch_dialog(dialog.upcast_ref::<adw::Dialog>(), &self.viewport);
        dialog.present(Some(&self.window));
    }

    /// Builds and presents the keyboard shortcuts window (`Ctrl+?`
    /// or the menu). Rows come from `shortcuts::SHORTCUTS` — the same
    /// table the key matcher uses (M11).
    fn open_shortcuts(&mut self) {
        #[allow(deprecated)]
        let window = crate::shortcuts::build_window();
        window.set_transient_for(Some(&self.window));
        window.set_modal(true);
        window.set_title(Some(strings::SHORTCUTS_LABEL));
        // Focus lands back on the viewport when the window closes so
        // single-key shortcuts keep working (CONVENTIONS §7).
        let viewport = self.viewport.clone();
        window.connect_destroy(move |_| {
            viewport.grab_focus();
        });
        window.present();
    }

    /// Builds and presents the About dialog (M11).
    fn open_about(&mut self) {
        crate::about::build().present(Some(&self.window));
    }

    /// Builds and presents the welcome tour — on first run and from
    /// the menu (M11). Closing it (any way) marks the tour as seen.
    fn open_tour(&mut self, sender: &ComponentSender<Self>) {
        let dialog = crate::tour::build({
            let tx = sender.input_sender().clone();
            move || {
                let _ = tx.send(Input::TourFinished);
            }
        });
        crate::focus::watch_dialog(&dialog, &self.viewport);
        dialog.present(Some(&self.window));
    }
}

/// Logs the visible widget tree (window → content → …) at debug level.
/// Written for the M4 orphan-overlay bug: `AppModel` kept an
/// `adw::ToastOverlay::new()` that was never in the tree, so every toast
/// went to an off-screen overlay — this dump makes that class of bug
/// visible in `RUST_LOG=debug` output.
fn dump_widget_tree(widget: &(impl IsA<gtk::Widget> + gtk::prelude::BuildableExt), depth: usize) {
    let id = widget
        .buildable_id()
        .map(|id| format!(" #{id}"))
        .unwrap_or_default();
    tracing::debug!("{}{}{}", "  ".repeat(depth), widget.type_().name(), id);
    let mut child = widget.first_child();
    while let Some(c) = child {
        dump_widget_tree(&c, depth + 1);
        child = c.next_sibling();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fake_camera(name: &str, node: &str, usb_path: Option<&str>) -> CameraDescriptor {
        CameraDescriptor {
            dev_node: PathBuf::from(node),
            name: name.into(),
            usb_vid: Some(0x1111),
            usb_pid: Some(0x2222),
            usb_path: usb_path.map(str::to_string),
            is_microscope: false,
            modes: Vec::new(),
        }
    }

    /// Regression for the M1 review bug: the menu action target must be
    /// the stable id (usb path), never the /dev node that selected_for
    /// cannot match.
    #[test]
    fn menu_target_is_stable_id_not_device_node() {
        let cam = fake_camera("HD camera", "/dev/video2", Some("pci-usb-0:5"));
        let (_, target) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam), None);
        assert_eq!(target, "pci-usb-0:5");
        assert_ne!(target, cam.dev_node.to_string_lossy());
    }

    #[test]
    fn menu_label_is_friendly_name_without_node() {
        let cam = fake_camera("ACER HD User Facing: ACER HD Us", "/dev/video0", Some("p"));
        let (label, _) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam), None);
        assert_eq!(label, "ACER HD User Facing");
    }

    #[test]
    fn menu_label_appends_node_only_for_duplicate_names() {
        let a = fake_camera("HD camera", "/dev/video0", Some("p1"));
        let b = fake_camera("HD camera", "/dev/video2", Some("p2"));
        let cams = [a.clone(), b];
        let (label, _) = AppModel::camera_menu_entry(&a, &cams, None);
        assert_eq!(label, "HD camera (/dev/video0)");
    }

    /// The menu model must only rebuild when the camera list changes;
    /// re-setting it on frame updates closes the open popover.
    #[test]
    fn menu_needs_rebuild_only_when_camera_list_changes() {
        let a = fake_camera("HD camera", "/dev/video0", Some("p1"));
        let b = fake_camera("HD camera", "/dev/video2", Some("p2"));
        let cams = [a.clone(), b.clone()];
        let sig = Some(AppModel::camera_menu_signature(&cams, None));
        // A frame update does not touch the camera list: no rebuild.
        assert!(!AppModel::menu_needs_rebuild(&sig, &cams, None));
        // Hotplug or a name collision changes the signature: rebuild.
        let after = [b, fake_camera("Other cam", "/dev/video4", Some("p3"))];
        assert!(AppModel::menu_needs_rebuild(&sig, &after, None));
        assert!(AppModel::menu_needs_rebuild(
            &sig,
            std::slice::from_ref(&a),
            None
        ));
    }

    /// The menu must exist even with zero cameras (M11 fix): before the
    /// first build the signature is `None`, which forces a rebuild —
    /// otherwise an empty camera list would match an empty signature
    /// and the menu would never appear.
    #[test]
    fn menu_builds_even_without_cameras() {
        assert!(AppModel::menu_needs_rebuild(&None, &[], None));
        let sig = Some(AppModel::camera_menu_signature(&[], None));
        assert!(!AppModel::menu_needs_rebuild(&sig, &[], None));
    }

    /// M12: the searching page carries the menu hint only while
    /// another camera exists — with none, the plain text stays.
    #[test]
    fn searching_page_gains_menu_hint_only_with_alternatives() {
        let with = AppModel::searching_description(true);
        assert!(with.contains(strings::SEARCHING_DESCRIPTION));
        assert!(with.contains(strings::SEARCHING_OTHER_CAMERA_HINT));
        assert_eq!(
            AppModel::searching_description(false),
            strings::SEARCHING_DESCRIPTION
        );
    }

    #[test]
    fn microscope_keeps_fixed_label() {
        let mut cam = fake_camera("HD camera", "/dev/video2", Some("p"));
        cam.is_microscope = true;
        let (label, _) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam), None);
        assert_eq!(label, strings::MICROSCOPE_LABEL);
    }

    /// The Explorar popover contract (§6.11, round-2 regression):
    /// it must hold exactly the Brilho and Contraste rows, each
    /// carrying a label + one Scale, and every twin must share
    /// the same `ImageAdjust` — same `Input::SetAdjust` emitted
    /// on drag, same value after `sync_scales` — so the two UIs
    /// can never diverge.
    ///
    /// NOT a `#[test]`: GTK can only init on one thread, so this
    /// runs inside the single `crate::tests::gtk_ui_contracts`.
    /// Source contract (v1.1.2): a `set_tooltip_*` property must never
    /// sit behind `#[watch]`/`#[track]` — every update_view would
    /// re-call the setter, re-triggering GTK's tooltip query → pick
    /// under the pointer → the GTK 4.14 pick-recursion freeze. This
    /// test fails if anyone re-adds a watched tooltip, and is what
    /// guarantees a spacing-only update never calls `set_tooltip_*`.
    #[test]
    fn no_watched_tooltips() {
        for src in [
            include_str!("app.rs"),
            include_str!("measure_dialogs.rs"),
            include_str!("measure_ui.rs"),
        ] {
            let mut watched = false;
            for line in src.lines() {
                let t = line.trim();
                if t.starts_with("#[watch]") || t.starts_with("#[track") {
                    watched = true;
                    continue;
                }
                if t.starts_with("set_tooltip_text") || t.starts_with("set_tooltip_markup") {
                    assert!(
                        !watched,
                        "`{t}` follows a #[watch]/#[track] — \
                         tooltips must be set once at construction"
                    );
                }
                // A blank/comment line keeps the attr context; any
                // other property breaks it.
                if !t.is_empty() && !t.starts_with("//") && !t.starts_with('#') {
                    watched = false;
                }
            }
        }
        // measure_dialogs must not call the setter in runtime
        // callbacks at all (the collision hint is a label).
        let md = include_str!("measure_dialogs.rs");
        assert!(
            !md.lines()
                .any(|l| l.trim_start().starts_with("name.set_tooltip")),
            "measure_dialogs.rs: runtime set_tooltip_* call found"
        );
    }

    pub(crate) fn check() {
        let emitted = Rc::new(std::cell::RefCell::new(Vec::<(AdjustKind, f64)>::new()));
        let sink: Rc<dyn Fn(Input)> = {
            let emitted = emitted.clone();
            Rc::new(move |input| {
                if let Input::SetAdjust(kind, value) = input {
                    emitted.borrow_mut().push((kind, value));
                }
            })
        };

        // The same factory the app uses fills the declared box.
        let pop_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let mut pop_scales = Vec::new();
        AppModel::build_popover_rows(&pop_box, &sink, &mut pop_scales);

        // Exactly two rows, in EXPLORE_ADJUST_KINDS order.
        assert_eq!(AppModel::EXPLORE_ADJUST_KINDS.len(), 2);
        assert_eq!(pop_scales.len(), 2);
        assert_eq!(pop_scales[0].0, AdjustKind::Brightness);
        assert_eq!(pop_scales[1].0, AdjustKind::Contrast);
        let mut rows = Vec::new();
        let mut child = pop_box.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            rows.push(c);
        }
        assert_eq!(rows.len(), 2);
        for ((kind, scale), row) in pop_scales.iter().zip(&rows) {
            let Some(row) = row.downcast_ref::<gtk::Box>() else {
                panic!("row is a Box")
            };
            let label = row
                .first_child()
                .and_then(|w| w.downcast::<gtk::Label>().ok());
            let Some(label) = label else {
                panic!("row starts with a Label")
            };
            assert_eq!(label.label(), AppModel::adjust_label(*kind));
            let twin = label
                .next_sibling()
                .and_then(|w| w.downcast::<gtk::Scale>().ok());
            let Some(twin) = twin else {
                panic!("row's second child is a Scale")
            };
            assert_eq!(&twin, scale);
            // The row carries exactly the label and the scale.
            assert_eq!(row.last_child(), Some(twin.upcast()));
        }

        // Shared state — model -> twins: a Bancada twin built by the
        // same factory and the popover twin must move together.
        let (_, bench_scale) = AppModel::adjust_row(AdjustKind::Brightness, &sink);
        let mut all_scales = vec![(AdjustKind::Brightness, bench_scale.clone())];
        all_scales.extend(pop_scales.iter().cloned());
        let mut adjust = ImageAdjust::default();
        adjust.set(AdjustKind::Brightness, 0.42);
        AppModel::sync_scales(&adjust, &all_scales);
        assert!((bench_scale.value() - 0.42).abs() < 1e-9);
        assert!((pop_scales[0].1.value() - 0.42).abs() < 1e-9);

        // Shared state — twins -> model: dragging either row emits
        // the same SetAdjust input (one logical value, §6.11).
        pop_scales[1].1.set_value(1.6);
        assert!(
            emitted
                .borrow()
                .iter()
                .any(|(k, v)| *k == AdjustKind::Contrast && (*v - 1.6).abs() < 1e-9)
        );
        bench_scale.set_value(-0.5);
        assert!(
            emitted
                .borrow()
                .iter()
                .any(|(k, v)| *k == AdjustKind::Brightness && (*v + 0.5).abs() < 1e-9)
        );

        // The §13 keymap — every letter/digit the panel advertises must
        // reach a win.* action, and `Ctrl` must not leak into the
        // single-key table (`Ctrl+G` is the gallery, not the grid).
        use gtk::gdk::{Key, ModifierType};
        assert_eq!(
            shortcut_action(Key::g, ModifierType::empty()),
            Some("win.toggle-grid")
        );
        assert_eq!(
            shortcut_action(Key::G, ModifierType::SHIFT_MASK),
            Some("win.toggle-grid")
        );
        assert_eq!(
            shortcut_action(Key::g, ModifierType::CONTROL_MASK),
            Some("win.gallery")
        );
        assert_eq!(
            shortcut_action(Key::l, ModifierType::empty()),
            Some("win.measure-ruler")
        );
        assert_eq!(
            shortcut_action(Key::a, ModifierType::empty()),
            Some("win.measure-angle")
        );
        assert_eq!(
            shortcut_action(Key::o, ModifierType::empty()),
            Some("win.measure-circle")
        );
        assert_eq!(
            shortcut_action(Key::space, ModifierType::empty()),
            Some("win.take-photo")
        );
        assert_eq!(
            shortcut_action(Key::Escape, ModifierType::empty()),
            Some("win.escape")
        );
        assert_eq!(
            shortcut_action(Key::comma, ModifierType::CONTROL_MASK),
            Some("win.preferences")
        );
        // M10.5: Shift+R toggles microphone recording; bare R still
        // starts/stops the recording itself.
        assert_eq!(
            shortcut_action(Key::R, ModifierType::SHIFT_MASK),
            Some("win.toggle-mic")
        );
        assert_eq!(
            shortcut_action(Key::r, ModifierType::empty()),
            Some("win.record")
        );
        // Not shortcuts: bare Ctrl+letter combos other than the two
        // defined, Alt-modified keys, unbound letters.
        assert_eq!(shortcut_action(Key::x, ModifierType::CONTROL_MASK), None);
        assert_eq!(shortcut_action(Key::g, ModifierType::ALT_MASK), None);
        assert_eq!(shortcut_action(Key::x, ModifierType::empty()), None);
    }
}
