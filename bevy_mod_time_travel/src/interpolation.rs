//! Entity state interpolation for fixed tickrate logic.
//!
//! This module and everything in it handles *only* automatically interpolating between two last
//! states of an interpolated component after the fixed timestep logic has run. This is just enough
//! to create appearance of smooth motion when fixed timestep rate is less than the frame rate.
//!
//! Note that this does not gracefully handle the fixed timestep length value being edited (yet?);
//! when that happens, the interpolation logic results in a jarring jump.
//!
//! # How this works
//!
//! This works by saving world state at the end of [`FixedMainScheduleOrder`], keeping up to two
//! saved states at any time. Then, in [`RunFixedMainLoop`], a system uses interpolation to create
//! a world state that would fit the current [`Time<Fixed>::elapsed`] plus
//! [`Time<Fixed>::overstep`] minus [`Time<Fixed>::timestep`]. In other words, it interpolates to
//! the current time delayed by the timestep.
//!
//! The delay is necessary because interpolation requires having two recorded states to interpolate
//! between. The latest current state is already "outdated" from the real time by the value of
//! [`Time<Fixed>::overstep`], and the next state after it cannot be known until the full timestep
//! is reached. This only leaves using the current state and the one before it for interpolation,
//! and [`Time<Fixed>::timestep`] is the minimum amount of delay that can be added to the target
//! interpolation time for the result to always be within range encompassed by these two states.
//!
//!
//! # Examples
//!
//! To use with [`Transform`], ensure the crate feature `interpolation_for_transform` is enabled,
//! then:
//!
//! ```rust
//! use bevy::prelude::*;
//! use bevy_mod_time_travel::interpolation::{InterpolationPlugin, Interpolated};
//!
//! let mut app = App::new();
//!
//! // Add the plugin *after* the time plugin (included in DefaultPlugins)
//! app.add_plugins((bevy::time::TimePlugin, InterpolationPlugin::default()));
//!
//! // Add `Interpolated::<Transform>::default()` to the entities.
//! app
//!     .world_mut()
//!     .spawn((
//!         Transform::default(),
//!         Interpolated::<Transform>::default()
//!     ));
//!
//!  // Enjoy.
//!  app.run();
//! ```
//!
//! To use with other components, or without `interpolation_for_transform` feature, you also need
//! to register the component with an interpolation function.
//!
//! ```
//! use bevy::prelude::*;
//! use bevy_mod_time_travel::prelude::*;
//! use bevy_mod_time_travel::interpolation::{Interpolated, InterpolationPlugin};
//!
//! #[derive(Clone, Component)]
//! struct MyComponent(f32);
//!
//! let mut app = App::new();
//!
//! // Add the plugin *after* the time plugin (included in DefaultPlugins)
//! app.add_plugins((bevy::time::TimePlugin, InterpolationPlugin::default()));
//!
//! // Register the timeline with that particular component.
//! app.world_mut()
//!     .register_timeline::<Interpolated<MyComponent>>()
//!     .interpolate_with(
//!     // Linear interpolation formula
//!     |a, b, factor| MyComponent(a.0 + (b.0 - a.0) * factor),
//!     )
//!     .register_component();
//!
//! // Add `Interpolated::<MyComponent>::default()` to the entities.
//! app.world_mut()
//!     .spawn((Transform::default(), Interpolated::<MyComponent>::default()));
//!
//! // Enjoy.
//! app.run();
//! ```

use core::ops::{Deref, DerefMut};
use core::time::Duration;

use bevy_app::{App, FixedMainScheduleOrder, Plugin, RunFixedMainLoop, RunFixedMainLoopSystems};
use bevy_ecs::component::{Mutable, StorageType};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_time::{Fixed, Time, Virtual};

use super::prelude::*;
use super::timekeep::{ContinuumTime, Timekeep};

#[cfg(feature = "bevy_reflect")]
use bevy_reflect::Reflect;

#[cfg(feature = "interpolation_for_transform")]
use bevy_transform::components::Transform;

/// Plugin that configures the app to run the relevant time travel schedules where reasonable.
/// Feel free to skip this plugin if you have other ideas where to put them.
///
/// See description of [`crate::interpolation`] for details.
///
/// Note that for every type of component of resource you wish to interpolate, you first need to
/// run [`world.register_timeline<Interpolated<Something>>()`] first.
///
/// [`world.register_timeline<Interpolated<Something>>()`]:
/// super::world_methods::WorldTimeTravel::register_timeline
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct InterpolationPlugin(pub InterpolationVariables);

/// Settings and variables for interpolation.
#[derive(Clone, Copy, Debug, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct InterpolationVariables {
    /// If true, any time a change outside of fixed timestep logic happens, recorded state is
    /// overwritten with current state.
    ///
    /// This is useful primarily for development, for example editing world state with
    /// [`bevy-inspector-egui`], to not have it be immediately overwritten to old state by
    /// interpolation.
    ///
    /// However, this incurs extra processing cost, and causes the relevant components to be
    /// completely static for the rest of the interpolation cycle.
    ///
    /// Default is false.
    ///
    /// [`bevy-inspector-egui`]: https://github.com/jakobhellermann/bevy-inspector-egui/
    pub account_for_changes: bool,
    /// If false, all systems defined within this module do not run.
    ///
    /// Default is true.
    pub run_interpolation_systems: bool,
    /// Policy with which world state will be rewound.
    ///
    /// Default is [`TickRestorePolicy::RestoreOldTicks`] to keep interpolation transparent to
    /// fixed timestep logic.
    pub rewind_policy: TickRestorePolicy,
    /// Extra time for which old state is kept. This is useful in case you want to keep more of the
    /// old state around, but don't want to make whole new timelines and instead reuse the
    /// interpolation ones.
    ///
    /// Default is [`Duration::ZERO`].
    pub store_extra_backlog: Duration,
}

impl Default for InterpolationVariables {
    fn default() -> Self {
        Self {
            account_for_changes: false,
            run_interpolation_systems: true,
            rewind_policy: TickRestorePolicy::RestoreOldTicks,
            store_extra_backlog: Duration::ZERO,
        }
    }
}

impl Plugin for InterpolationPlugin {
    fn name(&self) -> &'static str {
        "Interpolation plugin"
    }

    fn build(&self, app: &mut App) {
        app.insert_resource(self.0);

        #[cfg(feature = "bevy_reflect")]
        {
            app.register_type::<InterpolationVariables>();
            app.register_type::<InterpolationPlugin>();
        }

        // First of all, this adds two custom schedules at start and end of FixedMainScheduleOrder.
        // Everything in this order runs once every fixed timestep.
        let fmso = &mut app
            .world_mut()
            .resource_mut::<FixedMainScheduleOrder>()
            .labels;
        fmso.insert(0, PreReturnToFixedSchedule.intern());
        fmso.push(PostReturnToFixedSchedule.intern());

        // First custom schedule runs one system that returns everything to the last recorded state
        // from fixed timesteps, returning world state to as if there is no interpolation. There's
        // logic in there that prevents it from running multiple times per frame, in case there's
        // multiple steps to go through.
        app.add_systems(PreReturnToFixedSchedule, return_to_fixed);

        // After it, normal fixed main schedules and systems run and do their thing.

        // After that, a system in the second custom schedule rotates buffers, recording new state
        // and dropping old.
        app.add_systems(PostReturnToFixedSchedule, record_new_state);

        // After fixed main loop, which runs every frame unconditionally and does 0 or more steps,
        // this system is run. It performs the actual interpolation.
        app.add_systems(
            RunFixedMainLoop,
            perform_interpolation.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
        );

        #[cfg(feature = "interpolation_for_transform")]
        #[allow(clippy::items_after_statements)]
        {
            fn interpolate_transform(a: &Transform, b: &Transform, t: f32) -> Transform {
                Transform {
                    translation: a.translation.lerp(b.translation, t),
                    rotation: a.rotation.slerp(b.rotation, t),
                    scale: a.scale.lerp(b.scale, t),
                }
            }

            let register = app
                .world_mut()
                .register_timeline::<Interpolated<Transform>>()
                .interpolate_with(interpolate_transform);

            #[cfg(feature = "bevy_reflect")]
            let register = register.reflect();

            register.register_component();
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
struct PreReturnToFixedSchedule;
#[derive(Clone, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
struct PostReturnToFixedSchedule;

/// A system that returns state of the world to last state recorded by [`record_new_state`], if
/// any. This should run before fixed timestep logic to avoid exposing interpolated values to it.
///
/// # Panics
///
/// Panics if there is no [`InterpolationVariables`] in the world, or no [`Time<Virtual>`].
pub fn return_to_fixed(world: &mut World) {
    let variables = world.resource::<InterpolationVariables>();
    if !variables.run_interpolation_systems {
        return;
    }

    // Avoid running it many times over in case we're doing multiple fixed
    // timestep ticks one after another.
    let current_time = world.resource::<Time<Virtual>>().elapsed();

    let Some(current_interpolation_timeline_time) = world
        .get_resource::<ContinuumTime<InterpolatedContinuum>>()
        .map(|x| x.time)
    else {
        // If there's none such, that means we haven't interacted with the timeline at all
        // yet. This means we did not record anything yet.
        return;
    };

    if current_interpolation_timeline_time == current_time {
        // We have already rewinded to this time. No need to rewind.
        return;
    }

    // We want to rewind to the last recorded tick's time, if any.
    let Some(target_time) = world
        .get_resource::<Timekeep<InterpolatedContinuum>>()
        .and_then(|x| x.last_moment())
        .map(|x| x.time)
    else {
        // If there's no last recorded tick, that means we did not record anything yet.
        // Don't rewind, just go with whatever.
        return;
    };

    let policy = variables.rewind_policy;

    if variables.account_for_changes {
        // Overwrite last 1 state where it changed.
        world.account_for_changes::<InterpolatedContinuum>(1);
    }

    if let Ok(resulting_time) = world.rewind_to_with_policies::<InterpolatedContinuum>(
        target_time,
        policy,
        // If the buffer wasn't populated yet, we don't want to delete anything.
        OutOfTimelineRangePolicy::DoNothing,
    ) {
        debug_assert_eq!(resulting_time, target_time);
    }
}

/// A system that records new world state. This should run after every finished fixed timestep tick
/// to record data that we can interpolate with.
///
/// # Panics
///
/// Panics if there is no [`InterpolationVariables`] in the world, or no [`Time<Fixed>`], or its
/// timestep value is zero.
pub fn record_new_state(world: &mut World) {
    let variables = world.resource::<InterpolationVariables>();
    if !variables.run_interpolation_systems {
        return;
    }

    let fixed = *world.resource::<Time<Fixed>>();
    let elapsed = fixed.elapsed();
    let timestep = fixed.timestep();
    let extra_backlog = variables.store_extra_backlog;

    // If timestep is zero, this explodes the rewind buffers in the `rotate_buffers` call below
    // with their own internal assertions.
    //
    // Make the assertion more descriptive and here instead.
    assert_ne!(
        timestep,
        core::time::Duration::ZERO,
        "Timestep should never be zero."
    );

    if variables.account_for_changes {
        // Discard changes made in this tick.
        world.discard_changes::<InterpolatedContinuum>();
    }

    world.rotate_buffers::<InterpolatedContinuum>(
        elapsed.saturating_sub(timestep + extra_backlog),
        elapsed,
    );
}

/// A system that performs interpolation. This should be run every frame after fixed timestep logic
/// potentially runs.
///
/// # Panics
///
/// Panics if there is no [`InterpolationVariables`] in the world, or no [`Time<Fixed>`], or its
/// timestep value is zero.
pub fn perform_interpolation(world: &mut World) {
    let variables = *world.resource::<InterpolationVariables>();
    if !variables.run_interpolation_systems {
        return;
    }

    let fixed = *world.resource::<Time<Fixed>>();

    // We want to interpolate to the current time (fixed time plus overstep) minus exactly one
    // timestep.
    let target_time = fixed
        .elapsed()
        .saturating_add(fixed.overstep())
        .saturating_sub(fixed.timestep());

    if variables.account_for_changes {
        world.account_for_changes::<InterpolatedContinuum>(2);
    }

    if let Some(continuum_time) = world.get_resource::<ContinuumTime<InterpolatedContinuum>>()
        && target_time == continuum_time.time
    {
        // We are about to interpolate to the same time where we went last time.
        // No need lol
    } else {
        #[allow(unused_variables)]
        if let Ok(resulting_time) = world.interpolate_to_with_policy::<InterpolatedContinuum>(
            target_time,
            // If the buffer wasn't populated yet, we don't want to delete anything.
            OutOfTimelineRangePolicy::DoNothing,
        ) {
            // Generally speaking, resulting_time and target_time should be roughly the same.
            // This can wildly change if the timestep value is changed suddenly.
            // As such, the below assert does not work out well.
            //debug_assert!(
            //    (resulting_time.as_secs_f32() - target_time.as_secs_f32()).abs() < 0.0001f32
            //);
        }
    }

    if variables.account_for_changes {
        world.discard_changes::<InterpolatedContinuum>();
    }
}

/// A timeline for the corresponding component/resource `T` that stores states produced within the
/// fixed timestep schedules and is used to interpolate between them.
///
/// Note that for every type of component of resource you wish to interpolate, you need to
/// run [`world.register_timeline<Interpolated<Something>>()`] first.
///
/// [`world.register_timeline<Interpolated<Something>>()`]:
/// super::world_methods::WorldTimeTravel::register_timeline
#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct Interpolated<T: Clone + Send + Sync + 'static> {
    /// Buffer containing the values to interpolate between.
    pub buf: RewindBuffer<T>,
    /// Whether or not the value was "teleported" in the
    /// current tick. If set to `true`, the next moment written into
    /// the rewind buffer should have `snap_to` set to true, and
    /// after that this should be set to false.
    pub teleported: bool,
}

impl<T: Clone + Send + Sync + 'static> Default for Interpolated<T> {
    fn default() -> Self {
        Self {
            buf: RewindBuffer::with_capacity(2),
            teleported: false,
        }
    }
}

// You should probably depend on `bevy_derive` and do `#[derive(Deref, DerefMut)]`
// for your timelines. This code opts to implement them manually to avoid the extra
// dependency.
impl<T: Clone + Send + Sync + 'static> Deref for Interpolated<T> {
    type Target = RewindBuffer<T>;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl<T: Clone + Send + Sync + 'static> DerefMut for Interpolated<T> {
    fn deref_mut(&mut self) -> &mut RewindBuffer<T> {
        &mut self.buf
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, ScheduleLabel)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
/// A [`Continuum`] for [`Interpolated<T>`].
pub struct InterpolatedContinuum;
impl Continuum for InterpolatedContinuum {}

// It's a timeline. Yeah.
impl<T: Clone + Send + Sync + 'static> Timeline for Interpolated<T> {
    type Item = T;
    type Continuum = InterpolatedContinuum;
    fn discontinuity(&self) -> bool {
        self.teleported
    }
    fn reset_discontinuity(&mut self) {
        self.teleported = false;
    }
}

// If T is Component, then this is too.
impl<T: Component<Mutability = Mutable> + Clone> Component for Interpolated<T> {
    const STORAGE_TYPE: StorageType = StorageType::Table;
    type Mutability = Mutable;
}

impl<T: Resource + Clone> Resource for Interpolated<T> {}
