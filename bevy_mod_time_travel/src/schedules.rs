// To deal with clippy's passionate fury at Bevy systems...
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::type_complexity)]

use alloc::borrow::ToOwned;
use bevy_ecs::{
    change_detection::DetectChangesMut,
    component::Component,
    entity::Entity,
    entity_disabling::Disabled,
    lifecycle::RemovedComponents,
    query::{Added, Allow, AnyOf, Changed, Or, With, Without},
    resource::Resource,
    schedule::{ScheduleLabel, SystemSet},
    system::{Commands, IntoSystem, Query, Res, ResMut, System},
    world::{Ref, World},
};
use core::{sync::atomic::AtomicBool, time::Duration};

use crate::OutOfRecordedRangeError;

use super::InterpFunc;
use super::continuum::{Continuum, TimelineComponent, TimelineResource};
use super::rewind_buffer::{ChangeDetectionState, Moment};

#[cfg(feature = "bevy_reflect")]
use bevy_ecs::reflect::{ReflectComponent, ReflectResource};
#[cfg(feature = "std")]
use bevy_ecs::system::ParallelCommands;
#[cfg(feature = "bevy_reflect")]
use bevy_reflect::Reflect;

/// Policy determining how to rewind change detection state when rewinding an item.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub enum TickRestorePolicy {
    /// Restore the same `last_changed` and `last_added` ticks on the items.
    ///
    /// This should effectively
    /// leave it as "not changed" from perspective of all systems that had observed this item with
    /// this state last time, unless change tick rollover has happened between then and now.
    ///
    /// Note that this likely *guarantees* that the item is perceived as not changed/not created if
    /// such systems were already run, since they will have observed new change detection ticks
    /// already.
    RestoreOldTicks,
    /// Retrigger `last_changed` and `last_added` state based on what was observed at the time the
    /// item was saved.
    ///
    /// If the system doing the saving has detected that the item was changed since its last run,
    /// rewinding with this policy will mark the item as changed anew. Otherwise, the old
    /// `last_changed` tick value is restored.
    ///
    /// The above process is applied the same way with `last_created` tick.
    Retrigger,
    /// Either applies [`Self::RestoreOldTicks`] if rewinding to the latest saved item, or
    /// [`Self::Retrigger`] otherwise.
    ///
    /// Generally preserves rewound state the best, but might be not what you want, especially when
    /// "latest saved" item in the current timeline is actually a state from the past relative to
    /// what was run since then.
    #[default]
    Adaptive,
    /// Just always mark the items as changed/created based on the state we are rewinding to
    /// compared to current state of the item.
    MarkAllChanged,
    /// Do not touch change ticks at all and bypass change detection when rewinding. This will
    /// still mark the item as created if it was created.
    Bypass,
}

impl TickRestorePolicy {
    /// Apply `state` to `item` with this policy, with a boolean flag indicating if this we are
    /// rewinding to the latest item from the buffer.
    pub fn apply_ticks<T: DetectChangesMut>(
        self,
        item: &mut T,
        state: &ChangeDetectionState,
        is_latest_in_buffer: bool,
    ) {
        match self {
            Self::RestoreOldTicks => state.apply_ticks(item),
            Self::Retrigger => state.trigger_same_change_detection(item),
            Self::Adaptive => {
                if is_latest_in_buffer {
                    state.apply_ticks(item);
                } else {
                    state.trigger_same_change_detection(item);
                }
            }
            Self::MarkAllChanged => {
                item.set_changed();
            }
            Self::Bypass => {}
        }
    }

    /// Returns true if this policy acts differently if used to rewind change ticks to a latest
    /// item in the buffer or not. Used for optimization.
    #[must_use]
    pub fn do_we_care_about_latest_in_buffer(self) -> bool {
        matches!(self, Self::Adaptive)
    }
}

/// Policy determining what to do when trying to rewind or interpolate to a time that is out of
/// range of a timeline, but if it is in range of the continuum.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub enum OutOfTimelineRangePolicy {
    /// Do nothing and leave the currently present state as is.
    DoNothing,
    /// Assume the state of the item is [`None`] whenever it's not in the timeline range. Default.
    #[default]
    AssumeNone,
}

/// Labels for schedules used to perform time travel across a continuum.
///
/// These schedules should normally be executed by methods in [`WorldTimeTravel`].
///
/// [`WorldTimeTravel`]: super::world_methods::WorldTimeTravel
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, ScheduleLabel)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub enum TimeTravelSchedules<C: Continuum + Send + Sync + core::fmt::Debug> {
    /// Rewinding to a fixed recorded point in time.
    ///
    /// When this is run, [`InterpolateTo<C>`] should be present to describe where to.
    Rewinding(C),
    /// Producing a whole new world state by interpolating recorded points in time.
    ///
    /// When this is run, [`InterpolateTo<C>`] should be present to describe where to.
    Interpolating(C),
    /// Rotating buffers, i.e. clearing older recorded state and recording the current one.
    ///
    /// When this is run, [`RotateBuffers<C>`] should be present to describe parameters.
    RotatingBuffers(C),
    /// Deleting all recorded state for everything after a specific point in time.
    ///
    /// When this is run, [`DeleteAfter<C>`] should be present to describe the point in time.
    DeletingAfter(C),
    /// Deleting all stored state.
    Clearing(C),
    /// Accounting for changes. See [`WorldTimeTravel::account_for_changes<C>`] for more
    /// information.
    ///
    /// When this is run, [`AccountForChanges<C>`] should be present to describe parameters.
    ///
    /// [`WorldTimeTravel::account_for_changes<C>`]:
    /// super::world_methods::WorldTimeTravel::account_for_changes<C>
    AccountingForChanges(C),
    /// Detecting items to clean up across the timeline. See
    /// [`WorldTimeTravel::clean_up_empty<C>`] for more information.
    ///
    /// After this, the schedule [`TimeTravelSchedules::CleaningUpEmptyPerforming`] should be run.
    ///
    /// When this is run, [`CleanUpEmpty<C>`] should be present to carry the continuum.
    ///
    /// [`WorldTimeTravel::clean_up_empty<C>`]:
    /// super::world_methods::WorldTimeTravel::clean_up_empty<C>
    CleaningUpEmptyDetecting(C),
    /// Cleaning up items across the timeline. See [`WorldTimeTravel::clean_up_empty<C>`] for more
    /// information.
    ///
    /// Before this, the schedule [`TimeTravelSchedules::CleaningUpEmptyDetecting`] should be run.
    ///
    /// [`WorldTimeTravel::clean_up_empty<C>`]:
    /// super::world_methods::WorldTimeTravel::clean_up_empty<C>
    CleaningUpEmptyPerforming(C),
    /// Cleaning up disabled entities across the timeline. See
    /// [`WorldTimeTravel::clean_up_disabled<C>`] for more information.
    ///
    /// [`WorldTimeTravel::clean_up_disabled<C>`]:
    /// super::world_methods::WorldTimeTravel::clean_up_disabled<C>
    CleaningUpDisabledEntities(C),
}

/// A set for systems that do the actual functionality in [`TimeTravelSchedules`]. Useful to target
/// with system params for extra functionality.
#[derive(Clone, PartialEq, Eq, Debug, Hash, SystemSet)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct TimeTravelSystemSet;

// Parameter resources for the systems...

/// Parameters for [`TimeTravelSchedules::Rewinding`].
#[derive(Clone, PartialEq, Eq, Debug, Hash, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct RewindTo<C: Continuum> {
    pub to: Duration,
    pub tick_restore_policy: TickRestorePolicy,
    pub out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    pub continuum: C,
}

/// Parameters for [`TimeTravelSchedules::Interpolating`].
#[derive(Clone, PartialEq, Eq, Debug, Hash, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct InterpolateTo<C: Continuum> {
    pub to: Duration,
    pub out_of_timeline_range_policy: OutOfTimelineRangePolicy,
    pub continuum: C,
}

/// Parameters for [`TimeTravelSchedules::RotatingBuffers`].
#[derive(Clone, PartialEq, Eq, Debug, Hash, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct RotateBuffers<C: Continuum> {
    /// All moments with time before this must be deleted.
    pub delete_before: Duration,
    /// Record new moments with this time.
    pub current_time: Duration,
    /// Timeline continuum.
    pub continuum: C,
}

/// Parameters for [`TimeTravelSchedules::DeletingAfter`].
#[derive(Clone, PartialEq, Eq, Debug, Hash, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct DeleteAfter<C: Continuum> {
    /// All moments with time after this must be deleted.
    pub delete_after: Duration,
    /// Timeline continuum.
    pub continuum: C,
}

/// Parameters for [`TimeTravelSchedules::AccountingForChanges`].
#[derive(Debug, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct AccountForChanges<C: Continuum> {
    /// How many states, from the end, to overwrite with latest.
    pub overwrite_states: usize,
    /// Set to true if any change was detected.
    pub change_detected: AtomicBool,
    /// Timeline continuum.
    pub continuum: C,
}

/// Parameters for [`TimeTravelSchedules::CleaningUpEmptyDetecting`].
#[derive(Debug, Resource)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Resource))]
pub struct CleanUpEmpty<C: Continuum> {
    /// Timeline continuum.
    pub continuum: C,
}

/// A component added to entities during [`TimeTravelSchedules::CleaningUpEmptyDetecting`] and
/// checked for and cleaned up during [`TimeTravelSchedules::CleaningUpEmptyPerforming`]. Indicates
/// that the entity this is on has timelines with no present moments.
#[derive(Component, Default, Debug)]
#[component(storage = "SparseSet")]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Component))]
pub struct HasEmptyBuffers<C: Continuum>(C);

/// A component added to entities during [`TimeTravelSchedules::CleaningUpEmptyDetecting`] and
/// checked for and cleaned up during [`TimeTravelSchedules::CleaningUpEmptyPerforming`]. Indicates
/// that the entity this is on has timelines with moments present.
#[derive(Component, Default, Debug)]
#[component(storage = "SparseSet")]
pub struct HasNonEmptyBuffers<C: Continuum>(C);

// Some utility...

/// Little helper to use commands.
fn do_in_cmd(
    #[cfg(feature = "std")] commands: &ParallelCommands,
    #[cfg(not(feature = "std"))] commands: &spin::Mutex<Commands>,
    func: impl FnOnce(&mut Commands),
) {
    #[cfg(feature = "std")]
    commands.command_scope(|mut x| func(&mut x));
    #[cfg(not(feature = "std"))]
    func(&mut commands.lock());
}

// Here come the systems.

/// Instantiator for a system run in [`TimeTravelSchedules::Interpolating`] with a custom provided function.
pub fn component_interpolate_to_instantiator<T: TimelineComponent>(
    func: impl InterpFunc<T::Item>,
) -> impl System<In = (), Out = ()> {
    let sys = move |interpolate_to: Res<InterpolateTo<T::Continuum>>,
                    mut items: Query<(Entity, &T, Option<&mut T::Item>), Allow<Disabled>>,
                    #[cfg(feature = "std")] commands: ParallelCommands,
                    #[cfg(not(feature = "std"))] commands: Commands| {
        #[cfg(not(feature = "std"))]
        let commands = spin::Mutex::new(commands);

        items.par_iter_mut().for_each(|(entity, buf, item)| {
            let should_be = buf.interpolate_with_function(func, interpolate_to.to);

            if let Err(OutOfRecordedRangeError) = should_be
                && interpolate_to.out_of_timeline_range_policy
                    == OutOfTimelineRangePolicy::DoNothing
            {
                // Do as it says.
                return;
            }

            let should_be = should_be.ok().flatten();

            let match_up = |commands: &mut Commands| match (item, &should_be) {
                (None, None) => (),
                (None, Some(i)) => {
                    commands.entity(entity).insert(i.clone());
                }
                (Some(_), None) => {
                    commands.entity(entity).remove::<T::Item>();
                }
                (Some(mut i), Some(new)) => {
                    new.clone_into(&mut *i);
                }
            };

            #[cfg(feature = "std")]
            commands.command_scope(|mut x| match_up(&mut x));
            #[cfg(not(feature = "std"))]
            match_up(&mut commands.lock());
        });
    };

    IntoSystem::into_system(sys)
}

/// System run in [`TimeTravelSchedules::Rewinding`].
#[allow(clippy::missing_panics_doc)] // The "expect" call below will never panic.
pub fn component_rewind_to<T: TimelineComponent>(
    rewind_to: Res<RewindTo<T::Continuum>>,
    mut items: Query<(Entity, &T, Option<&mut T::Item>), Allow<Disabled>>,
    #[cfg(feature = "std")] commands: ParallelCommands,
    #[cfg(not(feature = "std"))] commands: Commands,
) {
    #[cfg(not(feature = "std"))]
    let commands = spin::Mutex::new(commands);

    items.par_iter_mut().for_each(|(entity, buf, item)| {
        let should_be = buf.rewind_to(rewind_to.to);

        if let Err(OutOfRecordedRangeError) = should_be
            && rewind_to.out_of_timeline_range_policy == OutOfTimelineRangePolicy::DoNothing
        {
            // Do as it says.
            return;
        }

        let (should_be, is_latest_in_buffer) = if let Ok(should_be) = should_be {
            let is_latest_in_buffer =
                if rewind_to
                    .tick_restore_policy
                    .do_we_care_about_latest_in_buffer()
                {
                    buf.last_moment()
                .expect("the fact that pick_b returned a moment guarantees the buffer has moments")
                .time
                == should_be.time
                } else {
                    false // If we don't care, then it's obviously false.
                };
            (&should_be.item, is_latest_in_buffer)
        } else {
            // If the target time is out of range, assume `None` instead.
            (&None, false)
        };

        match (item, should_be) {
            (None, None) => (),
            (None, Some(i)) => {
                let (ticks, item) = i.clone();
                let policy = rewind_to.tick_restore_policy;

                do_in_cmd(&commands, |commands| {
                    commands.queue(move |world: &mut World| {
                        let mut entity = world.entity_mut(entity);

                        // Will invariably run `set_created`
                        entity.insert(item);
                        let mut component = entity
                            .get_mut::<T::Item>()
                            .expect("Component was just inserted");

                        policy.apply_ticks(&mut component, &ticks, is_latest_in_buffer);
                    });
                });
            }
            (Some(_), None) => {
                do_in_cmd(&commands, |commands| {
                    commands.entity(entity).remove::<T::Item>();
                });
            }
            (Some(mut current), Some((ticks, item))) => {
                item.clone_into(current.bypass_change_detection());

                rewind_to
                    .tick_restore_policy
                    .apply_ticks(&mut current, ticks, is_latest_in_buffer);
            }
        }
    });
}

/// System run in [`TimeTravelSchedules::RotatingBuffers`].
pub fn component_rotate_buffers<T: TimelineComponent>(
    rotate_buffers: Res<RotateBuffers<T::Continuum>>,
    mut items: Query<(&mut T, Option<Ref<T::Item>>), Allow<Disabled>>,
) {
    items.par_iter_mut().for_each(|(mut buf, item)| {
        let moment = Moment {
            time: rotate_buffers.current_time,
            snap_to: buf.discontinuity(),
            item: item.map(|x| (ChangeDetectionState::from(&x), x.clone())),
        };

        buf.reset_discontinuity();
        buf.rotate(rotate_buffers.delete_before, moment);
    });
}

/// System run in [`TimeTravelSchedules::DeletingAfter`].
pub fn component_delete_after<T: TimelineComponent>(
    delete_after: Res<DeleteAfter<T::Continuum>>,
    mut items: Query<&mut T, Allow<Disabled>>,
) {
    items.par_iter_mut().for_each(|mut buf| {
        buf.delete_after(delete_after.delete_after);
    });
}

/// System run in [`TimeTravelSchedules::Clearing`].
pub fn component_clear<T: TimelineComponent>(mut items: Query<&mut T, Allow<Disabled>>) {
    items.par_iter_mut().for_each(|mut buf| {
        buf.clear();
    });
}

/// System run in [`TimeTravelSchedules::AccountingForChanges`].
#[allow(clippy::type_complexity)] // shuddup
pub fn component_account_for_changes<T: TimelineComponent>(
    account_for_changes: Res<AccountForChanges<T::Continuum>>,
    mut items: Query<(Entity, &mut T, Option<Ref<T::Item>>), Allow<Disabled>>,
    adds_changes: Query<
        (),
        (
            Or<(Added<T::Item>, Changed<T::Item>)>,
            With<T>,
            Allow<Disabled>,
        ),
    >,
    mut deletions: RemovedComponents<T::Item>,
) {
    if account_for_changes.overwrite_states > 0 {
        items.par_iter_mut().for_each(|(entity, mut buf, item)| {
            if adds_changes.contains(entity) {
                account_for_changes
                    .change_detected
                    .store(true, core::sync::atomic::Ordering::Relaxed);
                buf.enforce_for_n_last(
                    account_for_changes.overwrite_states,
                    item.as_ref()
                        .map(|x| (ChangeDetectionState::from(x), x.as_ref())),
                );
            }
        });
    } else if !adds_changes.is_empty() {
        // The query has something. Hence, set to true.
        account_for_changes
            .change_detected
            .store(true, core::sync::atomic::Ordering::Relaxed);
    }

    // Also account for deletions.
    for d in deletions.read() {
        // Note: not all deletions are from entities with the timeline.
        if let Ok((_, mut buf, item)) = items.get_mut(d) {
            account_for_changes
                .change_detected
                .store(true, core::sync::atomic::Ordering::Relaxed);

            if account_for_changes.overwrite_states > 0 {
                // It might still exist or whatnot else... Probably worth doing the enforcing.
                buf.enforce_for_n_last(
                    account_for_changes.overwrite_states,
                    item.as_ref()
                        .map(|x| (ChangeDetectionState::from(x), x.as_ref())),
                );
            }
        }
    }
}

/// System run in [`TimeTravelSchedules::CleaningUpEmptyDetecting`].
pub fn component_clean_up_detect<T: TimelineComponent>(
    clean_up: Res<CleanUpEmpty<T::Continuum>>,
    items: Query<(Entity, &T), (Without<HasNonEmptyBuffers<T::Continuum>>, Allow<Disabled>)>,
    #[cfg(feature = "std")] commands: ParallelCommands,
    #[cfg(not(feature = "std"))] commands: Commands,
) {
    #[cfg(not(feature = "std"))]
    let commands = spin::Mutex::new(commands);
    items.par_iter().for_each(|(entity, buf)| {
        if buf.iter().all(|x| x.item.is_none()) {
            do_in_cmd(&commands, |c| {
                c.entity(entity)
                    .insert(HasEmptyBuffers(clean_up.continuum.clone()));
            });
        } else {
            do_in_cmd(&commands, |c| {
                c.entity(entity)
                    .insert(HasNonEmptyBuffers(clean_up.continuum.clone()));
            });
        }
    });
}

/// System run in [`TimeTravelSchedules::CleaningUpEmptyPerforming`].
pub fn component_clean_up_perform<C: Continuum>(
    items: Query<(Entity, AnyOf<(&HasEmptyBuffers<C>, &HasNonEmptyBuffers<C>)>), Allow<Disabled>>,
    #[cfg(feature = "std")] commands: ParallelCommands,
    #[cfg(not(feature = "std"))] commands: Commands,
) {
    #[cfg(not(feature = "std"))]
    let commands = spin::Mutex::new(commands);

    items
        .par_iter()
        .for_each(|(entity, (has_empty, has_non_empty))| {
            match (has_empty, has_non_empty) {
                (None, None) => {
                    unreachable!("AnyOf guarantees that either of the two types must be present")
                }
                (_, Some(_)) => {
                    // Has non-empty buffers. Just clean up the components.
                    if has_empty.is_some() {
                        do_in_cmd(&commands, |c| {
                            c.entity(entity).remove::<HasEmptyBuffers<C>>();
                        });
                    }
                    do_in_cmd(&commands, |c| {
                        c.entity(entity).remove::<HasNonEmptyBuffers<C>>();
                    });
                }
                (Some(_), None) => {
                    // Has only empty buffers. Goodbye!
                    do_in_cmd(&commands, |c| {
                        c.entity(entity).despawn();
                    });
                }
            }
        });
}

/// System run in [`TimeTravelSchedules::CleaningUpDisabledEntities`]. Should really only be
/// instanced with a timeline with `Item = Disabled`, but is more generic to workaround a Rust
/// limitation.
pub fn clean_up_disabled<T: TimelineComponent>(
    items: Query<(Entity, &T), (Allow<Disabled>, Allow<T::Item>)>,
    #[cfg(feature = "std")] commands: ParallelCommands,
    #[cfg(not(feature = "std"))] commands: Commands,
) {
    #[cfg(not(feature = "std"))]
    let commands = spin::Mutex::new(commands);

    items.par_iter().for_each(|(entity, buf)| {
        if buf.iter().all(|x| x.item.is_some()) {
            do_in_cmd(&commands, |c| c.entity(entity).despawn());
        }
    });
}

/// System run in [`TimeTravelSchedules::DeletingAfter`].
pub fn resource_delete_after<T: TimelineResource>(
    delete_after: Res<DeleteAfter<T::Continuum>>,
    buf: Option<ResMut<T>>,
) {
    if let Some(mut buf) = buf {
        buf.delete_after(delete_after.delete_after);
    }
}

/// System run in [`TimeTravelSchedules::Clearing`].
pub fn resource_clear<T: TimelineResource>(buf: Option<ResMut<T>>) {
    if let Some(mut buf) = buf {
        buf.clear();
    }
}

/// Instantiator for a system run in [`TimeTravelSchedules::Interpolating`] with a custom provided function.
///
/// [`pick_b_if_nonzero`]: super::pick_b_if_nonzero
pub fn resource_interpolate_to_instantiator<T: TimelineResource>(
    func: impl InterpFunc<T::Item>,
) -> impl System<In = (), Out = ()> {
    let sys = move |interpolate_to: Res<InterpolateTo<T::Continuum>>,
                    buf: Option<Res<T>>,
                    item: Option<ResMut<T::Item>>,
                    #[cfg(feature = "std")] commands: ParallelCommands,
                    #[cfg(not(feature = "std"))] commands: Commands| {
        #[cfg(not(feature = "std"))]
        let commands = spin::Mutex::new(commands);

        let Some(buf) = buf else {
            return;
        };

        let should_be = buf.interpolate_with_function(func, interpolate_to.to);

        if let Err(OutOfRecordedRangeError) = should_be
            && interpolate_to.out_of_timeline_range_policy == OutOfTimelineRangePolicy::DoNothing
        {
            // Do as it says.
            return;
        }

        let should_be = should_be.ok().flatten();

        let match_up = |commands: &mut Commands| match (item, &should_be) {
            (None, None) => (),
            (None, Some(i)) => {
                commands.insert_resource(i.clone());
            }
            (Some(_), None) => {
                commands.remove_resource::<T::Item>();
            }
            (Some(mut i), Some(new)) => {
                new.clone_into(&mut *i);
            }
        };
        #[cfg(feature = "std")]
        commands.command_scope(|mut x| match_up(&mut x));
        #[cfg(not(feature = "std"))]
        match_up(&mut commands.lock());
    };

    IntoSystem::into_system(sys)
}

/// System run in [`TimeTravelSchedules::Rewinding`].
#[allow(clippy::missing_panics_doc)] // The "expect" call below will never panic.
pub fn resource_rewind_to<T: TimelineResource>(
    rewind_to: Res<RewindTo<T::Continuum>>,
    buf: Option<Res<T>>,
    item: Option<ResMut<T::Item>>,
    mut commands: Commands,
) {
    let Some(buf) = buf else {
        return;
    };

    let should_be = buf.rewind_to(rewind_to.to);

    if let Err(OutOfRecordedRangeError) = should_be
        && rewind_to.out_of_timeline_range_policy == OutOfTimelineRangePolicy::DoNothing
    {
        // Do as it says.
        return;
    }

    let (should_be, is_latest_in_buffer) = if let Ok(should_be) = should_be {
        let is_latest_in_buffer = if rewind_to
            .tick_restore_policy
            .do_we_care_about_latest_in_buffer()
        {
            buf.last_moment()
                .expect("the fact that pick_b returned a moment guarantees the buffer has moments")
                .time
                == should_be.time
        } else {
            false // If we don't care, then it's obviously false.
        };
        (&should_be.item, is_latest_in_buffer)
    } else {
        // If the target time is out of range, assume `None` instead.
        (&None, false)
    };

    match (item, should_be) {
        (None, None) => (),
        (None, Some(i)) => {
            let (ticks, item) = i.clone();
            let policy = rewind_to.tick_restore_policy;

            commands.queue(move |world: &mut World| {
                // Will invariably run `set_created`
                world.insert_resource(item);

                let mut resource = world.resource_mut::<T::Item>();

                policy.apply_ticks(&mut resource, &ticks, is_latest_in_buffer);
            });
        }
        (Some(_), None) => {
            commands.remove_resource::<T::Item>();
        }
        (Some(mut current), Some((ticks, item))) => {
            item.clone_into(current.bypass_change_detection());

            rewind_to
                .tick_restore_policy
                .apply_ticks(&mut current, ticks, is_latest_in_buffer);
        }
    }
}

/// System run in [`TimeTravelSchedules::RotatingBuffers`].
pub fn resource_rotate_buffers<T: TimelineResource>(
    rotate_buffers: Res<RotateBuffers<T::Continuum>>,
    buf: Option<ResMut<T>>,
    item: Option<Res<T::Item>>,
) {
    let Some(mut buf) = buf else {
        return;
    };

    let moment = Moment {
        time: rotate_buffers.current_time,
        snap_to: buf.discontinuity(),
        item: item.map(|x| (ChangeDetectionState::from(&x), x.clone())),
    };

    buf.reset_discontinuity();
    buf.rotate(rotate_buffers.delete_before, moment);
}

/// System run in [`TimeTravelSchedules::AccountingForChanges`]. Assumes the resource has indeed changed.
///
/// Use [`resource_added::<T>`]`.or(`[`resource_changed_or_removed::<T>`]`)` as a resource condition for this system.
///
/// [`resource_added::<T>`]: bevy_ecs::schedule::common_conditions::resource_added
/// [`resource_changed_or_removed::<T>`]: bevy_ecs::schedule::common_conditions::resource_changed_or_removed
pub fn resource_account_for_changes_if_changed<T: TimelineResource>(
    account_for_changes: Res<AccountForChanges<T::Continuum>>,
    buf: Option<ResMut<T>>,
    item: Option<Res<T::Item>>,
) {
    if let Some(mut buf) = buf {
        account_for_changes
            .change_detected
            .store(true, core::sync::atomic::Ordering::Relaxed);
        buf.enforce_for_n_last(
            account_for_changes.overwrite_states,
            item.as_ref()
                .map(|x| (ChangeDetectionState::from(x), x.as_ref())),
        );
    }
}

/// System run in [`TimeTravelSchedules::CleaningUpEmptyPerforming`].
pub fn resource_clean_up_perform<C: TimelineResource>(
    mut commands: Commands,
    buf: Option<ResMut<C>>,
) {
    if let Some(buf) = buf
        && !buf.has_present_items()
    {
        commands.remove_resource::<C>();
        commands.remove_resource::<C::Item>();
    }
}
