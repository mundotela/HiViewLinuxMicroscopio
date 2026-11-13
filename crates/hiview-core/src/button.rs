// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Snap-button domain logic (docs/ARCHITECTURE.md §2.1, §6.10).
//!
//! Three pure pieces, shared by `hiview-buttond` (parser + filter) and
//! `hiview-app` (press interpretation):
//!
//! - [`parse_status_packet`] decodes the 4-byte UVC status packet sent on
//!   the interrupt endpoint `0x83` when the Snap button moves;
//! - [`snap_button_event`] is the §6.10 record filter applied to usbmon
//!   binary records — the only path allowed to produce events;
//! - [`ButtonTracker`] turns press/release edges into photo/record
//!   actions with debounce and a configurable long-press threshold.

/// One physical edge of the Snap button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    /// The button went down (`bValue` = 1).
    Pressed,
    /// The button came back up (`bValue` = 0).
    Released,
}

/// A [`ButtonEvent`] stamped by the kernel's monotonic host clock at
/// capture time (µs — the same domain as `glib::monotonic_time`, the
/// usbmon header and evdev timestamps). Press durations must be
/// computed from `ts_us`, never from arrival time: delivery batching
/// and reconnects must not distort the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonEdge {
    /// Which edge happened.
    pub event: ButtonEvent,
    /// Kernel capture timestamp, microseconds (host monotonic clock).
    pub ts_us: u64,
}

impl ButtonEdge {
    /// The timestamp in milliseconds, the unit [`ButtonTracker`] uses.
    #[must_use]
    pub fn ts_ms(&self) -> u64 {
        self.ts_us / 1_000
    }
}

/// usbmon event type: URB callback (completion). Only completions carry
/// the payload read back from the device.
pub const EVENT_COMPLETE: u8 = b'C';
/// usbmon transfer type for interrupt pipes (`PIPE_INTERRUPT`).
pub const XFER_INTERRUPT: u8 = 1;
/// The microscope's status endpoint: interrupt IN, endpoint 3 (§2.1).
pub const SNAP_EPNUM: u8 = 0x83;

/// Decodes the 4-byte UVC VideoStreaming status packet (§2.1):
/// `02 01 00 01` = pressed, `02 01 00 00` = released.
///
/// Byte 0 (`bStatusType` = 2) marks a VideoStreaming-originated event,
/// byte 1 (`bOriginator` = 1) the streaming interface, byte 2 (`bEvent`
/// = 0) the button event and byte 3 (`bValue`) the edge.
#[must_use]
pub fn parse_status_packet(data: &[u8]) -> Option<ButtonEvent> {
    match data {
        [0x02, 0x01, 0x00, 0x01, ..] => Some(ButtonEvent::Pressed),
        [0x02, 0x01, 0x00, 0x00, ..] => Some(ButtonEvent::Released),
        _ => None,
    }
}

/// The fields of one usbmon binary record (`struct usbmon_packet`,
/// 64 bytes) that the §6.10 filter reads. `hiview-buttond` fills this
/// from the ioctl; tests synthesize it directly.
pub struct UsbmonRecord<'a> {
    /// Event type byte: `b'S'` submit, `b'C'` complete, `b'E'` error.
    pub event_type: u8,
    /// Transfer type: 0 iso, 1 interrupt, 2 control, 3 bulk.
    pub xfer_type: u8,
    /// Endpoint number including the direction bit (`0x83` = IN 3).
    pub epnum: u8,
    /// Device address on the bus — changes on every replug.
    pub devnum: u8,
    /// Bus number.
    pub busnum: u16,
    /// Captured payload length reported by the header.
    pub len_cap: u32,
    /// The captured payload (`len_cap` bytes at most).
    pub data: &'a [u8],
    /// Event timestamp from the header — the kernel's monotonic host
    /// clock at capture, `ts_sec * 1_000_000 + ts_usec` by the caller.
    pub ts_us: i64,
}

/// The §6.10 filter: a completion record on the microscope's interrupt
/// endpoint `0x83`, on the bus/device being tracked, carrying exactly
/// the 4-byte UVC button status packet.
///
/// `len_cap` must equal 4: a larger value is another UVC status
/// message or a payload the reader truncated (the kernel caps copies
/// at `alloc` but reports the original `len_cap`), and a smaller one
/// is a malformed record.
///
/// Returns the decoded edge or `None`. Everything else — other devices,
/// other endpoints, submissions, malformed payloads — is discarded.
#[must_use]
pub fn snap_button_event(rec: &UsbmonRecord<'_>, busnum: u16, devnum: u8) -> Option<ButtonEvent> {
    if rec.event_type != EVENT_COMPLETE
        || rec.xfer_type != XFER_INTERRUPT
        || rec.epnum != SNAP_EPNUM
        || rec.busnum != busnum
        || rec.devnum != devnum
        || rec.len_cap != 4
    {
        return None;
    }
    rec.data.get(..4).and_then(parse_status_packet)
}

/// Result of feeding a `Pressed` edge to [`ButtonTracker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressOutcome {
    /// Edge accepted — the press is now being timed.
    Accepted,
    /// Edge dropped inside the debounce window.
    Bounced,
}

/// Result of feeding a `Released` edge to [`ButtonTracker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// Held less than the debounce window — ignore the whole press.
    Bounced,
    /// Held less than the long threshold — a photo press.
    Short,
    /// The long-press action already fired while held — nothing to do.
    AfterLong,
    /// No press was being tracked.
    Ignored,
}

/// Turns press/release edges into actions (§6.10):
///
/// - edges closer than `debounce_ms` to the previous accepted edge are
///   dropped;
/// - a release under `long_press_ms` is a photo press;
/// - once the hold crosses `long_press_ms`, [`ButtonTracker::fire_long`]
///   reports the record action exactly once; the following release is
///   a no-op ([`ReleaseOutcome::AfterLong`]).
///
/// Timestamps are caller-supplied milliseconds so tests drive a fake
/// clock; the app feeds `Instant`-based monotonic ms.
#[derive(Debug, Clone)]
pub struct ButtonTracker {
    debounce_ms: u64,
    long_press_ms: u64,
    /// Start time of the currently held press.
    pressed_at: Option<u64>,
    /// Timestamp of the last accepted edge — `None` before the first
    /// one, so a press at time 0 is never debounced away.
    last_edge: Option<u64>,
    /// Long action already fired for the current hold.
    long_fired: bool,
}

impl ButtonTracker {
    /// A tracker with the given debounce and long-press times (ms).
    #[must_use]
    pub fn new(debounce_ms: u64, long_press_ms: u64) -> Self {
        Self {
            debounce_ms,
            long_press_ms,
            pressed_at: None,
            last_edge: None,
            long_fired: false,
        }
    }

    /// Whether `now` sits outside the debounce window of the last edge.
    fn accepts_edge(&self, now: u64) -> bool {
        self.last_edge
            .is_none_or(|e| now.saturating_sub(e) >= self.debounce_ms)
    }

    /// Changes the long-press threshold (preferences).
    pub fn set_long_press_ms(&mut self, ms: u64) {
        self.long_press_ms = ms;
    }

    /// The timestamp at which the held press becomes "long" — the app
    /// schedules its timer for then. `None` when not held.
    #[must_use]
    pub fn long_deadline(&self) -> Option<u64> {
        self.pressed_at.map(|at| at + self.long_press_ms)
    }

    /// A `Pressed` edge arrived at `now`.
    pub fn press(&mut self, now: u64) -> PressOutcome {
        if !self.accepts_edge(now) {
            return PressOutcome::Bounced;
        }
        self.last_edge = Some(now);
        self.pressed_at = Some(now);
        self.long_fired = false;
        PressOutcome::Accepted
    }

    /// A `Released` edge arrived at `now`.
    pub fn release(&mut self, now: u64) -> ReleaseOutcome {
        let Some(at) = self.pressed_at.take() else {
            return ReleaseOutcome::Ignored;
        };
        self.last_edge = Some(now);
        if now.saturating_sub(at) < self.debounce_ms {
            return ReleaseOutcome::Bounced;
        }
        if self.long_fired {
            ReleaseOutcome::AfterLong
        } else {
            ReleaseOutcome::Short
        }
    }

    /// Whether the held press just crossed the long threshold. Fires
    /// once per hold; the app calls this from its deadline timer.
    pub fn fire_long(&mut self, now: u64) -> bool {
        match self.pressed_at {
            Some(at) if !self.long_fired && now.saturating_sub(at) >= self.long_press_ms => {
                self.long_fired = true;
                true
            }
            _ => false,
        }
    }

    /// Drops any in-progress press (e.g. the source reconnected).
    pub fn reset(&mut self) {
        self.pressed_at = None;
        self.long_fired = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUS: u16 = 3;
    const DEV: u8 = 8;

    fn rec<'a>(data: &'a [u8]) -> UsbmonRecord<'a> {
        UsbmonRecord {
            event_type: EVENT_COMPLETE,
            xfer_type: XFER_INTERRUPT,
            epnum: SNAP_EPNUM,
            devnum: DEV,
            busnum: BUS,
            len_cap: data.len() as u32,
            data,
            ts_us: 0,
        }
    }

    /// §2.1 real bytes: `02 01 00 01` pressed, `02 01 00 00` released.
    #[test]
    fn status_packet_press_is_parsed() {
        assert_eq!(
            parse_status_packet(&[0x02, 0x01, 0x00, 0x01]),
            Some(ButtonEvent::Pressed)
        );
        assert_eq!(
            parse_status_packet(&[0x02, 0x01, 0x00, 0x00]),
            Some(ButtonEvent::Released)
        );
    }

    #[test]
    fn status_packet_rejects_other_values() {
        // Origin interface must be 1; event byte must be 0.
        assert_eq!(parse_status_packet(&[0x02, 0x02, 0x00, 0x01]), None);
        assert_eq!(parse_status_packet(&[0x02, 0x01, 0x01, 0x01]), None);
        assert_eq!(parse_status_packet(&[0x02, 0x01, 0x00, 0x02]), None);
        assert_eq!(parse_status_packet(&[0x02, 0x01, 0x00]), None);
        assert_eq!(parse_status_packet(&[]), None);
    }

    /// §6.10 filter acceptance: the whole path produces both edges.
    #[test]
    fn filter_accepts_the_real_button_records() {
        assert_eq!(
            snap_button_event(&rec(&[0x02, 0x01, 0x00, 0x01]), BUS, DEV),
            Some(ButtonEvent::Pressed)
        );
        assert_eq!(
            snap_button_event(&rec(&[0x02, 0x01, 0x00, 0x00]), BUS, DEV),
            Some(ButtonEvent::Released)
        );
    }

    #[test]
    fn filter_rejects_wrong_endpoint() {
        // Control endpoint 0x80 carries other UVC status events.
        let mut r = rec(&[0x02, 0x01, 0x00, 0x01]);
        r.epnum = 0x80;
        assert_eq!(snap_button_event(&r, BUS, DEV), None);
        r.epnum = 0x03; // OUT direction on ep 3
        assert_eq!(snap_button_event(&r, BUS, DEV), None);
    }

    #[test]
    fn filter_rejects_other_devices_and_buses() {
        // Same payload from another device on the bus (e.g. keyboard).
        let mut other = rec(&[0x02, 0x01, 0x00, 0x01]);
        other.devnum = 9;
        assert_eq!(snap_button_event(&other, BUS, DEV), None);
        other.devnum = DEV;
        other.busnum = 1;
        assert_eq!(snap_button_event(&other, BUS, DEV), None);
    }

    #[test]
    fn filter_rejects_submit_and_error_records() {
        // 'S' submissions carry the request, not the device answer.
        let mut s = rec(&[0x02, 0x01, 0x00, 0x01]);
        s.event_type = b'S';
        assert_eq!(snap_button_event(&s, BUS, DEV), None);
        s.event_type = b'E';
        assert_eq!(snap_button_event(&s, BUS, DEV), None);
    }

    #[test]
    fn filter_rejects_other_transfer_types() {
        for xfer in [0u8, 2, 3] {
            let mut r = rec(&[0x02, 0x01, 0x00, 0x01]);
            r.xfer_type = xfer;
            assert_eq!(snap_button_event(&r, BUS, DEV), None);
        }
    }

    #[test]
    fn filter_rejects_short_payloads() {
        assert_eq!(snap_button_event(&rec(&[0x02, 0x01, 0x00]), BUS, DEV), None);
        assert_eq!(snap_button_event(&rec(&[]), BUS, DEV), None);
        // len_cap claims 4 but the buffer is shorter — belt & braces.
        let mut r = rec(&[0x02, 0x01]);
        r.len_cap = 4;
        assert_eq!(snap_button_event(&r, BUS, DEV), None);
        // Header len_cap < 4 even though a longer buffer was passed.
        let mut r = rec(&[0x02, 0x01, 0x00, 0x01]);
        r.len_cap = 3;
        assert_eq!(snap_button_event(&r, BUS, DEV), None);
        // len_cap > 4: another UVC status message — or a video record
        // the reader truncated (kernel caps the copy at `alloc` but
        // reports the original len_cap). Both must be dropped.
        for len_cap in [5u32, 3072] {
            let mut r = rec(&[0x02, 0x01, 0x00, 0x01]);
            r.len_cap = len_cap;
            assert_eq!(snap_button_event(&r, BUS, DEV), None);
        }
    }

    /// Short press → photo (§6.10).
    #[test]
    fn short_press_is_a_photo() {
        let mut t = ButtonTracker::new(50, 700);
        assert_eq!(t.press(1_000), PressOutcome::Accepted);
        assert_eq!(t.release(1_160), ReleaseOutcome::Short);
    }

    /// Hold ≥ long threshold → the record action fires once at the
    /// deadline; the later release does nothing (§6.10).
    #[test]
    fn long_press_fires_once_and_release_is_quiet() {
        let mut t = ButtonTracker::new(50, 700);
        t.press(1_000);
        assert!(!t.fire_long(1_600), "699 ms is not long yet");
        assert!(t.fire_long(1_700), "700 ms crosses the threshold");
        assert!(!t.fire_long(2_000), "fires once per hold");
        assert_eq!(t.release(2_100), ReleaseOutcome::AfterLong);
    }

    /// A second long press toggles the recording back off.
    #[test]
    fn second_long_press_toggles_again() {
        let mut t = ButtonTracker::new(50, 700);
        t.press(0);
        assert!(t.fire_long(700));
        t.release(1_000);
        t.press(2_000);
        assert!(t.fire_long(2_700));
        t.release(3_000);
    }

    /// Debounce: edges inside 50 ms are dropped (§6.10).
    #[test]
    fn debounce_drops_fast_edges() {
        let mut t = ButtonTracker::new(50, 700);
        t.press(1_000);
        // A bounce release at +30 ms kills the press — no photo.
        assert_eq!(t.release(1_030), ReleaseOutcome::Bounced);
        // The press at +40 ms after that release is still inside the
        // debounce window of the last edge.
        assert_eq!(t.press(1_070), PressOutcome::Bounced);
        // A clean press after the window works.
        assert_eq!(t.press(1_100), PressOutcome::Accepted);
        assert_eq!(t.release(1_300), ReleaseOutcome::Short);
    }

    #[test]
    fn release_without_press_is_ignored() {
        let mut t = ButtonTracker::new(50, 700);
        assert_eq!(t.release(500), ReleaseOutcome::Ignored);
    }

    /// The configurable threshold actually moves the boundary.
    #[test]
    fn long_threshold_is_configurable() {
        let mut t = ButtonTracker::new(50, 700);
        t.set_long_press_ms(400);
        t.press(0);
        assert!(!t.fire_long(399));
        assert!(t.fire_long(400));
        assert_eq!(t.release(600), ReleaseOutcome::AfterLong);
    }

    /// The app exposes the deadline to schedule its timer.
    #[test]
    fn long_deadline_tracks_the_hold() {
        let mut t = ButtonTracker::new(50, 700);
        assert_eq!(t.long_deadline(), None);
        t.press(1_000);
        assert_eq!(t.long_deadline(), Some(1_700));
        t.release(1_200);
        assert_eq!(t.long_deadline(), None);
    }
}
