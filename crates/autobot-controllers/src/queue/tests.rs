use super::*;
use autobot_kernel::profile::{ControlWork, Profile};
use proptest::prelude::*;
use std::collections::{BTreeSet, VecDeque};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

const M0: &str = include_str!("../../../../profiles/m0.toml");
const CONTROL: [ControlWork; 3] = [
    ControlWork::Hold,
    ControlWork::Fence,
    ControlWork::ReceiptRepair,
];
const NANOS_PER_SEC: u128 = 1_000_000_000;

fn m0() -> Profile {
    match Profile::parse(M0) {
        Ok(p) => p,
        Err(e) => panic!("profiles/m0.toml does not parse: {e}"),
    }
}

fn nz(n: u32) -> NonZeroU32 {
    match NonZeroU32::new(n) {
        Some(n) => n,
        None => panic!("zero"),
    }
}

fn queue<K: Clone + Eq + std::hash::Hash>(
    capacity: u32,
    reserved: BTreeSet<ControlWork>,
) -> WorkQueue<K> {
    match WorkQueue::new(nz(capacity), reserved) {
        Ok(q) => q,
        Err(e) => panic!("{e}"),
    }
}

fn all_reserved() -> BTreeSet<ControlWork> {
    CONTROL.into_iter().collect()
}

fn priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        prop::sample::select(CONTROL.to_vec()).prop_map(Priority::Control),
        (0u8..4).prop_map(Priority::Ordinary),
    ]
}

fn rank(p: Priority) -> u16 {
    match p {
        Priority::Control(_) => 256,
        Priority::Ordinary(l) => u16::from(l),
    }
}

#[derive(Debug, Clone)]
enum Op {
    Enqueue(u8, Priority),
    Dequeue,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0u8..60, priority()).prop_map(|(k, p)| Op::Enqueue(k, p)),
        1 => Just(Op::Dequeue),
    ]
}

/// The specification the queue is checked against: every queued key with its priority and the
/// sequence number of its last placement.
#[derive(Default)]
struct Model {
    items: Vec<(u8, Priority, u64)>,
    seq: u64,
}

impl Model {
    fn find(&self, key: u8) -> Option<usize> {
        self.items.iter().position(|&(k, _, _)| k == key)
    }

    fn place(&mut self, key: u8, p: Priority) {
        self.seq += 1;
        self.items.push((key, p, self.seq));
    }

    /// The highest rank, and within it the earliest placement.
    fn pop(&mut self) -> Option<(u8, Priority)> {
        let i = (0..self.items.len())
            .min_by_key(|&i| (std::cmp::Reverse(rank(self.items[i].1)), self.items[i].2))?;
        let (k, p, _) = self.items.remove(i);
        Some((k, p))
    }
}

proptest! {
    /// Random interleavings of enqueues and dequeues: admission follows the capacity rules,
    /// dequeues are FIFO within priority with control first, and every accepted key comes out.
    #[test]
    fn queue_follows_priority_order_and_drops_nothing_accepted(
        capacity in 2u32..30,
        reserved in prop::collection::btree_set(prop::sample::select(CONTROL.to_vec()), 0..=3),
        ops in prop::collection::vec(op(), 0..300),
    ) {
        let mut q = queue::<u8>(capacity, reserved.clone());
        let mut model = Model::default();
        let cap = q.capacity();
        let reserve = q.reserve();
        prop_assert_eq!(cap, capacity as usize);
        prop_assert!(reserve < cap);
        prop_assert_eq!(reserve == 0, reserved.is_empty());
        let is_reserved = |p: Priority| matches!(p, Priority::Control(c) if reserved.contains(&c));
        for op in ops {
            match op {
                Op::Enqueue(key, p) => {
                    let admits = q.admits(p);
                    let got = q.enqueue(key, p);
                    if let Some(i) = model.find(key) {
                        let (_, old, _) = model.items[i];
                        if rank(p) > rank(old) {
                            prop_assert_eq!(got, Ok(Admission::Raised));
                            model.items.remove(i);
                            model.place(key, p);
                        } else {
                            prop_assert_eq!(got, Ok(Admission::AlreadyQueued));
                        }
                    } else {
                        let len = model.items.len();
                        let reserved_len =
                            model.items.iter().filter(|&&(_, p, _)| is_reserved(p)).count();
                        let allowed = len < cap && (is_reserved(p) || len - reserved_len < cap - reserve);
                        if is_reserved(p) && reserved_len < reserve {
                            prop_assert!(allowed, "reserved control work must be admitted");
                        }
                        prop_assert_eq!(admits, allowed);
                        if allowed {
                            prop_assert_eq!(got, Ok(Admission::Queued));
                            model.place(key, p);
                        } else {
                            prop_assert_eq!(got, Err(QueueFull(key)));
                        }
                    }
                }
                Op::Dequeue => prop_assert_eq!(q.dequeue(), model.pop()),
            }
            prop_assert_eq!(q.len(), model.items.len());
            prop_assert!(q.len() <= cap);
        }
        while let Some(expected) = model.pop() {
            prop_assert_eq!(q.dequeue(), Some(expected));
        }
        prop_assert_eq!(q.dequeue(), None);
        prop_assert!(q.is_empty());
    }

    /// Over every interval between two grants the limiter grants at most the burst plus what
    /// the rate earns in that interval.
    #[test]
    fn limiter_never_exceeds_burst_plus_rate(
        rate in 1u32..50,
        burst in 1u32..50,
        steps in prop::collection::vec(0u64..200_000_000, 1..300),
    ) {
        let t0 = Instant::now();
        let mut limiter = ApiLimiter::new(nz(rate), nz(burst), t0);
        let mut at = Duration::ZERO;
        let mut grants = Vec::new();
        for step in steps {
            at += Duration::from_nanos(step);
            if limiter.try_acquire(t0 + at).is_ok() {
                grants.push(at.as_nanos());
            }
        }
        for i in 0..grants.len() {
            for j in i..grants.len() {
                let earned = u128::from(rate) * (grants[j] - grants[i]) / NANOS_PER_SEC;
                let granted = (j - i + 1) as u128;
                prop_assert!(
                    granted <= u128::from(burst) + earned,
                    "{} grants in {} ns at {}/s, burst {}", granted, grants[j] - grants[i], rate, burst,
                );
            }
        }
    }

    /// A refusal names exactly when the next request is available: not a nanosecond earlier.
    #[test]
    fn limiter_refusal_names_the_exact_wait(
        rate in 1u32..50,
        burst in 1u32..50,
        steps in prop::collection::vec(0u64..200_000_000, 1..100),
    ) {
        let t0 = Instant::now();
        let mut limiter = ApiLimiter::new(nz(rate), nz(burst), t0);
        let mut now = t0;
        for step in steps {
            now += Duration::from_nanos(step);
            if let Err(wait) = limiter.try_acquire(now) {
                prop_assert!(wait > Duration::ZERO);
                let mut early = limiter.clone();
                prop_assert!(early.try_acquire(now + wait - Duration::from_nanos(1)).is_err());
                now += wait;
                prop_assert_eq!(limiter.try_acquire(now), Ok(()));
            }
        }
    }

    /// An operator loop under saturation: ordinary work keeps the queue full and every API
    /// grant dequeues one key. While the reserve lasts every reserved control key is admitted;
    /// control keys dequeue in arrival order before any ordinary key, so a control key waits
    /// only for the control keys ahead of it; and grants keep to the rate.
    #[test]
    fn control_work_dequeues_under_saturation_at_the_configured_rate(
        arrivals in prop::collection::vec(prop::option::of(prop::sample::select(CONTROL.to_vec())), 1..400),
        tick_ms in 1u64..120,
    ) {
        let budget = m0().values().api.clone();
        let t0 = Instant::now();
        let mut limiter = ApiLimiter::from_budget(&budget, t0);
        let mut q = match WorkQueue::<u32>::from_budget(&budget) {
            Ok(q) => q,
            Err(e) => panic!("{e}"),
        };
        let mut next_key = 0u32;
        let mut control_order: VecDeque<u32> = VecDeque::new();
        let mut grants: Vec<u128> = Vec::new();
        for (step, arrival) in arrivals.into_iter().enumerate() {
            let now = t0 + Duration::from_millis(tick_ms * step as u64);
            while q.admits(Priority::Ordinary(0)) {
                next_key += 1;
                prop_assert_eq!(q.enqueue(next_key, Priority::Ordinary(0)), Ok(Admission::Queued));
            }
            next_key += 1;
            prop_assert_eq!(q.enqueue(next_key, Priority::Ordinary(0)), Err(QueueFull(next_key)));
            if let Some(class) = arrival {
                next_key += 1;
                let got = q.enqueue(next_key, Priority::Control(class));
                if control_order.len() < q.reserve() {
                    prop_assert_eq!(got, Ok(Admission::Queued));
                    control_order.push_back(next_key);
                } else {
                    // The reserve is spent and ordinary work holds the rest: admission stops.
                    prop_assert_eq!(got, Err(QueueFull(next_key)));
                }
            }
            while limiter.try_acquire(now).is_ok() {
                grants.push((now - t0).as_nanos());
                let Some((key, p)) = q.dequeue() else { break };
                if let Priority::Control(_) = p {
                    prop_assert_eq!(control_order.pop_front(), Some(key));
                } else {
                    prop_assert!(control_order.is_empty(), "ordinary work dequeued ahead of control work");
                }
            }
        }
        for key in control_order {
            prop_assert_eq!(q.dequeue().map(|(k, _)| k), Some(key));
        }
        let burst = u128::from(budget.burst.get());
        let rate = u128::from(budget.requests_per_second.get());
        if let (Some(first), Some(last)) = (grants.first(), grants.last()) {
            prop_assert!(grants.len() as u128 <= burst + rate * (last - first) / NANOS_PER_SEC);
        }
    }
}

#[test]
fn m0_queue_saturated_by_ordinary_work_still_takes_and_serves_control_work() {
    let budget = m0().values().api.clone();
    let mut q = match WorkQueue::<u32>::from_budget(&budget) {
        Ok(q) => q,
        Err(e) => panic!("{e}"),
    };
    assert_eq!((q.capacity(), q.reserve()), (500, 50));
    for key in 0..450 {
        assert_eq!(q.enqueue(key, Priority::Ordinary(1)), Ok(Admission::Queued));
    }
    assert_eq!(q.enqueue(450, Priority::Ordinary(9)), Err(QueueFull(450)));
    for (i, key) in (1000..1050).enumerate() {
        let class = CONTROL[i % CONTROL.len()];
        assert_eq!(
            q.enqueue(key, Priority::Control(class)),
            Ok(Admission::Queued)
        );
    }
    assert_eq!(q.len(), 500);
    assert_eq!(
        q.enqueue(2000, Priority::Control(ControlWork::Hold)),
        Err(QueueFull(2000))
    );
    let order: Vec<u32> = std::iter::from_fn(|| q.dequeue().map(|(k, _)| k)).collect();
    let expected: Vec<u32> = (1000..1050).chain(0..450).collect();
    assert_eq!(order, expected);
}

#[test]
fn raising_a_queued_key_moves_it_and_keeps_one_entry() {
    let mut q = queue::<&str>(10, all_reserved());
    assert_eq!(q.enqueue("a", Priority::Ordinary(0)), Ok(Admission::Queued));
    assert_eq!(
        q.enqueue("b", Priority::Control(ControlWork::Fence)),
        Ok(Admission::Queued)
    );
    assert_eq!(
        q.enqueue("a", Priority::Ordinary(0)),
        Ok(Admission::AlreadyQueued)
    );
    assert_eq!(
        q.enqueue("a", Priority::Control(ControlWork::Hold)),
        Ok(Admission::Raised)
    );
    assert_eq!(
        q.enqueue("b", Priority::Ordinary(3)),
        Ok(Admission::AlreadyQueued)
    );
    assert_eq!(q.len(), 2);
    assert_eq!(
        q.dequeue(),
        Some(("b", Priority::Control(ControlWork::Fence)))
    );
    assert_eq!(
        q.dequeue(),
        Some(("a", Priority::Control(ControlWork::Hold)))
    );
    assert_eq!(q.dequeue(), None);
}

#[test]
fn an_unreserved_control_class_dequeues_first_but_uses_ordinary_capacity() {
    let mut q = queue::<u32>(10, BTreeSet::from([ControlWork::Hold]));
    assert_eq!(q.reserve(), 1);
    for key in 0..9 {
        assert_eq!(q.enqueue(key, Priority::Ordinary(0)), Ok(Admission::Queued));
    }
    assert_eq!(
        q.enqueue(9, Priority::Control(ControlWork::Fence)),
        Err(QueueFull(9))
    );
    assert_eq!(
        q.enqueue(10, Priority::Control(ControlWork::Hold)),
        Ok(Admission::Queued)
    );
    assert_eq!(q.dequeue().map(|(k, _)| k), Some(10));
    assert_eq!(q.dequeue().map(|(k, _)| k), Some(0));
    assert_eq!(
        q.enqueue(9, Priority::Control(ControlWork::Fence)),
        Ok(Admission::Queued)
    );
    assert_eq!(q.dequeue().map(|(k, _)| k), Some(9));
}

#[test]
fn a_queue_the_reserve_would_fill_is_refused() {
    assert_eq!(
        WorkQueue::<u8>::new(nz(1), all_reserved()).err(),
        Some(QueueConfigError {
            capacity: 1,
            reserve: 1
        })
    );
    assert!(WorkQueue::<u8>::new(nz(1), BTreeSet::new()).is_ok());
}

#[test]
fn m0_limiter_grants_the_burst_then_one_request_per_tenth_of_a_second() {
    let t0 = Instant::now();
    let mut limiter = ApiLimiter::from_budget(&m0().values().api, t0);
    for _ in 0..20 {
        assert_eq!(limiter.try_acquire(t0), Ok(()));
    }
    assert_eq!(limiter.try_acquire(t0), Err(Duration::from_millis(100)));
    let later = t0 + Duration::from_millis(250);
    assert_eq!(limiter.try_acquire(later), Ok(()));
    assert_eq!(limiter.try_acquire(later), Ok(()));
    assert_eq!(limiter.try_acquire(later), Err(Duration::from_millis(50)));
    assert_eq!(limiter.try_acquire(t0), Err(Duration::from_millis(50)));
}
