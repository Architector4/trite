use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_ecs::component::Mutable;
use bevy_mod_time_travel::prelude::*;

/// A timeline to time travel a singular component along.
///
/// It's a component in and of itself as well. An instance of a `MyTimeline<T>` component on an
/// entity is responsible for tracking and rewinding the component `T` on the entity.
#[derive(Component, Clone, Debug, Deref, DerefMut, Reflect, Default)]
struct MyTimelineComp<T: Component<Mutability = Mutable> + Clone + Send + Sync + 'static> {
    buf: RewindBuffer<T>,
}

/// A timeline to time travel a singular resource along.
///
/// It's a resource in and of itself as well. An instance of a `MyTimeline<T>` resource in a world
/// is responsible for tracking and rewinding the component `T` in that world.
#[derive(Resource, Clone, Debug, Deref, DerefMut, Reflect, Default)]
struct MyTimelineRes<T: Resource<Mutability = Mutable> + Clone + Send + Sync + 'static> {
    buf: RewindBuffer<T>,
}

/// Continuum type to group all instances of the generic timeline components/resources together.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Reflect, ScheduleLabel)]
pub struct MyContinuum;

// Implement the necessary traits.

impl Continuum for MyContinuum {}

impl<T: Component<Mutability = Mutable> + Clone + Send + Sync + 'static> Timeline
    for MyTimelineComp<T>
{
    type Item = T;
    type Continuum = MyContinuum;
}

impl<T: Resource<Mutability = Mutable> + Clone + Send + Sync + 'static> Timeline
    for MyTimelineRes<T>
{
    type Item = T;
    type Continuum = MyContinuum;
}

// Example resource. The API for them is slightly different in this crate compared to components.
#[derive(Resource, Clone, Default)]
struct SomeResource(f32);

fn main() {
    // Let's go on a bizzare adventure.
    let mut world = World::new();

    // Register the timelines into the world. This creates the correct schedules and systems that will
    // perform the time travel.
    world
        .register_timeline::<MyTimelineComp<Transform>>()
        .animatable()
        .reflect()
        .register_component();
    world
        .register_timeline::<MyTimelineRes<SomeResource>>()
        // Standard linear interpolation formula
        .interpolate_with(|a, b, f| SomeResource(a.0 + (b.0 - a.0) * f))
        .register_resource();

    // Now we can perform all the good stuff.

    let ent = world
        .spawn((Transform::default(), MyTimelineComp::<Transform>::default()))
        .id();

    world.insert_resource(SomeResource(0.0));
    // For resources, the timeline is stored as a separate resource.
    world.insert_resource(MyTimelineRes::<SomeResource>::default());

    assert!(world.get_resource::<SomeResource>().is_some());

    // Store the current state into the timelines. This stores the state of ALL tracked components
    // and resources with a corresponding timeline, which including the two inserted above.
    world
        .continuum::<MyContinuum>()
        .insert_into_buffers(Duration::ZERO);

    // Now edit the current world state.
    world
        .entity_mut(ent)
        .get_components_mut::<&mut Transform>()
        .unwrap()
        .translation
        .z = 1.0;

    world.resource_mut::<SomeResource>().0 = 1.0;
    assert!(world.get_resource::<SomeResource>().is_some());

    // When you're about to do multiple things on a continuum, for brevity,
    // it can be a good idea to grab the interface into a separate variable first.
    let mut cont = world.continuum::<MyContinuum>();

    // Store the new state too.
    cont.insert_into_buffers(Duration::from_secs(1));

    // Interpolate exactly halfway between the two states.
    cont.interpolate_to(Duration::from_millis(500)).unwrap();

    let new_transform = world.entity(ent).get_components::<&Transform>().unwrap();
    // Waow! It interpolate!
    assert_eq!(new_transform.translation.z, 0.5);

    // The resource is, too!
    let new_resource = world.resource::<SomeResource>();
    assert_eq!(new_resource.0, 0.5);
}

// A quick wrapper to let `cargo test` run the above as a test lol
#[test]
fn main_but_as_a_test() {
    main();
}
