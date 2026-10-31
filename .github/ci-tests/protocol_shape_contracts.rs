//! Reuse message contracts while preserving observable choice entries.
use super::{Protocol, localside};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const COUNT: usize = 6;

/// Source positions whose sends have the same public selector in opposite
/// parallel arms. The receiver is inferred by send(), so it cannot distinguish
/// these calls. Receives have distinct source-occurrence evidence.
pub(super) fn parallel_selector_witnesses(source: &Protocol) -> Vec<(u8, u8)> {
    fn collect(source: &Protocol, witnesses: &mut Vec<(u8, u8)>) -> BTreeMap<(u8, u8, bool), u8> {
        match source {
            Protocol::Message {
                from,
                event,
                contract,
                owned,
                ..
            } => BTreeMap::from([((*from, *contract, *owned), *event)]),
            Protocol::Roll(body) => collect(body, witnesses),
            Protocol::Seq(left, right)
            | Protocol::Par(left, right)
            | Protocol::Route(left, right)
            | Protocol::Resolved(left, right, _) => {
                let mut a = collect(left, witnesses);
                let b = collect(right, witnesses);
                if matches!(source, Protocol::Par(..)) {
                    for (selector, right) in &b {
                        if let Some(left) = a.get(selector) {
                            witnesses.push((*left, *right));
                        }
                    }
                }
                for (selector, event) in b {
                    a.entry(selector).or_insert(event);
                }
                a
            }
        }
    }
    let mut witnesses = Vec::new();
    collect(source, &mut witnesses);
    witnesses
}

/// A global head has no earlier action in its visit, regardless of the
/// resource holder. Compare those sends with their sequential continuations,
/// including continuations after a parallel join. Other local heads are left
/// unclassified rather than guessed from a logical role's earlier messages.
pub(super) fn reentry_selector_witnesses(source: &Protocol) -> Vec<(u8, u8, u8)> {
    fn heads<'a>(source: &'a Protocol, found: &mut Vec<&'a Protocol>) {
        match source {
            Protocol::Message { .. } => found.push(source),
            Protocol::Seq(left, _) | Protocol::Roll(left) => heads(left, found),
            Protocol::Par(left, right)
            | Protocol::Route(left, right)
            | Protocol::Resolved(left, right, _) => {
                heads(left, found);
                heads(right, found);
            }
        }
    }
    fn walk(source: &Protocol, continuation: &[&Protocol], found: &mut Vec<(u8, u8, u8)>) {
        match source {
            Protocol::Message { .. } => {}
            Protocol::Seq(left, right) => {
                let mut following = Vec::new();
                right.messages(&mut following);
                following.extend_from_slice(continuation);
                walk(left, &following, found);
                walk(right, continuation, found);
            }
            Protocol::Par(left, right)
            | Protocol::Route(left, right)
            | Protocol::Resolved(left, right, _) => {
                walk(left, continuation, found);
                walk(right, continuation, found);
            }
            Protocol::Roll(body) => {
                let mut first = Vec::new();
                heads(body, &mut first);
                for head in first {
                    let Protocol::Message {
                        from,
                        event,
                        contract,
                        owned,
                        ..
                    } = head
                    else {
                        unreachable!()
                    };
                    for later in continuation {
                        if let Protocol::Message {
                            from: other,
                            event: next,
                            contract: label,
                            owned: kind,
                            ..
                        } = later
                            && (from, contract, owned) == (other, label, kind)
                        {
                            found.push((*event, *next, *from));
                        }
                    }
                }
                walk(body, continuation, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(source, &[], &mut found);
    found
}

pub(super) fn reentry_diagnostic(role: u8) -> &'static str {
    match role {
        0 => {
            "rolled reentry endpoint operations must be unambiguous; role=0; help: make rolled continuation and reentry endpoint selectors distinct"
        }
        1 => {
            "rolled reentry endpoint operations must be unambiguous; role=1; help: make rolled continuation and reentry endpoint selectors distinct"
        }
        2 => {
            "rolled reentry endpoint operations must be unambiguous; role=2; help: make rolled continuation and reentry endpoint selectors distinct"
        }
        3 => {
            "rolled reentry endpoint operations must be unambiguous; role=3; help: make rolled continuation and reentry endpoint selectors distinct"
        }
        _ => unreachable!("catalog role domain"),
    }
}

/// Contract collisions across sequential sends, opposite directions and
/// independent parallel fragments, with and without an actual Endpoint loan.
pub(super) fn protocol(rank: usize) -> Protocol {
    assert!(rank < COUNT);
    let message = |from, to, owned| Protocol::Message {
        from,
        to,
        event: 0,
        contract: 0,
        owned,
    };
    let seq = |left, right| Protocol::Seq(Box::new(left), Box::new(right));
    if rank < 2 {
        return if rank == 0 {
            seq(message(0, 1, false), message(0, 2, false))
        } else {
            seq(message(1, 0, false), message(0, 2, false))
        };
    }
    let owned = rank >= 4;
    let controller = if owned { 1 } else { 0 };
    let peer = 1 - controller;
    let parallel = |left, right| Protocol::Par(Box::new(left), Box::new(right));
    let choice = Protocol::Resolved(
        Box::new(parallel(
            message(controller, peer, false),
            message(controller, 2, false),
        )),
        Box::new(parallel(
            seq(
                message(controller, peer, false),
                message(peer, controller, false),
            ),
            message(controller, 2, false),
        )),
        0,
    );
    let body = if owned {
        seq(
            message(0, 1, true),
            seq(choice, seq(message(3, 2, false), message(1, 0, true))),
        )
    } else {
        choice
    };
    if rank % 2 == 1 {
        Protocol::Roll(Box::new(body))
    } else {
        body
    }
}

fn choice_entries(source: &Protocol, entries: &mut BTreeSet<u8>) {
    match source {
        Protocol::Message { .. } => {}
        Protocol::Roll(body) => choice_entries(body, entries),
        Protocol::Seq(left, right) | Protocol::Par(left, right) => {
            choice_entries(left, entries);
            choice_entries(right, entries);
        }
        Protocol::Route(left, right) | Protocol::Resolved(left, right, _) => {
            // A participant may first observe an arm after its global head.
            // Include every possible holder of Endpoint<3>, not only role 0.
            for arm in [left.as_ref(), right.as_ref()] {
                for actor in 0..4 {
                    for holder in 0..4 {
                        entries.extend(
                            localside::first_local(arm, actor, holder)
                                .into_iter()
                                .map(|(_, event)| event),
                        );
                    }
                }
                choice_entries(arm, entries);
            }
        }
    }
}

fn assign(source: &mut Protocol, protected: &BTreeSet<u8>, contracts: [Option<u8>; 2]) {
    match source {
        Protocol::Message {
            event,
            contract,
            owned,
            ..
        } => {
            if !protected.contains(event)
                && let Some(reused) = contracts[usize::from(*owned)]
            {
                *contract = reused;
            }
        }
        Protocol::Roll(body) => assign(body, protected, contracts),
        Protocol::Seq(left, right)
        | Protocol::Par(left, right)
        | Protocol::Route(left, right)
        | Protocol::Resolved(left, right, _) => {
            assign(left, protected, contracts);
            assign(right, protected, contracts);
        }
    }
}

/// Source identity stays fixed; only the public `g::Msg` label changes.
/// Branch entries remain distinct because generated localsides dispatch their
/// alternatives by `offer().label()`. These are three concrete assignments,
/// not an enumeration of every partition of messages into shared contracts.
pub(super) fn variants(source: &Protocol) -> Vec<(&'static str, Protocol)> {
    let mut protected = BTreeSet::new();
    choice_entries(source, &mut protected);
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let mut result = vec![("distinct", source.clone())];
    for (name, last) in [("reuse-first", false), ("reuse-last", true)] {
        let mut selected = [None; 2];
        for owned in [false, true] {
            let protected = &protected;
            let candidates = |protected_only: bool| {
                messages.iter().filter_map(move |message| {
                    let Protocol::Message {
                        event,
                        contract,
                        owned: kind,
                        ..
                    } = message
                    else {
                        unreachable!()
                    };
                    (*kind == owned && (!protected_only || protected.contains(event)))
                        .then_some(*contract)
                })
            };
            let mut contracts = candidates(true).collect::<Vec<_>>();
            if contracts.is_empty() {
                contracts.extend(candidates(false));
            }
            selected[usize::from(owned)] = if last {
                contracts.last()
            } else {
                contracts.first()
            }
            .copied();
        }
        let mut variant = source.clone();
        assign(&mut variant, &protected, selected);
        if result.iter().all(|(_, previous)| previous != &variant) {
            result.push((name, variant));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from: u8, to: u8, event: u8, owned: bool) -> Protocol {
        Protocol::Message {
            from,
            to,
            event,
            contract: event,
            owned,
        }
    }

    #[test]
    fn repeated_contracts_keep_source_identity_and_payload_kind() {
        let source = Protocol::Seq(
            Box::new(message(0, 1, 0, true)),
            Box::new(Protocol::Seq(
                Box::new(message(3, 2, 1, false)),
                Box::new(message(1, 0, 2, true)),
            )),
        );
        let variants = variants(&source);
        assert_eq!(variants.len(), 3);
        let mut messages = Vec::new();
        variants[1].1.messages(&mut messages);
        assert!(matches!(
            messages.as_slice(),
            [
                Protocol::Message {
                    event: 0,
                    contract: 0,
                    owned: true,
                    ..
                },
                Protocol::Message {
                    event: 1,
                    contract: 1,
                    owned: false,
                    ..
                },
                Protocol::Message {
                    event: 2,
                    contract: 0,
                    owned: true,
                    ..
                },
            ]
        ));
    }

    #[test]
    fn an_interior_receive_can_share_an_independent_send_contract() {
        let source = Protocol::Resolved(
            Box::new(Protocol::Par(
                Box::new(message(0, 1, 0, false)),
                Box::new(message(0, 2, 1, false)),
            )),
            Box::new(Protocol::Par(
                Box::new(Protocol::Seq(
                    Box::new(message(0, 1, 2, false)),
                    Box::new(message(1, 0, 3, false)),
                )),
                Box::new(message(0, 2, 4, false)),
            )),
            0,
        );
        let variants = variants(&source);
        assert_eq!(variants.len(), 3);
        let reused = &variants[2].1;
        assert_eq!(reused.contract_for(3), reused.contract_for(4));
        for entry in [0, 1, 2, 4] {
            assert_eq!(reused.contract_for(entry), entry);
        }
    }

    #[test]
    fn an_uninvolved_actor_first_observing_an_interior_message_is_protected() {
        let source = Protocol::Route(
            Box::new(Protocol::Seq(
                Box::new(message(0, 1, 0, false)),
                Box::new(message(1, 2, 1, false)),
            )),
            Box::new(Protocol::Seq(
                Box::new(message(0, 1, 2, false)),
                Box::new(message(1, 2, 3, false)),
            )),
        );
        assert_eq!(variants(&source).len(), 1);
    }

    #[test]
    fn reused_parallel_contracts_are_source_proven_compile_failures() {
        let source = Protocol::Par(
            Box::new(message(0, 1, 0, false)),
            Box::new(message(0, 2, 1, false)),
        );
        let variants = variants(&source);
        assert_eq!(variants.len(), 3);
        assert!(super::super::syntax_violations(&variants[0].1).is_empty());
        for (_, variant) in &variants[1..] {
            assert!(
                super::super::syntax_violations(variant)
                    .iter()
                    .any(|(problem, _)| *problem == "ParallelSelector")
            );
        }
    }

    #[test]
    fn parallel_selector_witnesses_distinguish_role_contract_and_payload() {
        for left_role in 0..4 {
            for right_role in 0..4 {
                for left_label in 0..3 {
                    for right_label in 0..3 {
                        for left_owned in [false, true] {
                            for right_owned in [false, true] {
                                let left = message(left_role, (left_role + 1) % 4, 0, left_owned);
                                let mut right =
                                    message(right_role, (right_role + 2) % 4, 1, right_owned);
                                let mut left = left;
                                if let Protocol::Message { contract, .. } = &mut left {
                                    *contract = left_label;
                                }
                                if let Protocol::Message { contract, .. } = &mut right {
                                    *contract = right_label;
                                }
                                let expected = left_role == right_role
                                    && left_label == right_label
                                    && left_owned == right_owned;
                                assert_eq!(
                                    parallel_selector_witnesses(&Protocol::Par(
                                        Box::new(left.clone()),
                                        Box::new(right.clone())
                                    )),
                                    if expected { vec![(0, 1)] } else { vec![] }
                                );
                                for sequential in [
                                    Protocol::Seq(Box::new(left.clone()), Box::new(right.clone())),
                                    Protocol::Route(Box::new(left), Box::new(right)),
                                ] {
                                    assert!(parallel_selector_witnesses(&sequential).is_empty());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_collisions_survive_compound_children_and_nested_scopes() {
        let left = Protocol::Seq(
            Box::new(message(1, 0, 0, false)),
            Box::new(Protocol::Roll(Box::new(message(0, 1, 1, false)))),
        );
        let mut right = message(0, 2, 3, false);
        if let Protocol::Message { contract, .. } = &mut right {
            *contract = 1;
        }
        let right = Protocol::Resolved(Box::new(message(2, 0, 2, false)), Box::new(right), 0);
        let source = Protocol::Roll(Box::new(Protocol::Par(Box::new(left), Box::new(right))));
        assert_eq!(parallel_selector_witnesses(&source), [(1, 3)]);
    }

    #[test]
    fn reentry_heads_collide_with_a_continuation_after_a_join() {
        let mut after = message(1, 2, 2, true);
        if let Protocol::Message { contract, .. } = &mut after {
            *contract = 0;
        }
        let source = Protocol::Seq(
            Box::new(Protocol::Par(
                Box::new(Protocol::Roll(Box::new(message(1, 0, 0, true)))),
                Box::new(message(0, 2, 1, false)),
            )),
            Box::new(after),
        );
        assert_eq!(reentry_selector_witnesses(&source), [(0, 2, 1)]);
    }

    #[test]
    fn mutually_exclusive_arms_do_not_supply_reentry_continuations() {
        let source = Protocol::Route(
            Box::new(Protocol::Roll(Box::new(message(0, 1, 0, false)))),
            Box::new(message(0, 2, 0, false)),
        );
        assert!(reentry_selector_witnesses(&source).is_empty());
    }

    #[test]
    fn an_earlier_input_is_not_guessed_to_be_an_outbound_reentry_head() {
        let mut after = message(0, 2, 2, false);
        if let Protocol::Message { contract, .. } = &mut after {
            *contract = 1;
        }
        let source = Protocol::Seq(
            Box::new(Protocol::Roll(Box::new(Protocol::Seq(
                Box::new(message(1, 0, 0, false)),
                Box::new(message(0, 2, 1, false)),
            )))),
            Box::new(after),
        );
        assert!(reentry_selector_witnesses(&source).is_empty());
    }
}
