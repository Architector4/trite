use core::{
    hash::Hash,
    ops::{Deref, DerefMut},
};

use bevy_ecs::{
    component::{Component, Mutable},
    resource::Resource,
    schedule::ScheduleLabel,
};

use super::rewind_buffer::RewindBuffer;

/// Trait for types representing a series of timestamped states, i.e. [`Moment`]s. Used by Bevy systems in this
/// crate.
///
/// For example, a [`Timeline<Item = Transform>`] is a component that holds a
/// [`RewindBuffer<Transform>`] and is responsible for saving and manipulating the state of the
/// `Transform` component on the same entity across time.
///
/// [`Moment`]: super::rewind_buffer::Moment
pub trait Timeline:
    Deref<Target = RewindBuffer<Self::Item>> + DerefMut<Target = RewindBuffer<Self::Item>>
{
    /// Item type that this timeline is for.
    // Trait bound as needed by `RewindBuffer`
    type Item: Clone + Send + Sync + 'static;

    /// What [`Continuum`] is this timeline a part of.
    type Continuum: Continuum;

    /// Returns true if the item is to be considered "teleported" in the current moment. This is
    /// used in the "rotate" time travel schedule as the value to set the `snap_to` field to in the
    /// next [`Moment`] that will be stored in the [`RewindBuffer`].
    ///
    /// Setting the discontinuity value is left up to the implementor of this trait to figure out.
    ///
    /// Default implementation always returns `false`.
    ///
    /// [`Moment`]: super::rewind_buffer::Moment
    fn discontinuity(&self) -> bool {
        false
    }

    /// Reset the discontinuity value. This is called after the [`Self::discontinuity()`] function
    /// by the "rotate" time travel schedule.
    ///
    /// Default implementation does nothing.
    fn reset_discontinuity(&mut self) {}
}

/// A type used to group [`Timeline`]s into a unified set of [`TimeTravelSchedules`]. As such,
/// it must conform to trait bounds of [`ScheduleLabel`] and typically should be a zero sized type.
///
/// [`TimeTravelSchedules`]: super::schedules::TimeTravelSchedules
/// [`ScheduleLabel`]: bevy_ecs::schedule::ScheduleLabel
pub trait Continuum: ScheduleLabel + Default + Clone + Eq + Hash {}

/// Convenience auto-impl trait representing [Timeline] for a Bevy [Component].
pub trait TimelineComponent:
    Timeline<Item: Component<Mutability = Mutable>> + Component<Mutability = Mutable>
{
}

impl<T: Timeline<Item: Component<Mutability = Mutable>> + Component<Mutability = Mutable>>
    TimelineComponent for T
{
}

/// Convenience auto-impl trait representing [Timeline] for a Bevy [Resource].
pub trait TimelineResource: Timeline<Item: Resource> + Resource {}

impl<T: Timeline<Item: Resource> + Resource> TimelineResource for T {}
