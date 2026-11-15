//! Necessary receive-before-send order for distinct public call contracts.
//! A witness stays in the same roll scopes and cannot skip a choice containing
//! the receive. This excludes retained continuations from an earlier visit.
use super::Protocol;

fn required(source: &Protocol, label: u8) -> bool {
    match source {
        Protocol::Message { event: found, .. } => *found == label,
        Protocol::Seq(a, b) | Protocol::Par(a, b) => required(a, label) || required(b, label),
        Protocol::Route(..) | Protocol::Resolved(..) | Protocol::Roll(..) => false,
    }
}

fn without_roll(source: &Protocol, label: u8) -> bool {
    match source {
        Protocol::Message { event: found, .. } => *found == label,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => without_roll(a, label) || without_roll(b, label),
        Protocol::Roll(..) => false,
    }
}

fn precedes(source: &Protocol, before: u8, target: u8) -> bool {
    match source {
        Protocol::Message { .. } => false,
        Protocol::Seq(a, b) => {
            (required(a, before) && without_roll(b, target))
                || precedes(a, before, target)
                || precedes(b, before, target)
        }
        Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            precedes(a, before, target) || precedes(b, before, target)
        }
        Protocol::Roll(body) => precedes(body, before, target),
    }
}

pub(super) fn pairs(source: &Protocol) -> Vec<(u8, u8, u8)> {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let mut result = Vec::new();
    for before in &messages {
        let Protocol::Message {
            from,
            to: role,
            event: label,
            owned: false,
            ..
        } = before
        else {
            continue;
        };
        if from == role {
            continue;
        }
        if messages
            .iter()
            .filter(|m| matches!(m, Protocol::Message{event: other,..} if other == label))
            .count()
            != 1
        {
            continue;
        }
        for target in &messages {
            let Protocol::Message {
                from,
                event: next,
                contract,
                owned: false,
                ..
            } = target
            else {
                continue;
            };
            if from != role
                || messages
                    .iter()
                    .filter(|m| matches!(m, Protocol::Message{from: sender,contract: other,owned:false,..} if sender == from && other == contract))
                    .count()
                    != 1
            {
                continue;
            }
            if precedes(source, *label, *next) {
                result.push((*label, *next, *role));
            }
        }
    }
    result
}

/// An independent two-word obstruction for an unowned, sequential choice body.
/// This is deliberately narrower than production's actor/path-aware check.
/// Message identities and call order come from the source, never a diagnostic.
pub(super) fn repeated_passive_entry_witnesses(source: &Protocol) -> Vec<u8> {
    fn linear(source: &Protocol, word: &mut Vec<(u8, u8)>) -> bool {
        match source {
            Protocol::Message {
                from,
                to,
                owned: false,
                ..
            } => {
                word.push((*from, *to));
                true
            }
            Protocol::Seq(a, b) => linear(a, word) && linear(b, word),
            _ => false,
        }
    }
    fn head(word: &[(u8, u8)], actor: u8) -> Option<(usize, u8)> {
        word.iter()
            .enumerate()
            .find_map(|(index, &(from, to))| {
                (from == actor || to == actor).then_some((index, from))
            })
            .filter(|&(_, from)| from != actor)
    }
    fn handoff(first: &[(u8, u8)], second: &[(u8, u8)], seed: usize, target: usize) -> bool {
        let count = 2 * (first.len() + second.len());
        let mut edges = vec![Vec::new(); count];
        let mut latest: [Option<usize>; 256] = [None; 256];
        for (index, &(from, to)) in first.iter().chain(second).enumerate() {
            let send = index * 2;
            let recv = send + 1;
            if let Some(previous) = latest[usize::from(from)] {
                edges[previous].push(send);
            }
            edges[send].push(recv);
            latest[usize::from(from)] = Some(send);
            if let Some(previous) = latest[usize::from(to)] {
                edges[previous].push(recv);
            }
            latest[usize::from(to)] = Some(recv);
        }
        let target = 2 * (first.len() + target);
        let mut reached = vec![false; count];
        reached[seed * 2 + 1] = true;
        for before in 0..count {
            if reached[before] {
                for &after in &edges[before] {
                    reached[after] = true;
                }
            }
        }
        reached[target]
    }
    fn walk(source: &Protocol, result: &mut std::collections::BTreeSet<u8>) {
        match source {
            Protocol::Roll(body) => {
                if let Protocol::Route(a, b) | Protocol::Resolved(a, b, _) = body.as_ref() {
                    let mut left = Vec::new();
                    let mut right = Vec::new();
                    if linear(a, &mut left) && linear(b, &mut right) {
                        for (first, second) in [(&left, &right), (&right, &left)] {
                            for actor in 0..4 {
                                let (Some((seed, from)), Some((target, next))) =
                                    (head(first, actor), head(second, actor))
                                else {
                                    continue;
                                };
                                if from == next {
                                    continue;
                                }
                                let fifo = first[seed + 1..]
                                    .iter()
                                    .any(|&(sender, receiver)| sender == next && receiver == actor);
                                if !fifo && !handoff(first, second, seed, target) {
                                    result.insert(actor);
                                }
                            }
                        }
                    }
                }
                walk(body, result);
            }
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                walk(a, result);
                walk(b, result);
            }
            Protocol::Message { .. } => {}
        }
    }
    let mut messages = Vec::new();
    source.messages(&mut messages);
    if messages
        .iter()
        .any(|message| matches!(message, Protocol::Message { owned: true, .. }))
        || contains_parallel(source)
    {
        return Vec::new();
    }
    let mut result = std::collections::BTreeSet::new();
    walk(source, &mut result);
    result.into_iter().collect()
}

fn contains_parallel(source: &Protocol) -> bool {
    match source {
        Protocol::Par(..) => true,
        Protocol::Seq(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            contains_parallel(a) || contains_parallel(b)
        }
        Protocol::Roll(body) => contains_parallel(body),
        Protocol::Message { .. } => false,
    }
}

pub(super) fn reentry_diagnostic(role: u8) -> &'static str {
    const REASONS: [&str; 4] = [
        "receive lane sender change requires a causal handoff or exclusive route arms; role=0; help: add a causal handoff from the earlier receiver to the later sender; on reentry, close that handoff inside the roll",
        "receive lane sender change requires a causal handoff or exclusive route arms; role=1; help: add a causal handoff from the earlier receiver to the later sender; on reentry, close that handoff inside the roll",
        "receive lane sender change requires a causal handoff or exclusive route arms; role=2; help: add a causal handoff from the earlier receiver to the later sender; on reentry, close that handoff inside the roll",
        "receive lane sender change requires a causal handoff or exclusive route arms; role=3; help: add a causal handoff from the earlier receiver to the later sender; on reentry, close that handoff inside the roll",
    ];
    REASONS[usize::from(role)]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn msg(from: u8, to: u8, label: u8) -> Protocol {
        Protocol::Message {
            from,
            to,
            event: label,
            owned: false,
            contract: label,
        }
    }
    #[test]
    fn receiving_before_a_choice_excludes_both_early_sends() {
        let source = Protocol::Roll(Box::new(Protocol::Seq(
            Box::new(msg(2, 3, 0)),
            Box::new(Protocol::Resolved(
                Box::new(msg(3, 2, 1)),
                Box::new(msg(3, 2, 2)),
                0,
            )),
        )));
        assert_eq!(pairs(&source), [(0, 1, 3), (0, 2, 3)]);
    }
    #[test]
    fn parallel_siblings_and_retained_continuations_are_excluded() {
        for source in [
            Protocol::Par(Box::new(msg(2, 3, 0)), Box::new(msg(3, 2, 1))),
            Protocol::Seq(
                Box::new(Protocol::Roll(Box::new(msg(2, 3, 0)))),
                Box::new(msg(3, 2, 1)),
            ),
            Protocol::Seq(
                Box::new(msg(2, 3, 0)),
                Box::new(Protocol::Roll(Box::new(msg(3, 2, 1)))),
            ),
            Protocol::Seq(
                Box::new(Protocol::Route(
                    Box::new(msg(2, 3, 0)),
                    Box::new(msg(1, 3, 2)),
                )),
                Box::new(msg(3, 2, 1)),
            ),
        ] {
            assert!(pairs(&source).is_empty());
        }
    }
    #[test]
    fn a_reused_label_does_not_identify_a_future_source_step() {
        let source = Protocol::Seq(
            Box::new(msg(2, 3, 0)),
            Box::new(Protocol::Seq(
                Box::new(msg(3, 2, 1)),
                Box::new(msg(3, 2, 1)),
            )),
        );
        assert!(pairs(&source).is_empty());
    }

    #[test]
    fn mixed_peer_reentry_needs_the_actual_receiver_handoff() {
        for first in 0..3 {
            for relay in 0..3 {
                for receiver in 0..3 {
                    if first == relay || first == receiver || relay == receiver {
                        continue;
                    }
                    for closed in [false, true] {
                        let left = Protocol::Seq(
                            Box::new(msg(first, relay, 0)),
                            Box::new(msg(first, receiver, 1)),
                        );
                        let right = Protocol::Seq(
                            Box::new(msg(first, relay, 2)),
                            Box::new(msg(relay, receiver, 3)),
                        );
                        let (left, right) = if closed {
                            (
                                Protocol::Seq(Box::new(left), Box::new(msg(receiver, first, 4))),
                                Protocol::Seq(Box::new(right), Box::new(msg(receiver, first, 5))),
                            )
                        } else {
                            (left, right)
                        };
                        let source = Protocol::Roll(Box::new(Protocol::Resolved(
                            Box::new(left),
                            Box::new(right),
                            0,
                        )));
                        assert_eq!(
                            repeated_passive_entry_witnesses(&source),
                            if closed { vec![] } else { vec![receiver] }
                        );
                    }
                }
            }
        }
    }
}
