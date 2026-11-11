//! Tracks the toast currently on screen so a new toast *replaces* it
//! instead of queueing behind it. `AdwToastOverlay` displays one toast
//! at a time; a persistent toast (`timeout = 0`) would hide every toast
//! queued after it until dismissed — which is why "never visible" was
//! the reported symptom (see report §6).
//!
//! The tracker is generic so the replacement invariant is testable
//! without a display (`adw::Toast` requires an initialized GTK).

/// A toast that can be removed from the overlay.
pub(crate) trait Dismissible {
    fn dismiss(&self);
}

impl Dismissible for adw::Toast {
    fn dismiss(&self) {
        adw::Toast::dismiss(self);
    }
}

/// The toast currently displayed by the overlay, if any.
pub(crate) struct ToastTracker<T> {
    current: Option<T>,
}

// Manual impl: `current: None` doesn't need `T: Default`.
impl<T> Default for ToastTracker<T> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<T: Dismissible> ToastTracker<T> {
    /// Dismisses the toast on screen, if any, and remembers `toast` as
    /// the new current one. The caller still has to add it to the
    /// overlay.
    pub(crate) fn replace(&mut self, toast: T) {
        if let Some(current) = self.current.take() {
            current.dismiss();
        }
        self.current = Some(toast);
    }
}

#[cfg(test)]
mod tests {
    use super::{Dismissible, ToastTracker};
    use std::cell::Cell;
    use std::rc::Rc;

    struct MockToast {
        dismissed: Rc<Cell<bool>>,
    }

    impl Dismissible for MockToast {
        fn dismiss(&self) {
            self.dismissed.set(true);
        }
    }

    fn mock() -> (MockToast, Rc<Cell<bool>>) {
        let flag = Rc::new(Cell::new(false));
        (
            MockToast {
                dismissed: flag.clone(),
            },
            flag,
        )
    }

    /// Regression (M4): a persistent toast hid every toast queued after
    /// it. A new toast must dismiss the displayed one, not queue.
    #[test]
    fn replace_dismisses_the_displayed_toast() {
        let mut tracker = ToastTracker::<MockToast>::default();

        let (first, first_flag) = mock();
        tracker.replace(first);
        assert!(!first_flag.get(), "nothing was on screen yet");

        let (second, second_flag) = mock();
        tracker.replace(second);
        assert!(first_flag.get(), "first toast was not dismissed");
        assert!(!second_flag.get(), "new toast dismissed itself");

        let (third, _) = mock();
        tracker.replace(third);
        assert!(second_flag.get(), "second toast was not dismissed");
    }
}
