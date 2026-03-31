//! Example demonstrating how to use `teleported` field of the [`Interpolated<Transform>`]
//! component in order to depict an object being abruptly teleported, but otherwise moving
//! smoothly.
//!
//! This is mostly an expansion of the `interpolated.rs` example.

#![allow(clippy::type_complexity)]
#![allow(clippy::needless_pass_by_value)]

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy_mod_time_travel::interpolation::{Interpolated, InterpolationPlugin};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, InterpolationPlugin::default()))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, (move_around, teleport).chain())
        .add_systems(Update, teleporter_bling)
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

    commands.spawn((
        MeshMaterial2d(materials.add(Color::WHITE.with_luminance(0.5))),
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        Transform::from_xyz(150.0, 0.0, 0.0).with_scale(Vec3::new(50.0, 150.0, 1.0)),
        Interpolated::<Transform>::default(),
        Teleporter {
            offset: Vec3::new(-300.0, 0.0, 0.0),
            bling: 0.0,
        },
    ));
}

fn move_around(mut stuff: Query<&mut Transform, With<Moving>>, time: Res<Time>) {
    stuff.par_iter_mut().for_each(|mut transform| {
        transform.translation.x +=
            (time.elapsed_secs() * 2.5).sin().abs() * 150.0 * time.delta_secs();
    });
}

#[derive(Component)]
struct Teleporter {
    offset: Vec3,
    bling: f32,
}

fn teleport(
    mut moving_stuff: Query<
        (&mut Transform, Option<&mut Interpolated<Transform>>),
        Without<Teleporter>,
    >,
    mut teleporters: Query<(&Transform, &mut Teleporter)>,
) {
    for (mut transform, mut buf) in &mut moving_stuff {
        for (teleporter_transform, mut teleporter) in &mut teleporters {
            // Check if the center of the `transform` is within the teleporter.
            // Assumes the teleporter is a cube with a size of 1x1x1.
            let intersecting = {
                // Transform the center into local space to the teleporter's transform...
                let mut center = transform.translation;
                center -= teleporter_transform.translation;
                center = teleporter_transform.rotation.inverse() * center;
                center /= teleporter_transform.scale;

                center.x.abs() <= 0.5 && center.y.abs() <= 0.5 && center.z.abs() <= 0.5
            };

            if intersecting {
                transform.translation += teleporter.offset;
                teleporter.bling += 1.0;
                if let Some(ref mut buf) = buf {
                    buf.teleported = true;
                }
            }
        }
    }
}

fn teleporter_bling(
    teleporters: Query<(&MeshMaterial2d<ColorMaterial>, &mut Teleporter)>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (material, mut teleporter) in teleporters {
        materials
            .get_mut(&material.0)
            .expect("Material should exist")
            .color = (Color::WHITE
            .to_srgba()
            .with_luminance(teleporter.bling * 0.25 + 0.5))
        .into();
        teleporter.bling = f32::max(teleporter.bling - time.delta_secs() * 4.0, 0.0);
    }
}
