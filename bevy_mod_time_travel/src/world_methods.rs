use bevy_ecs::world::World;

use crate::world_continuum_interface::WorldContinuumInterface;

use super::continuum::{Continuum, Timeline};
use super::registration::RegisterTimeline;

/// Methods to quickly manage world state. Implemented on [`World`].
pub trait WorldTimeTravel {
    /// Create a builder for registering a timeline.
    ///
    /// For an example, see the [crate] documentation page.
    fn register_timeline<T: Timeline>(&mut self) -> RegisterTimeline<'_, T>;

    /// Create an interface for managing a continuum.
    ///
    /// For an example, see the [crate] documentation page.
    fn continuum<C: Continuum>(&mut self) -> WorldContinuumInterface<'_, C>;
}

impl WorldTimeTravel for World {
    fn register_timeline<T: Timeline>(&mut self) -> RegisterTimeline<'_, T> {
        RegisterTimeline::<'_, T>::new(self)
    }

    fn continuum<C: Continuum>(&mut self) -> WorldContinuumInterface<'_, C> {
        WorldContinuumInterface::<'_, C>::new(self)
    }
}
