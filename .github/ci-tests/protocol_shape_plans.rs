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

/// A proposed change at one executed source node, never a runtime decision.
/// The resulting proof and operation occurrences must still be checked by EPF.
#[derive(Clone, Copy)]
enum ProposedExpansion<'s> {
    Copies(&'s Execution, usize),
    Resume {
        at: &'s Execution,
        scope: u64,
        body: &'s Protocol,
        next: &'s Execution,
    },
    Alongside {
        at: &'s Execution,
        scope: u64,
        body: &'s Protocol,
        next: &'s Execution,
    },
}

fn contains_execution(root: &Execution, target: &Execution) -> bool {
    std::ptr::eq(root, target)
        || match root {
            Execution::Message(_) => false,
            Execution::Seq(a, b) | Execution::Par(a, b, _) | Execution::Roll(a, b) => {
                contains_execution(a, target) || contains_execution(b, target)
            }
            Execution::Branch(body, _) => contains_execution(body, target),
        }
}

fn sequence_parts<'s>(
    source: &'s Protocol,
    execution: &'s Execution,
    out: &mut Vec<(&'s Protocol, &'s Execution)>,
) {
    if let (Protocol::Seq(a, b), Execution::Seq(left, right)) = (source, execution) {
        sequence_parts(a, left, out);
        sequence_parts(b, right, out);
    } else {
        out.push((source, execution));
    }
}

fn proposed_expansions<'s>(
    root: &Protocol,
    source: &'s Protocol,
    execution: &'s Execution,
    out: &mut Vec<ProposedExpansion<'s>>,
) {
    fn scope(root: &Protocol, target: &Protocol, ordinal: &mut u64) -> Option<u64> {
        if !matches!(root, Protocol::Message { .. } | Protocol::Seq(..)) {
            let current = *ordinal;
            *ordinal += 1;
            if std::ptr::eq(root, target) {
                return Some((1 << 13) | current);
            }
        }
        match root {
            Protocol::Message { .. } => None,
            Protocol::Roll(body) => scope(body, target, ordinal),
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                scope(a, target, ordinal).or_else(|| scope(b, target, ordinal))
            }
        }
    }
    fn resumptions<'s>(
        root: &Protocol,
        source: &'s Protocol,
        execution: &'s Execution,
        at: &'s Execution,
        out: &mut Vec<ProposedExpansion<'s>>,
    ) {
        match (source, execution) {
            (Protocol::Message { .. }, Execution::Message(_)) => {}
            (Protocol::Roll(body), Execution::Roll(first, second)) => {
                let scope = scope(root, source, &mut 0).expect("missing source roll");
                for next in [first.as_ref(), second.as_ref()] {
                    out.push(ProposedExpansion::Resume {
                        at,
                        scope,
                        body,
                        next,
                    });
                    out.push(ProposedExpansion::Alongside {
                        at,
                        scope,
                        body,
                        next,
                    });
                    resumptions(root, body, next, at, out);
                }
            }
            (Protocol::Seq(a, b), Execution::Seq(first, second))
            | (Protocol::Par(a, b), Execution::Par(first, second, _)) => {
                resumptions(root, a, first, at, out);
                resumptions(root, b, second, at, out);
            }
            (
                Protocol::Route(a, b) | Protocol::Resolved(a, b, _),
                Execution::Branch(body, right),
            ) => {
                resumptions(root, if *right { b } else { a }, body, at, out);
            }
            _ => panic!("source and candidate visit plan disagree"),
        }
    }
    match (source, execution) {
        (Protocol::Message { .. }, Execution::Message(_)) => {}
        (Protocol::Seq(..), Execution::Seq(..)) => {
            let mut segments = Vec::new();
            sequence_parts(source, execution, &mut segments);
            for &(source, execution) in &segments[..segments.len() - 1] {
                resumptions(root, source, execution, execution, out);
            }
            for (source, execution) in segments {
                proposed_expansions(root, source, execution, out);
            }
        }
        (Protocol::Par(a, b), Execution::Par(first, second, _)) => {
            proposed_expansions(root, a, first, out);
            proposed_expansions(root, b, second, out);
        }
        (Protocol::Route(a, b) | Protocol::Resolved(a, b, _), Execution::Branch(body, right)) => {
            proposed_expansions(root, if *right { b } else { a }, body, out);
        }
        (Protocol::Roll(body), Execution::Roll(first, second)) => {
            out.push(ProposedExpansion::Copies(execution, 1));
            out.push(ProposedExpansion::Copies(execution, 3));
            proposed_expansions(root, body, first, out);
            proposed_expansions(root, body, second, out);
        }
        _ => panic!("source and candidate visit plan disagree"),
    }
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

    pub fn write_witnesses(
        &self,
        owner: u8,
        mut output: impl std::io::Write,
    ) -> std::io::Result<()> {
        for (script, execution) in self.executions().enumerate() {
            let mut plan = super::witness::SourcePlan {
                script,
                unfolding: Vec::new(),
                choices: Vec::new(),
                calls: Vec::new(),
            };
            if expand(self.source(), Some(&execution), owner, 0, &mut plan, &[]).is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "baseline source script transfers an Endpoint from the wrong holder",
                ));
            }
            serde_json::to_writer(&mut output, &plan)?;
            writeln!(output)?;
        }
        output.flush()
    }

    /// Extra proposed source witnesses for one-call queries at the baseline
    /// cuts. These do not alter the executed scripts or the semantic rules.
    /// EPF must check each candidate before it is used as execution evidence.
    pub fn write_query_witnesses(
        &self,
        owner: u8,
        mut output: impl std::io::Write,
    ) -> std::io::Result<()> {
        let mut seen = BTreeSet::new();
        for execution in self.executions() {
            let mut emit = |mut plan: super::witness::SourcePlan| -> std::io::Result<()> {
                if seen.insert((
                    plan.unfolding.clone(),
                    plan.choices.clone(),
                    plan.calls.clone(),
                )) {
                    plan.script = seen.len() - 1;
                    serde_json::to_writer(&mut output, &plan)?;
                    writeln!(output)?;
                }
                Ok(())
            };
            let mut changes = Vec::new();
            proposed_expansions(self.source(), self.source(), &execution, &mut changes);
            let mut variants = vec![Vec::new()];
            variants.extend(changes.iter().map(|change| vec![*change]));
            // A parent may take a later visit after each earlier child took
            // only one. Vary those nested counts together: changing just the
            // parent or just one child cannot witness that same source word.
            for change in &changes {
                if let ProposedExpansion::Copies(parent, 3) = change {
                    let mut shorter = changes
                        .iter()
                        .filter(|change| {
                            matches!(change,
                            ProposedExpansion::Copies(child, 1)
                                if !std::ptr::eq(*parent, *child)
                                    && contains_execution(parent, child))
                        })
                        .copied()
                        .collect::<Vec<_>>();
                    if !shorter.is_empty() {
                        shorter.push(*change);
                        variants.push(shorter);
                    }
                }
            }
            for changed in variants {
                let mut plan = super::witness::SourcePlan {
                    script: 0,
                    unfolding: Vec::new(),
                    choices: Vec::new(),
                    calls: Vec::new(),
                };
                let expanded = expand(
                    self.source(),
                    Some(&execution),
                    owner,
                    0,
                    &mut plan,
                    &changed,
                );
                if expanded.is_some() {
                    emit(plan)?;
                } else if changed.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "baseline query witness transfers an Endpoint from the wrong holder",
                    ));
                }
            }
        }
        output.flush()
    }
}

/// Construct a candidate from the same source and visit plan as the localside.
/// EPF checks the syntax, selected ownership and every captured operation; this
/// generator is not trusted to establish any of those properties.
fn expand(
    source: &Protocol,
    execution: Option<&Execution>,
    holder: u8,
    offset: usize,
    plan: &mut super::witness::SourcePlan,
    changes: &[ProposedExpansion<'_>],
) -> Option<(usize, usize, u8)> {
    let push = |plan: &mut super::witness::SourcePlan, row| {
        let index = plan.unfolding.len();
        plan.unfolding.push(row);
        index
    };
    let Some(execution) = execution else {
        let mut messages = Vec::new();
        source.messages(&mut messages);
        return Some((push(plan, [0; 6]), messages.len(), holder));
    };
    let (row, size, after) = match (source, execution) {
        (
            Protocol::Message {
                from,
                to,
                event,
                owned,
                ..
            },
            Execution::Message(label),
        ) => {
            assert_eq!(event, label);
            // An extra visit after the continuation may no longer own the
            // transferred Endpoint. It is not a source witness, while the
            // unchanged baseline remains a mandatory, checked script.
            if *owned && *from != holder {
                return None;
            }
            let actor = |role| if role == 3 { holder } else { role };
            plan.calls.push([
                u64::from(actor(*from)),
                u64::from(*from),
                u64::from(*event),
                0,
                offset as u64,
            ]);
            if from != to {
                plan.calls.push([
                    u64::from(actor(*to)),
                    u64::from(*to),
                    u64::from(*event),
                    1,
                    offset as u64,
                ]);
            }
            ([0; 6], 1, if *owned { *to } else { holder })
        }
        (Protocol::Seq(..), Execution::Seq(..)) => {
            // The packed source preserves sequence order, not Rust's binary
            // association. EPF reconstructs a right-associated sequence of
            // messages and intact scope subtrees from those same boundaries.
            let mut segments = Vec::new();
            sequence_parts(source, execution, &mut segments);
            let mut nodes = Vec::new();
            let mut segment_ends = Vec::new();
            let mut segment_holders = Vec::new();
            let mut size = 0;
            let mut after = holder;
            for &(source, execution) in &segments {
                let (id, count, next) =
                    expand(source, Some(execution), after, offset + size, plan, changes)?;
                nodes.push(id);
                size += count;
                after = next;
                segment_ends.push(offset + size);
                segment_holders.push(after);
            }
            let mut root = nodes.pop().unwrap();
            for (index, left) in nodes.into_iter().enumerate().rev() {
                let change = changes.iter().find(|change| match change {
                    ProposedExpansion::Alongside { at, .. }
                    | ProposedExpansion::Resume { at, .. } => std::ptr::eq(*at, segments[index].1),
                    ProposedExpansion::Copies(..) => false,
                });
                if let Some(ProposedExpansion::Alongside {
                    at,
                    scope,
                    body,
                    next,
                }) = change
                    && std::ptr::eq(*at, segments[index].1)
                {
                    let start = segment_ends[index];
                    let stop = offset + size;
                    let calls = plan.calls.len();
                    let choices = plan.choices.len();
                    let (resumed, count, next_holder) =
                        expand(body, Some(next), segment_holders[index], start, plan, &[])?;
                    if next_holder != segment_holders[index] {
                        return None;
                    }
                    let resumed = push(plan, [4, resumed as u64, 0, 0, 0, 0]);
                    // The expanded order is earlier ++ par(next, tail).
                    // Preserve every existing tail occurrence, shifting its
                    // expanded position without changing its source identity.
                    for call in &mut plan.calls[..calls] {
                        if (start as u64..stop as u64).contains(&call[4]) {
                            call[4] += count as u64;
                        }
                    }
                    for choice in &mut plan.choices[..choices] {
                        if start as u64 <= choice[0] && choice[1] <= stop as u64 {
                            choice[0] += count as u64;
                            choice[1] += count as u64;
                        }
                    }
                    let earlier = push(plan, [1, left as u64, root as u64, 0, 0, 0]);
                    let left_start = if index == 0 {
                        offset
                    } else {
                        segment_ends[index - 1]
                    };
                    root = push(
                        plan,
                        [
                            8,
                            *scope,
                            earlier as u64,
                            resumed as u64,
                            (start - left_start) as u64,
                            (stop - left_start) as u64,
                        ],
                    );
                    size += count;
                    continue;
                }
                root = push(plan, [1, left as u64, root as u64, 0, 0, 0]);
                if let Some(ProposedExpansion::Resume {
                    at,
                    scope,
                    body,
                    next,
                }) = change
                    && std::ptr::eq(*at, segments[index].1)
                {
                    // Keep this canonical sequence suffix intact, then resume
                    // its retained roll. This applies inside choices, parallel
                    // children and individual outer visits as well as at root.
                    let (resumed, count, holder) =
                        expand(body, Some(next), after, offset + size, plan, &[])?;
                    root = push(plan, [7, *scope, root as u64, resumed as u64, 0, 0]);
                    size += count;
                    after = holder;
                }
            }
            return Some((root, size, after));
        }
        (Protocol::Par(a, b), Execution::Par(left, right, _)) => {
            let (left_id, left_size, left_holder) =
                expand(a, Some(left), holder, offset, plan, changes)?;
            let (right_id, right_size, right_holder) =
                expand(b, Some(right), holder, offset + left_size, plan, changes)?;
            let after = if super::localside::mobile(a) {
                left_holder
            } else {
                right_holder
            };
            (
                [2, left_id as u64, right_id as u64, 0, 0, 0],
                left_size + right_size,
                after,
            )
        }
        (Protocol::Route(a, b) | Protocol::Resolved(a, b, _), Execution::Branch(body, right)) => {
            let (left_id, left_size, left_holder) = expand(
                a,
                (!*right).then_some(body.as_ref()),
                holder,
                offset,
                plan,
                changes,
            )?;
            let (right_id, right_size, right_holder) = expand(
                b,
                right.then_some(body.as_ref()),
                holder,
                offset + left_size,
                plan,
                changes,
            )?;
            plan.choices.push([
                offset as u64,
                (offset + left_size + right_size) as u64,
                u64::from(*right),
            ]);
            (
                [3, left_id as u64, right_id as u64, 0, 0, 0],
                left_size + right_size,
                if *right { right_holder } else { left_holder },
            )
        }
        (Protocol::Roll(body), Execution::Roll(first, second)) => {
            let copies = changes
                .iter()
                .find_map(|change| match change {
                    ProposedExpansion::Copies(at, copies) if std::ptr::eq(*at, execution) => {
                        Some(*copies)
                    }
                    _ => None,
                })
                .unwrap_or(2);
            let (first_id, first_size, first_holder) =
                expand(body, Some(first), holder, offset, plan, changes)?;
            let once = push(plan, [4, first_id as u64, 0, 0, 0, 0]);
            if copies == 1 {
                return Some((once, first_size, first_holder));
            }
            let (second_id, second_size, second_holder) = expand(
                body,
                Some(second),
                first_holder,
                offset + first_size,
                plan,
                changes,
            )?;
            if copies == 3 {
                let earlier = push(plan, [5, once as u64, second_id as u64, 0, 0, 0]);
                let (third, third_size, third_holder) = expand(
                    body,
                    Some(second),
                    second_holder,
                    offset + first_size + second_size,
                    plan,
                    changes,
                )?;
                return Some((
                    push(plan, [5, earlier as u64, third as u64, 0, 0, 0]),
                    first_size + second_size + third_size,
                    third_holder,
                ));
            }
            assert_eq!(copies, 2);
            (
                [5, once as u64, second_id as u64, 0, 0, 0],
                first_size + second_size,
                second_holder,
            )
        }
        _ => panic!("source and candidate visit plan disagree"),
    };
    Some((push(plan, row), size, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invalid_baseline_witness_is_an_error_not_an_omitted_script() {
        let source = Protocol::Message {
            from: 1,
            to: 2,
            event: 0,
            contract: 0,
            owned: true,
        };
        let plans = Plans::new(&source, Coverage::Exhaustive);
        assert_eq!(
            plans.write_witnesses(0, Vec::new()).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
        );
        assert_eq!(
            plans
                .write_query_witnesses(0, Vec::new())
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData,
        );
    }

    #[test]
    fn query_witnesses_do_not_resume_a_loan_after_its_endpoint_was_transferred() {
        for rank in [12, 13] {
            let mut source = super::super::contracts::protocol(rank);
            super::super::syntax::number(&mut source);
            let plans = Plans::new(&source, Coverage::Exhaustive);
            let mut bytes = Vec::new();
            plans.write_query_witnesses(0, &mut bytes).unwrap();
            let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(!candidates.is_empty());
            let mut messages = Vec::new();
            source.messages(&mut messages);
            for mut candidate in candidates {
                candidate.calls.sort_by_key(|call| call[4]);
                let mut holder = 0;
                for call in candidate.calls {
                    let Protocol::Message {
                        from, to, owned, ..
                    } = messages[call[2] as usize]
                    else {
                        unreachable!()
                    };
                    if *owned && call[3] == 0 {
                        assert_eq!(*from, holder);
                        holder = *to;
                    }
                }
            }
        }
    }

    #[test]
    fn later_parent_visits_can_each_have_shorter_child_visits() {
        let message = |event| Protocol::Message {
            from: 0,
            to: 0,
            event,
            contract: event,
            owned: false,
        };
        let source = Protocol::Roll(Box::new(Protocol::Par(
            Box::new(Protocol::Roll(Box::new(message(0)))),
            Box::new(message(1)),
        )));
        let mut bytes = Vec::new();
        Plans::new(&source, Coverage::Exhaustive)
            .write_query_witnesses(0, &mut bytes)
            .unwrap();
        let plans: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(plans.iter().any(|plan| {
            plan.calls.iter().map(|call| call[2]).collect::<Vec<_>>() == [0, 1, 0, 1, 0, 1]
        }));
    }

    #[test]
    fn query_candidates_cover_shorter_longer_and_post_continuation_repeats() {
        let message = |event| Protocol::Message {
            from: 0,
            to: 0,
            event,
            contract: event,
            owned: false,
        };
        let source = Protocol::Seq(
            Box::new(Protocol::Roll(Box::new(message(0)))),
            Box::new(message(1)),
        );
        let plans = Plans::new(&source, Coverage::Exhaustive);
        let mut bytes = Vec::new();
        plans.write_query_witnesses(0, &mut bytes).unwrap();
        let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(candidates.len(), 5);
        assert!(
            candidates
                .iter()
                .enumerate()
                .all(|(id, plan)| id == plan.script)
        );
        let words: BTreeSet<_> = candidates
            .iter()
            .map(|plan| plan.calls.iter().map(|call| call[2]).collect::<Vec<_>>())
            .collect();
        assert_eq!(
            words,
            BTreeSet::from([
                vec![0, 1],
                vec![0, 0, 1],
                vec![0, 0, 0, 1],
                vec![0, 0, 1, 0]
            ])
        );
        assert_eq!(
            candidates
                .iter()
                .filter(|plan| plan.unfolding.last().unwrap()[0] == 7)
                .count(),
            1
        );
        assert!(
            candidates
                .iter()
                .all(|plan| plan.calls.iter().filter(|call| call[2] == 1).count() == 1)
        );
    }

    #[test]
    fn a_returned_endpoint_can_reenter_alongside_an_unreceived_continuation() {
        let message = |from, to, event, owned| Protocol::Message {
            from,
            to,
            event,
            contract: event,
            owned,
        };
        let source = Protocol::Seq(
            Box::new(Protocol::Roll(Box::new(Protocol::Par(
                Box::new(Protocol::Seq(
                    Box::new(message(0, 1, 0, true)),
                    Box::new(message(1, 0, 1, true)),
                )),
                Box::new(message(2, 0, 2, false)),
            )))),
            Box::new(message(2, 0, 3, false)),
        );
        let mut bytes = Vec::new();
        Plans::new(&source, Coverage::Exhaustive)
            .write_query_witnesses(0, &mut bytes)
            .unwrap();
        let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let plan = candidates
            .iter()
            .find(|plan| plan.unfolding.last().unwrap()[0] == 8)
            .expect("missing resumption alongside the continuation");
        let [_, _, earlier, _, start, stop] = *plan.unfolding.last().unwrap();
        assert_eq!(plan.unfolding[earlier as usize][0], 1);
        assert_eq!([start, stop], [6, 7]);
        assert_eq!(
            plan.calls
                .iter()
                .filter(|call| call[2] == 0 && call[3] == 0)
                .map(|call| call[4])
                .collect::<Vec<_>>(),
            [0, 3, 6]
        );
        assert_eq!(
            plan.calls
                .iter()
                .filter(|call| call[2] == 3)
                .map(|call| call[4])
                .collect::<Vec<_>>(),
            [9, 9]
        );
        assert_eq!(
            plan.calls
                .iter()
                .filter(|call| call[2] == 1 && call[3] == 1)
                .map(|call| (call[0], call[1], call[4]))
                .collect::<Vec<_>>(),
            [(0, 0, 1), (0, 0, 4), (0, 0, 7)]
        );
    }

    #[test]
    fn query_resumption_preserves_a_canonical_sequence_prefix() {
        let message = |event| Protocol::Message {
            from: 0,
            to: 0,
            event,
            contract: event,
            owned: false,
        };
        let source = Protocol::Seq(
            Box::new(Protocol::Seq(
                Box::new(message(0)),
                Box::new(Protocol::Roll(Box::new(message(1)))),
            )),
            Box::new(message(2)),
        );
        let mut bytes = Vec::new();
        Plans::new(&source, Coverage::Exhaustive)
            .write_query_witnesses(0, &mut bytes)
            .unwrap();
        let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let resumed = candidates
            .iter()
            .find(|plan| plan.unfolding.iter().any(|row| row[0] == 7))
            .unwrap();
        assert_eq!(
            resumed.calls.iter().map(|call| call[2]).collect::<Vec<_>>(),
            [0, 1, 1, 2, 1]
        );
        assert_eq!(
            resumed.calls.iter().map(|call| call[4]).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        // The resumed suffix is the right child of the original prefix, not
        // an illegal left-associated sequence in the packed source grammar.
        let root = resumed.unfolding.last().unwrap();
        assert_eq!(root[0], 1);
        assert_eq!(resumed.unfolding[root[2] as usize][0], 7);
    }

    #[test]
    fn query_resumption_reaches_each_executed_nested_visit() {
        let message = |event| Protocol::Message {
            from: 0,
            to: 0,
            event,
            contract: event,
            owned: false,
        };
        let source = Protocol::Roll(Box::new(Protocol::Par(
            Box::new(Protocol::Resolved(
                Box::new(message(0)),
                Box::new(Protocol::Seq(
                    Box::new(Protocol::Roll(Box::new(message(1)))),
                    Box::new(message(2)),
                )),
                0,
            )),
            Box::new(message(3)),
        )));
        let mut bytes = Vec::new();
        Plans::new(&source, Coverage::Exhaustive)
            .write_query_witnesses(0, &mut bytes)
            .unwrap();
        let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let resumed: Vec<_> = candidates
            .iter()
            .filter(|plan| plan.unfolding.iter().any(|row| row[0] == 7))
            .collect();
        assert!(!resumed.is_empty());
        for plan in &resumed {
            let steps: Vec<_> = plan.unfolding.iter().filter(|row| row[0] == 7).collect();
            assert_eq!(steps.len(), 1);
            // Outer roll, par and route precede the retained inner roll.
            assert_eq!(steps[0][1], (1 << 13) | 3);
            assert_eq!(plan.calls.iter().filter(|call| call[2] == 3).count(), 2);
        }
        let words: BTreeSet<_> = resumed
            .iter()
            .map(|plan| plan.calls.iter().map(|call| call[2]).collect::<Vec<_>>())
            .collect();
        assert!(words.contains(&vec![1, 1, 2, 1, 3, 1, 1, 2, 3]));
        assert!(words.contains(&vec![1, 1, 2, 3, 1, 1, 2, 1, 3]));
    }

    #[test]
    fn query_resumption_can_retain_an_inner_roll_after_the_outer_tail() {
        let message = |event| Protocol::Message {
            from: 0,
            to: 0,
            event,
            contract: event,
            owned: false,
        };
        let source = Protocol::Seq(
            Box::new(Protocol::Roll(Box::new(Protocol::Par(
                Box::new(Protocol::Resolved(
                    Box::new(message(0)),
                    Box::new(Protocol::Seq(
                        Box::new(Protocol::Roll(Box::new(message(1)))),
                        Box::new(message(2)),
                    )),
                    0,
                )),
                Box::new(message(3)),
            )))),
            Box::new(message(4)),
        );
        let mut bytes = Vec::new();
        Plans::new(&source, Coverage::Exhaustive)
            .write_query_witnesses(0, &mut bytes)
            .unwrap();
        let candidates: Vec<super::super::witness::SourcePlan> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let resumed = candidates
            .iter()
            .find(|plan| {
                let root = plan.unfolding.last().unwrap();
                root[0] == 7
                    && root[1] == (1 << 13) | 3
                    && plan
                        .calls
                        .iter()
                        .map(|call| call[2])
                        .eq([1, 1, 2, 3, 1, 1, 2, 3, 4, 1])
            })
            .expect("missing inner resumption after the outer continuation");
        assert_eq!(resumed.calls.iter().filter(|call| call[2] == 4).count(), 1);
        assert_eq!(resumed.calls.last().unwrap()[4], 11);
    }

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
