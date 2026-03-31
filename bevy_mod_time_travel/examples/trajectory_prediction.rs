//! Example demonstrating how to use the time travel machinery to predict trajectory of a rocket.
//!
//! In this example, `RocketPredictionPlugin`, every frame, iterates over every possible input
//! state and runs fixed timestep schedules 50 times for each. Rocket position and state is stored
//! for every run, then the world state rewound to what it originally was. The saved positions,
//! comprising trajectories, are then charted on the screen in differently colored lines.
//!
//! This is all done in separate timelines from world state interpolation.

#![allow(clippy::needless_pass_by_value)]

use bevy::ecs::schedule::ScheduleLabel;
use bevy::{input::keyboard::KeyboardInput, prelude::*};

use bevy_mod_time_travel::{
    interpolation::{Interpolated, InterpolationPlugin, InterpolationVariables},
    prelude::*,
};

use crate::prediction::Predicted;

#[derive(Component, Default, Clone)]
struct Rocket {
    velocity: Vec3,
    rotation: f32,
    has_crashed: bool,
}

#[derive(Component)]
struct CameraFollowingRocket;

#[derive(Component)]
struct Cloud;

#[derive(Component)]
struct Ground;

#[derive(Component)]
struct Explosion {
    /// From 0 (just started) to 1 (faded out completely).
    progress: f32,
}

/// Schedule containing predictable gameplay logic.
///
/// This schedule is the one that `RocketPredictionPlugin` will repeatedly run to gather
/// predictions and rewind.
///
/// This schedule exists separately from `FixedUpdate` or any other schedules in `FixedMain`
/// because those tend to contain extra systems that aren't well fit for prediction. One example of
/// such is [`bevy::ecs::message::signal_message_update_system`]; its effects cannot be rewound.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, ScheduleLabel)]
struct PredictableGameplayLogic;

fn run_predictable_gameplay_logic(world: &mut World) {
    world.run_schedule(PredictableGameplayLogic)
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins,
        InterpolationPlugin(InterpolationVariables {
            // Prediction systems will run all fixed timestep systems many times every frame,
            // incrementing their internal change detection tick counters. This makes restoring old
            // ticks pointless, since they will always be stale to those systems, so it's better to
            // keep everything marked as always changed.
            //
            // Nothing in this example does not utilize change detection, but this fact is
            // important to note in case your code does.
            rewind_policy: TickRestorePolicy::MarkAllChanged,
            ..default()
        }),
        crate::prediction::RocketPredictionPlugin,
    ));

    app.world_mut()
        .register_timeline::<Interpolated<Rocket>>()
        .interpolate_with(|a, b, factor| Rocket {
            velocity: Animatable::interpolate(&a.velocity, &b.velocity, factor),
            rotation: Animatable::interpolate(&a.rotation, &b.rotation, factor),
            has_crashed: a.has_crashed || b.has_crashed,
        })
        .register_component();

    app.add_systems(Startup, setup)
        // Run the predictable gameplay logic schedule
        .add_systems(PredictableGameplayLogic, (rocket_movement).chain())
        .add_systems(
            FixedUpdate,
            (
                run_predictable_gameplay_logic,
                handle_crashed_rocket,
                respawn_rocket,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                (
                    camera_follow_rocket,
                    (scroll_clouds_around_camera, move_ground_below_camera),
                    handle_crashed_rocket,
                )
                    .chain(),
                explosion_animate,
            ),
        )
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Time::<Fixed>::from_hz(20.0))
        .run();
}

fn spawn_rocket(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    commands.spawn((
        Mesh2d(meshes.add(Triangle2d::new(
            Vec2::new(-0.5, -0.5),
            Vec2::new(0.5, -0.5),
            Vec2::new(0.0, 1.0),
        ))),
        MeshMaterial2d(materials.add(Color::WHITE)),
        Transform::default(),
        Interpolated::<Transform>::default(),
        Interpolated::<Rocket>::default(),
        Rocket::default(),
        Predicted::<Transform>::default(),
        Predicted::<Rocket>::default(),
    ));
}

fn spawn_explosion(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    commands: &mut Commands,
    transform: Transform,
) {
    let mut material: ColorMaterial = Color::srgb(1.0, 1.0, 0.0).into();
    material.alpha_mode = bevy::sprite_render::AlphaMode2d::Blend;

    commands.spawn((
        Explosion { progress: 0.0 },
        Mesh2d(meshes.add(Circle::new(2.0))),
        MeshMaterial2d(materials.add(material)),
        transform.with_scale(Vec3::ZERO),
    ));
}

fn setup(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = bevy::camera::ScalingMode::AutoMax {
        max_width: 80.0,
        max_height: 80.0,
    };

    commands.spawn((
        Camera2d,
        CameraFollowingRocket,
        Projection::Orthographic(projection),
    ));

    // The ground (a very big downward triangle lol)
    commands.spawn((
        Mesh2d(meshes.add(Triangle2d::new(
            Vec2::new(-100_000.0, 0.0),
            Vec2::new(100_000.0, 0.0),
            Vec2::new(0.0, -100_000.0),
        ))),
        MeshMaterial2d(materials.add(Color::WHITE.darker(0.5))),
        Transform::from_translation(-Vec3::Z),
        Ground,
    ));

    commands.spawn((
        Text::new(concat!(
            "Hold up arrow to accelerate,\n",
            "left/right arrows to turn.\n",
            "Press space to respawn the rocket."
        )),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0),
            ..default()
        },
    ));

    commands.spawn((
        Text::new(concat!(
            "Curved lines from the rocket represent possible trajectories.\n",
            "Gray trajectory is if you press nothing,\n",
            "Green is if you only accelerate, and blue if you accelerate and turn."
        )),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));

    // The clouds
    let cloud_mesh = Mesh2d(meshes.add(Rectangle::new(4.0, 0.6)));
    let cloud_material = MeshMaterial2d(materials.add(Color::WHITE.with_alpha(0.05)));
    for i in 1..100_i64 {
        // A poor man's way of generating "random" numbers.
        // Still makes a pattern, but it's fine.
        // The constants are computed via keysmashing and then changing random digits until it
        // looked good enough.
        let x = i.wrapping_mul(91_648_718_712_847_148) % 100_000 - 50_000;
        let y = i.wrapping_mul(129_127_864_187_264_113) % 100_000 - 50_000;

        #[allow(clippy::cast_precision_loss)]
        commands.spawn((
            cloud_mesh.clone(),
            cloud_material.clone(),
            Transform::from_translation(Vec3::new((x as f32) * 0.001, (y as f32) * 0.001, -10.0)),
            Cloud,
        ));
    }

    spawn_rocket(meshes, materials, commands);
}

fn respawn_rocket(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
    mut input: MessageReader<KeyboardInput>,
    rocket: Option<Single<(Entity, &Transform), With<Rocket>>>,
) {
    if input
        .read()
        .any(|x| !x.repeat && x.key_code == KeyCode::Space && x.state.is_pressed())
    {
        if let Some(rocket) = rocket {
            commands.entity(rocket.0).despawn();
            spawn_explosion(&mut meshes, &mut materials, &mut commands, *rocket.1);
        }

        spawn_rocket(meshes, materials, commands);
    }
}

fn rocket_movement(
    rocket: Option<Single<(&mut Transform, &mut Rocket)>>,
    time: Res<Time<()>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if let Some(mut data) = rocket {
        let (ref mut transform, ref mut rocket) = *data;

        if rocket.has_crashed {
            return;
        }

        // Do rotation first...
        let mut rotation = 0.0;
        if input.pressed(KeyCode::ArrowLeft) {
            rotation -= 180.0 * time.delta_secs();
        }

        if input.pressed(KeyCode::ArrowRight) {
            rotation += 180.0 * time.delta_secs();
        }

        rocket.rotation += rotation;
        transform.rotation = Quat::from_axis_angle(Vec3::Z, -rocket.rotation.to_radians());

        // Compute new velocity...
        if input.pressed(KeyCode::ArrowUp) {
            rocket.velocity += transform.up() * 33.0 * time.delta_secs();
        }
        rocket.velocity.y += -9.8 * time.delta_secs();

        // Will be useful in touching ground logic below.
        let old_translation = transform.translation;

        // Apply velocity.
        transform.translation += rocket.velocity * time.delta_secs();

        // If we're touching ground...
        if transform.translation.y <= 0.0 {
            if rocket.velocity.length_squared() > 400.0 {
                rocket.has_crashed = true;
            }

            rocket.velocity = Vec3::ZERO;

            // We want to snap the rocket to the ground. Setting translation's Y coordinate to 0
            // mostly works, but potentially produces a little slide across the ground that is bigger the
            // smaller the fixed timestep is and causes jitter in the predicted trajectory's crash
            // site. This is all because doing that would nullify vertical movement but not
            // horizontal.
            //
            // Nullifying both can be done with a little geometry.

            let old_y_diff_to_zero = 0.0 - old_translation.y;
            let old_y_diff_to_new = transform.translation.y - old_translation.y;

            // Compute time of impact. This is approaching 0.0 the closer the rocket was to the
            // ground before this tick, and 1.0 the closer the rocket is to the ground now.
            let time_of_impact = old_y_diff_to_zero / old_y_diff_to_new;

            if time_of_impact.is_finite() {
                use bevy::animation::animatable::Animatable;
                transform.translation = Animatable::interpolate(
                    &old_translation,
                    &transform.translation,
                    time_of_impact,
                );
            } else {
                // Probably already on the ground. Just do the simple thing.
                transform.translation.y = 0.0;
            }
        }
    }
}

fn handle_crashed_rocket(
    rocket: Option<Single<(Entity, &Transform, &Rocket)>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if let Some(data) = rocket {
        let (entity, transform, rocket) = *data;
        if rocket.has_crashed {
            commands.entity(entity).despawn();

            // A rocket can only ever crash against the ground. For clearest visuals, align the
            // explosion object to the ground.
            let mut transform = *transform;
            transform.translation.y = 0.0;

            spawn_explosion(&mut meshes, &mut materials, &mut commands, transform);
        }
    }
}

fn explosion_animate(
    explosions: Query<(
        Entity,
        &mut Transform,
        &mut Explosion,
        &MeshMaterial2d<ColorMaterial>,
    )>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
    time: Res<Time<()>>,
) {
    for (entity, mut transform, mut explosion, material) in explosions {
        explosion.progress += time.delta_secs();
        if explosion.progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::ONE * explosion.progress;

        let material = materials
            .get_mut(&material.0)
            .expect("Material should be present");
        material.color.set_alpha((1.0 - explosion.progress).powi(2));
    }
}

fn camera_follow_rocket(
    mut camera: Single<&mut Transform, (With<CameraFollowingRocket>, Without<Rocket>)>,
    rocket: Option<Single<&Transform, With<Rocket>>>,
) {
    if let Some(rocket) = rocket {
        camera.translation = rocket.translation;
    }
}

fn scroll_clouds_around_camera(
    camera: Single<&Transform, (With<CameraFollowingRocket>, Without<Cloud>)>,
    mut clouds: Query<&mut Transform, With<Cloud>>,
) {
    static SCROLL_DISTANCE: f32 = 50.0;
    clouds.par_iter_mut().for_each(|mut cloud| {
        let camera = camera.translation;
        let cloud = &mut cloud.translation;
        while (camera.x - cloud.x).abs() > SCROLL_DISTANCE {
            cloud.x += (camera.x - cloud.x).signum() * SCROLL_DISTANCE * 2.0;
        }
        while (camera.y - cloud.y).abs() > SCROLL_DISTANCE {
            cloud.y += (camera.y - cloud.y).signum() * SCROLL_DISTANCE * 2.0;
        }
    });
}

fn move_ground_below_camera(
    camera: Single<&Transform, (With<CameraFollowingRocket>, Without<Ground>)>,
    mut ground: Single<&mut Transform, With<Ground>>,
) {
    ground.translation.x = camera.translation.x;
}

// All that above is fancy, but we want to get FANCY.
// Contain it in a separate module.
mod prediction {
    use bevy::ecs::schedule::ScheduleLabel;
    use bevy::prelude::*;
    use bevy_mod_time_travel::{interpolation::InterpolationVariables, prelude::*};

    use super::{PredictableGameplayLogic, Rocket};

    // Define the timeline...
    #[derive(Clone, Debug, Deref, DerefMut, Default, Component)]
    pub struct Predicted<T: Clone + Send + Sync + 'static> {
        pub buf: RewindBuffer<T>,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, ScheduleLabel)]
    pub struct PredictedContinuum;
    impl Continuum for PredictedContinuum {}

    impl<T: Clone + Send + Sync + 'static> Timeline for Predicted<T> {
        type Item = T;
        type Continuum = PredictedContinuum;
    }

    /// A prediction is a list of XY positions, a crash position, and a color.
    #[derive(Resource, Default)]
    struct StoredPredictions(Vec<(Vec<Vec2>, Option<Vec2>, Color)>);

    // Now define the plugin itself...
    pub struct RocketPredictionPlugin;

    impl Plugin for RocketPredictionPlugin {
        fn build(&self, app: &mut App) {
            // Register proper systems for stuff...
            app.world_mut()
                .register_timeline::<Predicted<Transform>>()
                .without_interpolation()
                .register_component();

            app.world_mut()
                .register_timeline::<Predicted<Rocket>>()
                .without_interpolation()
                .register_component();

            app.add_systems(Update, (do_predictions, draw_predictions).chain());
            app.world_mut()
                .insert_resource(StoredPredictions::default());

            app.world_mut()
                .resource_mut::<GizmoConfigStore>()
                .config_mut::<DefaultGizmoConfigGroup>()
                .0
                .line
                .width = 5.0;
        }
    }

    fn do_predictions(world: &mut World) {
        // Do predictions. The idea is to simulate the world forward repeatedly and record every
        // single state.

        // First, save the current fixed time. We will be modifying it, but we want to restore it
        // later.
        let original_time = *world.resource::<Time<Fixed>>();
        let original_time_generic = *world.resource::<Time<()>>();

        let step = original_time.timestep();

        // Ideally we'd want to undo interpolation and work off of latest exact data, because then
        // the shown predictions would be 100% accurate, but it causes behavior that appears
        // jittery for various predictions because of mismatch between frame rate and timestep.
        //
        //let last_interpolated_to = world
        //    .get_resource::<ContinuumTime<InterpolatedContinuum>>()
        //    .map(|x| x.time);
        //world.rewind_to::<InterpolatedContinuum>(original_time.elapsed());
        //
        // Note: if you restore the code above, you should also put a Local<Duration> parameter to
        // this system and compare it against original_time.elapsed() and return early if equal,
        // otherwise set it to that elapsed value. This will prevent needlessly recomputing the
        // same set of predictions from the same data repeatedly.

        // Skip interpolation for the stuff below.
        world
            .resource_mut::<InterpolationVariables>()
            .run_interpolation_systems = false;

        // Save current state.
        world.insert_into_buffers::<PredictedContinuum>(original_time.elapsed());

        // Clear the store of predictions too.
        world.resource_mut::<StoredPredictions>().0.clear();

        // We want to do this for every input permutation.
        // Store current input state and substitute with various permutations.
        let original_input = std::mem::take(&mut *world.resource_mut::<ButtonInput<KeyCode>>());

        let states_to_test_with = [
            ([].as_slice(), Color::WHITE.darker(0.7)),
            ([KeyCode::ArrowUp].as_slice(), Color::srgb(0.0, 1.0, 0.0)),
            (
                [KeyCode::ArrowUp, KeyCode::ArrowLeft].as_slice(),
                Color::srgb(0.4, 0.4, 1.0),
            ),
            (
                [KeyCode::ArrowUp, KeyCode::ArrowRight].as_slice(),
                Color::srgb(0.4, 0.4, 1.0),
            ),
        ];

        for prediction_params in states_to_test_with {
            let mut stored_inputs = world.resource_mut::<ButtonInput<KeyCode>>();
            stored_inputs.reset_all();
            for input in prediction_params.0 {
                stored_inputs.press(*input);
            }

            for _ in 0..50 {
                // Step time.
                //
                // Note: we technically don't need to do so much management of time here, since
                // rocket code only ever cares about Time<()>::delta() value, which can just be set
                // once at the top of this function and forgotten. But, for the sake of the
                // example, more rigorous time management is included here.
                //
                // This rigor will be important to do if your systems rely on the Time::elapsed()
                // value, for example.
                let mut new_fixed = world.resource_mut::<Time<Fixed>>();
                new_fixed.advance_by(step);

                // Record new elapsed to know what to write the new state at.
                let new_elapsed = new_fixed.elapsed();
                *world.resource_mut::<Time<()>>() = new_fixed.as_generic();

                // Simulate world.
                world.run_schedule(PredictableGameplayLogic);

                // Record new state. This accumulates with no limit, but cleanup code below runs
                // `delete_after` and eventually `clear_timeline`.
                world.insert_into_buffers::<PredictedContinuum>(new_elapsed);

                // Did rocket crash?
                let crashed = world
                    .run_system_cached(did_rocket_crash)
                    .expect("System to check if rocket crashed should not be broken");

                if crashed {
                    // Exit early; no need to run empty ticks.
                    break;
                }
            }

            // We're done. Restore original state.
            // We don't need to care about restoring change detection here anyway because it would
            // get reset by interpolation anyway.
            world
                .rewind_to_with_policies::<PredictedContinuum>(
                    original_time.elapsed(),
                    TickRestorePolicy::Bypass,
                    OutOfTimelineRangePolicy::default(),
                )
                .expect("State at original time should still be there");
            *world.resource_mut::<Time<Fixed>>() = original_time;
            *world.resource_mut::<Time<()>>() = original_time_generic;

            // Now store data from the buffers.
            world
                .run_system_cached_with(store_predictions, prediction_params.1.with_alpha(0.1))
                .expect("System to store predictions should not be broken");

            // Now get rid of it.
            world.delete_after::<PredictedContinuum>(original_time.elapsed());
        }

        // Final cleanup.
        world.clear_timelines::<PredictedContinuum>();
        world.insert_resource(original_input);
        world
            .resource_mut::<InterpolationVariables>()
            .run_interpolation_systems = true;
        // No need to return to last interpolated state if we didn't go there. See commented out
        // block somewhere above.
        //
        //if let Some(last_interpolated_to) = last_interpolated_to {
        //    world.interpolate_to::<InterpolatedContinuum>(last_interpolated_to);
        //}
    }

    fn did_rocket_crash(rocket: Option<Single<&Rocket>>) -> bool {
        rocket.is_none_or(|x| x.has_crashed)
    }

    fn store_predictions(
        In(mut color): In<Color>,
        predictions: Option<Single<(&Predicted<Transform>, &Predicted<Rocket>)>>,
        mut store: ResMut<StoredPredictions>,
    ) {
        let Some(predictions) = predictions else {
            return;
        };

        let mut points: Vec<Vec2> = Vec::with_capacity(predictions.0.len());
        let mut crash_spot: Option<Vec2> = None;

        for (transform, rocket) in predictions.0.iter().zip(predictions.1.iter()) {
            if let (Some(transform), Some(rocket)) = (&transform.item, &rocket.item) {
                points.push(transform.1.translation.xy());
                if rocket.1.has_crashed {
                    crash_spot = Some(transform.1.translation.xy());
                    color = Color::srgb(1.0, 0.0, 0.0).with_alpha(color.alpha());
                    break;
                }
            }
        }

        store.0.push((points, crash_spot, color));
    }

    fn draw_predictions(mut gizmos: Gizmos, predictions: Res<StoredPredictions>) {
        for (points, crash_spot, color) in &predictions.0 {
            gizmos.linestrip_2d(points.iter().copied(), *color);

            if let Some(crash_spot) = crash_spot {
                // Draw a big red X.
                let size = 1.0;
                gizmos.line_2d(
                    crash_spot + Vec2::new(-size, -size),
                    crash_spot + Vec2::new(size, size),
                    color.with_alpha(1.0),
                );
                gizmos.line_2d(
                    crash_spot + Vec2::new(size, -size),
                    crash_spot + Vec2::new(-size, size),
                    color.with_alpha(1.0),
                );
            }
        }
    }
}
