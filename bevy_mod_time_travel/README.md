# bevy\_mod\_time\_travel

Facilities for world state snapshot management and interpolation in Bevy.

This crate allows snapshotting, rewinding, and interpolating Bevy world state across user defined
timelines. This mostly works for components and resources, but it gets a little thorny since Bevy
fundamentally is not built for that.

This also includes an interpolation plugin that works out of the box with `Transform` and can be
easily configured to interpolate any other component. Its code also serves as an example of how to
use the rest of the crate. See module `interpolation` for details.

For a general overview of the primary API surface of this crate, see file `examples/basics.rs`.

# Here you get:

- `#![no_std]` support.
- A complete interpolation plugin.
- Tracking, rewinding, and interpolating multiple coexisting continuums of world state separately.
- Tracking, rewinding, and interpolating any arbitrary components and resources.
- Best-effort preservation of change detection state when rewinding, or options for otherwise.
- A method for detecting change in the world state tracked by a continuum.
- Quick and easy API to "time travel" a world across a continuum once you're set up.

# Here you don't get:

These things the crate currently does **not** do, but might in some future:
- API for tracking/rewinding/interpolating only specific entities and resources. These can be
  grouped into separate continuums instead if needed.
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

In some cases, continuous interpolation between two states is not possible. For example, it's
possible that such interpolation has to be performed between a state representing an absence of a
component, and one that represents its presence.

For cases like this, this crate chooses to **pick B**, i.e. the latter state, regardless of if the
interpolation factor is closer to one or the other.

The biggest rationale towards this decision is that it makes the most sense in the use case of basic
interpolation like in the `InterpolationPlugin`. This way, with that plugin, everything that is not
interpolatable matches up with everything that lies outside of what the interpolation continuum
tracks, which also happens to match up with the latest available state of the world in general. Any
other behavior could cause a lot of unnecessary creations/deletions of components, as well as less
predictable behavior.

## Feature flags

All feature flags are enabled by default except `bevy_transform-libm`:

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

