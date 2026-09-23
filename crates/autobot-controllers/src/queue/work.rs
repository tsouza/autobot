//! The bounded work queue, FIFO within priority, with reserved control capacity.

use autobot_kernel::profile::{ApiBudget, ControlWork};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::hash::Hash;
use std::num::NonZeroU32;

/// The share of the queue's keys held back for reserved control work: one key in this many,
/// rounded up.
const RESERVE_DIVISOR: usize = 10;

/// The priority of a queued key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// Control work, which dequeues before any ordinary work.
    Control(ControlWork),
    /// Ordinary work; a higher level dequeues first.
    Ordinary(u8),
}

impl Priority {
    /// The rank the queue orders by; every control class ranks above every ordinary level.
    fn rank(self) -> u16 {
        match self {
            Self::Control(_) => u16::from(u8::MAX) + 1,
            Self::Ordinary(level) => u16::from(level),
        }
    }
}

/// How [`WorkQueue::enqueue`] accepted a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The key was not queued and now is, at the back of its priority's lane.
    Queued,
    /// The key was already queued at the same or a higher rank and keeps its place.
    AlreadyQueued,
    /// The key was already queued at a lower rank and moved to the back of the new priority's
    /// lane.
    Raised,
}

/// A key the queue refused because it is full for the key's priority. Nothing was evicted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueFull<K>(pub K);

impl<K> fmt::Display for QueueFull<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the work queue is full for this priority; admission is stopped")
    }
}

impl<K: fmt::Debug> std::error::Error for QueueFull<K> {}

/// A queue size that leaves no capacity for ordinary work once the reserve is held back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueConfigError {
    /// The configured number of keys.
    pub capacity: usize,
    /// The keys the reserve would hold back.
    pub reserve: usize,
}

impl fmt::Display for QueueConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a queue of {} keys with {} reserved for control work leaves none for ordinary work",
            self.capacity, self.reserve
        )
    }
}

impl std::error::Error for QueueConfigError {}

/// A bounded queue of distinct keys, FIFO within priority, with capacity reserved for control
/// work.
#[derive(Debug, Clone)]
pub struct WorkQueue<K> {
    /// Most keys queued at once.
    capacity: usize,
    /// Keys only reserved control work may take.
    reserve: usize,
    /// Control classes that may take reserved keys.
    reserved: BTreeSet<ControlWork>,
    /// The control lane.
    control: VecDeque<K>,
    /// One lane per ordinary level.
    ordinary: BTreeMap<u8, VecDeque<K>>,
    /// Every queued key and the priority it is queued at.
    queued: HashMap<K, Priority>,
    /// Queued keys whose priority is a reserved control class.
    reserved_len: usize,
}

impl<K: Clone + Eq + Hash> WorkQueue<K> {
    /// An empty queue of `capacity` keys reserving capacity for the `reserved` control classes.
    ///
    /// # Errors
    ///
    /// [`QueueConfigError`] if the reserve leaves no key for ordinary work.
    pub fn new(
        capacity: NonZeroU32,
        reserved: BTreeSet<ControlWork>,
    ) -> Result<Self, QueueConfigError> {
        let capacity = usize::try_from(capacity.get()).unwrap_or(usize::MAX);
        let reserve = if reserved.is_empty() {
            0
        } else {
            capacity.div_ceil(RESERVE_DIVISOR)
        };
        if reserve >= capacity {
            return Err(QueueConfigError { capacity, reserve });
        }
        Ok(Self {
            capacity,
            reserve,
            reserved,
            control: VecDeque::new(),
            ordinary: BTreeMap::new(),
            queued: HashMap::new(),
            reserved_len: 0,
        })
    }

    /// The queue for a profile's API budget.
    ///
    /// # Errors
    ///
    /// As [`WorkQueue::new`].
    pub fn from_budget(budget: &ApiBudget) -> Result<Self, QueueConfigError> {
        Self::new(budget.queue_keys, budget.reserved_control.clone())
    }

    /// Most keys queued at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Keys only reserved control work may take.
    #[must_use]
    pub fn reserve(&self) -> usize {
        self.reserve
    }

    /// Keys queued now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    /// Whether no key is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// Whether `key` is queued.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.queued.contains_key(key)
    }

    /// Whether a key not yet queued would be admitted at `priority` now.
    #[must_use]
    pub fn admits(&self, priority: Priority) -> bool {
        let len = self.queued.len();
        len < self.capacity
            && (self.is_reserved(priority)
                || len - self.reserved_len < self.capacity - self.reserve)
    }

    /// Queues `key` at `priority`.
    ///
    /// A key already queued is always accepted; see [`Admission`].
    ///
    /// # Errors
    ///
    /// [`QueueFull`] with `key` if it is not queued and the queue does not admit `priority`.
    pub fn enqueue(&mut self, key: K, priority: Priority) -> Result<Admission, QueueFull<K>> {
        if let Some(&queued) = self.queued.get(&key) {
            if priority.rank() <= queued.rank() {
                return Ok(Admission::AlreadyQueued);
            }
            self.remove_from_lane(&key, queued);
            self.push(key, priority);
            return Ok(Admission::Raised);
        }
        if !self.admits(priority) {
            return Err(QueueFull(key));
        }
        self.push(key, priority);
        Ok(Admission::Queued)
    }

    /// Removes and returns the front key of the highest-ranked non-empty lane.
    pub fn dequeue(&mut self) -> Option<(K, Priority)> {
        let key = match self.control.pop_front() {
            Some(key) => key,
            None => {
                let mut lane = self.ordinary.last_entry()?;
                let key = lane.get_mut().pop_front();
                if lane.get().is_empty() {
                    lane.remove();
                }
                key?
            }
        };
        let priority = self.queued.remove(&key)?;
        if self.is_reserved(priority) {
            self.reserved_len -= 1;
        }
        Some((key, priority))
    }

    /// Whether `priority` may take reserved keys.
    fn is_reserved(&self, priority: Priority) -> bool {
        matches!(priority, Priority::Control(class) if self.reserved.contains(&class))
    }

    /// Appends `key` to the lane of `priority` and records it.
    fn push(&mut self, key: K, priority: Priority) {
        match priority {
            Priority::Control(_) => self.control.push_back(key.clone()),
            Priority::Ordinary(level) => self
                .ordinary
                .entry(level)
                .or_default()
                .push_back(key.clone()),
        }
        if self.is_reserved(priority) {
            self.reserved_len += 1;
        }
        self.queued.insert(key, priority);
    }

    /// Removes `key`, queued at `priority`, from its lane and from the record.
    fn remove_from_lane(&mut self, key: &K, priority: Priority) {
        match priority {
            Priority::Control(_) => self.control.retain(|k| k != key),
            Priority::Ordinary(level) => {
                if let Some(lane) = self.ordinary.get_mut(&level) {
                    lane.retain(|k| k != key);
                    if lane.is_empty() {
                        self.ordinary.remove(&level);
                    }
                }
            }
        }
        if self.is_reserved(priority) {
            self.reserved_len -= 1;
        }
        self.queued.remove(key);
    }
}
