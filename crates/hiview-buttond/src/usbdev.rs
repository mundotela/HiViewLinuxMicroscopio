// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
#![forbid(unsafe_code)]

//! Resolving the microscope's bus/devnum via read-only sysfs
//! enumeration through udev (ARCHITECTURE §2.1). The matching logic is
//! pure — the udev glue is thin and isolated so the replug rules can
//! be tested with fake device lists. There is NO hotplug monitor:
//! `PrivateNetwork=yes` hides kernel uevents, so `reader` calls
//! [`enumerate`] on a timer tick instead.

/// The microscope USB descriptor (ARCHITECTURE §2.1 / §6.1).
pub const MICROSCOPE_VID: u16 = 0x05e3;
pub const MICROSCOPE_PID: u16 = 0xf12a;

/// Where the device sits right now (devnum changes on every replug).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// USB bus number → `/dev/usbmon<busnum>`.
    pub busnum: u16,
    /// Device address on that bus.
    pub devnum: u8,
}

/// A flattened usb_device enumeration entry (one per sysfs node).
#[derive(Clone, Copy, Debug)]
pub struct UsbDev {
    /// idVendor, parsed from hex.
    pub vid: u16,
    /// idProduct, parsed from hex.
    pub pid: u16,
    /// Kernel bus number.
    pub busnum: u16,
    /// Device address (re-assigned on replug).
    pub devnum: u8,
}

/// Picks the microscope out of an enumeration — pure, so hotplug
/// transitions (gone → back with a new devnum) are testable without
/// real udev.
pub fn resolve(devs: &[UsbDev]) -> Option<Target> {
    devs.iter()
        .find(|d| d.vid == MICROSCOPE_VID && d.pid == MICROSCOPE_PID)
        .map(|d| Target {
            busnum: d.busnum,
            devnum: d.devnum,
        })
}

/// Enumerates `usb_device` nodes and reads vid/pid/busnum/devnum —
/// thin udev glue over [`resolve`].
pub fn enumerate() -> Vec<UsbDev> {
    let Ok(mut en) = udev::Enumerator::new() else {
        return Vec::new();
    };
    let _ = en.match_subsystem("usb");
    en.scan_devices()
        .map(|it| it.filter_map(flatten).collect())
        .unwrap_or_default()
}

/// Flattened view of one udev `usb_device` node; non-usb_device types
/// (interfaces, endpoints) are dropped — the Snap button lives on the
/// device descriptor.
fn flatten(dev: udev::Device) -> Option<UsbDev> {
    if dev.devtype().and_then(|s| s.to_str()) != Some("usb_device") {
        return None;
    }
    Some(UsbDev {
        vid: attr_hex(&dev, "idVendor")?,
        pid: attr_hex(&dev, "idProduct")?,
        busnum: attr_dec(&dev, "busnum")?,
        devnum: attr_dec(&dev, "devnum")?,
    })
}

fn attr(dev: &udev::Device, name: &str) -> Option<String> {
    dev.attribute_value(name)?.to_str().map(str::to_owned)
}

fn attr_hex(dev: &udev::Device, name: &str) -> Option<u16> {
    u16::from_str_radix(&attr(dev, name)?, 16).ok()
}

fn attr_dec<T: std::str::FromStr>(dev: &udev::Device, name: &str) -> Option<T> {
    attr(dev, name)?.parse().ok()
}

// NOTE: there is intentionally no udev *monitor* here — the unit runs
// under `PrivateNetwork=yes`, so kernel uevents
// (`NETLINK_KOBJECT_UEVENT`) never reach a netlink socket inside the
// private namespace. `reader` re-enumerates sysfs on a tick instead,
// which is the only reliable hotplug signal in this sandbox.

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(busnum: u16, devnum: u8) -> UsbDev {
        UsbDev {
            vid: MICROSCOPE_VID,
            pid: MICROSCOPE_PID,
            busnum,
            devnum,
        }
    }

    fn other(busnum: u16, devnum: u8) -> UsbDev {
        UsbDev {
            vid: 0x046d,
            pid: 0xc077,
            busnum,
            devnum,
        }
    }

    #[test]
    fn resolve_finds_the_microscope() {
        let devs = vec![other(1, 2), scope(3, 7)];
        assert_eq!(
            resolve(&devs),
            Some(Target {
                busnum: 3,
                devnum: 7
            })
        );
    }

    #[test]
    fn resolve_is_none_when_unplugged() {
        assert_eq!(resolve(&[other(1, 2)]), None);
        assert_eq!(resolve(&[]), None);
    }

    #[test]
    fn devnum_changes_on_replug() {
        // Hotplug contract: the SAME vid/pid returns with a new devnum
        // — resolve must report the new address, not a cached one.
        let mut devs = vec![scope(3, 7)];
        assert_eq!(
            resolve(&devs)
                .unwrap_or_else(|| panic!("scope gone"))
                .devnum,
            7
        );

        devs.clear(); // unplug
        assert_eq!(resolve(&devs), None);

        devs.push(scope(3, 12)); // replug → same bus, new devnum
        let t = resolve(&devs).unwrap_or_else(|| panic!("scope gone"));
        assert_eq!((t.busnum, t.devnum), (3, 12));
    }

    #[test]
    fn a_second_scope_is_ignored() {
        // Only one scope expected, but never panic on duplicates.
        let devs = vec![scope(3, 7), scope(3, 8)];
        assert_eq!(
            resolve(&devs)
                .unwrap_or_else(|| panic!("scope gone"))
                .devnum,
            7
        );
    }
}
