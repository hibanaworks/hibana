//! Reuse message contracts while preserving observable choice entries.
use super::{Protocol, localside};
use std::collections::BTreeSet;

pub(super) const COUNT: usize = 6;

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
}
