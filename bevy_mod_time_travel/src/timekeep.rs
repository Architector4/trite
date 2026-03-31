use core::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use bevy_ecs::{resource::Resource, world::World};

use super::continuum::*;
use super::rewind_buffer::RewindBuffer;

#[cfg(feature = "bevy_reflect")]
use bevy_ecs::reflect::ReflectResource;
#[cfg(feature = "bevy_reflect")]
use bevy_reflect::Reflect;

/// Linear interpolation of [`Duration`] values.
#[must_use]
pub fn interpolate_duration(a: Duration, b: Duration, factor: f32) -> Duration {
    a.min(b) + a.abs_diff(b).mul_f32(factor)
}

/// A struct that represents the current time, as established by [`WorldTimeTravel`] methods, of a
/// [`Continuum`].
///
/// This is used to observe the effect of the aforementioned methods and have useful values to
/// return. This is also the root for the somewhat more important [`Timekeep`] resource.
///
/// [`WorldTimeTravel`]: super::world_methods::WorldTimeTravel
#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct ContinuumTime<C: Continuum> {
    /// Time at which this timeline is currently believed to be at.
    pub time: Duration,
    /// Timeline continuum.
    pub continuum: C,
}

impl<C: Continuum> ContinuumTime<C> {
    /// Perform linear interpolation.
    pub fn interpolate(a: &Self, b: &Self, factor: f32) -> Self {
        Self {
            time: interpolate_duration(a.time, b.time, factor),
            continuum: b.continuum.clone(),
        }
    }
}

/// A resource timeline used by [`WorldTimeTravel`] methods to keep track of what span of time is
/// recorded in a continuum.
///
/// In every stored [`Moment`], the `time` field should be equal to the value of [`ContinuumTime`]
/// stored within it.
///
/// [`WorldTimeTravel`]: super::world_methods::WorldTimeTravel
/// [`Moment`]: super::rewind_buffer::Moment
#[derive(Clone, Debug, Default, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct Timekeep<C: Continuum> {
    /// Rewind buffer containing the moments recorded in this timeline.
    ///
    /// Typically, if there is a moment for some time here, then there should be an equivalent
    /// moment for that same time in every other timeline for this continuum.
    pub buf: RewindBuffer<ContinuumTime<C>>,
    /// Continuum this timekeep is timekeeping for.
    pub continuum: C,
}

impl<C: Continuum> Timekeep<C> {
    /// Produce a new timekeep for the given continuum.
    pub fn with_continuum(continuum: C) -> Self {
        Self {
            continuum,
            buf: RewindBuffer::default(),
        }
    }

    /// Register this timekeep into the world. This is safe to run multiple times, but generally
    /// shouldn't be necessary to run manually at all, as [`WorldTimeTravel`] methods run this
    /// automatically if needed.
    ///
    /// [`WorldTimeTravel`]: super::world_methods::WorldTimeTravel
    #[allow(clippy::missing_panics_doc)] // Cannot really panic.
    pub fn register_into_world(world: &mut World) {
        use bevy_ecs::schedule::Schedules;

        use super::schedules::TimeTravelSchedules;
        use super::schedules::resource_account_for_changes_if_changed;
        use super::world_methods::WorldTimeTravel;

        let register = world
            .register_timeline::<Self>()
            .interpolate_with(ContinuumTime::<C>::interpolate);

        // It'd be cool to also reflect the resource into the world here, but that needs a bunch
        // of extra reflect trait bounds on Continuum, and I'm unsure if I should commit to
        // that.

        register.register_resource();

        // Exclude it from one thing though: accounting for changes. Otherwise all hell breaks
        // loose when that function is used.

        let mut schedule = world
            .resource_mut::<Schedules>()
            .remove(TimeTravelSchedules::AccountingForChanges(C::default()))
            .expect("Schedule should have been created");

        schedule
            .remove_systems_in_set(
                resource_account_for_changes_if_changed::<Self>,
                world,
                bevy_ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .expect("System we're trying to remove should be present");

        world.resource_mut::<Schedules>().insert(schedule);
    }
}

impl<C: Continuum> Deref for Timekeep<C> {
    type Target = RewindBuffer<ContinuumTime<C>>;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl<C: Continuum> DerefMut for Timekeep<C> {
    fn deref_mut(&mut self) -> &mut RewindBuffer<ContinuumTime<C>> {
        &mut self.buf
    }
}

impl<C: Continuum> Timeline for Timekeep<C> {
    type Item = ContinuumTime<C>;
    type Continuum = C;
}
