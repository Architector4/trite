use alloc::collections::vec_deque::{Iter, VecDeque};
use bevy_ecs::change_detection::{ComponentTicks, DetectChanges, DetectChangesMut};
use core::{ops::RangeInclusive, time::Duration};

#[cfg(feature = "bevy_animation")]
use bevy_animation::animatable::Animatable;
#[cfg(feature = "bevy_reflect")]
use bevy_reflect::Reflect;

use super::{InterpFunc, OutOfRecordedRangeError};

/// Last detected change between this and the previous moment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub enum LastDetectedChange {
    /// No change was detected; this item is presumably identical to the last one.
    NoChange,
    /// The item was presumably changed between this and the previous moment.
    Changed,
    /// The item presumably did not exist in the previous moment and was added here.
    Added,
}

/// Recorded change detection state at the time a [`Moment<T>`] was saved.
///
/// Used to preserve state detection as observed by systems that depend on items within a momentum.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct ChangeDetectionState {
    /// Store of change detection ticks of this item.
    pub ticks: ComponentTicks,
    /// Last detected change between this and the previous moment.
    pub change: LastDetectedChange,
}

impl ChangeDetectionState {
    /// Set change ticks held within this state to this value. This does not take into account the
    /// fact that change ticks warp, so it might still trigger "changed" or "added" state that was
    /// not there before.
    pub fn apply_ticks<T: DetectChangesMut>(&self, value: &mut T) {
        value.set_last_added(self.ticks.added);
        value.set_last_changed(self.ticks.changed);
    }

    /// Trigger change detection on this type if change detection was detected at the time this
    /// state was recorded. If no change was recorded, sets change ticks instead.
    pub fn trigger_same_change_detection<T: DetectChangesMut>(&self, value: &mut T) {
        match self.change {
            LastDetectedChange::NoChange => {
                value.set_last_added(self.ticks.added);
                value.set_last_changed(self.ticks.changed);
            }
            LastDetectedChange::Changed => {
                value.set_last_added(self.ticks.added);
                value.set_changed();
            }
            LastDetectedChange::Added => {
                // Typically this also counts as "changed".
                value.set_added();
            }
        }
    }
}

impl<T: DetectChanges> From<&T> for ChangeDetectionState {
    fn from(value: &T) -> Self {
        let change = if value.is_added() {
            LastDetectedChange::Added
        } else if value.is_changed() {
            LastDetectedChange::Changed
        } else {
            LastDetectedChange::NoChange
        };

        let ticks = ComponentTicks {
            added: value.added(),
            changed: value.last_changed(),
        };

        ChangeDetectionState { ticks, change }
    }
}

/// Stores a single state of `T`, its change detection state, a time it's true to, and whether or
/// not it should be snapped to.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct Moment<T> {
    /// Time at which this moment has occurred.
    pub time: Duration,
    /// If `true`, and interpolation is run between an earlier point in time and this, this moment
    /// should be snapped to instead as per the "Pick B" rule.
    ///
    /// This is to simulate cases of "teleportation", i.e. when change is intended to be
    /// instantenous, rather than continuous.
    pub snap_to: bool,
    /// Item that was recorded at this moment and its change detection state.
    pub item: Option<(ChangeDetectionState, T)>,
}

impl<T> Moment<T> {
    /// Clone inner item, if any, disregarding change detection state.
    #[must_use]
    pub fn clone_item(&self) -> Option<T>
    where
        T: Clone,
    {
        self.item_ref().cloned()
    }

    /// Get reference to inner item, if any.
    #[must_use]
    pub fn item_ref(&self) -> Option<&T> {
        self.item.as_ref().map(|x| &x.1)
    }

    /// Get mutable reference to inner item, if any.
    #[must_use]
    pub fn item_mut(&mut self) -> Option<&mut T> {
        self.item.as_mut().map(|x| &mut x.1)
    }

    /// Interpolate between this and another moment, given a specific time, using the custom
    /// function, without time bounds checking.
    ///
    /// The interpolating function is not guaranteed to be run. If one or both of the moments are
    /// absent, or the supplied time is equal to that of one of the moments, cloning is used
    /// instead.
    ///
    /// This does not check if the time is not in the inclusive range between two of the
    /// moments or if they represent the same time. Because of this, running the interpolating
    /// function with a factor below 0, or above 1, or even infinity, is possible.
    #[must_use]
    pub fn interpolate_against_with_function_unchecked<F>(
        &self,
        mut interp: F,
        other: &Moment<T>,
        time: Duration,
    ) -> Option<T>
    where
        T: Clone,
        F: InterpFunc<T>,
    {
        // Check for exact time matches first.
        if self.time == time {
            return self.clone_item();
        }

        if other.time == time {
            return other.clone_item();
        }

        // Ensure chronological order.
        let (first, second) = if self.time < other.time {
            (self, other)
        } else {
            (other, self)
        };

        if second.snap_to {
            return second.clone_item();
        }

        // If one of the items is missing, do "Pick B" behavior.
        let (Some((_, first_item)), Some((_, second_item))) = (&first.item, &second.item) else {
            return second.clone_item();
        };

        // Now we know that `time` is specifically inbetween the two moments,
        // second.snap_to is false, and both moments have an existing item.
        // Time to interpolate.

        // Check above ensures chronological ordering.
        #[allow(clippy::unchecked_time_subtraction)]
        let time_difference = second.time - first.time;
        #[allow(clippy::unchecked_time_subtraction)]
        let time_to_interpolate_to = time - first.time;

        let factor = time_to_interpolate_to.div_duration_f32(time_difference);

        let new_item = interp(first_item, second_item, factor);

        Some(new_item)
    }

    /// Interpolate between this and another moment, given a specific time, using the custom
    /// function.
    ///
    /// The interpolating function is not guaranteed to be run. If one or both of the moments are
    /// absent, or the supplied time is equal to that of one of the moments, cloning is used
    /// instead.
    ///
    /// # Panics
    ///
    /// Panics if specified time is not in the inclusive range between time of the two moments, or
    /// if they have the same time.
    #[must_use]
    pub fn interpolate_against_with_function<F>(
        &self,
        interp: F,
        other: &Moment<T>,
        time: Duration,
    ) -> Option<T>
    where
        T: Clone,
        F: InterpFunc<T>,
    {
        // Ensure chronological order.
        let (first, second) = if self.time < other.time {
            (self, other)
        } else {
            (other, self)
        };

        assert!(
            time >= first.time && time <= second.time,
            "Specified time is outside the range specified by two moments."
        );
        assert_ne!(
            first.time, second.time,
            "Specified moments are for the same time."
        );

        self.interpolate_against_with_function_unchecked(interp, other, time)
    }

    /// Interpolate between this and another moment, given a specific time, without time bounds
    /// checking.
    ///
    /// The interpolating function is not guaranteed to be run. If one or both of the moments are
    /// absent, or the supplied time is equal to that of one of the moments, cloning is used
    /// instead.
    ///
    /// This does not check if the time is not in the inclusive range between two of the moments or
    /// if they represent the same time. Because of this, interpolation can happen with a factor
    /// below 0, or above 1, or even infinity.
    #[cfg(feature = "bevy_animation")]
    #[must_use]
    pub fn interpolate_against_unchecked(&self, other: &Moment<T>, time: Duration) -> Option<T>
    where
        T: Clone + Animatable,
    {
        self.interpolate_against_with_function_unchecked(Animatable::interpolate, other, time)
    }

    /// Interpolate between this and another moment, given a specific time, using
    /// [`Animatable::interpolate`].
    ///
    /// The interpolating function is not guaranteed to be run. If one or both of the moments are
    /// absent, or the supplied time is equal to that of one of the moments, cloning is used
    /// instead.
    ///
    /// # Panics
    ///
    /// Panics if specified time is not in the inclusive range between time of the two moments, or
    /// if they have the same time.
    #[cfg(feature = "bevy_animation")]
    #[must_use]
    pub fn interpolate_against(&self, other: &Moment<T>, time: Duration) -> Option<T>
    where
        T: Clone + Animatable,
    {
        self.interpolate_against_with_function(Animatable::interpolate, other, time)
    }

    /// Expose this moment as a proxy that allows modifying everything except the time.
    #[must_use]
    pub fn as_proxy(&mut self) -> MomentMutProxy<'_, T> {
        MomentMutProxy {
            _noconstruct: (),
            time: &self.time,
            snap_to: &mut self.snap_to,
            item: &mut self.item,
        }
    }
}

/// Return type for [`RewindBuffer::find_for_interpolation`].
#[derive(Clone, Copy, Debug)]
pub enum FindForInterpResult<'a, T> {
    /// Only an exact match was found, which is also first in the buffer.
    OnlyExact(&'a Moment<T>),
    /// Exact match was found; previous one is also provided.
    PreviousAndExact(&'a Moment<T>, &'a Moment<T>),
    /// No exact match was found, but found one moment before and one moment after.
    BeforeAndAfter(&'a Moment<T>, &'a Moment<T>),
}

impl<'a, T> FindForInterpResult<'a, T> {
    /// Snap to an exact match if available, or to the latter moment.
    ///
    /// This is equivalent to interpolating with [`pick_b_if_nonzero`], but preserves the rest of
    /// the [`Moment<T>`] structure and avoids cloning.
    ///
    /// [`pick_b_if_nonzero`]: super::pick_b_if_nonzero
    #[must_use]
    pub fn pick_b(&self) -> &'a Moment<T> {
        match &self {
            Self::OnlyExact(m) | Self::PreviousAndExact(_, m) | Self::BeforeAndAfter(_, m) => m,
        }
    }

    /// Produce a new [`Option<T>`] at a specific time via interpolation of found moments by a
    /// custom function.
    ///
    /// The interpolating function is not guaranteed to be run. If one or both of the moments are
    /// absent, or the supplied time is equal to that of one of the moments, cloning is used
    /// instead.
    ///
    /// As the result is a whole new value, just [`Option<T>`] is returned; there is no
    /// [`ChangeDetectionState`] that would make sense to return alongside it, making [`Moment<T>`]
    /// inapplicable.
    ///
    /// # Errors
    ///
    /// Errors if the specified time is outside of the range represented by this result.
    pub fn interpolate_with_function<F>(
        &self,
        interp: F,
        time: Duration,
    ) -> Result<Option<T>, OutOfRecordedRangeError>
    where
        T: Clone,
        F: InterpFunc<T>,
    {
        match &self {
            FindForInterpResult::OnlyExact(m) | FindForInterpResult::PreviousAndExact(_, m) => {
                Ok(m.clone_item())
            }
            FindForInterpResult::BeforeAndAfter(before, after) => {
                if after.snap_to {
                    Ok(after.clone_item())
                } else {
                    Ok(before.interpolate_against_with_function(interp, after, time))
                }
            }
        }
    }
}

/// A mutable proxy for a [`Moment<T>`] that allows editing `snap_to` and `item` but not `time`.
#[derive(Debug)]
// The intent is to make this not constructable outside of this crate, but not in a way that has
// anything to do with struct fields that might be added in the future.
#[allow(clippy::manual_non_exhaustive)]
pub struct MomentMutProxy<'a, T> {
    /// Prevent this from being constructed outside of this crate. Why would you?
    _noconstruct: (),
    /// Time at which this moment has occurred.
    pub time: &'a Duration,
    /// If `true`, and interpolation is run between an earlier point in time and this, this moment
    /// should be snapped to instead as per the "Pick B" rule.
    ///
    /// This is to simulate cases of "teleportation", i.e. when change is intended to be
    /// instantenous, rather than continuous.
    pub snap_to: &'a mut bool,
    /// Item that was recorded at this moment and its change detection state.
    pub item: &'a mut Option<(ChangeDetectionState, T)>,
}

impl<T> MomentMutProxy<'_, T> {
    /// Clone the underlying moment.
    #[must_use]
    pub fn clone_to_moment(&self) -> Moment<T>
    where
        T: Clone,
    {
        Moment {
            time: *self.time,
            snap_to: *self.snap_to,
            item: self.item.clone(),
        }
    }
}

/// A buffer that holds a rolling queue of [`Moment<T>`] values and can be used to rewind and
/// interpolate state within the stored interval.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct RewindBuffer<T> {
    /// Must always remain sorted by a moment's time,
    /// and never have two moments at exactly the same time.
    moments: VecDeque<Moment<T>>,
}

impl<T> Default for RewindBuffer<T> {
    fn default() -> Self {
        Self {
            moments: VecDeque::default(),
        }
    }
}

impl<T> RewindBuffer<T> {
    /// Create a new rewind buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty rewind buffer with space for at least capacity moments.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            moments: VecDeque::with_capacity(capacity),
        }
    }

    /// Find a moment for exactly this time within this buffer.
    #[must_use]
    pub fn find_moment_exact(&self, time: Duration) -> Option<&Moment<T>> {
        self.moments.iter().find(|x| x.time == time)
    }

    /// Returns the range of time this buffer is currently representing. This range is from the
    /// first moment to last moment, inclusively.
    ///
    /// Returns [`None`] if there are no moments, or a range consisting of the same time twice if
    /// there's only one moment.
    #[must_use]
    pub fn represented_range(&self) -> Option<RangeInclusive<Duration>> {
        let (Some(first), Some(last)) = (self.first_moment(), self.last_moment()) else {
            return None;
        };

        // If there's only one moment, first == last.

        Some(first.time..=last.time)
    }

    /// Determine if the provided time is within range currently represented by this buffer.
    #[must_use]
    pub fn time_in_range(&self, time: Duration) -> bool {
        self.represented_range().is_some_and(|r| r.contains(&time))
    }

    /// Returns the first moment contained in this buffer, if any.
    #[must_use]
    pub fn first_moment(&self) -> Option<&Moment<T>> {
        self.moments.front()
    }

    /// Returns the last moment contained in this buffer, if any.
    #[must_use]
    pub fn last_moment(&self) -> Option<&Moment<T>> {
        self.moments.back()
    }

    /// Find up to two moments that should be used for interpolation to the target time.
    ///
    /// # Errors
    /// Errors if the specified time is outside of range represented by this buffer.
    #[inline] // *Assuming* this will make things better lol
    pub fn find_for_interpolation(
        &self,
        time: Duration,
    ) -> Result<FindForInterpResult<'_, T>, OutOfRecordedRangeError> {
        let mut moment_before = None;

        if self.first_moment().is_none_or(|x| x.time > time) {
            return Err(OutOfRecordedRangeError);
        }

        for moment in &self.moments {
            if moment.time >= time {
                let Some(moment_before) = moment_before else {
                    return Ok(FindForInterpResult::OnlyExact(moment));
                };

                if moment.time == time {
                    return Ok(FindForInterpResult::PreviousAndExact(moment_before, moment));
                }

                return Ok(FindForInterpResult::BeforeAndAfter(moment_before, moment));
            }

            moment_before = Some(moment);
        }

        // Shouldn't be reachable here, but eh.
        Err(OutOfRecordedRangeError)
    }

    /// Produce a new [`Option<T>`] at a specific time via interpolation of existing moments by a
    /// custom function.
    ///
    /// This is a convenience shortcut that runs [`Self::find_for_interpolation`] then
    /// [`FindForInterpResult::interpolate_with_function`] on the return type; see that function
    /// for details.
    ///
    /// # Errors
    /// Errors if the specified time is outside of range represented by this buffer.
    pub fn interpolate_with_function<F>(
        &self,
        interp: F,
        time: Duration,
    ) -> Result<Option<T>, OutOfRecordedRangeError>
    where
        T: Clone,
        F: InterpFunc<T>,
    {
        self.find_for_interpolation(time)?
            .interpolate_with_function(interp, time)
    }

    /// Produce a new [`Option<T>`] at a specific time via interpolation of existing moments.
    ///
    /// This is a convenience shortcut for [`Self::interpolate_with_function`] run with
    /// [`Animatable::interpolate`]; see these functions for details.
    ///
    /// # Errors
    /// Errors if the specified time is outside of range represented by this buffer.
    #[cfg(feature = "bevy_animation")]
    pub fn interpolate(&self, time: Duration) -> Result<Option<T>, OutOfRecordedRangeError>
    where
        T: Clone + Animatable,
    {
        self.interpolate_with_function(Animatable::interpolate, time)
    }

    /// Pick the best moment applicable for the specified time with "Pick B" behavior.
    ///
    /// This is convenience shortcut for `buf.find_for_interpolation(time)?.pick_b()`.
    ///
    /// # Errors
    /// Errors if the specified time is outside of range represented by this buffer.
    pub fn rewind_to(&self, time: Duration) -> Result<&Moment<T>, OutOfRecordedRangeError> {
        Ok(self.find_for_interpolation(time)?.pick_b())
    }

    /// Insert a new moment at the end.
    ///
    /// This does not delete any moments, which might allow for accidental infinite growth; thus
    /// it's recommended to use [`Self::rotate`] instead whenever applicable.
    ///
    /// # Panics
    ///
    /// Panics if this moment is earlier than or at the same time as the currently last recorded
    /// moment, because that violates the internal ordering.
    pub fn push(&mut self, new: Moment<T>) {
        if let Some(last) = self.last_moment() {
            assert!(
                new.time > last.time,
                concat!(
                    "Discontinuity in recorded moments:\n",
                    "Last moment's time is {:?},\n",
                    "but tried inserting a moment at time {:?}."
                ),
                last.time,
                new.time
            );
        }

        self.moments.push_back(new);
    }

    /// Delete all moments older than specified time and then insert a new one. This is the
    /// recommended method for inserting new moments in order to prevent infinite growth.
    pub fn rotate(&mut self, delete_older_than: Duration, moment: Moment<T>) {
        self.delete_before(delete_older_than);
        self.push(moment);
    }

    /// Delete all moments older than the specified time.
    pub fn delete_before(&mut self, delete_before: Duration) {
        while self.moments.front().is_some_and(|m| m.time < delete_before) {
            self.moments.pop_front();
        }
    }

    /// Delete all moments newer than the specified time.
    pub fn delete_after(&mut self, delete_after: Duration) {
        while self.moments.back().is_some_and(|m| m.time > delete_after) {
            self.moments.pop_back();
        }
    }

    /// Overwrite the item in last `n` moments (or all moments, if there's less than `n` of them).
    pub fn enforce_for_n_last(&mut self, n: usize, item: Option<(ChangeDetectionState, &T)>)
    where
        T: Clone,
    {
        for m in self.moments.iter_mut().rev().take(n) {
            m.item = item.map(|x| (x.0, x.1.clone()));
        }
    }

    /// Delete all moments stored in this buffer.
    pub fn clear(&mut self) {
        self.moments.clear();
    }
    /// Returns true if the buffer is empty i.e. has no recorded moments at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.moments.is_empty()
    }

    /// Returns true if the buffer has at least one moment with an item present, false otherwise.
    #[must_use]
    pub fn has_present_items(&self) -> bool {
        self.iter().any(|x| x.item.is_some())
    }

    /// Returns an iterator over all moments stored inside this buffer.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, Moment<T>> {
        self.moments.iter()
    }

    /// Returns the amount of moments stored inside this buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.moments.len()
    }

    /// Returns a reference to the inner buffer.
    // Not an `impl Deref` because that pollutes the methods list for this type.
    #[must_use]
    pub fn inner(&self) -> &VecDeque<Moment<T>> {
        &self.moments
    }

    /// Returns a mutable reference to the inner buffer.
    ///
    /// # Safety
    ///
    /// Caller must ensure that any modifications of the timestamps of the stored moments never put
    /// them out of order and never make two moments have an equal timestamp. In other words, for
    /// all pairs of moments `a` and `b`, it must always be true that `a.time < b.time`.
    ///
    /// A safe alternative for this is [`RewindBuffer::iter_mut`].
    #[must_use]
    pub unsafe fn inner_mut(&mut self) -> &mut VecDeque<Moment<T>> {
        &mut self.moments
    }

    /// Returns an iterator over proxies for all moments.
    #[must_use]
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = MomentMutProxy<'_, T>> {
        self.moments.iter_mut().map(|m| m.as_proxy())
    }
}

impl<'a, T> IntoIterator for &'a RewindBuffer<T> {
    type Item = &'a Moment<T>;
    type IntoIter = Iter<'a, Moment<T>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// `Duration::from_secs`, for brevity.
    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    /// Returns a dummy moment at specified time, for brevity.
    fn dummymoment(seconds: u64) -> Moment<()> {
        Moment {
            time: secs(seconds),
            snap_to: false,
            item: None,
        }
    }

    #[test]
    fn insert() {
        let mut buf = RewindBuffer::<()>::new();

        buf.push(dummymoment(10));
        buf.push(dummymoment(20));

        let mut iter = buf.iter();

        assert_eq!(iter.next().unwrap().time, secs(10));
        assert_eq!(iter.next().unwrap().time, secs(20));
        assert!(iter.next().is_none());
    }

    #[test]
    #[should_panic(
        expected = "Discontinuity in recorded moments:\nLast moment's time is 10s,\nbut tried inserting a moment at time 10s."
    )]
    fn insert_at_same_time() {
        let mut buf = RewindBuffer::<()>::new();

        buf.push(dummymoment(10));
        buf.push(dummymoment(10));
    }

    #[test]
    #[should_panic(
        expected = "Discontinuity in recorded moments:\nLast moment's time is 10s,\nbut tried inserting a moment at time 5s."
    )]
    fn insert_out_or_order() {
        let mut buf = RewindBuffer::<()>::new();

        buf.push(dummymoment(10));
        buf.push(dummymoment(5));
    }

    #[test]
    fn find_for_interpolation() {
        let mut buf = RewindBuffer::<()>::new();

        buf.push(dummymoment(10));
        buf.push(dummymoment(20));
        buf.push(dummymoment(30));

        if let Ok(FindForInterpResult::BeforeAndAfter(
            Moment { time: before, .. },
            Moment { time: after, .. },
        )) = buf.find_for_interpolation(secs(15))
        {
            assert_eq!(*before, secs(10));
            assert_eq!(*after, secs(20));
        } else {
            panic!("before and after epic fail");
        }

        if let Ok(FindForInterpResult::OnlyExact(Moment { time: exact, .. })) =
            buf.find_for_interpolation(secs(10))
        {
            assert_eq!(*exact, secs(10));
        } else {
            panic!("only exact epic fail");
        }

        if let Ok(FindForInterpResult::PreviousAndExact(
            Moment { time: previous, .. },
            Moment { time: exact, .. },
        )) = buf.find_for_interpolation(secs(20))
        {
            assert_eq!(*previous, secs(10));
            assert_eq!(*exact, secs(20));
        } else {
            panic!("previous and exact epic fail");
        }

        let Err(OutOfRecordedRangeError) = std::dbg!(buf.find_for_interpolation(secs(0))) else {
            panic!("out of range epic fail");
        };

        let Err(OutOfRecordedRangeError) = buf.find_for_interpolation(secs(99999)) else {
            panic!("out of range epic fail");
        };
    }
}
