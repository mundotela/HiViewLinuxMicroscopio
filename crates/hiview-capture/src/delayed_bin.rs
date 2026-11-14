// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! `DelayedBin` — a `gst::Bin` whose NULL→READY transition sleeps a
//! configured amount (M10.5). A real `pulsesrc` can take hundreds of
//! milliseconds to open the device *inside* the pipeline's
//! `set_state(Playing)` call; this bin reproduces that timing in tests
//! so the deterministic `base_time` setup is exercised against the
//! same condition. Production code never instantiates it — only the
//! `AudioSource::Delayed` test injection does.

use std::sync::atomic::Ordering;
use std::time::Duration;

use gst::glib;
use gst::glib::subclass::types::ObjectSubclassIsExt;
use gst::prelude::*;

mod imp {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use gst::glib;
    use gst::subclass::prelude::*;

    #[derive(Default)]
    pub struct DelayedBin {
        pub(super) delay_ms: AtomicU64,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DelayedBin {
        const NAME: &'static str = "HiViewDelayedBin";
        type Type = super::DelayedBin;
        type ParentType = gst::Bin;
    }

    impl ObjectImpl for DelayedBin {}

    impl GstObjectImpl for DelayedBin {}

    impl ElementImpl for DelayedBin {
        fn change_state(
            &self,
            transition: gst::StateChange,
        ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
            if transition == gst::StateChange::NullToReady {
                let ms = self.delay_ms.load(Ordering::Relaxed);
                if ms > 0 {
                    std::thread::sleep(Duration::from_millis(ms));
                }
            }
            self.parent_change_state(transition)
        }
    }

    impl BinImpl for DelayedBin {}
}

glib::wrapper! {
    pub struct DelayedBin(ObjectSubclass<imp::DelayedBin>)
        @extends gst::Bin, gst::Element, gst::Object;
}

impl DelayedBin {
    /// Wraps `inner` in a bin that sleeps `delay` during NULL→READY —
    /// i.e. inside the parent pipeline's `set_state` call. The bin's
    /// src ghost pad exposes `inner`'s src pad.
    pub fn new(inner: &gst::Element, delay: Duration) -> Result<Self, crate::Error> {
        let bin: Self = glib::Object::new();
        bin.imp()
            .delay_ms
            .store(delay.as_millis() as u64, Ordering::Relaxed);
        bin.add(inner)
            .map_err(|e| crate::Error::VideoDevice(format!("delayed bin add: {e}")))?;
        let src_pad = inner
            .static_pad("src")
            .ok_or_else(|| crate::Error::VideoDevice("delayed source has no src pad".into()))?;
        let ghost = gst::GhostPad::with_target::<gst::Pad>(&src_pad)
            .map_err(|e| crate::Error::VideoDevice(format!("ghost pad: {e}")))?;
        ghost
            .set_active(true)
            .map_err(|e| crate::Error::VideoDevice(format!("ghost activate: {e}")))?;
        bin.add_pad(&ghost)
            .map_err(|e| crate::Error::VideoDevice(format!("ghost add: {e}")))?;
        Ok(bin)
    }
}
