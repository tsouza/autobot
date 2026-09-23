//! Lossy deliveries of a message sequence: one message dropped, duplicated or swapped with
//! another, and every order of the sequence.

/// One delivery fault on a sequence of messages, by position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mishap {
    /// The message at this position is never delivered.
    Drop(usize),
    /// The message at this position is delivered twice in a row.
    Duplicate(usize),
    /// The messages at these two positions, the first one lower, trade places.
    Swap(usize, usize),
}

/// Every single mishap on a sequence of `len` messages: each drop, then each duplicate, then
/// each swap of two positions.
#[must_use]
pub fn mishaps(len: usize) -> Vec<Mishap> {
    let drops = (0..len).map(Mishap::Drop);
    let duplicates = (0..len).map(Mishap::Duplicate);
    let swaps = (0..len).flat_map(|i| (i + 1..len).map(move |j| Mishap::Swap(i, j)));
    drops.chain(duplicates).chain(swaps).collect()
}

/// `messages` as delivered under `mishap`. A position outside `messages` leaves it unchanged.
#[must_use]
pub fn deliver<T: Clone>(messages: &[T], mishap: Mishap) -> Vec<T> {
    let mut delivered = messages.to_vec();
    match mishap {
        Mishap::Drop(i) if i < delivered.len() => {
            delivered.remove(i);
        }
        Mishap::Duplicate(i) if i < delivered.len() => {
            delivered.insert(i, messages[i].clone());
        }
        Mishap::Swap(i, j) if i < delivered.len() && j < delivered.len() => {
            delivered.swap(i, j);
        }
        _ => {}
    }
    delivered
}

/// Every order of `messages`, each once, starting with `messages` as given. The count is the
/// factorial of the length, so it is meant for the few messages of one scenario.
#[must_use]
pub fn orders<T: Clone>(messages: &[T]) -> Vec<Vec<T>> {
    let mut all = Vec::new();
    let mut indices: Vec<usize> = (0..messages.len()).collect();
    loop {
        all.push(indices.iter().map(|&i| messages[i].clone()).collect());
        if !next_permutation(&mut indices) {
            return all;
        }
    }
}

/// Advances `indices` to the next permutation in lexicographic order; `false` after the last.
fn next_permutation(indices: &mut [usize]) -> bool {
    let Some(pivot) = (1..indices.len())
        .rev()
        .find(|&i| indices[i - 1] < indices[i])
        .map(|i| i - 1)
    else {
        return false;
    };
    let Some(successor) = (pivot + 1..indices.len())
        .rev()
        .find(|&i| indices[i] > indices[pivot])
    else {
        return false;
    };
    indices.swap(pivot, successor);
    indices[pivot + 1..].reverse();
    true
}
