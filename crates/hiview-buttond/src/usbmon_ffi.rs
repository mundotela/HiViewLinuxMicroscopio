// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia
//
// The ONLY file allowed to contain `unsafe` in this crate
// (docs/CONVENTIONS.md §2). Every block carries a `// SAFETY:` note.
#![allow(unsafe_code)]

//! usbmon binary-API FFI: the `MON_IOCX_GETX` ioctl and the 64-byte
//! `usbmon_packet` header (Linux `Documentation/usb/usbmon.rst`).
//!
//! Layout is a kernel ABI — every offset is load-bearing. The comments
//! reproduce the doc's own offsets so a review can diff them directly.

use std::os::unix::io::{AsRawFd, BorrowedFd};

use nix::errno::Errno;

/// `MON_IOC_MAGIC` from usbmon.rst.
const MON_IOC_MAGIC: u8 = 0x92;

/// `struct usbmon_packet` — what `MON_IOCX_GETX` writes to `hdr`
/// (64 bytes, kernel ABI). Field types mirror the C declaration
/// verbatim; `setup/iso_rec` is a union we never read, kept opaque.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UsbmonPacket {
    /// 0: URB ID (submission → callback).
    pub id: u64,
    /// 8: event type — `b'S'` submit, `b'C'` complete, `b'E'` error.
    pub event_type: u8,
    /// 9: transfer type — 0 iso, 1 interrupt, 2 control, 3 bulk.
    pub xfer_type: u8,
    /// 10: endpoint number + direction bit (`0x83` = IN, ep 3).
    pub epnum: u8,
    /// 11: device address on the bus.
    pub devnum: u8,
    /// 12: bus number.
    pub busnum: u16,
    /// 14: same as text API.
    pub flag_setup: i8,
    /// 15: same as text API; zero means captured data present.
    pub flag_data: i8,
    /// 16: timestamp, seconds.
    pub ts_sec: i64,
    /// 24: timestamp, microseconds.
    pub ts_usec: i32,
    /// 28: URB status / error code.
    pub status: i32,
    /// 32: data length (requested on submit, actual on complete).
    pub length: u32,
    /// 36: bytes actually captured into `data` for this event.
    pub len_cap: u32,
    /// 40: `setup[8]` (control submit) or `iso_rec` — never read by us.
    pub setup_or_iso: [u8; 8],
    /// 48: interrupt/iso interval.
    pub interval: i32,
    /// 52: iso start frame.
    pub start_frame: i32,
    /// 56: copy of the URB `transfer_flags`.
    pub xfer_flags: u32,
    /// 60: number of iso descriptors.
    pub ndesc: u32,
}

const _: () = assert!(
    size_of::<UsbmonPacket>() == 64,
    "usbmon_packet must stay exactly 64 bytes (kernel ABI)"
);

/// `struct mon_get_arg` — the ioctl's argument (usbmon.rst).
#[repr(C)]
struct MonGetArg {
    /// Points at a caller-provided `UsbmonPacket` to be filled.
    hdr: *mut UsbmonPacket,
    /// Points at a caller-provided payload buffer (`alloc` bytes).
    data: *mut u8,
    /// Size of `data` — the kernel caps the capture at this.
    alloc: usize,
}

nix::ioctl_write_ptr!(mon_iocx_getx, MON_IOC_MAGIC, 10, MonGetArg);

/// Payload scratch size — button packets are 4 bytes; a margin costs
/// nothing and survives a future, longer status message.
const DATA_BUF_LEN: usize = 256;

/// Fetches the next usbmon event from `fd` (blocking until one
/// arrives). Returns the header plus `len_cap` payload bytes copied
/// into `data`.
///
/// # Errors
/// `Errno` from the ioctl (`EINTR`/`EBADF`/`ENODEV`…), or
/// `Errno::EINVAL` when `len_cap` exceeds the scratch buffer (means
/// our header layout diverged from the kernel's).
pub fn fetch_event(fd: BorrowedFd<'_>, data: &mut Vec<u8>) -> Result<UsbmonPacket, Errno> {
    let mut packet = UsbmonPacket::default();
    let mut raw = [0u8; DATA_BUF_LEN];
    let arg = MonGetArg {
        hdr: &mut packet,
        data: raw.as_mut_ptr(),
        alloc: raw.len(),
    };
    // SAFETY: `arg.hdr` points to a valid 64-byte `UsbmonPacket` and
    // `arg.data` to `alloc` writable bytes — both live for the call;
    // `fd` is an open /dev/usbmonN descriptor held by the caller.
    unsafe { mon_iocx_getx(fd.as_raw_fd(), &arg) }?;
    let len = usize::try_from(packet.len_cap).map_err(|_| Errno::EOVERFLOW)?;
    if len > raw.len() {
        // A header/payload size mismatch would corrupt the stream;
        // drop the event rather than forward garbage.
        return Err(Errno::EOVERFLOW);
    }
    data.clear();
    data.extend_from_slice(&raw[..len]);
    Ok(packet)
}

/// Maps a raw header + payload onto the pure filter record
/// (hiview-core does the matching; this crate owns only the ABI).
pub fn record_of<'a>(packet: &UsbmonPacket, data: &'a [u8]) -> hiview_core::UsbmonRecord<'a> {
    hiview_core::UsbmonRecord {
        event_type: packet.event_type,
        xfer_type: packet.xfer_type,
        epnum: packet.epnum,
        devnum: packet.devnum,
        busnum: packet.busnum,
        len_cap: packet.len_cap,
        data,
    }
}
