use core::{any::TypeId, marker::PhantomData};

use bevy_ecs::{
    schedule::{
        IntoScheduleConfigs, Schedule, Schedules, SystemCondition,
        common_conditions::{resource_added, resource_changed_or_removed},
    },
    system::IntoSystem,
    world::World,
};

#[cfg(feature = "bevy_animation")]
use bevy_animation::animatable::Animatable;

#[cfg(feature = "bevy_reflect")]
use bevy_reflect::GetTypeRegistration;

use super::continuum::{Timeline, TimelineComponent, TimelineResource};
use super::schedules::*;
use super::{InterpFunc, pick_b_if_nonzero};

/// Error that signifies that the timeline you're trying to register has already been registered.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub struct AlreadyRegisteredError;

impl core::fmt::Display for AlreadyRegisteredError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Timeline was already registered")
    }
}

impl core::fmt::Debug for AlreadyRegisteredError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

impl core::error::Error for AlreadyRegisteredError {
    fn description(&self) -> &'static str {
        "The specified timeline was already registered into this world."
    }
}

/// Represents an unspecified interpolation or reflection function. If you've run into a compiler
/// error with this, you might have missed specifying an interpolation function when registering
/// your timeline.
#[doc(hidden)]
#[non_exhaustive]
pub struct Unspecified;

/// A "builder" for registering a timeline into the world, i.e. setting up the necessary systems
/// and schedules for [`WorldTimeTravel`] methods to work.
///
/// For an example, see the [crate] documentation page.
///
/// [`WorldTimeTravel`]: super::world_methods::WorldTimeTravel
/// [`WorldTimeTravel::register_timeline`]: super::world_methods::WorldTimeTravel::register_timeline
#[must_use = "does not do anything until `.register_component` or `.register_resource` is called."]
pub struct RegisterTimeline<'a, T: Timeline, Interp = Unspecified, Reflect = Unspecified> {
    /// World we're registering into.
    world: &'a mut World,
    /// Timeline type we're registering.
    timeline: PhantomData<T>,
    /// Function for interpolation, or [`Unspecified`] if not yet specified.
    interp_func: Interp,
    /// Function for reflection, or [`Unspecified`] if not yet specified. This field exists
    /// regardless of if the `bevy_reflect` feature is enabled for ease of development.
    reflect_func: Reflect,
}

impl<'a, T: Timeline> RegisterTimeline<'a, T> {
    /// Create a new instance. For ergonomics, you might want to use the equivalent function
    /// [`WorldTimeTravel::register_timeline`] instead.
    ///
    /// [`WorldTimeTravel::register_timeline`]: super::world_methods::WorldTimeTravel::register_timeline
    pub fn new(world: &'a mut World) -> Self {
        Self {
            world,
            timeline: PhantomData,
            interp_func: Unspecified,
            reflect_func: Unspecified,
        }
    }
}

impl<'a, T: Timeline, Reflect> RegisterTimeline<'a, T, Unspecified, Reflect> {
    /// Sets this to use the provided interpolation function when registering.
    pub fn interpolate_with<F: InterpFunc<T::Item>>(
        self,
        interp_func: F,
    ) -> RegisterTimeline<'a, T, F, Reflect> {
        RegisterTimeline {
            world: self.world,
            timeline: self.timeline,
            interp_func,
            reflect_func: self.reflect_func,
        }
    }

    /// Sets this to register without interpolation support.
    ///
    /// Shorthand for [`Self::interpolate_with`] run with [`pick_b_if_nonzero`].
    pub fn without_interpolation(
        self,
    ) -> RegisterTimeline<'a, T, impl InterpFunc<T::Item>, Reflect> {
        self.interpolate_with(pick_b_if_nonzero)
    }

    /// Sets this to register with [`Animatable::interpolate`] as the interpolation function.
    #[cfg(feature = "bevy_animation")]
    pub fn animatable(self) -> RegisterTimeline<'a, T, impl InterpFunc<T::Item>, Reflect>
    where
        T::Item: Animatable,
    {
        self.interpolate_with(Animatable::interpolate)
    }
}

#[cfg(feature = "bevy_reflect")]
impl<
    'a,
    T: Timeline<Item: GetTypeRegistration, Continuum: GetTypeRegistration> + GetTypeRegistration,
    Interp,
> RegisterTimeline<'a, T, Interp, Unspecified>
{
    /// Sets this to also reflect the timeline in the type registry of this world.
    pub fn reflect(self) -> RegisterTimeline<'a, T, Interp, impl Fn(&mut World)> {
        /// Do reflection stuff for a timeline.
        #[cfg(feature = "bevy_reflect")]
        fn reflect_timeline<
            T: super::continuum::Timeline<
                    Item: GetTypeRegistration,
                    Continuum: GetTypeRegistration,
                > + GetTypeRegistration,
        >(
            world: &mut World,
        ) {
            let registry = world.get_resource_or_init::<bevy_ecs::reflect::AppTypeRegistry>();
            let mut registry_write = registry.write();

            // Do the funny.
            registry_write.register::<T>();

            // These appear to be redundant.
            //registry_write.register::<T::Item>();
            //registry_write.register::<super::rewind_buffer::RewindBuffer<T::Item>>();
            //registry_write.register::<super::rewind_buffer::Moment<T::Item>>();

            // While we're here, register the continuum itself too.
            registry_write.register::<T::Continuum>();
        }

        RegisterTimeline {
            world: self.world,
            timeline: self.timeline,
            interp_func: self.interp_func,
            reflect_func: reflect_timeline::<T>,
        }
    }
}

/// Check if a schedule, initialized or not, has a system.
fn has_system(sched: &Schedule, system_id: TypeId) -> bool {
    if let Ok(systems) = sched.systems() {
        for s in systems {
            if s.1.type_id() == system_id {
                return true;
            }
        }
    } else {
        for s in sched.graph().systems.iter() {
            if s.1.type_id() == system_id {
                return true;
            }
        }
    }

    false
}

#[cfg(feature = "bevy_reflect")]
impl<T: TimelineComponent, Interp: InterpFunc<T::Item>, Reflect: Fn(&mut World)>
    RegisterTimeline<'_, T, Interp, Reflect>
{
    /// Register this component timeline with reflection.
    ///
    /// # Panics
    /// Panics if this was already done for this timeline.
    ///
    /// For a version that errors instead, use [`Self::try_register_component`].
    #[inline]
    #[track_caller]
    pub fn register_component(self) {
        if let Err(AlreadyRegisteredError) = self.try_register_component() {
            panic!("{}", AlreadyRegisteredError);
        }
    }

    /// Try to register this component timeline with reflection.
    ///
    /// # Errors
    /// Errors if this was already done for this timeline.
    pub fn try_register_component(self) -> Result<(), AlreadyRegisteredError> {
        (self.reflect_func)(self.world);

        // Now that we ran the reflect function, clear that, and run the non-reflect variant of
        // this.
        let noreflect = RegisterTimeline {
            world: self.world,
            timeline: self.timeline,
            interp_func: self.interp_func,
            reflect_func: Unspecified,
        };

        noreflect.try_register_component()
    }
}

impl<T: TimelineComponent, Interp: InterpFunc<T::Item>>
    RegisterTimeline<'_, T, Interp, Unspecified>
{
    /// Register this component timeline **without** reflection.
    ///
    /// To use reflection, run [`RegisterTimeline::reflect`] first.
    ///
    /// # Panics
    /// Panics if this was already done for this timeline.
    ///
    /// For a version that errors instead, use [`Self::try_register_component`].
    #[inline]
    #[track_caller]
    pub fn register_component(self) {
        if let Err(AlreadyRegisteredError) = self.try_register_component() {
            panic!("{}", AlreadyRegisteredError);
        }
    }

    /// Try to register this component timeline **without** reflection.
    ///
    /// To use reflection, run [`RegisterTimeline::reflect`] first.
    ///
    /// # Errors
    /// Errors if this was already done for this timeline.
    pub fn try_register_component(self) -> Result<(), AlreadyRegisteredError> {
        /// Try to do common stuff for adding a component.
        ///
        /// # Errors
        /// Errors if this was already done for this timeline.
        // TODO: inline this lol
        fn try_register_component_common<T: TimelineComponent>(
            continuum_instance: T::Continuum,
            world: &mut World,
        ) -> Result<(), AlreadyRegisteredError> {
            let mut schedules = world.get_resource_or_init::<Schedules>();
            let rot_buf_sched = schedules.entry(TimeTravelSchedules::RotatingBuffers(
                continuum_instance.clone(),
            ));

            // We want to check if we did this before with this timeline.
            if has_system(
                rot_buf_sched,
                component_rotate_buffers::<T>.system_type_id(),
            ) {
                return Err(AlreadyRegisteredError);
            }

            // We're all good, go forward.
            rot_buf_sched.add_systems(component_rotate_buffers::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::Rewinding(continuum_instance.clone()))
                .add_systems(component_rewind_to::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::DeletingAfter(
                    continuum_instance.clone(),
                ))
                .add_systems(component_delete_after::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::Clearing(continuum_instance.clone()))
                .add_systems(component_clear::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::AccountingForChanges(
                    continuum_instance.clone(),
                ))
                .add_systems(component_account_for_changes::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::CleaningUpEmptyDetecting(
                    continuum_instance.clone(),
                ))
                .add_systems(component_clean_up_detect::<T>.in_set(TimeTravelSystemSet));

            // Clean up performing schedule is a little different from the rest: there's only one of it per
            // a timeline continuum, while the above ones are one per timeline.
            //
            // Therefore, it needs a separate presence check.

            let clean_up_sched = schedules.entry(TimeTravelSchedules::CleaningUpEmptyPerforming(
                continuum_instance.clone(),
            ));
            let has_clean_up_system = has_system(
                clean_up_sched,
                component_clean_up_perform::<T::Continuum>.system_type_id(),
            );
            if !has_clean_up_system {
                clean_up_sched.add_systems(
                    component_clean_up_perform::<T::Continuum>.in_set(TimeTravelSystemSet),
                );
            }

            if TypeId::of::<T::Item>() == TypeId::of::<bevy_ecs::entity_disabling::Disabled>() {
                schedules
                    .entry(TimeTravelSchedules::CleaningUpDisabledEntities(
                        continuum_instance,
                    ))
                    .add_systems(clean_up_disabled::<T>.in_set(TimeTravelSystemSet));
            }

            Ok(())
        }

        let continuum_instance = T::Continuum::default();
        try_register_component_common::<T>(continuum_instance.clone(), self.world)?;

        let interp_func = component_interpolate_to_instantiator::<T>(self.interp_func);

        self.world
            .get_resource_or_init::<Schedules>()
            .entry(TimeTravelSchedules::Interpolating(continuum_instance))
            .add_systems(interp_func.in_set(TimeTravelSystemSet));

        Ok(())
    }
}

#[cfg(feature = "bevy_reflect")]
impl<T: TimelineResource, Interp: InterpFunc<T::Item>, Reflect: Fn(&mut World)>
    RegisterTimeline<'_, T, Interp, Reflect>
{
    /// Register this resource timeline with reflection.
    ///
    /// # Panics
    /// Panics if this was already done for this timeline.
    ///
    /// For a version that errors instead, use [`Self::try_register_resource`].
    #[inline]
    #[track_caller]
    pub fn register_resource(self) {
        if let Err(AlreadyRegisteredError) = self.try_register_resource() {
            panic!("{}", AlreadyRegisteredError);
        }
    }

    /// Try to register this resource timeline with reflection.
    ///
    /// # Errors
    /// Errors if this was already done for this timeline.
    pub fn try_register_resource(self) -> Result<(), AlreadyRegisteredError> {
        (self.reflect_func)(self.world);

        let noreflect = RegisterTimeline {
            world: self.world,
            timeline: self.timeline,
            interp_func: self.interp_func,
            reflect_func: Unspecified,
        };

        noreflect.try_register_resource()
    }
}

impl<T: TimelineResource, Interp: InterpFunc<T::Item>>
    RegisterTimeline<'_, T, Interp, Unspecified>
{
    /// Register this resource timeline **without** reflection.
    ///
    /// To use reflection, run [`RegisterTimeline::reflect`] first.
    ///
    /// # Panics
    /// Panics if this was already done for this timeline.
    ///
    /// For a version that errors instead, use [`Self::try_register_resource`].
    #[inline]
    #[track_caller]
    pub fn register_resource(self) {
        if let Err(AlreadyRegisteredError) = self.try_register_resource() {
            panic!("{}", AlreadyRegisteredError);
        }
    }

    /// Try to register this resource timeline **without** reflection.
    ///
    /// To use reflection, run [`Self::reflect`] first.
    ///
    /// # Errors
    /// Errors if this was already done for this timeline.
    pub fn try_register_resource(self) -> Result<(), AlreadyRegisteredError> {
        /// Try to do common stuff for adding a resource.
        ///
        /// # Errors
        /// Errors if this was already done for this timeline.
        // TODO: inline this lol
        fn register_resource_common<T: TimelineResource>(
            continuum_instance: T::Continuum,
            world: &mut World,
        ) -> Result<(), AlreadyRegisteredError> {
            let mut schedules = world.get_resource_or_init::<Schedules>();
            let rot_buf_sched = schedules.entry(TimeTravelSchedules::RotatingBuffers(
                continuum_instance.clone(),
            ));

            // We want to check if we did this before with this timeline.
            if has_system(rot_buf_sched, resource_rotate_buffers::<T>.system_type_id()) {
                return Err(AlreadyRegisteredError);
            }

            // We're all good, go forward.
            rot_buf_sched.add_systems(resource_rotate_buffers::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::Rewinding(continuum_instance.clone()))
                .add_systems(resource_rewind_to::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::DeletingAfter(
                    continuum_instance.clone(),
                ))
                .add_systems(resource_delete_after::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::Clearing(continuum_instance.clone()))
                .add_systems(resource_clear::<T>.in_set(TimeTravelSystemSet));

            schedules
                .entry(TimeTravelSchedules::AccountingForChanges(
                    continuum_instance.clone(),
                ))
                .add_systems(
                    resource_account_for_changes_if_changed::<T>
                        .run_if(
                            resource_added::<T::Item>.or(resource_changed_or_removed::<T::Item>),
                        )
                        .in_set(TimeTravelSystemSet),
                );

            // Don't have much detecting to do here lol

            schedules
                .entry(TimeTravelSchedules::CleaningUpEmptyPerforming(
                    continuum_instance,
                ))
                .add_systems(resource_clean_up_perform::<T>.in_set(TimeTravelSystemSet));

            Ok(())
        }

        let continuum_instance = T::Continuum::default();
        register_resource_common::<T>(continuum_instance.clone(), self.world)?;

        let interp_func = resource_interpolate_to_instantiator::<T>(self.interp_func);

        self.world
            .get_resource_or_init::<Schedules>()
            .entry(TimeTravelSchedules::Interpolating(continuum_instance))
            .add_systems(interp_func.in_set(TimeTravelSystemSet));

        Ok(())
    }
}
