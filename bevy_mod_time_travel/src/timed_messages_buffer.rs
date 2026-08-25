use alloc::collections::vec_deque::{Iter, VecDeque};
use core::{
    ops::{Deref, DerefMut, RangeInclusive},
    time::Duration,
};

#[cfg(feature = "bevy_reflect")]
use bevy_reflect::Reflect;

use bevy_ecs::message::Message;

/// A reversed message.
///
/// If there are messages stored in the timeline, and time is rewinded backward, messages that were
/// rewinded through are sent out in this wrapper instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[derive(Message)]
pub struct Reverse<M>(pub M);

impl<M> AsRef<M> for Reverse<M> {
    fn as_ref(&self) -> &M {
        &self.0
    }
}
impl<M> AsMut<M> for Reverse<M> {
    fn as_mut(&mut self) -> &mut M {
        &mut self.0
    }
}
impl<M> std::ops::Deref for Reverse<M> {
    type Target = M;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<M> std::ops::DerefMut for Reverse<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Stores a single message `M` with a timestamp.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct TimedMessage<M> {
    /// Time at which this message was sent.
    pub time: Duration,
    /// The message in question.
    pub message: M,
}

// boiled plate

impl<M> AsRef<M> for TimedMessage<M> {
    fn as_ref(&self) -> &M {
        &self.message
    }
}
impl<M> AsMut<M> for TimedMessage<M> {
    fn as_mut(&mut self) -> &mut M {
        &mut self.message
    }
}
impl<M> Deref for TimedMessage<M> {
    type Target = M;
    fn deref(&self) -> &Self::Target {
        &self.message
    }
}
impl<M> DerefMut for TimedMessage<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.message
    }
}

/// A buffer that holds a rolling queue of [`TimedMessage<M>`] values and can be used to rewind or
/// replay messages within the stored interval.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
pub struct TimedMessagesBuffer<M> {
    /// Must always remain sorted by every timed message's time.
    messages: VecDeque<TimedMessage<M>>,
}

impl<M> Default for TimedMessagesBuffer<M> {
    fn default() -> Self {
        Self {
            messages: VecDeque::default(),
        }
    }
}

impl<M> TimedMessagesBuffer<M> {
    /// Create a new buffer for timed messages.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty buffer with space for at least capacity timed messages.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(capacity),
        }
    }

    /// Returns the first message contained in this buffer, if any.
    #[must_use]
    pub fn first_message(&self) -> Option<&TimedMessage<M>> {
        self.messages.front()
    }

    /// Returns the last message contained in this buffer, if any.
    #[must_use]
    pub fn last_message(&self) -> Option<&TimedMessage<M>> {
        self.messages.back()
    }

    /// Find timed messages for exactly this time within this buffer. Messages are returned in
    /// chronological order.
    pub fn find_messages_exact(&self, time: Duration) -> impl Iterator<Item = &TimedMessage<M>> {
        self.messages.iter().filter(move |m| m.time == time)
    }

    /// Returns the range of messages this buffer is currently representing. This range is from the
    /// first message to last message, inclusively.
    ///
    /// Note that this might be a bit misleading: it is possible that a wider range of messages was
    /// recorded into this buffer, but during the first or last part of that range, there were no
    /// messages. Such a range is not represented within this buffer, and the resulting represented
    /// range is "trimmed" from the first message to last.
    ///
    /// Returns [`None`] if there are no messages, or a range consisting of the same time twice if
    /// there's only one message.
    #[must_use]
    pub fn represented_range(&self) -> Option<RangeInclusive<Duration>> {
        let (Some(first), Some(last)) = (self.first_message(), self.last_message()) else {
            return None;
        };

        // If there's only one moment, first == last.

        Some(first.time..=last.time)
    }

    /// Determine if the provided time is within range currently represented by this buffer.
    ///
    /// Note that this might be a bit misleading: it is possible that a wider range of messages was
    /// recorded into this buffer, but during the first or last part of that range, there were no
    /// messages. Such a range is not represented within this buffer, and the resulting represented
    /// range is "trimmed" from the first message to last.
    #[must_use]
    pub fn time_in_range(&self, time: Duration) -> bool {
        self.represented_range().is_some_and(|r| r.contains(&time))
    }

    /// Returns an iterator over all moments stored inside this buffer.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, TimedMessage<M>> {
        self.messages.iter()
    }

    /// Returns the amount of messages stored inside this buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Returns a reference to the inner buffer.
    // Not an `impl Deref` because that pollutes the methods list for this type.
    #[must_use]
    pub fn inner(&self) -> &VecDeque<TimedMessage<M>> {
        &self.messages
    }

    /// Returns a mutable reference to the inner buffer.
    ///
    /// # Safety
    ///
    /// Caller must ensure that any modifications of the timestamps of the stored messages never put
    /// them out of order.
    #[must_use]
    pub unsafe fn inner_mut(&mut self) -> &mut VecDeque<TimedMessage<M>> {
        &mut self.messages
    }

    /// Delete all messages stored in this buffer.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Returns true if the buffer is empty i.e. has no recorded messages at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Return an iterator of messages that need to be sent when rewinding from `from` to `to`,
    /// without reversing the order chronologically.
    ///
    /// Items returned by this range are not chronological to the input, but are to the order within
    /// the buffer. In other words, if your `from` value is greater than `to`, you should use the
    /// returned iterator in reverse.
    ///
    /// This returns all messages within this range, inclusive to `to`, but *exclusive* to `from`.
    /// This way, using this method to rewind from 5s to 10s then from 10s to 15s returns no
    /// duplicates.
    ///
    /// If you need messages for exactly the `from` time, use [`Self::find_messages_exact`].
    #[must_use]
    pub fn rewind_from_to_nonchronological(
        &self,
        from: Duration,
        to: Duration,
    ) -> impl DoubleEndedIterator<Item = &TimedMessage<M>> {
        // Sort them over real quick...
        let (a, b) = if from > to { (to, from) } else { (from, to) };

        self.iter()
            .filter(move |m| m.time >= a && m.time <= b && m.time != from)
    }

    /// Insert a new message at the end.
    ///
    /// This does not delete any messages, which might allow for accidental infinite growth; thus
    /// it's recommended to use [`Self::rotate`] instead whenever applicable.
    ///
    /// # Panics
    ///
    /// Panics if the message is earlier than the currently last recorded message, because that
    /// violates internal ordering. If you need to insert a message at an arbitrary time, use
    /// [`Self::insert_in_order`] instead.
    pub fn push(&mut self, new: TimedMessage<M>) {
        if let Some(last) = self.last_message() {
            assert!(
                new.time >= last.time,
                concat!(
                    "Discontinuity in recorded messages:\n",
                    "Last message's time is {:?},\n",
                    "but tried inserting a message at time {:?}."
                ),
                last.time,
                new.time
            );
        }

        self.messages.push_back(new);
    }

    /// Overwrite a given range with given messages. Messages within the range are deleted, then
    /// provided messages are inserted at start of the range.
    pub fn overwrite_range(
        &mut self,
        range: &RangeInclusive<Duration>,
        with: impl Iterator<Item = M>,
    ) {
        self.messages
            .retain(|m| &m.time < range.start() || &m.time > range.end());

        self.insert_many_in_order(*range.start(), with);
    }

    /// Insert a single message at a specified time. Order is preserved. If you have many messages,
    /// use [`Self::insert_many_in_order`] instead.
    pub fn insert_in_order(&mut self, message: TimedMessage<M>) {
        self.insert_many_in_order(message.time, [message.message].into_iter());
    }

    /// Insert many messages at a specified time. Order is preserved.
    pub fn insert_many_in_order(&mut self, time: Duration, messages: impl Iterator<Item = M>) {
        let messages = messages.map(|message| TimedMessage { time, message });

        let Some(first_message_after_time_idx) = self
            .iter()
            .enumerate()
            .find(|x| x.1.time > time)
            .map(|x| x.0)
        else {
            // Empty buffer? Don't mind if I do.
            self.messages.extend(messages);
            return;
        };

        for (i, message) in messages.enumerate() {
            self.messages
                .insert(first_message_after_time_idx + i, message);
        }
    }

    /// Delete all messages older than the specified time.
    pub fn delete_before(&mut self, delete_before: Duration) {
        while self
            .messages
            .front()
            .is_some_and(|m| m.time < delete_before)
        {
            self.messages.pop_front();
        }
    }

    /// Delete all messages newer than the specified time.
    pub fn delete_after(&mut self, delete_after: Duration) {
        while self.messages.back().is_some_and(|m| m.time > delete_after) {
            self.messages.pop_back();
        }
    }

    /// Delete all messages older than the specified time and then insert a new one. This is the
    /// recommended method for inserting new moments in order to prevent infinite growth.
    pub fn rotate(&mut self, delete_older_than: Duration, new: TimedMessage<M>) {
        self.delete_before(delete_older_than);
        self.push(new);
    }
}

impl<'a, M> IntoIterator for &'a TimedMessagesBuffer<M> {
    type Item = &'a TimedMessage<M>;
    type IntoIter = Iter<'a, TimedMessage<M>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
