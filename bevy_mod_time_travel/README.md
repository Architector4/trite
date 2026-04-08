# bevy\_mod\_time\_travel

Facilities for world state snapshot management and interpolation in Bevy.

This crate allows snapshotting, rewinding, and interpolating Bevy world state across user defined
timelines. This mostly works for components and resources, but it gets a little thorny since Bevy
fundamentally is not built for that.

This also includes an interpolation plugin that works out of the box with `Transform` and can be
easily configured to interpolate any other component. Its code also serves as an example of how to
use the rest of the crate. See module `interpolation` for details.

# Here you get:

- `#![no_std]` support.
- A complete interpolation plugin.
- Tracking, rewinding, and interpolating multiple coexisting continuums of world state separately.
- Tracking, rewinding, and interpolating any arbitrary components and resources.
- Best-effort preservation of change detection state when rewinding, or options for otherwise.
- A method for tracking change in relation to the timeline.
- Quick and easy API to "time travel" a world across a continuum once you're set up.

# Here you don't get:

These things the crate currently does **not** do, but might in some future:
- API for tracking/rewinding/interpolating only specific entities and resources. These can be
  grouped into separate continuums instead if needed
- Automatically inserting time travel systems per timeline (i.e. per component/resource). For this,
  use `WorldTimeTravel::register_timeline` on the relevant timeline.
- Any interaction with `Local` parameters of systems, including message reader cursors. Because of
  this, Bevy messages are not supported.
- Any interpolation that involves more than just two points of input data and a single scalar
  factor; for example Hermite interpolation. It is possible to implement such, but no API is
  provided for this here.
- Handling of entity deletion. In Bevy, deleting an entity is guaranteed to despawn it. This will
  always also destroy timeline components on it, unlink relations, cause relevant events/observers
  to fire, and its exact entity ID is not guaranteed to be available to respawn as is.
- Anything relating to events or observers. I'm not as intimately familiar with those features of
  Bevy, and I'm unsure if there's anything of use I could do with them here either way.
- Any optimization with `PartialEq`, yet.

# "Pick B" behavior

In some cases, continuous interpolation between two states is not possible, for example if one of
them is an absent component but the other is present.

For cases like this, this crate chooses to **pick B**, i.e. the latter state, regardless of if the
interpolation factor is closer to one or the other.

The biggest rationale towards this decision is that it makes the most sense in the use case of basic
interpolation like in the `InterpolationPlugin`. This way, with that plugin, everything that is not
interpolatable matches up with everything that lies outside of what the interpolation continuum
tracks, which also happens to match up with the latest available state of the world in general. Any
other behavior could cause a lot of unnecessary creations/deletions of components, as well as less
predictable behavior.

# Example

```rust
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_mod_time_travel::prelude::*;

// A timeline to time travel along
#[derive(Component, Clone, Debug, Deref, DerefMut, Reflect, Default)]
struct MyTimeline<T: Clone + Send + Sync + 'static> {
    buf: RewindBuffer<T>,
}

// Continuum type to group all instances of the generic timeline together.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Reflect, ScheduleLabel)]
pub struct MyContinuum;

// Implement the necessary traits.
impl Continuum for MyContinuum {}

impl<T: Clone + Send + Sync + 'static> Timeline for MyTimeline<T> {
    type Item = T;
    type Continuum = MyContinuum;
}

let mut world = World::new();

// Put systems for a timeline of a concrete type into the world.
world
    .register_timeline::<MyTimeline<Transform>>()
    .animatable()
    .reflect()
    .register_component();

// Now we can perform all the good stuff.

let ent = world
    .spawn((Transform::default(), MyTimeline::<Transform>::default()))
    .id();

// Store the current state into the timelines.
world.insert_into_buffers::<MyContinuum>(Duration::ZERO);

// Edit the current state.
world.entity_mut(ent).get_components_mut::<&mut Transform>().unwrap().translation.z = 1.0;

// Store the new one too.
world.insert_into_buffers::<MyContinuum>(Duration::from_secs(1));

// Interpolate between the two states.
world.interpolate_to::<MyContinuum>(Duration::from_millis(500)).unwrap();

let new_transform = world.entity(ent).get_components::<&Transform>().unwrap();
// Waow! It interpolate!
assert_eq!(new_transform.translation.z, 0.5);
```


## Crate features

All features are enabled by default except `bevy_transform-libm`:

- `bevy_animation` - enables a convenience method for using
  `bevy_animation::Animatable::interpolate` as the interpolation function when registering a
  timeline. Enables `std` and `bevy_reflect`.
- `bevy_reflect` - enables Bevy's reflection support for all types in the crate.
- `bevy_transform-libm` - internal feature used for development. Enables `libm` feature in
  `bevy_transform` crate if `interpolation_for_transform` feature is enabled. Using this is not
  recommended.
- `interpolation_for_transform` - if enabled, makes `InterpolationPlugin` automatically handle
  interpolation for `Transform` components. Enables `interpolation` feature flag, obviously.
- `interpolation` - enables `interpolation` module.
- `logging` - enables logging in `WorldTimeTravel` methods.
- `std` - enables `std` support. This does not add any new features, but is recommended to enable if
  possible, as it allows for extra optimizations.
