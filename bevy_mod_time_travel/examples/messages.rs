//! This example is basically the same as `basic.rs` but focused on messages.

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_mod_time_travel::prelude::*;

/// A timeline that stores messages.
#[derive(Resource, Clone, Debug, Deref, DerefMut, Reflect, Default)]
struct MyTimelineMessages<M: Message + Clone> {
    buf: TimedMessagesBuffer<M>,
}

/// Continuum type to group all instances of the generic timeline components/resources together.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Reflect, ScheduleLabel)]
pub struct MyContinuum;

// Implement the necessary traits.

impl Continuum for MyContinuum {}

impl<M: Message + Clone> MessageTimeline for MyTimelineMessages<M> {
    type Message = M;
    type Continuum = MyContinuum;
}

#[derive(Message, Clone, Debug, PartialEq, Eq, Reflect, Default)]
struct SomeMessage(i32);

fn main() {
    let mut world = World::new();

    // Register the timeline into the world. This creates the correct schedules and systems that will
    // perform the time travel. This also registers the `SomeMessage` type as a message in this
    // world.
    world
        .register_timeline::<MyTimelineMessages<SomeMessage>>()
        .reflect_message_timeline()
        .register_message();

    // Insert the actual buffer into the world.
    world.insert_resource(MyTimelineMessages::<SomeMessage>::default());

    // Now we can perform all the good stuff.
    world.write_message(SomeMessage(5));
    world.write_message(SomeMessage(10));

    // Store the current state into the timelines. This stores the state of ALL tracked messages
    // with a corresponding timeline.
    world
        .continuum::<MyContinuum>()
        .insert_into_buffers(Duration::ZERO);

    // Now let's make some more
    world.write_message(SomeMessage(100));
    world.write_message(SomeMessage(105));

    // To check results later, we'll need to have a message cursor. So, make one real quick.
    let mut cursor = world
        .resource::<Messages<SomeMessage>>()
        .get_cursor_current();

    // Same for reverse messages, which will be emitted when rewinding backward.
    let mut cursor_rev = world
        .resource::<Messages<Reverse<SomeMessage>>>()
        .get_cursor_current();

    // When you're about to do multiple things on a continuum, for brevity,
    // it can be a good idea to grab the interface into a separate variable first.
    let mut cont = world.continuum::<MyContinuum>();

    // Store the new state too.
    cont.insert_into_buffers(Duration::from_secs(1));

    // Rewind exactly halfway between the two states. (interpolating works the same for messages)
    cont.rewind_to(Duration::from_millis(500)).unwrap();

    // Since nothing was written in the period between 500ms and 1s (except what we just
    // rewound *from*), no messages will exist.
    assert!(cursor.is_empty(world.resource::<Messages<SomeMessage>>()));
    assert!(cursor_rev.is_empty(world.resource::<Messages<Reverse<SomeMessage>>>()));

    // Rewind all the way back...
    world
        .continuum::<MyContinuum>()
        .rewind_to(Duration::ZERO)
        .unwrap();

    // We should see reversed messages being output for the first two written in there.
    let mut reader = cursor_rev.read(world.resource::<Messages<Reverse<SomeMessage>>>());

    let mut wtf = world
        .resource::<Messages<Reverse<SomeMessage>>>()
        .get_cursor();
    for thing in wtf.read(world.resource::<Messages<Reverse<SomeMessage>>>()) {
        dbg!(thing);
    }

    assert_eq!(reader.next(), Some(&Reverse(SomeMessage(10))));
    assert_eq!(reader.next(), Some(&Reverse(SomeMessage(5))));
    assert_eq!(reader.next(), None);

    // Rewind all the way forward...
    world
        .continuum::<MyContinuum>()
        .rewind_to(Duration::from_secs(1))
        .unwrap();

    // Now we should see the last two messages being output normally, as we rewound through the time
    // they were registered at.
    let mut reader = cursor.read(world.resource::<Messages<SomeMessage>>());
    assert_eq!(reader.next(), Some(&SomeMessage(100)));
    assert_eq!(reader.next(), Some(&SomeMessage(105)));
    assert_eq!(reader.next(), None);
}

// A quick wrapper to let `cargo test` run the above as a test lol
#[test]
fn main_but_as_a_test() {
    main();
}
