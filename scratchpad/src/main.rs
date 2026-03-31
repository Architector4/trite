// To deal with Clippy Pedantic's passionate hate for Bevy systems...
#![allow(clippy::needless_pass_by_value)]

// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao
// NOTE: This is just a scratchpad for fucking around lmao

use std::{collections::VecDeque, time::Duration};

use bevy::{
    ecs::entity_disabling::Disabled,
    input::{ButtonState, keyboard::KeyboardInput},
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{EguiGlobalSettings, EguiPlugin};
use bevy_mod_time_travel::{
    interpolation::{Interpolated, InterpolationPlugin, InterpolationVariables},
    prelude::*,
};

use bevy::input::common_conditions::input_toggle_active;
use bevy_inspector_egui::quick::*;

use scratchpad::{
    cam_orbiting::*,
    trajectory_prediction::{NowPredicting, Predicted, PredictionPlugin},
};

use avian3d::prelude::*;

fn main() {
    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins.set(bevy::render::RenderPlugin {
            // use software renderer lol
            //render_creation: bevy::render::settings::RenderCreation::Automatic(bevy::render::settings::WgpuSettings { adapter_name: Some(String::from("llvmpipe (LLVM 21.1.6, 256 bits)")), ..default()}),
            ..default()
        }), //.set(bevy::log::LogPlugin {
            //    filter: "bevy_mod_time_travel=debug".to_string(),
            //    ..default()
            //}),
    );

    app.add_plugins(CameraOrbitPlugin);

    app.add_plugins(EguiPlugin::default());
    app.add_plugins(
        WorldInspectorPlugin::default().run_if(input_toggle_active(true, KeyCode::Escape)),
    );

    app.world_mut()
        .resource_mut::<EguiGlobalSettings>()
        .enable_absorb_bevy_input_system = true;

    app.world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_millis(50));

    app.world_mut()
        .insert_resource(DefaultRestitution(Restitution::new(0.5)));

    app.world_mut()
        .insert_resource(DirectionalLightShadowMap { size: 4096 });

    //app.world_mut().insert_resource(TimeToSleep(0.1));

    //app.world_mut().insert_resource(DefaultFriction(Friction {
    //    static_coefficient: 10.0,
    //    ..default()
    //}));

    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, control);
    app.add_systems(FixedPreUpdate, spawn_cube);
    //app.add_systems(
    //    PhysicsSchedule,
    //    change_detect_wtf.before(SolverSystems::Finalize),
    //);

    app.add_systems(FixedPreUpdate, delete_fallen);

    app.add_plugins(InterpolationPlugin(InterpolationVariables {
        //rewind_policy: TickRestorePolicy::Retrigger,
        ..default()
    }));

    //app.add_systems(Update, poke_eepy);
    app.add_plugins(PredictionPlugin);

    macro_rules! register_timelines {
        ($thing: ty) => {
            app.world_mut()
                .register_timeline::<Predicted<$thing>>()
                .without_interpolation()
                .reflect()
                .register_component();
        };
    }

    register_timelines!(AngularVelocity);
    register_timelines!(Disabled);
    register_timelines!(LinearVelocity);
    register_timelines!(Position);
    register_timelines!(Rotation);
    register_timelines!(SleepTimer);
    register_timelines!(Sleeping);
    register_timelines!(Transform);

    #[allow(unused_macros)]
    macro_rules! register_continuums_resource {
        ($thing: ty) => {
            app.world_mut()
                .register_continuum::<Predicted<$thing>>()
                .without_interpolation()
                .register_resource();

            app.world_mut()
                .insert_resource(Predicted::<$thing>::default());
        };
    }

    //register_continuums_resource!(ContactGraph);
    //register_continuums_resource!(PhysicsIslands);
    //register_continuums_resource!(ContactGraph);
    //register_continuums_resource!(ConstraintGraph);

    app.add_plugins(PhysicsPlugins::default());
    app.add_plugins(PhysicsDebugPlugin);

    app.run();

    println!("Hello, world!");
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if let Ok(mut w) = window.single_mut() {
        w.title = "TEST".to_string();
    }

    let ground_bundle = (
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::new(1.0, 1.0)))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        RigidBody::Static,
        Collider::cuboid(2.0, 0.01, 2.0),
        //CollisionMargin(1.0),
        Name::new("THE GROUND"),
    );

    //commands.spawn((
    //    ground_bundle.clone(),
    //    Transform::from_scale(Vec3::new(30.0, 1.0, 30.0))
    //        .with_rotation(Quat::from_rotation_z(-0.5)),
    //));

    //commands.spawn((
    //    ground_bundle.clone(),
    //    Transform::from_scale(Vec3::new(30.0, 1.0, 30.0)).with_rotation(Quat::from_rotation_z(0.5)),
    //));

    // falldown

    commands.spawn((
        ground_bundle.clone(),
        Transform::from_scale(Vec3::new(30.0, 1.0, 30.0))
            .with_translation(Vec3::new(-25.0, 0.0, 0.0))
            .with_rotation(Quat::from_rotation_z(-0.5)),
    ));

    commands.spawn((
        ground_bundle.clone(),
        Transform::from_scale(Vec3::new(30.0, 1.0, 30.0))
            .with_translation(Vec3::new(25.0, -20.0, 0.0))
            .with_rotation(Quat::from_rotation_z(0.5)),
    ));

    commands.spawn((
        ground_bundle.clone(),
        Transform::from_scale(Vec3::new(30.0, 1.0, 30.0))
            .with_translation(Vec3::new(0.0, -5.0, 25.0))
            .with_rotation(Quat::from_rotation_x(-0.5)),
    ));

    commands.spawn((
        ground_bundle.clone(),
        Transform::from_scale(Vec3::new(30.0, 1.0, 30.0))
            .with_translation(Vec3::new(0.0, -30.0, -25.0))
            .with_rotation(Quat::from_rotation_x(0.5)),
    ));

    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 3000.0,
            shadow_depth_bias: 0.002,
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: 500.0,
            first_cascade_far_bound: 5.0,
            overlap_proportion: 0.35,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            135.0f32.to_radians(),
            -70f32.to_radians(),
            0.0,
        ))
        .with_translation(Vec3::new(2.0, 3.0, 2.0)),
        Name::new("THE SUN"),
    ));

    commands.spawn((
        Text::new(concat!(
            "Hold C to spawn dynamic cubes.\n",
            "Hold V to spawn static cubes.\n",
            "Use WASD, Ctrl, Space to move cubes.\n",
            "Use middle mouse button/scroll wheel\n",
            "and SHIFT to control the camera."
        )),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0),
            ..default()
        },
    ));

    //commands.insert_resource(PhysicsTransformConfig {
    //    transform_to_position: false,
    //    ..default()
    //});

    let camera_transform =
        Transform::from_xyz(0.8, 2.7, 3.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
    commands.spawn((
        Camera3d::default(),
        camera_transform,
        Projection::Perspective(PerspectiveProjection {
            fov: 60.0_f32.to_radians(),
            ..default()
        }),
        CameraOrbit {
            distance: camera_transform
                .translation
                .distance(Vec3::new(0.0, 1.0, 0.0)),
            ..default()
        },
    ));
}

#[derive(Component, Clone, Copy)]
struct ControllableCube;

/// Control cubes by pushing their positions directly. It's more "correct" to add/subtract to
/// velocity or something, but this is funnier and lets me phase cubes through ground lmao
fn control(
    mut cube: Query<(&mut Position, &ControllableCube)>,
    time: Res<Time>,
    mut input_events: MessageReader<KeyboardInput>,
    mut events_local: Local<VecDeque<KeyboardInput>>,
    mut input: Local<ButtonInput<KeyCode>>,
) {
    use avian3d::math::Scalar as AScalar;
    use avian3d::math::Vector as AVec;

    let relevant = &[
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::ControlLeft,
        KeyCode::Space,
        KeyCode::KeyW,
        KeyCode::KeyS,
    ];
    for event in events_local.drain(..).chain(
        input_events
            .read()
            .filter(|x| !x.repeat && relevant.contains(&x.key_code))
            .cloned(),
    ) {
        let KeyboardInput {
            key_code, state, ..
        } = event;
        match state {
            ButtonState::Pressed => input.press(key_code),
            ButtonState::Released => input.release(key_code),
        }
    }

    macro_rules! input_as_signum {
        ($in_neg: expr, $in_pos: expr) => {{ i8::from(input.pressed($in_pos)) - i8::from(input.pressed($in_neg)) }};
    }

    let movement = AVec::new(
        AScalar::from(input_as_signum!(KeyCode::KeyA, KeyCode::KeyD)),
        AScalar::from(input_as_signum!(KeyCode::ControlLeft, KeyCode::Space)),
        AScalar::from(input_as_signum!(KeyCode::KeyW, KeyCode::KeyS)),
    );

    //for mut entity in cube.iter_mut() {
    //    entity.0.translation += 0.0;
    //}

    if movement == AVec::ZERO {
        return;
    }

    for (mut position, _control) in cube.iter_mut() {
        position.0 += movement * (time.delta_secs_f64() * 5.0) as AScalar;
    }
}

/// System that deletes fallen cubes.
fn delete_fallen(
    query: Query<(Entity, &Transform), With<ControllableCube>>,
    now_predicting: Option<Res<NowPredicting>>,
    commands: ParallelCommands,
) {
    query.par_iter().for_each(|(entity, transform)| {
        let limit = -100.0;
        if transform.translation.y < limit {
            if now_predicting.is_some() {
                commands.command_scope(|mut commands| {
                    commands.entity(entity).insert(Disabled);
                });
            } else {
                commands.command_scope(|mut commands| commands.entity(entity).despawn());
            }
        }
    });
}

#[derive(Resource, Clone)]
struct CubeAsset {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

fn spawn_cube(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    asset: Option<Res<CubeAsset>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.pressed(KeyCode::KeyC) || input.pressed(KeyCode::KeyV) {
        let asset_super_tmp;
        let asset = if let Some(asset) = &asset {
            asset
        } else {
            asset_super_tmp = CubeAsset {
                mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                material: materials.add(Color::srgb_u8(124, 144, 255)),
            };
            commands.insert_resource(asset_super_tmp.clone());
            &asset_super_tmp
        };

        let cube_transform = Transform::from_xyz(0.0, 5.0, 0.0);

        let cube = (
            Mesh3d(asset.mesh.clone()),
            MeshMaterial3d(asset.material.clone()),
            cube_transform,
            Collider::cuboid(1.0, 1.0, 1.0),
            ControllableCube,
            (
                (
                    //Interpolated::<AngularVelocity>::default(),
                    //Interpolated::<LinearVelocity>::default(),
                    //Interpolated::<Position>::default(),
                    //Interpolated::<PreSolveDeltaPosition>::default(),
                    //Interpolated::<PreSolveDeltaRotation>::default(),
                    //Interpolated::<Rotation>::default(),
                    //Interpolated::<SleepTimer>::default(),
                    //Interpolated::<Sleeping>::default(),
                    Interpolated::<Transform>::default(),
                ),
                (
                    Predicted::<AngularVelocity>::default(),
                    Predicted::<Disabled>::default(),
                    Predicted::<LinearVelocity>::default(),
                    Predicted::<Position>::default(),
                    Predicted::<Rotation>::default(),
                    Predicted::<SleepTimer>::default(),
                    Predicted::<Sleeping>::default(),
                    Predicted::<Transform>::default(),
                ),
            ),
        );

        if input.pressed(KeyCode::KeyC) {
            commands.spawn((
                cube.clone(),
                RigidBody::Dynamic,
                Name::new("THE CUBE DYNAMIC"),
            ));
        }
        if input.pressed(KeyCode::KeyV) {
            commands.spawn((cube, RigidBody::Static, Name::new("THE CUBE STATIC")));
        }
    }
}

#[allow(unused)]
fn poke_eepy(query: Query<&mut SleepTimer>, mut printed: Local<u8>) {
    for mut item in query {
        println!("{}", item.changed_by());
        //item.set_changed();
        //if *printed == 0 {
        //    let address = item.bypass_change_detection() as *const SleepTimer as usize;
        //    println!("{:#x}", address);
        //}

        //*printed = printed.wrapping_add(1);
    }
}
