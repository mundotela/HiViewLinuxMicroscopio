// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Camera service: owns discovery, hotplug monitoring and the capture
//! pipeline, and drives the camera state machine (docs/ARCHITECTURE.md §6.3).

use std::sync::Arc;

use hiview_capture::{
    CameraDescriptor, CameraEvent, CameraState, CapturePipeline, CaptureSource, DeviceWatcher,
    Error, FrameBus,
};
use hiview_core::{ImageAdjust, ViewOrientation};
use tracing::{debug, error, info, warn};

use crate::cli::Source;

/// Camera chosen by the user in the camera menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Prefer a known microscope; nothing starts without one.
    Auto,
    /// A specific physical camera, keyed by its stable id (udev `ID_PATH`),
    /// so the selection survives `/dev/videoN` renumbering on replug.
    Manual(String),
}

/// Resolves a selection to a camera in the current list (pure logic,
/// unit-tested).
fn selected_for<'a>(
    selection: &Selection,
    cameras: &'a [CameraDescriptor],
) -> Option<&'a CameraDescriptor> {
    match selection {
        Selection::Auto => cameras.iter().find(|c| c.is_microscope),
        Selection::Manual(key) => cameras.iter().find(|c| c.stable_id() == *key),
    }
}

/// Whether the "use another camera" page applies (M12 — pure, unit
/// tested): device mode, nothing streaming yet, at least one camera
/// known and NONE of them the microscope.
fn offers_alternatives_for(mode: Mode, has_pipeline: bool, cameras: &[CameraDescriptor]) -> bool {
    mode == Mode::Device
        && !has_pipeline
        && !cameras.is_empty()
        && cameras.iter().all(|c| !c.is_microscope)
}

/// Whether a pipeline bus error means the device was physically removed
/// rather than a real failure (CONVENTIONS §6: `error` only for failures
/// the user perceives). The `v4l2src` poll error fires as soon as the node
/// dies, usually before the udev remove event arrives.
fn is_disconnect_error(detail: &str) -> bool {
    detail.contains("poll error") || detail.contains("ENODEV") || detail.contains("ENOENT")
}

/// The config key a source's adjustments persist under — `"VID:PID"`
/// for USB devices so the same physical camera keeps its values across
/// `/dev/videoN` renumbering and replugs (§6.12). Pure logic, unit
/// tested with fake descriptors.
fn adjust_key_for(source: &CaptureSource) -> String {
    match source {
        CaptureSource::Device(desc) => match (desc.usb_vid, desc.usb_pid) {
            (Some(vid), Some(pid)) => hiview_core::usb_device_key(vid, pid),
            _ => desc.stable_id(),
        },
        CaptureSource::Test => "test".into(),
        CaptureSource::File(_) => "file".into(),
    }
}

/// Whether cameras come from hardware discovery or a fixed CLI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `--source=device`: enumerate, watch hotplug, user can pick cameras.
    Device,
    /// `--source=test` / `--source=file:`: fixed source, no device menu.
    Fixed,
}

/// Owns the whole capture side of the app. All methods run on the GTK thread.
pub struct CameraService {
    mode: Mode,
    state: CameraState,
    selection: Selection,
    cameras: Vec<CameraDescriptor>,
    pipeline: Option<CapturePipeline>,
    frame_bus: FrameBus,
    /// Current mirror/rotation, reapplied every time a pipeline starts
    /// (docs/ARCHITECTURE.md §6.4).
    orientation: ViewOrientation,
    /// Current software image adjustments, reapplied every time a
    /// pipeline starts (docs/ARCHITECTURE.md §6.11).
    adjust: ImageAdjust,
    /// Set when device enumeration itself failed (e.g. the v4l2
    /// provider is missing): the UI shows a dedicated error page
    /// instead of a silent "0 cameras found".
    discovery_error: Option<String>,
    on_event: Arc<dyn Fn(CameraEvent) + Send + Sync>,
    _watcher: Option<DeviceWatcher>,
}

impl CameraService {
    /// Creates the service for `source` and starts whatever can start.
    ///
    /// `on_event` receives [`CameraEvent`]s from the monitor and bus threads;
    /// it must forward them back to the GTK thread (e.g. `sender.input`).
    #[must_use]
    pub fn new(source: &Source, on_event: Arc<dyn Fn(CameraEvent) + Send + Sync>) -> Self {
        let frame_bus = FrameBus::default();
        let mut service = Self {
            mode: match source {
                Source::Device => Mode::Device,
                _ => Mode::Fixed,
            },
            state: CameraState::Searching,
            selection: Selection::Auto,
            cameras: Vec::new(),
            pipeline: None,
            frame_bus,
            orientation: ViewOrientation::default(),
            adjust: ImageAdjust::default(),
            discovery_error: None,
            on_event,
            _watcher: None,
        };

        match source {
            Source::Device => match service.start_watcher() {
                Ok(watcher) => service._watcher = Some(watcher),
                Err(err) => error!("udev watch failed, no hotplug: {err}"),
            },
            Source::Test => service.restart(CaptureSource::Test),
            Source::File(path) => service.restart(CaptureSource::File(path.clone())),
        }

        if service.mode == Mode::Device {
            match hiview_capture::enumerate() {
                Ok(cameras) => {
                    info!("{} camera(s) found", cameras.len());
                    service.cameras = cameras;
                    service.try_start_selected();
                }
                Err(err) => {
                    error!("camera enumeration failed: {err}");
                    service.discovery_error = Some(err.to_string());
                    service.state = CameraState::Error;
                }
            }
        }

        service
    }

    /// Current state of the camera state machine.
    #[must_use]
    pub fn state(&self) -> CameraState {
        self.state
    }

    /// Technical detail when device enumeration itself failed (e.g.
    /// the GStreamer v4l2 provider is missing). `None` when discovery
    /// succeeded — even with an empty camera list.
    #[must_use]
    pub fn discovery_error(&self) -> Option<&str> {
        self.discovery_error.as_deref()
    }

    /// Cameras currently known to the system (device mode only).
    #[must_use]
    pub fn cameras(&self) -> &[CameraDescriptor] {
        &self.cameras
    }

    /// Frame handoff for the UI.
    #[must_use]
    pub fn frame_bus(&self) -> &FrameBus {
        &self.frame_bus
    }

    /// Menu target identifying the current selection (`"auto"` or a stable id).
    #[must_use]
    pub fn selection_target(&self) -> String {
        match &self.selection {
            Selection::Auto => "auto".into(),
            Selection::Manual(key) => key.clone(),
        }
    }

    /// Whether the device-mode camera menu should be shown (M12:
    /// with exactly one camera the user still sees it — the item
    /// confirms what is in use and the microscope may still arrive).
    #[must_use]
    pub fn has_camera_menu(&self) -> bool {
        self.mode == Mode::Device && !self.cameras.is_empty()
    }

    /// "No microscope, but other cameras exist" (M12): the
    /// `Auto` selection resolves to nothing because every known
    /// camera is a plain UVC device. The UI offers them on the
    /// status page instead of searching forever.
    #[must_use]
    pub fn offers_alternatives(&self) -> bool {
        offers_alternatives_for(self.mode, self.pipeline.is_some(), &self.cameras)
    }

    /// Stable id of a connected microscope, for the "Usar" action of
    /// the microscope-connected toast.
    #[must_use]
    pub fn microscope_id(&self) -> Option<String> {
        self.cameras
            .iter()
            .find(|c| c.is_microscope)
            .map(|c| c.stable_id())
    }

    /// Whether the running pipeline is on the microscope (M12: the
    /// connect toast is offered only while using another camera).
    #[must_use]
    pub fn active_is_microscope(&self) -> bool {
        matches!(
            self.pipeline.as_ref().map(|p| p.source()),
            Some(CaptureSource::Device(d)) if d.is_microscope
        )
    }

    /// Per-device config key of the active camera — `"VID:PID"` or
    /// the stable id (§6.12): shared by image adjustments (§6.11) and
    /// calibration (§6.9/M12). `None` while no pipeline runs.
    #[must_use]
    pub fn device_key(&self) -> Option<String> {
        self.adjust_key()
    }

    /// Applies a new view orientation live on the running pipeline and
    /// keeps it across camera re-selection and hotplug restarts.
    pub fn set_orientation(&mut self, orientation: ViewOrientation) {
        self.orientation = orientation;
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_orientation(orientation.method());
        }
    }

    /// Applies the software image adjustments live on the running
    /// pipeline and keeps them across camera re-selection and hotplug
    /// restarts (docs/ARCHITECTURE.md §6.11).
    pub fn set_adjust(&mut self, adjust: ImageAdjust) {
        self.adjust = adjust;
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_adjust(&adjust);
        }
    }

    /// udev `ID_PATH` of the camera the pipeline is using — the Snap
    /// button sits on the same USB device, so the evdev probe matches
    /// its input node against this (docs/ARCHITECTURE.md §6.10).
    /// `None` for non-USB sources or while no pipeline runs.
    #[must_use]
    pub fn active_usb_path(&self) -> Option<String> {
        match self.pipeline.as_ref()?.source() {
            CaptureSource::Device(desc) => desc.usb_path.clone(),
            _ => None,
        }
    }

    /// Config key identifying the active source for per-device
    /// adjustment persistence (docs/ARCHITECTURE.md §6.12): `"VID:PID"`
    /// for USB cameras, the stable id when a device has no USB
    /// identity, `"test"`/`"file"` for the fixed CLI sources. `None`
    /// while no pipeline is running.
    #[must_use]
    pub fn adjust_key(&self) -> Option<String> {
        self.pipeline.as_ref().map(|p| adjust_key_for(p.source()))
    }

    /// `/dev` node of the camera the pipeline is using — the menu
    /// marks it as the active entry (M12). `None` while no pipeline
    /// runs.
    #[must_use]
    pub fn active_dev_node(&self) -> Option<std::path::PathBuf> {
        self.pipeline.as_ref().and_then(|p| p.dev_node().cloned())
    }

    /// The capture pipeline's clock reference for A/V sync (M10.5):
    /// the recorder shares this clock and translates video PTS into its
    /// own running time. `None` while no pipeline runs.
    #[must_use]
    pub fn clock_ref(&self) -> Option<hiview_capture::ClockRef> {
        self.pipeline.as_ref().and_then(|p| p.clock_ref())
    }

    /// Applies a camera event arriving from the monitor or bus threads.
    pub fn on_event(&mut self, event: CameraEvent) {
        debug!("camera event: {event:?}");
        match event {
            CameraEvent::Connected(desc) => self.on_connected(desc),
            CameraEvent::Disconnected(desc) => self.on_disconnected(desc),
            CameraEvent::Streaming => {
                if self.pipeline.is_some() {
                    self.state = CameraState::Streaming;
                }
            }
            CameraEvent::Error(detail) => {
                let device_gone = self
                    .pipeline
                    .as_ref()
                    .and_then(|p| p.dev_node())
                    .is_some_and(|n| !self.cameras.iter().any(|c| &c.dev_node == n));
                if self.mode == Mode::Device && (device_gone || is_disconnect_error(&detail)) {
                    debug!("capture ended by device removal: {detail}");
                    self.pipeline = None;
                    self.state = CameraState::Disconnected;
                } else {
                    error!("capture pipeline error: {detail}");
                    self.pipeline = None;
                    self.state = self.fallback_state();
                }
            }
        }
    }

    /// Handles a camera menu selection (`"auto"` or a camera stable id).
    pub fn select(&mut self, target: &str) {
        if self.mode != Mode::Device {
            return;
        }
        self.selection = if target == "auto" {
            Selection::Auto
        } else {
            Selection::Manual(target.to_string())
        };
        info!("camera selection changed to {target}");

        self.pipeline = None;
        self.state = CameraState::Searching;
        self.try_start_selected();
    }

    fn start_watcher(&self) -> Result<DeviceWatcher, Error> {
        let on_event = Arc::clone(&self.on_event);
        DeviceWatcher::start(move |event| on_event(event))
    }

    /// (Re)starts a pipeline, transitioning to `Starting` or `Error`.
    fn restart(&mut self, source: CaptureSource) {
        let on_event = Arc::clone(&self.on_event);
        match CapturePipeline::start(source, self.frame_bus.clone(), move |e| on_event(e)) {
            Ok(pipeline) => {
                pipeline.set_orientation(self.orientation.method());
                pipeline.set_adjust(&self.adjust);
                self.pipeline = Some(pipeline);
                self.state = CameraState::Starting;
            }
            Err(err) => {
                error!("pipeline failed to start: {err}");
                self.state = self.fallback_state();
            }
        }
    }

    /// Starts the selected camera if possible (auto: microscope only).
    fn try_start_selected(&mut self) {
        if self.pipeline.is_some() || self.mode != Mode::Device {
            return;
        }
        let target = selected_for(&self.selection, &self.cameras);
        if let Some(desc) = target {
            info!("starting capture on {}", desc.summary());
            self.restart(CaptureSource::Device(desc.clone()));
        }
    }

    fn on_connected(&mut self, desc: CameraDescriptor) {
        if !self.cameras.iter().any(|c| c.dev_node == desc.dev_node) {
            self.cameras.push(desc.clone());
            self.cameras
                .sort_by_key(|c| (!c.is_microscope, c.dev_node.clone()));
        }
        self.try_start_selected();
    }

    fn on_disconnected(&mut self, desc: CameraDescriptor) {
        self.cameras.retain(|c| c.dev_node != desc.dev_node);
        let in_use = self
            .pipeline
            .as_ref()
            .and_then(|p| p.dev_node().cloned())
            .is_some_and(|node| node == desc.dev_node);
        if in_use {
            warn!("active camera {} was disconnected", desc.dev_node.display());
            self.pipeline = None;
        }
        // A disconnect error may already have dropped the pipeline; the
        // selected device being gone still means Disconnected.
        let selected_removed = match &self.selection {
            Selection::Auto => desc.is_microscope,
            Selection::Manual(key) => desc.stable_id() == *key,
        };
        if in_use || (self.pipeline.is_none() && selected_removed) {
            self.state = CameraState::Disconnected;
        }
    }

    /// State to fall back to after a failure, based on what is still present.
    fn fallback_state(&self) -> CameraState {
        if self.mode != Mode::Device {
            return CameraState::Error;
        }
        if selected_for(&self.selection, &self.cameras).is_some() {
            CameraState::Error
        } else {
            CameraState::Disconnected
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use hiview_capture::CameraDescriptor;

    fn fake_camera(node: &str, usb_path: &str) -> CameraDescriptor {
        CameraDescriptor {
            dev_node: PathBuf::from(node),
            name: "Fake Cam".into(),
            usb_vid: Some(0x1111),
            usb_pid: Some(0x2222),
            usb_path: Some(usb_path.into()),
            is_microscope: false,
            modes: Vec::new(),
        }
    }

    /// After a replug the kernel may hand out different /dev/videoN nodes;
    /// a manual selection must still resolve to the same physical device.
    #[test]
    fn manual_selection_follows_usb_path_when_nodes_swap() {
        let before = vec![
            fake_camera("/dev/video0", "pci-usb-0:5"),
            fake_camera("/dev/video2", "pci-usb-0:6"),
        ];
        let selection = Selection::Manual("pci-usb-0:6".into());
        let picked = selected_for(&selection, &before)
            .unwrap_or_else(|| panic!("camera B missing before swap"));
        assert_eq!(picked.dev_node, PathBuf::from("/dev/video2"));

        let after = vec![
            fake_camera("/dev/video4", "pci-usb-0:6"),
            fake_camera("/dev/video6", "pci-usb-0:5"),
        ];
        let picked = selected_for(&selection, &after)
            .unwrap_or_else(|| panic!("camera B missing after swap"));
        assert_eq!(picked.dev_node, PathBuf::from("/dev/video4"));
        assert_eq!(picked.usb_path.as_deref(), Some("pci-usb-0:6"));
    }

    /// §6.12 persistence: a replugged device gets a new `/dev/videoN`
    /// node but the same VID:PID — the adjustment key must not change,
    /// so the stored values find the device again.
    #[test]
    fn adjust_key_survives_node_renumbering() {
        let key = |node: &str| adjust_key_for(&CaptureSource::Device(fake_camera(node, "usb-0:5")));
        assert_eq!(key("/dev/video0"), "1111:2222");
        assert_eq!(key("/dev/video4"), "1111:2222");

        // A device without USB identity falls back to the stable id.
        let mut no_usb = fake_camera("/dev/video3", "pci-usb-0:7");
        no_usb.usb_vid = None;
        no_usb.usb_pid = None;
        assert_eq!(
            adjust_key_for(&CaptureSource::Device(no_usb)),
            "pci-usb-0:7"
        );

        // The fixed CLI sources persist under their own keys.
        assert_eq!(adjust_key_for(&CaptureSource::Test), "test");
        assert_eq!(
            adjust_key_for(&CaptureSource::File(PathBuf::from("/tmp/clip.mp4"))),
            "file"
        );
    }

    /// M12 scope item 1: without a microscope the Auto selection
    /// resolves to NOTHING — but with cameras present the UI offers
    /// the alternatives page instead of searching forever.
    #[test]
    fn no_microscope_offers_the_alternatives() {
        let webcams = vec![
            fake_camera("/dev/video0", "pci-usb-0:6"),
            fake_camera("/dev/video2", "pci-usb-0:7"),
        ];
        assert!(selected_for(&Selection::Auto, &webcams).is_none());
        assert!(offers_alternatives_for(Mode::Device, false, &webcams));

        // An empty camera list still shows the plain searching page.
        assert!(!offers_alternatives_for(Mode::Device, false, &[]));
        // A running pipeline (a manual pick) is streaming, not offered.
        assert!(!offers_alternatives_for(Mode::Device, true, &webcams));
        // Fixed CLI sources never offer the page.
        assert!(!offers_alternatives_for(Mode::Fixed, false, &webcams));
    }

    /// M12 scope item 1/3: the microscope in the list resolves Auto —
    /// the alternatives page is NOT offered, whether it arrives at
    /// start-up or by hotplug before any manual choice.
    #[test]
    fn microscope_present_never_offers_alternatives() {
        let mut microscope = fake_camera("/dev/video4", "pci-usb-0:5");
        microscope.is_microscope = true;
        let cameras = vec![fake_camera("/dev/video0", "pci-usb-0:6"), microscope];
        assert!(!offers_alternatives_for(Mode::Device, false, &cameras));
        assert!(selected_for(&Selection::Auto, &cameras).is_some());
    }

    #[test]
    fn auto_selection_prefers_the_microscope() {
        let mut microscope = fake_camera("/dev/video2", "pci-usb-0:5");
        microscope.is_microscope = true;
        let cameras = vec![fake_camera("/dev/video0", "pci-usb-0:6"), microscope];
        let picked = selected_for(&Selection::Auto, &cameras)
            .unwrap_or_else(|| panic!("microscope not selected"));
        assert_eq!(picked.dev_node, PathBuf::from("/dev/video2"));
    }
}
