// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Camera service: owns discovery, hotplug monitoring and the capture
//! pipeline, and drives the camera state machine (docs/ARCHITECTURE.md §6.3).

use std::path::PathBuf;
use std::sync::Arc;

use hiview_capture::{
    CameraDescriptor, CameraEvent, CameraState, CapturePipeline, CaptureSource, DeviceWatcher,
    Error, FrameBus,
};
use tracing::{debug, error, info, warn};

use crate::cli::Source;

/// Camera chosen by the user in the camera menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Prefer a known microscope; nothing starts without one.
    Auto,
    /// A specific device node chosen in the menu.
    Manual(PathBuf),
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
                Err(err) => error!("camera enumeration failed: {err}"),
            }
        }

        service
    }

    /// Current state of the camera state machine.
    #[must_use]
    pub fn state(&self) -> CameraState {
        self.state
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

    /// Menu target identifying the current selection (`"auto"` or a devnode).
    #[must_use]
    pub fn selection_target(&self) -> String {
        match &self.selection {
            Selection::Auto => "auto".into(),
            Selection::Manual(node) => node.to_string_lossy().into_owned(),
        }
    }

    /// Whether the device-mode camera menu should be shown.
    #[must_use]
    pub fn has_camera_menu(&self) -> bool {
        self.mode == Mode::Device && self.cameras.len() > 1
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
                error!("capture pipeline error: {detail}");
                self.pipeline = None;
                self.state = self.fallback_state();
            }
        }
    }

    /// Handles a camera menu selection (`"auto"` or a device node path).
    pub fn select(&mut self, target: &str) {
        if self.mode != Mode::Device {
            return;
        }
        self.selection = if target == "auto" {
            Selection::Auto
        } else {
            Selection::Manual(PathBuf::from(target))
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
        let target = match &self.selection {
            Selection::Auto => self.cameras.iter().find(|c| c.is_microscope),
            Selection::Manual(node) => self.cameras.iter().find(|c| &c.dev_node == node),
        };
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
            self.state = CameraState::Disconnected;
        }
    }

    /// State to fall back to after a failure, based on what is still present.
    fn fallback_state(&self) -> CameraState {
        if self.mode != Mode::Device {
            return CameraState::Error;
        }
        let selected_present = match &self.selection {
            Selection::Auto => self.cameras.iter().any(|c| c.is_microscope),
            Selection::Manual(node) => self.cameras.iter().any(|c| &c.dev_node == node),
        };
        if selected_present {
            CameraState::Error
        } else {
            CameraState::Disconnected
        }
    }
}
