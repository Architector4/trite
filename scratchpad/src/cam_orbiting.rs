// To deal with clippy's passionate fury at Bevy systems...
#![allow(clippy::needless_pass_by_value)]
use bevy::{
    input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
};

pub struct CameraOrbitPlugin;

impl Plugin for CameraOrbitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, orbit);
        app.register_type::<CameraOrbit>();
    }
}

#[derive(Component, Reflect)]
pub struct CameraOrbit {
    pub upside_down: bool,
    pub distance: f32,
}

impl Default for CameraOrbit {
    fn default() -> Self {
        CameraOrbit {
            upside_down: false,
            distance: 5.0,
        }
    }
}

fn orbit(
    mut camera: Single<(&mut Transform, &mut CameraOrbit)>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut ev_scroll: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if !ev_scroll.is_empty() {
        let zoom = ev_scroll
            .read()
            .map(|e| {
                let mut scroll = e.y;
                if let MouseScrollUnit::Line = e.unit {
                    scroll *= 15.0;
                }

                scroll
            })
            .sum::<f32>();

        let old_distance = camera.1.distance;

        camera.1.distance = (camera.1.distance - zoom * camera.1.distance * 0.01).max(0.25);

        let camera_offset = camera.0.forward() * (old_distance - camera.1.distance);
        camera.0.translation += camera_offset;
    }

    if mouse_buttons.pressed(MouseButton::Middle) {
        if keyboard.pressed(KeyCode::ShiftLeft) {
            let x = camera.0.right().as_vec3() * motion.delta.x * -0.00085 * camera.1.distance;
            let y = camera.0.up().as_vec3() * motion.delta.y * 0.00085 * camera.1.distance;

            camera.0.translation += x;
            camera.0.translation += y;
        } else {
            if mouse_buttons.just_pressed(MouseButton::Middle) {
                camera.1.upside_down = camera.0.up().y < 0.0;
            }
            let center = camera.0.translation + camera.0.forward().as_vec3() * camera.1.distance;

            let old_dir = camera.0.rotation;

            let mut rotation = (motion.delta.x * -0.0025, motion.delta.y * -0.0025);

            if camera.1.upside_down {
                rotation.0 *= -1.0;
            }

            camera.0.rotate_y(rotation.0);
            camera.0.rotate_local_x(rotation.1);

            let new_dir = camera.0.rotation;

            let difference = new_dir * old_dir.inverse();

            camera.0.translate_around(center, difference);
        }
    }
}
