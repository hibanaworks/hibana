//! Independent finite oracle. Enumerate concrete choices, expand two visits
//! per roll, and build a send/receive happens-before DAG. There are no causal
//! fact sets, must joins, actor candidate unions or recursive choice queries.
//! This is test evidence for these sources, not an arbitrary-source theorem.
use super::super::*;
use crate::eff::EffAtom;
use std::{vec, vec::Vec};

#[derive(Clone)]
struct Row {
    atom: EffAtom,
    actors: (u8, u8),
}

#[derive(Clone, Default)]
struct Schedule {
    selections: Vec<ownership::Selection>,
    rows: Vec<Row>,
    edges: Vec<(usize, usize)>,
}

impl Schedule {
    fn actor(&self, node: usize) -> u8 {
        let pair = self.rows[node / 2].actors;
        if node.is_multiple_of(2) {
            pair.0
        } else {
            pair.1
        }
    }

    fn sequence(&mut self, start: usize, split: usize) {
        for left in 2 * start..2 * split {
            for right in 2 * split..2 * self.rows.len() {
                if self.actor(left) == self.actor(right) {
                    self.edges.push((left, right));
                }
            }
        }
    }

    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut visited = vec![false; 2 * self.rows.len()];
        visited[from] = true;
        for node in from..to {
            if visited[node] {
                for &(left, right) in &self.edges {
                    assert!(left < right, "source-order DAG has no backward edge");
                    if left == node {
                        visited[right] = true;
                    }
                }
            }
        }
        visited[to]
    }

    fn safe(&self) -> bool {
        for (earlier, first) in self.rows.iter().enumerate() {
            for (later, next) in self.rows.iter().enumerate().skip(earlier + 1) {
                if first.atom.from != first.atom.to
                    && next.atom.from != next.atom.to
                    && first.atom.to == next.atom.to
                    && first.atom.lane == next.atom.lane
                    && first.atom.from != next.atom.from
                    && !self.reaches(2 * earlier + 1, 2 * later)
                {
                    return false;
                }
            }
        }
        true
    }
}

fn expand<const E: usize>(
    node: &tree::Source<'_>,
    event: usize,
    run: Schedule,
    source: &crate::g::source::ProgramSourceData<E>,
) -> Vec<Schedule> {
    match node {
        tree::Source::Send(_) => {
            let mut run = run;
            let index = run.rows.len();
            let actors = source
                .actors()
                .unwrap()
                .actors_at(event, &run.selections)
                .expect("a concrete source path identifies both physical actors");
            run.rows.push(Row {
                atom: source.eff_list().atom_at(event),
                actors,
            });
            run.edges.push((2 * index, 2 * index + 1));
            vec![run]
        }
        tree::Source::Seq(left, right, _) | tree::Source::Par(left, right, _) => {
            let start = run.rows.len();
            let mut result = Vec::new();
            for run in expand(left, event, run, source) {
                let split = run.rows.len();
                for mut run in expand(right, event + left.event_count(), run, source) {
                    if matches!(node, tree::Source::Seq(..)) {
                        run.sequence(start, split);
                    }
                    result.push(run);
                }
            }
            result
        }
        tree::Source::Route { left, right, .. } => {
            let scope = ownership::Scope {
                start: event as u16,
                split: (event + left.event_count()) as u16,
                stop: (event + left.event_count() + right.event_count()) as u16,
            };
            let mut result = Vec::new();
            for (arm, branch, offset) in [
                (ownership::Arm::Left, *left, event),
                (ownership::Arm::Right, *right, event + left.event_count()),
            ] {
                let mut run = run.clone();
                run.selections.retain(|selection| selection.scope != scope);
                run.selections.push(ownership::Selection { scope, arm });
                result.extend(expand(branch, offset, run, source));
            }
            result
        }
        tree::Source::Roll(body, _) => {
            let start = run.rows.len();
            let mut result = vec![run];
            for _ in 0..2 {
                let mut next = Vec::new();
                for mut run in result {
                    let split = run.rows.len();
                    run.selections.retain(|selection| {
                        !(event <= selection.scope.start as usize
                            && selection.scope.stop as usize <= event + body.event_count())
                    });
                    for mut run in expand(body, event, run, source) {
                        // An enclosing parallel sibling is outside start.
                        // Link only this roll's already expanded visits.
                        run.sequence(start, split);
                        next.push(run);
                    }
                }
                result = next;
            }
            result
        }
    }
}

pub(super) fn check<const E: usize>(
    node: &tree::Source<'_>,
    source: &crate::g::source::ProgramSourceData<E>,
) -> bool {
    expand(node, 0, Schedule::default(), source)
        .iter()
        .all(Schedule::safe)
}
