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

/// `struct mon_bin_stats` — what `MON_IOCG_STATS` returns (usbmon.rst).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MonBinStats {
    /// Events currently queued in the ring.
    pub queued: u32,
    /// Events lost since the PREVIOUS `MON_IOCG_STATS` call — already
    /// a delta, so `> 0` alone means the ring overflowed.
    pub dropped: u32,
}

nix::ioctl_write_ptr!(mon_iocx_getx, MON_IOC_MAGIC, 10, MonGetArg);
// MON_IOCT_RING_SIZE = _IO(MON_IOC_MAGIC, 4): the ring size goes BY
// VALUE in the argument slot (not via a pointer), so the request code
// encodes no payload — the _bad variant supplies the exact code.
nix::ioctl_write_int_bad!(
    mon_ioct_ring_size,
    nix::request_code_none!(MON_IOC_MAGIC, 4)
);
// MON_IOCG_STATS = _IOR(MON_IOC_MAGIC, 3, struct mon_bin_stats).
nix::ioctl_read!(mon_iocg_stats, MON_IOC_MAGIC, 3, MonBinStats);

/// Capture-ring size set right after open. The default (~300 KiB)
/// overflows under live-view traffic on the same bus and the kernel
/// then DROPS events — possibly the button's. 1 MiB buys headroom.
pub const RING_BYTES: i32 = 1024 * 1024;

/// `MON_IOCT_RING_SIZE`: grow the capture ring. Must run right after
/// open, before traffic piles up (usbmon.rst).
pub fn set_ring_size(fd: BorrowedFd<'_>) -> Result<(), Errno> {
    // SAFETY: `fd` is an open usbmon descriptor; the request takes a
    // plain integer vararg — no pointers, no aliasing.
    unsafe { mon_ioct_ring_size(fd.as_raw_fd(), RING_BYTES)? };
    Ok(())
}

/// `MON_IOCG_STATS`: ring health. `dropped` is already the count lost
/// since the previous call — warn only when it is nonzero.
pub fn fetch_stats(fd: BorrowedFd<'_>) -> Result<MonBinStats, Errno> {
    let mut stats = MonBinStats::default();
    // SAFETY: `stats` points at a valid, correctly-sized
    // `mon_bin_stats` living for the whole call; `fd` is an open
    // usbmon descriptor.
    unsafe { mon_iocg_stats(fd.as_raw_fd(), &mut stats)? };
    Ok(stats)
}

/// Payload scratch size — button packets are 4 bytes; a margin costs
/// nothing and survives a future, longer status message.
const DATA_BUF_LEN: usize = 256;

/// Fetches the next usbmon event from `fd` (blocking until one
/// arrives). Returns the header plus `min(len_cap, DATA_BUF_LEN)`
/// payload bytes copied into `data`.
///
/// Truncation is NORMAL: the kernel copies `min(len_cap, alloc)` bytes,
/// consumes the event and still reports the original `len_cap` in the
/// header (usbmon.rst). The header value is kept intact — the §6.10
/// filter requires `len_cap == 4` and rejects the rest, so a truncated
/// video record is dropped silently instead of erroring the drain.
///
/// # Errors
/// `Errno` from the ioctl (`EINTR`/`EBADF`/`ENODEV`…).
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
    let len = usize::try_from(packet.len_cap)
        .map_err(|_| Errno::EOVERFLOW)?
        .min(raw.len());
    data.clear();
    data.extend_from_slice(&raw[..len]);
    Ok(packet)
}

/// Maps a raw header + payload onto the pure filter record
/// (hiview-core does the matching; this crate owns only the ABI).
/// `ts_us` folds the header's `ts_sec`/`ts_usec` — the kernel's
/// monotonic HOST clock at capture — so press durations never depend
/// on delivery latency.
pub fn record_of<'a>(packet: &UsbmonPacket, data: &'a [u8]) -> hiview_core::UsbmonRecord<'a> {
    hiview_core::UsbmonRecord {
        event_type: packet.event_type,
        xfer_type: packet.xfer_type,
        epnum: packet.epnum,
        devnum: packet.devnum,
        busnum: packet.busnum,
        len_cap: packet.len_cap,
        data,
        ts_us: packet
            .ts_sec
            .saturating_mul(1_000_000)
            .saturating_add(i64::from(packet.ts_usec)),
    }
}
