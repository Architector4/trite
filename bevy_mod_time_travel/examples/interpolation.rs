//! Example demonstrating the interpolation plugin.
//!
//! Two rhombuses are moved back and forth. One doesn't have an [`Interpolated<Transform>`]
//! component, but the other one does.
//!
//! Movement of the rhombuses is done additively; that is, their transform always depends on the
//! value on the previous fixed timestep. Despite that, they stay in sync. This demonstrates that
//! effects of interpolation are not observed by code within fixed timestep.
//!
//! You can use keyboard keys 1 to 0 to change the fixed timestep duration.

#![allow(clippy::needless_pass_by_value)]

use std::{f32::consts::PI, time::Duration};

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use bevy_mod_time_travel::interpolation::{Interpolated, InterpolationPlugin};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, InterpolationPlugin::default()))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, move_around)
        .add_systems(PreUpdate, change_timestep)
        .insert_resource(Time::<Fixed>::from_hz(5.0))
        .run();
}

#[derive(Component)]
struct Moving;

fn setup(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    let mesh = meshes.add(Rectangle::new(50.0, 50.0));
    let material = materials.add(Color::srgb(1.0, 1.0, 0.0));

    commands.spawn(Camera2d);

    commands.spawn((
        Mesh2d(mesh.clone()),
        MeshMaterial2d(material.clone()),
        Moving,
        Transform::from_xyz(-80.0, 50.0, 0.0).with_rotation(Quat::from_rotation_z(PI / 4.0)),
    ));

    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Moving,
        Transform::from_xyz(-80.0, -50.0, 0.0).with_rotation(Quat::from_rotation_z(PI / 4.0)),
        Interpolated::<Transform>::default(),
    ));
}

fn move_around(mut stuff: Query<&mut Transform, With<Moving>>, time: Res<Time>) {
    stuff.par_iter_mut().for_each(|mut transform| {
        transform.translation.x += (time.elapsed_secs() * 2.0).sin() * 150.0 * time.delta_secs();
    });
}

fn change_timestep(
    mut input: MessageReader<KeyboardInput>,
    mut time: ResMut<Time<Fixed>>,
    mut window: Single<&mut Window>,
) {
    for input in input.read() {
        let KeyboardInput {
            state: ButtonState::Pressed,
            logical_key: Key::Character(input),
            ..
        } = input
        else {
            continue;
        };

        let Ok(number): Result<u64, _> = input.as_str().parse() else {
            continue;
        };

        let new_timestep = Duration::from_millis(number * 100).max(Duration::from_millis(1));

        time.set_timestep(new_timestep);

        window.title = format!("New timestep: {new_timestep:?}");
    }
}
