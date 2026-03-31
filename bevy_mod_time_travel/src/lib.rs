#![doc = include_str!("../README.md")]
#![no_std]
#![warn(clippy::pedantic)]
#![allow(clippy::wildcard_imports)] // they're handy ok

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

/// Module for [`Moment<T>`] and [`RewindBuffer<T>`]. The core of the crate.
///
/// [`Moment<T>`]: rewind_buffer::Moment<T>
/// [`RewindBuffer<T>`]: rewind_buffer::RewindBuffer<T>
pub mod rewind_buffer;

/// Module for [`Timeline`] and [`Continuum`], traits to organize all time travel state and do
/// useful things.
///
/// [`Timeline`]: continuum::Timeline
/// [`Continuum`]: continuum::Continuum
pub mod continuum;

/// This module contains schedules and all the systems in them that are run to facilitate world
/// state management (i.e. "time travel")
///
/// These schedules and systems should not be instanced or run directly and are only exposed to
/// allow inserting your own systems to run inside of them [before/after/during] the primary logic
/// they are made for.
///
/// For running these schedules, use [`WorldTimeTravel`] methods. For creating them or inserting
/// systems into them, use [`RegisterTimeline`] instead.
///
/// [`WorldTimeTravel`]: world_methods::WorldTimeTravel
/// [before/after/during]: schedules::TimeTravelSystemSet
/// [`RegisterTimeline`]: registration::RegisterTimeline
pub mod schedules;

/// Module for [`RegisterTimeline`], a way to register timelines into the world.
///
/// [`RegisterTimeline`]: registration::RegisterTimeline
pub mod registration;

/// Resources that keep help keep track of the current time and stored moments within timelines.
/// Managed by functions in [`world_methods`] module.
pub mod timekeep;

/// Methods that run the timeline schedules and [`timekeep`]ing in a convenient manner.
pub mod world_methods;

#[cfg(feature = "interpolation")]
pub mod interpolation;

/// An auto trait for functions that do interpolation over T, with additional trait bounds needed
/// for this to be stored inside a closure used as a Bevy system.
pub trait InterpFunc<T>: FnMut(&T, &T, f32) -> T + Copy + Send + Sync + 'static {}
impl<T, F: FnMut(&T, &T, f32) -> T + Copy + Send + Sync + 'static> InterpFunc<T> for F {}

/// A function that implements [`InterpFunc`] that picks `b` if the factor is more than 0.0, `a`
/// otherwise. This can be used to simulate the "Pick B" behavior mentioned in the description of
/// the [crate].
pub fn pick_b_if_nonzero<T: Clone>(a: &T, b: &T, factor: f32) -> T {
    if factor > 0.0 { b.clone() } else { a.clone() }
}

/// Error that signifies that the provided time value is outside of available range.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub struct OutOfRecordedRangeError;

impl core::fmt::Display for OutOfRecordedRangeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Specified time is outside of recorded range")
    }
}

impl core::fmt::Debug for OutOfRecordedRangeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

impl core::error::Error for OutOfRecordedRangeError {
    fn description(&self) -> &'static str {
        "The provided time value is outside of range recorded within this buffer or continuum."
    }
}

/// Re-exports of common types needed for implementing timelines and utilizing time travel.
pub mod prelude {
    pub use super::continuum::{Continuum, Timeline};
    pub use super::rewind_buffer::{Moment, RewindBuffer};
    pub use super::schedules::{OutOfTimelineRangePolicy, TickRestorePolicy};
    pub use super::world_methods::WorldTimeTravel;
    pub use core::time::Duration;
}
