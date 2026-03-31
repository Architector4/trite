use bevy::app::FixedMain;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_mod_time_travel::interpolation::InterpolatedContinuum;
use bevy_mod_time_travel::interpolation::InterpolationVariables;
use bevy_mod_time_travel::prelude::*;
use bevy_mod_time_travel::timekeep::ContinuumTime;

// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao
// NOTE: mostly copied over from the trajectory_prediction example and beat into shape lmao

// Define the continuum...
#[derive(Clone, Debug, Deref, DerefMut, Reflect, Default, Component, Resource)]
pub struct Predicted<T: Clone + Send + Sync + 'static> {
    pub buf: RewindBuffer<T>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Reflect, ScheduleLabel)]
pub struct PredictedContinuum;
impl Continuum for PredictedContinuum {}

impl<T: Clone + Send + Sync + 'static> Timeline for Predicted<T> {
    type Item = T;
    type Continuum = PredictedContinuum;
}

/// A prediction is a list of transforms and colors.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
struct StoredPredictions(Vec<(Vec<Transform>, Color)>);

/// This resources only exists if the current ticks are prediction ticks.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct NowPredicting;

// Now define the plugin itself...
pub struct PredictionPlugin;

impl Plugin for PredictionPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut()
            .register_timeline::<Predicted<Transform>>()
            .without_interpolation()
            .reflect()
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

fn do_predictions(world: &mut World, mut last_run_at: Local<Option<Duration>>) {
    //bevy::log::info!("About to do predictions!");
    // Do predictions. The idea is to simulate the world forward repeatedly and record every
    // single state.

    // First, save the current fixed time. We will be modifying it, but we want to restore it
    // later.
    let original_time = *world.resource::<Time<Fixed>>();
    let original_time_generic = *world.resource::<Time<()>>();

    let step = original_time.timestep();

    let last_interpolated_to = world
        .get_resource::<ContinuumTime<InterpolatedContinuum>>()
        .map(|x| x.time);

    let last_time = world
        .get_resource::<bevy_mod_time_travel::timekeep::Timekeep<InterpolatedContinuum>>()
        .and_then(|x| x.last_moment())
        .map(|x| x.time);

    if let Some(last_time) = last_time {
        assert_eq!(last_time, original_time.elapsed());
    }

    if last_time == *last_run_at {
        // We already predicted from here.
        return;
    }

    *last_run_at = last_time;

    let _ = world.rewind_to_with_policies::<InterpolatedContinuum>(
        original_time.elapsed(),
        TickRestorePolicy::RestoreOldTicks,
        OutOfTimelineRangePolicy::default(),
    );
    //

    // Skip interpolation for the stuff below.
    if let Some(mut vars) = world.get_resource_mut::<InterpolationVariables>() {
        vars.run_interpolation_systems = false;
    }

    // Save current state.
    world.insert_into_buffers::<PredictedContinuum>(original_time.elapsed());

    // Clear the store of predictions too.
    world.resource_mut::<StoredPredictions>().0.clear();

    // Take current input so FixedMain doesn't do stuff with it every permutation lol
    let original_input = std::mem::take(&mut *world.resource_mut::<ButtonInput<KeyCode>>());
    //let original_input_messages = std::mem::take(&mut *world.resource_mut::<Messages<KeyboardInput>>());

    world.insert_resource(NowPredicting);

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
        world.run_schedule(FixedMain);

        // Record new state. This accumulates with no limit, but cleanup code below runs
        // `delete_after` and eventually `clear_continuum`.
        world.insert_into_buffers::<PredictedContinuum>(new_elapsed);
        // Mark everything as changed. This is done every "real" tick due to the same policy used
        // in the interpolation plugin with this crate's setup, so do it here to ensure symmetry.
        //world
        //    .rewind_to_with_policies::<PredictedContinuum>(
        //        new_elapsed,
        //        TickRestorePolicy::MarkAllChanged,
        //        OutOfTimelineRangePolicy::default(),
        //    )
        //    .expect("we literally just wrote this omg what the hell");
    }

    // We're done. Restore original state.
    world.remove_resource::<NowPredicting>();
    world
        .rewind_to_with_policies::<PredictedContinuum>(
            original_time.elapsed(),
            TickRestorePolicy::MarkAllChanged,
            OutOfTimelineRangePolicy::default(),
        )
        .expect("State at original time should still be there");
    *world.resource_mut::<Time<Fixed>>() = original_time;
    *world.resource_mut::<Time<()>>() = original_time_generic;

    // Now store data from the buffers.
    world
        .run_system_cached_with(store_predictions, Color::srgba(1.0, 0.0, 0.0, 0.5))
        .expect("System to store predictions should not be broken");

    // Final cleanup.
    world.clear_timelines::<PredictedContinuum>();
    world.insert_resource(original_input);
    //world.insert_resource(original_input_messages);
    if let Some(mut vars) = world.get_resource_mut::<InterpolationVariables>() {
        vars.run_interpolation_systems = true;
    }
    if let Some(last_interpolated_to) = last_interpolated_to {
        let _ = world.interpolate_to::<InterpolatedContinuum>(last_interpolated_to);
    }
}

fn store_predictions(
    In(color): In<Color>,
    predictions: Query<&Predicted<Transform>>,
    mut store: ResMut<StoredPredictions>,
) {
    for prediction in predictions {
        let mut points: Vec<Transform> = Vec::with_capacity(prediction.len());
        for transform in prediction.iter() {
            if let Some(transform) = &transform.item {
                points.push(transform.1);
            }
        }

        store.0.push((points, color));
    }
}

fn draw_predictions(mut gizmos: Gizmos, predictions: Res<StoredPredictions>) {
    for (points, color) in &predictions.0 {
        //gizmos.linestrip(points.iter().map(|x| x.translation), *color);

        for point in points.iter() {
            //let factor = (idx + 1) as f32 / points.len() as f32;
            //gizmos.cube(*point, color.with_alpha(1.0 - factor));

            gizmos.cube(*point, *color);
        }
    }
}
