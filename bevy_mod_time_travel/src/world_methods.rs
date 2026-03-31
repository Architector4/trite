use bevy_ecs::world::World;
use core::sync::atomic::AtomicBool;
use core::time::Duration;

use super::OutOfRecordedRangeError;
use super::continuum::{Continuum, Timeline};
use super::registration::RegisterTimeline;
use super::schedules::*;
use super::timekeep::{ContinuumTime, Timekeep};

#[cfg(feature = "logging")]
use bevy_log::prelude::*;

/// Methods to quickly manage world state. Implemented on [`World`].
pub trait WorldTimeTravel {
    /// Create a builder for registering a timeline.
    ///
    /// For an example, see the [crate] documentation page.
    fn register_timeline<T: Timeline>(&mut self) -> RegisterTimeline<'_, T>;

    /// Tries to rewind world state to specified time, without any interpolation.
    ///
    /// Returns the time the world state is at after the function is run.
    ///
    /// If there's no moment in recorded state corresponding to exactly the specified time, but it
    /// is inbetween two recorded states, the state *after* the specified time is chosen, as per
    /// the "Pick B" behavior.
    ///
    /// Change detection state is set based on [`TickRestorePolicy::Adaptive`]. If a timeline in
    /// this continuum does not cover the input time, but it is within the overall continuum's
    /// bounds, the state is set based on [`OutOfTimelineRangePolicy::AssumeNone`], i.e. simply
    /// removed.
    ///
    ///
    /// # Errors
    ///
    /// If the specified time is outside of range recorded for this continuum, this does nothing
    /// and returns an [`OutOfRecordedRangeError`].
    fn rewind_to<C: Continuum>(
        &mut self,
        to: Duration,
    ) -> Result<Duration, OutOfRecordedRangeError> {
        self.rewind_to_with_policies::<C>(
            to,
            TickRestorePolicy::default(),
            OutOfTimelineRangePolicy::default(),
        )
    }

    /// Tries to rewind world state to specified time, without any interpolation, with specific
    /// policies.
    ///
    /// Same as [`WorldTimeTravel::rewind_to`], but allows specifying change detection state and
    /// out of timeline range policies.
    ///
    /// # Errors
    ///
    /// If the specified time is outside of range recorded for this continuum, this does nothing
    /// and returns an [`OutOfRecordedRangeError`].
    fn rewind_to_with_policies<C: Continuum>(
        &mut self,
        to: Duration,
        tick_restore_policy: TickRestorePolicy,
        out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    ) -> Result<Duration, OutOfRecordedRangeError>;

    /// Tries to interpolate world state to specified time.
    ///
    /// Returns **approximately** the time the world state is at after the function is run. This
    /// value should be about the same as the `to` argument, but is susceptible to floating point
    /// precision errors.
    ///
    /// If a timeline in this continuum does not cover the input time, but it is within the overall
    /// continuum's bounds, the state is set based on [`OutOfTimelineRangePolicy::AssumeNone`],
    /// i.e. simply removed.
    ///
    /// All affected state's change ticks are bumped, as if with
    /// [`TickRestorePolicy::MarkAllChanged`].
    ///
    /// # Errors
    ///
    /// If the specified time is outside of range recorded for this continuum, this does nothing
    /// and returns an [`OutOfRecordedRangeError`].
    fn interpolate_to<C: Continuum>(
        &mut self,
        to: Duration,
    ) -> Result<Duration, OutOfRecordedRangeError> {
        self.interpolate_to_with_policy::<C>(to, OutOfTimelineRangePolicy::default())
    }

    /// Tries to interpolate world state to specified time with the provided out of timeline range
    /// policy.
    ///
    /// Same as [`WorldTimeTravel::interpolate_to`], but allows specifying said policy. policy.
    ///
    /// # Errors
    ///
    /// If the specified time is outside of range recorded for this continuum, this does nothing
    /// and returns an [`OutOfRecordedRangeError`].
    fn interpolate_to_with_policy<C: Continuum>(
        &mut self,
        to: Duration,
        out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    ) -> Result<Duration, OutOfRecordedRangeError>;

    /// Deletes current world state before the provided `delete_before` time, and saves state at
    /// the current time.
    ///
    /// To not delete any state, use [`WorldTimeTravel::insert_into_buffers`] or supply
    /// `delete_before` value of [`Duration::ZERO`]. In doing so, **take caution** to not
    /// accumulate state indefinitely and run out of memory.
    fn rotate_buffers<C: Continuum>(&mut self, delete_before: Duration, current_time: Duration);

    /// Save state at the current time, without deleting any state.
    ///
    /// **Take caution** to not accumulate state indefinitely and run out of memory. To avoid doing
    /// so, it's recommended to use [`Self::rotate_buffers`] instead.
    fn insert_into_buffers<C: Continuum>(&mut self, current_time: Duration) {
        self.rotate_buffers::<C>(Duration::ZERO, current_time);
    }

    /// Delete all recorded state after the specified time.
    ///
    /// An important part of an implementation of rollback networking.
    fn delete_after<C: Continuum>(&mut self, delete_after: Duration);

    /// Delete all recorded state.
    fn clear_timelines<C: Continuum>(&mut self);

    // TODO: read this garbage thoroughly and maybe rewrite it and maybe mention that this has
    // nothing to do with tick restore policy
    /// Detect changes made to tracked world state and account for them in a simple way.
    ///
    /// If any resource or entity's component is changed since the last time this method or
    /// [`Self::discard_changes`] is run, the last `overwrite_states` recorded states in the
    /// corresponding timeline are replaced with the latest state of it in the world.
    ///
    /// Returns true if any such change is detected.
    ///
    /// # Primitive use
    ///
    /// The overwrite behavior can be a minimum viable way to account for changes outside of a
    /// controlled simulation by ensuring it continues from the new state onward, if at least *one*
    /// last state is overwritten.
    ///
    /// For example, consider linear interpolation as implemented in this crate. Just before fixed
    /// timestep logic is run, world state is overwritten with the last moment recorded within the
    /// buffers. Then, just after it's run, new world state is saved. Afterward, on every frame,
    /// current world state is overwritten with interpolated results from last two saved states.
    ///
    /// Nowhere in this process are state changes made from *outside* the fixed timestep logic is
    /// read. What this means is that changes made in an [`Update`] schedule, or anywhere else,
    /// will never affect the simulation.
    ///
    /// This might be not the desired effect: you might be interested in overwriting the state
    /// manually via an inspector plugin or makeshift code or plugins incompatible with fixed
    /// timestep.
    ///
    /// This is what using this method allows: if you overwrite *two* last states any time a change
    /// is detected with the changed state, then both the per-frame interpolation or the logic that
    /// returns to last recorded state before each fixed timestep will all use just that new state.
    /// This effectively applies it.
    ///
    /// There's a caveat in the above case: because both states are used for interpolation, the
    /// object will appear to freeze up in this new state up until the next fixed timestep happens
    /// and new state is recorded and interpolated to. Nonetheless, it might be of use for
    /// diagnostic use cases.
    ///
    /// # Rollback use
    ///
    /// Alternatively, this can be used for a different, more expensive, but potentially more
    /// accurate method of accounting for change.
    ///
    /// If the current state of the world is from the past, and a change is detected, then it is
    /// more accurate, though more computationally expensive, to erase all state past the current
    /// time with [`Self::delete_after`], and then recompute it all with the modified state in
    /// mind.
    ///
    /// Besides computational cost, this may also bring floating point accuracy issues if you have
    /// to work from an interpolated state, or various breakage due to nondeterminism of systems in
    /// Bevy and inability to fully rewind them.
    ///
    /// [`Update`]: bevy_app::Update
    /// [`delete_after`]: WorldTimeTravel::delete_after
    /// [`InterpolationPlugin`]: super::interpolation::InterpolationPlugin
    fn account_for_changes<C: Continuum>(&mut self, overwrite_states: usize) -> bool;

    /// Discard all changes performed since the last time `account_for_changes` or
    /// `discard_changes` is run.
    fn discard_changes<C: Continuum>(&mut self) {
        let _ = self.account_for_changes::<C>(0);
    }

    /// Clean up entities/resources that are empty across their timeline.
    ///
    /// If an entity has at least one timeline component that belongs to this continuum, and all
    /// recorded moments in all timelines on this entity are empty, then this entity is deleted.
    /// Entities without any timelines of this continuum are untouched.
    ///
    /// The actual components are not checked for presence, only the timelines are.
    ///
    /// Timeline resources with all empty recorded moments, and their corresponding resources if
    /// present, are cleaned up too.
    fn clean_up_empty<C: Continuum>(&mut self);

    /// Clean up entities that are disabled across the continuum.
    ///
    /// If an entity has a timeline for the [`Disabled`] component within this continuum, and all
    /// recorded moments in it have the component, then this entity is deleted. Entities without
    /// such a timeline are untouched.
    ///
    /// Note that before using this you have to actually register such a timeline.
    ///
    /// The actual component is not checked for presence, only the timeline is.
    ///
    /// Resources are untouched.
    ///
    /// [`Disabled`]: bevy_ecs::entity_disabling::Disabled
    fn clean_up_disabled<C: Continuum>(&mut self);
}

impl WorldTimeTravel for World {
    fn register_timeline<T: Timeline>(&mut self) -> RegisterTimeline<'_, T> {
        RegisterTimeline::<'_, T>::new(self)
    }

    fn rewind_to_with_policies<C: Continuum>(
        &mut self,
        to: Duration,
        tick_restore_policy: TickRestorePolicy,
        out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    ) -> Result<Duration, OutOfRecordedRangeError> {
        if self
            .get_resource_mut::<Timekeep<C>>()
            .is_none_or(|x| !x.buf.time_in_range(to))
        {
            return Err(OutOfRecordedRangeError);
        }

        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Rewinding to {to:?}...");

        self.insert_resource(RewindTo {
            to,
            tick_restore_policy,
            out_of_timeline_range_policy,
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::Rewinding(continuum));
        self.remove_resource::<InterpolateTo<C>>();

        Ok(self.resource_mut::<ContinuumTime<C>>().time)
    }

    fn interpolate_to_with_policy<C: Continuum>(
        &mut self,
        to: Duration,
        out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    ) -> Result<Duration, OutOfRecordedRangeError> {
        if self
            .get_resource_mut::<Timekeep<C>>()
            .is_none_or(|x| !x.buf.time_in_range(to))
        {
            return Err(OutOfRecordedRangeError);
        }

        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Interpolating to {to:?}...");

        self.insert_resource(InterpolateTo {
            to,
            out_of_timeline_range_policy,
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::Interpolating(continuum));
        self.remove_resource::<InterpolateTo<C>>();

        Ok(self.resource_mut::<ContinuumTime<C>>().time)
    }

    fn rotate_buffers<C: Continuum>(&mut self, delete_before: Duration, current_time: Duration) {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Rotating buffers at {current_time:?}...");

        if let Some(mut cont_time) = self.get_resource_mut::<ContinuumTime<C>>() {
            // Need to bypass change detection here.
            // Otherwise, `account_for_changes` will likely see the change every frame lol
            use bevy_ecs::change_detection::DetectChangesMut;
            cont_time.bypass_change_detection().time = current_time;
        } else {
            // It's not set up. Set up timekeep.
            Timekeep::<C>::register_into_world(self);
            self.insert_resource(Timekeep::with_continuum(continuum.clone()));
            self.insert_resource(ContinuumTime {
                time: current_time,
                continuum: continuum.clone(),
            });
        }

        self.insert_resource(RotateBuffers {
            delete_before,
            current_time,
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::RotatingBuffers(continuum));
        self.remove_resource::<RotateBuffers<C>>();
    }

    fn delete_after<C: Continuum>(&mut self, delete_after: Duration) {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Rolling back (deleting everything after) {delete_after:?}...");

        self.insert_resource(DeleteAfter {
            delete_after,
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::DeletingAfter(continuum));
        self.remove_resource::<DeleteAfter<C>>();
    }

    fn clear_timelines<C: Continuum>(&mut self) {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Clearing timeline.");

        self.run_schedule(TimeTravelSchedules::Clearing(continuum));
    }

    fn account_for_changes<C: Continuum>(&mut self, overwrite_states: usize) -> bool {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Accounting for changes overwriting {overwrite_states} states...");

        self.insert_resource(AccountForChanges {
            overwrite_states,
            change_detected: AtomicBool::new(false),
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::AccountingForChanges(continuum));
        self.remove_resource::<AccountForChanges<C>>()
            .expect("AccountForChanges had spontaneously disappeared.")
            .change_detected
            .load(core::sync::atomic::Ordering::Relaxed)
    }

    fn clean_up_empty<C: Continuum>(&mut self) {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Clearing empty components and resources...");

        self.insert_resource(CleanUpEmpty {
            continuum: continuum.clone(),
        });
        self.run_schedule(TimeTravelSchedules::CleaningUpEmptyDetecting(
            continuum.clone(),
        ));
        self.run_schedule(TimeTravelSchedules::CleaningUpEmptyPerforming(continuum));
        self.remove_resource::<CleanUpEmpty<C>>();
    }

    fn clean_up_disabled<C: Continuum>(&mut self) {
        let continuum = C::default();

        #[cfg(feature = "logging")]
        trace!("{continuum:?}: Cleaning up entities disabled across timeline...");

        self.run_schedule(TimeTravelSchedules::CleaningUpDisabledEntities(continuum));
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy_ecs::{schedule::ScheduleLabel, world::World};

    use super::super::{
        continuum::Continuum,
        timekeep::{ContinuumTime, Timekeep},
        world_methods::OutOfRecordedRangeError,
    };
    use super::WorldTimeTravel;

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, ScheduleLabel)]
    pub struct TestC;
    impl Continuum for TestC {}

    /// To reduce boilerplate
    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    #[allow(clippy::unwrap_used)] // lmao
    fn adding_works() {
        let mut world = World::new();

        world.insert_into_buffers::<TestC>(secs(1));
        world.insert_into_buffers::<TestC>(secs(2));

        let mut moments = world.resource::<Timekeep<TestC>>().buf.iter();
        assert_eq!(moments.next().unwrap().time, secs(1));

        assert_eq!(moments.next().unwrap().time, secs(2));

        assert!(moments.next().is_none());
    }

    #[test]
    fn time_out_of_range() {
        let mut world = World::new();

        world.insert_into_buffers::<TestC>(secs(1));
        world.insert_into_buffers::<TestC>(secs(2));

        assert_eq!(world.rewind_to::<TestC>(secs(1)), Ok(secs(1)));
        assert_eq!(world.rewind_to::<TestC>(secs(2)), Ok(secs(2)));

        // Rewinding to outside
        assert_eq!(
            world.rewind_to::<TestC>(secs(0)),
            Err(OutOfRecordedRangeError)
        );
        assert_eq!(
            world.rewind_to::<TestC>(secs(5)),
            Err(OutOfRecordedRangeError)
        );

        // Last recorded value should still be true.
        assert_eq!(world.resource::<ContinuumTime<TestC>>().time, secs(2));
    }
}
