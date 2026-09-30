// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The app's actual logic: one counter and its rules.
//!
//! This module is the template's only real example of where logic belongs. It
//! must stay free of `web-sys`, `js-sys` and `wasm-bindgen` so that it
//! compiles and runs natively under `cargo test`. The browser layer in
//! `crate::ui` owns every DOM call and nothing else; everything a person can
//! reason about — what the count is, what it clamps to, what the button says,
//! what gets stored, which storage key holds it — is decided here and unit
//! tested here.
//!
//! When you copy this template, keep that split. The moment `counter.rs`
//! reaches for `document`, its tests stop running and the app becomes a blob.

/// Key prefix for every value this app writes to `localStorage`.
///
/// One prefix per app, so two PWAs served from the same origin cannot read or
/// overwrite each other's state.
pub const STORAGE_KEY: &str = "pwa_template.counter";

/// The smallest count the counter will hold.
pub const MIN: i64 = 0;

/// The largest count the counter will hold.
pub const MAX: i64 = 99;

/// The counter, as the app owns it.
///
/// A plain value with a handful of total functions, not a state machine. When
/// the app outgrows that, this is the type that grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counter {
    value: i64,
}

impl Counter {
    /// A counter starting at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// A counter starting at `value`, clamped into `MIN..=MAX`.
    ///
    /// Clamping on the way in means every later read can assume the
    /// invariant, so no render path has to re-check it.
    #[must_use]
    pub fn starting_at(value: i64) -> Self {
        Self {
            value: clamp(value),
        }
    }

    /// The current count.
    #[must_use]
    pub const fn value(&self) -> i64 {
        self.value
    }

    /// True when the count is at its ceiling and cannot go higher.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.value >= MAX
    }

    /// The count after one increment, saturating at `MAX`.
    ///
    /// `saturating_add` rather than `+ 1` and a clamp: the clamp form lets a
    /// bad value reach the state, which is the bug this avoids.
    #[must_use]
    pub fn incremented(&self) -> Self {
        Self {
            value: clamp(self.value.saturating_add(1)),
        }
    }

    /// The count after one decrement, saturating at `MIN`.
    #[must_use]
    pub fn decremented(&self) -> Self {
        Self {
            value: clamp(self.value.saturating_sub(1)),
        }
    }

    /// What the readout should say: the number, and words.
    ///
    /// A single function owns every piece of text the app shows about the
    /// count, so the screen can never disagree with the state.
    #[must_use]
    pub fn readout(&self) -> Readout<'_> {
        Readout {
            count: self.value,
            label: plural(self.value),
            next_action: if self.is_full() { "full" } else { "count" },
        }
    }

    /// A stored value, read back into a counter.
    ///
    /// Anything unreadable, missing or out of range becomes a fresh counter.
    /// A corrupt `localStorage` entry must never be able to stop the app from
    /// starting.
    #[must_use]
    pub fn from_stored(raw: Option<&str>) -> Self {
        raw.and_then(|text| text.trim().parse::<i64>().ok())
            .map(Self::starting_at)
            .unwrap_or_default()
    }

    /// The value to persist, or `None` when there is nothing worth keeping.
    ///
    /// Returning `None` for the default keeps the first visit from writing
    /// anything at all, so a user who never touches the app leaves no trace.
    #[must_use]
    pub fn to_stored(&self) -> Option<String> {
        (self.value != 0).then(|| self.value.to_string())
    }
}

/// Everything the shell needs to draw the counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readout<'a> {
    /// The count itself.
    pub count: i64,
    /// The count in words, e.g. `"3 taps"`, `"nothing yet"`.
    pub label: &'a str,
    /// What pressing the button will do: `"count"` or `"full"`.
    pub next_action: &'a str,
}

/// Constrain a count to the range the app allows.
fn clamp(value: i64) -> i64 {
    value.clamp(MIN, MAX)
}

/// The one place that turns a number into words.
///
/// Small, total, and tested. If a copy of this template ever grows a second
/// worded string, it grows a function here — not an `if` at a call site.
#[must_use]
fn plural(value: i64) -> &'static str {
    match value {
        0 => "nothing yet",
        1 => "1 tap",
        _ => "taps",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_counter_starts_at_nothing() {
        assert_eq!(Counter::new().value(), 0);
        assert_eq!(Counter::new().readout().label, "nothing yet");
    }

    #[test]
    fn counting_saturates_at_both_ends() {
        let counter = Counter::new();
        assert_eq!(counter.incremented().value(), 1);
        assert_eq!(counter.decremented().value(), 0);
        // Below zero is not a count; the floor holds.
        assert_eq!(counter.decremented().decremented().value(), MIN);
        // And the ceiling holds too, without wrapping.
        let full = Counter::starting_at(MAX);
        assert_eq!(full.incremented().value(), MAX);
        assert!(full.is_full());
        assert_eq!(full.incremented().readout().next_action, "full");
    }

    #[test]
    fn a_stored_value_is_clamped_on_the_way_in() {
        assert_eq!(Counter::from_stored(Some("12")).value(), 12);
        assert_eq!(Counter::from_stored(Some("9999")).value(), MAX);
        assert_eq!(Counter::from_stored(Some("-5")).value(), MIN);
        // Whitespace from a hand-edited entry is not corruption.
        assert_eq!(Counter::from_stored(Some(" 7 ")).value(), 7);
    }

    #[test]
    fn unreadable_storage_starts_the_app_rather_than_breaking_it() {
        for bad in [None, Some(""), Some("twelve"), Some("12.5"), Some("")] {
            assert_eq!(Counter::from_stored(bad).value(), 0, "input: {bad:?}");
        }
    }

    #[test]
    fn nothing_is_stored_until_the_user_counts() {
        assert_eq!(Counter::new().to_stored(), None);
        assert_eq!(Counter::starting_at(3).to_stored().as_deref(), Some("3"));
        // And it round-trips.
        let saved = Counter::starting_at(3).to_stored();
        assert_eq!(Counter::from_stored(saved.as_deref()).value(), 3);
    }

    #[test]
    fn the_readout_is_derived_from_the_count() {
        assert_eq!(Counter::starting_at(1).readout().label, "1 tap");
        assert_eq!(Counter::starting_at(4).readout().label, "taps");
        assert_eq!(Counter::starting_at(4).readout().next_action, "count");
    }
}
