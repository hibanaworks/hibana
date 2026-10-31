//! Source paths and parallel poll orders used by the Endpoint replay tests.
use super::{Execution, Protocol};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub(super) enum Coverage {
    Exhaustive,
    ParallelPairs,
}

pub(super) enum Plans<'s> {
    Exhaustive(&'s Protocol),
    ParallelPairs {
        source: &'s Protocol,
        ranks: Vec<usize>,
    },
}

// Each column identifies one active par in the two-visit expansion. Two
// distinct column numbers differ in some binary digit. That digit and its
// complement supply 01/10; the constant rows supply 00/11.
fn parallel_rows(width: usize) -> Vec<Vec<bool>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    let mut rows = BTreeSet::from([vec![false; width], vec![true; width]]);
    let digits = usize::BITS - (width - 1).leading_zeros();
    for digit in 0..digits {
        let row = (0..width)
            .map(|column| (column >> digit) & 1 != 0)
            .collect::<Vec<_>>();
        rows.insert(row.iter().map(|value| !value).collect());
        rows.insert(row);
    }
    rows.into_iter().collect()
}

fn path_count(source: &Protocol) -> usize {
    match source {
        Protocol::Message { .. } => Some(1),
        Protocol::Seq(a, b) | Protocol::Par(a, b) => path_count(a).checked_mul(path_count(b)),
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            path_count(a).checked_add(path_count(b))
        }
        Protocol::Roll(body) => path_count(body).checked_mul(path_count(body)),
    }
    .expect("source path count exceeds the host usize domain")
}

pub(super) fn parallel_pair_bound(source: &Protocol) -> usize {
    fn width(source: &Protocol) -> usize {
        match source {
            Protocol::Message { .. } => 0,
            Protocol::Seq(a, b) => width(a) + width(b),
            Protocol::Par(a, b) => 1 + width(a) + width(b),
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => width(a).max(width(b)),
            Protocol::Roll(body) => 2 * width(body),
        }
    }
    path_count(source)
        .checked_mul(parallel_rows(width(source)).len())
        .expect("source script work exceeds the host usize domain")
}

fn path_at(source: &Protocol, rank: usize) -> Execution {
    match source {
        Protocol::Message { event: label, .. } => {
            assert_eq!(rank, 0);
            Execution::Message(*label)
        }
        Protocol::Seq(a, b) | Protocol::Par(a, b) => {
            let width = path_count(b);
            let left = Box::new(path_at(a, rank / width));
            let right = Box::new(path_at(b, rank % width));
            match source {
                Protocol::Seq(..) => Execution::Seq(left, right),
                Protocol::Par(..) => Execution::Par(left, right, false),
                _ => unreachable!(),
            }
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let width = path_count(a);
            if rank < width {
                Execution::Branch(Box::new(path_at(a, rank)), false)
            } else {
                Execution::Branch(Box::new(path_at(b, rank - width)), true)
            }
        }
        Protocol::Roll(body) => {
            let width = path_count(body);
            Execution::Roll(
                Box::new(path_at(body, rank / width)),
                Box::new(path_at(body, rank % width)),
            )
        }
    }
}

fn parallel_count(execution: &Execution) -> usize {
    match execution {
        Execution::Message(_) => 0,
        Execution::Seq(a, b) | Execution::Roll(a, b) => parallel_count(a) + parallel_count(b),
        Execution::Par(a, b, _) => 1 + parallel_count(a) + parallel_count(b),
        Execution::Branch(body, _) => parallel_count(body),
    }
}

fn set_parallel_orders(execution: &mut Execution, values: &mut impl Iterator<Item = bool>) {
    match execution {
        Execution::Message(_) => (),
        Execution::Seq(a, b) | Execution::Roll(a, b) => {
            set_parallel_orders(a, values);
            set_parallel_orders(b, values);
        }
        Execution::Par(a, b, reverse) => {
            *reverse = values.next().expect("missing parallel poll order");
            set_parallel_orders(a, values);
            set_parallel_orders(b, values);
        }
        Execution::Branch(body, _) => set_parallel_orders(body, values),
    }
}

fn execution_rank(source: &Protocol, execution: &Execution) -> usize {
    match (source, execution) {
        (Protocol::Message { event: label, .. }, Execution::Message(actual)) => {
            assert_eq!(label, actual);
            0
        }
        (Protocol::Seq(a, b), Execution::Seq(left, right)) => {
            execution_rank(a, left) * b.execution_count() + execution_rank(b, right)
        }
        (Protocol::Par(a, b), Execution::Par(left, right, reverse)) => {
            2 * (execution_rank(a, left) * b.execution_count() + execution_rank(b, right))
                + usize::from(*reverse)
        }
        (Protocol::Route(a, b) | Protocol::Resolved(a, b, _), Execution::Branch(body, right)) => {
            if *right {
                a.execution_count() + execution_rank(b, body)
            } else {
                execution_rank(a, body)
            }
        }
        (Protocol::Roll(body), Execution::Roll(first, second)) => {
            execution_rank(body, first) * body.execution_count() + execution_rank(body, second)
        }
        _ => panic!("source and execution constructors differ"),
    }
}

impl<'s> Plans<'s> {
    pub fn new(source: &'s Protocol, coverage: Coverage) -> Self {
        match coverage {
            Coverage::Exhaustive => Self::Exhaustive(source),
            Coverage::ParallelPairs => {
                // Validate the rank domain before computing individual ranks.
                let total = source.execution_count();
                let mut ranks = BTreeSet::new();
                for path in 0..path_count(source) {
                    let skeleton = path_at(source, path);
                    for values in parallel_rows(parallel_count(&skeleton)) {
                        let mut execution = skeleton.clone();
                        let mut values = values.into_iter();
                        set_parallel_orders(&mut execution, &mut values);
                        assert!(values.next().is_none());
                        let rank = execution_rank(source, &execution);
                        assert!(rank < total);
                        ranks.insert(rank);
                    }
                }
                Self::ParallelPairs {
                    source,
                    ranks: ranks.into_iter().collect(),
                }
            }
        }
    }

    pub fn source(&self) -> &'s Protocol {
        match self {
            Self::Exhaustive(source) | Self::ParallelPairs { source, .. } => source,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Exhaustive(source) => source.execution_count(),
            Self::ParallelPairs { ranks, .. } => ranks.len(),
        }
    }

    pub fn executions(&self) -> impl ExactSizeIterator<Item = Execution> + '_ {
        (0..self.len()).map(|index| {
            self.source().execution_at(match self {
                Self::Exhaustive(_) => index,
                Self::ParallelPairs { ranks, .. } => ranks[index],
            })
        })
    }

    pub fn write_scripts(&self, mut output: impl std::io::Write) -> std::io::Result<()> {
        for execution in self.executions() {
            writeln!(output, "{}", execution.json())?;
        }
        output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_work_bound_covers_every_script_in_small_and_heavy_contexts() {
        for length in 0..=2 {
            for rank in 0..super::super::interactions::spine_count(length) {
                let source = super::super::interactions::spine(length, rank).source;
                assert!(
                    parallel_pair_bound(&source)
                        >= Plans::new(&source, Coverage::ParallelPairs).len()
                );
            }
        }
        let source = super::super::interactions::spine(5, 33610).source;
        assert_eq!(parallel_pair_bound(&source), 65_536);
        assert_eq!(Plans::new(&source, Coverage::ParallelPairs).len(), 65_536);
    }

    #[test]
    fn binary_columns_cover_every_parallel_pair_and_both_single_orders() {
        for width in 0..=128 {
            let rows = parallel_rows(width);
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| row.len() == width));
            for a in 0..width {
                for value in [false, true] {
                    assert!(rows.iter().any(|row| row[a] == value));
                }
                for b in a + 1..width {
                    for (x, y) in [(false, false), (false, true), (true, false), (true, true)] {
                        assert!(rows.iter().any(|row| row[a] == x && row[b] == y));
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_orders_do_not_change_the_source_choice_inventory() {
        for rank in [0, 1, 32896, 32914, 32915, 33516, 33600, 33610] {
            let source = super::super::interactions::spine(5, rank).source;
            let plans = Plans::new(&source, Coverage::ParallelPairs);
            let mut choices = BTreeSet::new();
            let mut previous = None;
            for execution in plans.executions() {
                let index = execution_rank(&source, &execution);
                assert!(previous.is_none_or(|before| before < index));
                assert_eq!(source.execution_at(index).json(), execution.json());
                previous = Some(index);
                let mut skeleton = execution;
                let count = parallel_count(&skeleton);
                set_parallel_orders(&mut skeleton, &mut core::iter::repeat_n(false, count));
                choices.insert(skeleton.json());
            }
            assert_eq!(choices.len(), path_count(&source));
            for path in 0..path_count(&source) {
                assert!(choices.contains(&path_at(&source, path).json()));
            }
        }
    }
}
