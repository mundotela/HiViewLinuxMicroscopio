// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Camera discovery by USB identity and V4L2 capability (docs/ARCHITECTURE.md
//! §6.1). Device nodes (`/dev/videoN`) are discovered, never hardcoded.
//!
//! USB identity (VID:PID) and hotplug come from udev; capture capabilities
//! and formats come from GStreamer's device monitor, which probes nodes under
//! the `Video/Source` class.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use gst::prelude::*;
use tracing::{debug, info, warn};

use crate::error::Error;
use crate::events::CameraEvent;

/// A capture mode advertised by a video device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureMode {
    /// Pixel format as a four-character code (e.g. `"YUYV"`).
    pub fourcc: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Supported frame rates as `(numerator, denominator)` frames-per-second
    /// fractions (e.g. `(30, 1)` for 30 fps).
    pub frame_rates: Vec<(u32, u32)>,
}

/// A discovered video capture device.
#[derive(Debug, Clone)]
pub struct CameraDescriptor {
    /// Capture device node (e.g. `/dev/video2`), discovered via udev.
    pub dev_node: PathBuf,
    /// Human-readable card name reported by the driver (e.g. `"HD camera"`).
    pub name: String,
    /// USB vendor ID of the parent device, when it is a USB camera.
    pub usb_vid: Option<u16>,
    /// USB product ID of the parent device, when it is a USB camera.
    pub usb_pid: Option<u16>,
    /// udev `ID_PATH` of the device (e.g. `pci-0000:00:14.0-usb-0:5:1.0`).
    /// Identifies the physical device/port and survives `/dev/videoN`
    /// renumbering across replugs.
    pub usb_path: Option<String>,
    /// Whether the USB identity matches a known microscope.
    pub is_microscope: bool,
    /// Capture modes advertised by the device (may be empty on unplug).
    pub modes: Vec<CaptureMode>,
}

impl CameraDescriptor {
    /// Stable identifier for the physical device: `ID_PATH` when available,
    /// otherwise the device node (stable only until the next replug).
    #[must_use]
    pub fn stable_id(&self) -> String {
        self.usb_path
            .clone()
            .unwrap_or_else(|| self.dev_node.to_string_lossy().into_owned())
    }

    /// Short technical description for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} ({}) vid:pid={}:{} modes={}",
            self.dev_node.display(),
            self.name,
            self.usb_vid
                .map(|v| format!("{v:04x}"))
                .unwrap_or_else(|| "----".into()),
            self.usb_pid
                .map(|p| format!("{p:04x}"))
                .unwrap_or_else(|| "----".into()),
            self.modes.len()
        )
    }
}

/// Enumerates all nodes with video capture capability, microscopes first.
///
/// Individual nodes that cannot be described are skipped with a log entry
/// instead of failing the whole scan.
///
/// # Errors
/// Returns an error only if udev or GStreamer enumeration itself fails.
pub fn enumerate() -> Result<Vec<CameraDescriptor>, Error> {
    crate::init()?;

    let mut enumerator = udev::Enumerator::new()?;
    enumerator.match_subsystem("video4linux")?;
    let nodes: Vec<(PathBuf, udev::Device)> = enumerator
        .scan_devices()?
        .filter_map(|d| d.devnode().map(|n| n.to_path_buf()).map(|n| (n, d)))
        .collect();

    let probes = probe_capture_nodes();
    let mut cameras = Vec::new();
    for (node, dev) in nodes {
        match describe(&node, &dev, &probes) {
            Ok(desc) => {
                debug!("discovered {}", desc.summary());
                cameras.push(desc);
            }
            Err(err) => debug!("skipping {}: {err}", node.display()),
        }
    }

    cameras.sort_by_key(|c| (!c.is_microscope, c.dev_node.clone()));
    Ok(cameras)
}

/// Watches udev for video4linux add/remove events.
///
/// The monitor thread lives for the process lifetime; dropping the watcher
/// detaches the thread (the process is its owner anyway).
pub struct DeviceWatcher {
    _private: (),
}

impl DeviceWatcher {
    /// Starts monitoring. `on_event` runs on the monitor thread.
    ///
    /// # Errors
    /// Returns an error if the udev monitor socket cannot be created or the
    /// thread cannot be spawned.
    pub fn start<F>(on_event: F) -> Result<Self, Error>
    where
        F: Fn(CameraEvent) + Send + 'static,
    {
        // udev monitor objects are !Send, so the socket is created inside
        // the monitor thread; a setup failure surfaces as a camera error.
        thread::Builder::new()
            .name("hiview-udev".into())
            .spawn(move || {
                match udev::MonitorBuilder::new()
                    .and_then(|b| b.match_subsystem("video4linux"))
                    .and_then(|b| b.listen())
                {
                    Ok(socket) => monitor_loop(&socket, &on_event),
                    Err(err) => {
                        on_event(CameraEvent::Error(format!("udev monitor: {err}")));
                    }
                }
            })
            .map_err(|source| Error::ThreadSpawn {
                name: "hiview-udev",
                source,
            })?;

        Ok(Self { _private: () })
    }
}

/// The udev monitor socket is nonblocking, so events are drained and the
/// thread sleeps between polls; ~100 ms latency is fine for hotplug.
fn monitor_loop(socket: &udev::MonitorSocket, on_event: &dyn Fn(CameraEvent)) {
    loop {
        for event in socket.iter() {
            on_monitor_event(&event, on_event);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn on_monitor_event(event: &udev::Event, on_event: &dyn Fn(CameraEvent)) {
    let dev = event.device();
    let Some(node) = dev.devnode().map(Path::to_path_buf) else {
        return;
    };
    match event.event_type() {
        udev::EventType::Add => match describe_with_retry(&node, &dev) {
            Ok(desc) => {
                info!("camera connected: {}", desc.summary());
                on_event(CameraEvent::Connected(desc));
            }
            Err(err) => warn!("added node {} unusable: {err}", node.display()),
        },
        udev::EventType::Remove => {
            let desc = describe_unplugged(&node, &dev);
            info!("camera disconnected: {}", desc.summary());
            on_event(CameraEvent::Disconnected(desc));
        }
        _ => {}
    }
}

/// Freshly plugged nodes can need a moment before the driver and the
/// GStreamer device monitor are ready.
fn describe_with_retry(node: &Path, dev: &udev::Device) -> Result<CameraDescriptor, Error> {
    let mut last_err = None;
    for attempt in 0..8 {
        match describe(node, dev, &probe_capture_nodes()) {
            Ok(desc) => return Ok(desc),
            Err(err) => {
                last_err = Some(err);
                if attempt < 7 {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::VideoDevice(node.display().to_string())))
}

/// Builds a descriptor for a node that is being removed (probing is no longer
/// possible; USB identity and name still come from udev).
fn describe_unplugged(node: &Path, dev: &udev::Device) -> CameraDescriptor {
    let (vid, pid) = usb_identity(dev).unzip();
    CameraDescriptor {
        dev_node: node.to_path_buf(),
        name: product_name(dev).unwrap_or_else(|| "video device".into()),
        usb_vid: vid,
        usb_pid: pid,
        is_microscope: vid
            .zip(pid)
            .is_some_and(|(v, p)| hiview_core::is_known_microscope(v, p)),
        usb_path: usb_path(dev),
        modes: Vec::new(),
    }
}

/// Describes one node: it must be a capture source (non-empty caps in the
/// `Video/Source` monitor) — this filters out metadata nodes.
fn describe(
    node: &Path,
    dev: &udev::Device,
    probes: &HashMap<PathBuf, Probe>,
) -> Result<CameraDescriptor, Error> {
    let Some(probe) = probes.get(node) else {
        return Err(Error::NoCaptureFormat(node.display().to_string()));
    };

    let (vid, pid) = usb_identity(dev).unzip();
    Ok(CameraDescriptor {
        dev_node: node.to_path_buf(),
        name: product_name(dev)
            .or_else(|| probe.display_name.clone())
            .unwrap_or_else(|| "video device".into()),
        usb_vid: vid,
        usb_pid: pid,
        is_microscope: vid
            .zip(pid)
            .is_some_and(|(v, p)| hiview_core::is_known_microscope(v, p)),
        usb_path: usb_path(dev),
        modes: probe.modes.clone(),
    })
}

fn product_name(dev: &udev::Device) -> Option<String> {
    dev.property_value("ID_V4L_PRODUCT")
        .or_else(|| dev.property_value("ID_MODEL"))
        .map(|s| s.to_string_lossy().into_owned())
}

/// Stable per-port path (`ID_PATH`) of a video4linux udev device, falling
/// back to the USB parent's sysfs path.
fn usb_path(dev: &udev::Device) -> Option<String> {
    dev.property_value("ID_PATH")
        .map(|s| s.to_string_lossy().into_owned())
        .or_else(|| {
            dev.parent_with_subsystem_devtype("usb", "usb_device")
                .ok()
                .flatten()
                .map(|usb| usb.syspath().to_string_lossy().into_owned())
        })
}

/// Extracts `(vid, pid)` from a video4linux udev device or its USB parent.
fn usb_identity(dev: &udev::Device) -> Option<(u16, u16)> {
    let from_dev = |d: &udev::Device| {
        Some((
            parse_hex_id(d.property_value("ID_VENDOR_ID")?)?,
            parse_hex_id(d.property_value("ID_MODEL_ID")?)?,
        ))
    };

    from_dev(dev).or_else(|| {
        dev.parent_with_subsystem_devtype("usb", "usb_device")
            .ok()
            .flatten()
            .and_then(|usb| from_dev(&usb))
    })
}

fn parse_hex_id(value: &OsStr) -> Option<u16> {
    u16::from_str_radix(&value.to_string_lossy(), 16).ok()
}

/// Capabilities of one `Video/Source` node, probed by GStreamer.
struct Probe {
    display_name: Option<String>,
    modes: Vec<CaptureMode>,
}

/// Snapshots the `Video/Source` device monitor into a `devnode → caps` map.
fn probe_capture_nodes() -> HashMap<PathBuf, Probe> {
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some("Video/Source"), None);
    if monitor.start().is_err() {
        warn!("gstreamer device monitor failed to start");
        return HashMap::new();
    }

    let mut probes = HashMap::new();
    for device in monitor.devices() {
        let Some(node) = device_node(&device) else {
            continue;
        };
        let modes = device
            .caps()
            .map(|caps| modes_from_caps(&caps))
            .unwrap_or_default();
        probes.insert(
            node,
            Probe {
                display_name: Some(device.display_name().to_string()),
                modes,
            },
        );
    }

    monitor.stop();
    probes
}

/// The `device.path` property set by the v4l2 device provider.
fn device_node(device: &gst::Device) -> Option<PathBuf> {
    let props = device.properties()?;
    let path = props.get::<String>("device.path").ok()?;
    Some(PathBuf::from(path))
}

/// Extracts discrete capture modes from a `Video/Source` caps object.
fn modes_from_caps(caps: &gst::Caps) -> Vec<CaptureMode> {
    let mut modes = Vec::new();
    for structure in caps.iter() {
        if structure.name() != "video/x-raw" {
            continue;
        }
        let formats = strings_field(structure, "format");
        let widths = ints_field(structure, "width");
        let heights = ints_field(structure, "height");
        let frame_rates: Vec<(u32, u32)> = fractions_field(structure, "framerate")
            .iter()
            .map(|f| (f.numer() as u32, f.denom() as u32))
            .collect();

        for fourcc in &formats {
            for &width in &widths {
                for &height in &heights {
                    modes.push(CaptureMode {
                        fourcc: fourcc.clone(),
                        width: width.max(0) as u32,
                        height: height.max(0) as u32,
                        frame_rates: frame_rates.clone(),
                    });
                }
            }
        }
    }
    modes
}

fn strings_field(s: &gst::StructureRef, name: &str) -> Vec<String> {
    let Ok(value) = s.get::<gst::glib::Value>(name) else {
        return Vec::new();
    };
    if let Ok(v) = value.get::<String>() {
        return vec![v];
    }
    if let Ok(list) = value.get::<gst::List>() {
        return list.iter().filter_map(|v| v.get::<String>().ok()).collect();
    }
    Vec::new()
}

fn ints_field(s: &gst::StructureRef, name: &str) -> Vec<i32> {
    let Ok(value) = s.get::<gst::glib::Value>(name) else {
        return Vec::new();
    };
    if let Ok(v) = value.get::<i32>() {
        return vec![v];
    }
    if let Ok(range) = value.get::<gst::IntRange<i32>>() {
        return vec![range.min(), range.max()];
    }
    if let Ok(list) = value.get::<gst::List>() {
        return list.iter().filter_map(|v| v.get::<i32>().ok()).collect();
    }
    Vec::new()
}

fn fractions_field(s: &gst::StructureRef, name: &str) -> Vec<gst::Fraction> {
    let Ok(value) = s.get::<gst::glib::Value>(name) else {
        return Vec::new();
    };
    if let Ok(v) = value.get::<gst::Fraction>() {
        return vec![v];
    }
    if let Ok(range) = value.get::<gst::FractionRange>() {
        return vec![range.min(), range.max()];
    }
    if let Ok(list) = value.get::<gst::List>() {
        return list
            .iter()
            .filter_map(|v| v.get::<gst::Fraction>().ok())
            .collect();
    }
    Vec::new()
}
