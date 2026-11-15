//! Reuse message contracts across source occurrences and choice prefixes.
use super::{Protocol, localside};
use std::collections::{BTreeMap, BTreeSet};

pub(super) use super::catalog_counts::CONTRACTS as COUNT;
pub(super) const VARIANTS: usize = 5;

/// An unresolved choice cannot distinguish two identical public sends at its
/// single entry. An enclosing callback does not resolve that inner choice.
/// Keep compound/parallel entry sets unclassified rather than guessing which
/// participant will first observe them.
pub(super) fn unresolved_entry_witnesses(source: &Protocol) -> Vec<(u8, u8)> {
    fn first(source: &Protocol) -> Option<(u8, u8, u8, bool, u8)> {
        match source {
            Protocol::Message {
                from,
                to,
                contract,
                owned,
                event,
            } if from != to => Some((*from, *to, *contract, *owned, *event)),
            Protocol::Seq(left, _) => first(left),
            _ => None,
        }
    }
    fn walk(source: &Protocol, found: &mut Vec<(u8, u8)>) {
        match source {
            Protocol::Message { .. } => (),
            Protocol::Roll(body) => walk(body, found),
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                if matches!(source, Protocol::Route(..))
                    && let (Some(a), Some(b)) = (first(a), first(b))
                    && (a.0, a.1, a.2, a.3) == (b.0, b.1, b.2, b.3)
                {
                    found.push((a.4, b.4));
                }
                walk(a, found);
                walk(b, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(source, &mut found);
    found
}

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
    if rank == 38 {
        // A completed review closes an inner request loop even when receive
        // coloring put that review on another lane. An idle parallel source
        // must not hide the second outer review behind its old completion.
        let requests = Protocol::Roll(Box::new(seq(
            message(0, 0, false),
            Protocol::Route(
                Box::new(seq(message(0, 1, false), message(1, 0, false))),
                Box::new(seq(
                    message(0, 1, false),
                    Protocol::Resolved(
                        Box::new(seq(message(1, 1, false), message(1, 0, false))),
                        Box::new(message(1, 0, false)),
                        1,
                    ),
                )),
            ),
        )));
        let work = Protocol::Roll(Box::new(seq(
            Protocol::Par(Box::new(requests), Box::new(message(2, 0, false))),
            seq(
                message(0, 2, false),
                Protocol::Resolved(
                    Box::new(seq(
                        message(2, 1, false),
                        seq(message(1, 2, false), message(2, 0, false)),
                    )),
                    Box::new(seq(
                        message(2, 1, false),
                        seq(message(1, 2, false), message(2, 0, false)),
                    )),
                    2,
                ),
            ),
        )));
        return Protocol::Par(
            Box::new(work),
            Box::new(Protocol::Roll(Box::new(Protocol::Route(
                Box::new(message(3, 1, false)),
                Box::new(message(3, 1, false)),
            )))),
        );
    }
    if rank == 33 || rank == 34 {
        // A foreign sequential tail must not hide the sender's three local
        // parallel leaves. The receiver still needs its actual inner join.
        let parallel = Protocol::Par(
            Box::new(message(if rank == 33 { 0 } else { 1 }, 2, false)),
            Box::new(seq(
                Protocol::Par(
                    Box::new(message(
                        if rank == 33 { 0 } else { 1 },
                        if rank == 33 { 2 } else { 0 },
                        rank == 34,
                    )),
                    Box::new(message(if rank == 33 { 0 } else { 1 }, 2, false)),
                ),
                message(
                    if rank == 33 { 2 } else { 0 },
                    if rank == 33 { 1 } else { 2 },
                    false,
                ),
            )),
        );
        return if rank == 33 {
            parallel
        } else {
            seq(
                message(0, 1, true),
                seq(message(3, 2, false), seq(message(2, 3, false), parallel)),
            )
        };
    }
    if rank == 32 {
        // One affine resource view and its holder's ordinary view originate
        // the same implicit choice. The actual public send chooses the arm;
        // neither an allocation flag nor a resolver preselects the request.
        return seq(
            message(0, 1, true),
            Protocol::Route(
                Box::new(seq(message(3, 0, false), message(1, 2, false))),
                Box::new(seq(message(1, 2, false), message(1, 0, false))),
            ),
        );
    }
    if rank == 37 {
        let loan = || {
            seq(
                message(0, 1, true),
                seq(
                    message(3, 2, false),
                    seq(message(2, 3, false), message(1, 0, true)),
                ),
            )
        };
        return Protocol::Roll(Box::new(Protocol::Roll(Box::new(Protocol::Resolved(
            Box::new(Protocol::Par(
                Box::new(Protocol::Resolved(Box::new(loan()), Box::new(loan()), 1)),
                Box::new(message(0, 2, false)),
            )),
            Box::new(loan()),
            0,
        )))));
    }
    if rank == 36 {
        return Protocol::Roll(Box::new(seq(
            Protocol::Resolved(
                Box::new(seq(message(0, 2, false), message(0, 1, false))),
                Box::new(seq(message(0, 2, false), message(2, 1, false))),
                0,
            ),
            Protocol::Par(
                Box::new(message(0, 1, false)),
                Box::new(message(2, 1, false)),
            ),
        )));
    }
    if rank == 35 {
        return Protocol::Resolved(
            Box::new(Protocol::Roll(Box::new(Protocol::Resolved(
                Box::new(seq(
                    message(0, 2, false),
                    seq(message(0, 1, false), message(1, 0, false)),
                )),
                Box::new(seq(
                    message(0, 2, false),
                    seq(message(2, 1, false), message(1, 0, false)),
                )),
                0,
            )))),
            Box::new(seq(message(0, 2, false), message(0, 1, false))),
            0,
        );
    }
    if rank == 31 {
        // Distinct peer lanes cannot identify the first receive of a repeated
        // choice when its old relay is delayed. Without an ACK or actual FIFO
        // fence this source must be rejected, including aliased contracts.
        return Protocol::Resolved(
            Box::new(Protocol::Roll(Box::new(Protocol::Resolved(
                Box::new(seq(message(0, 2, false), message(0, 1, false))),
                Box::new(seq(message(0, 2, false), message(2, 1, false))),
                0,
            )))),
            Box::new(seq(message(0, 2, false), message(0, 1, false))),
            0,
        );
    }
    if rank == 30 {
        // A receive waiting for a rolled retry must preserve an arriving
        // completion from the alternative arm for its own typed receive.
        return Protocol::Resolved(
            Box::new(Protocol::Roll(Box::new(message(0, 1, false)))),
            Box::new(message(0, 1, false)),
            0,
        );
    }
    if rank == 29 {
        // API close: after a rejected request and a typed forward operation,
        // the result resolver begins a parallel group. A legal repeat must
        // not hide this still-unexecuted local continuation from offer/par.
        let request = message(0, 1, false);
        let result = Protocol::Resolved(
            Box::new(Protocol::Par(
                Box::new(message(1, 2, false)),
                Box::new(message(1, 3, false)),
            )),
            Box::new(Protocol::Par(
                Box::new(message(1, 2, false)),
                Box::new(message(1, 3, false)),
            )),
            1,
        );
        let approved = seq(message(1, 2, false), seq(result, message(1, 0, false)));
        let retry = seq(message(1, 0, false), request.clone());
        return seq(
            request,
            Protocol::Resolved(
                Box::new(approved.clone()),
                Box::new(seq(Protocol::Roll(Box::new(retry)), approved)),
                0,
            ),
        );
    }
    if rank == 28 {
        // Dining: a completed rejection/request roll still has a legal next
        // preview when the controller borrows the parallel approved suffix.
        let request = message(0, 1, false);
        let approved = seq(
            Protocol::Par(
                Box::new(message(1, 2, false)),
                Box::new(message(1, 3, false)),
            ),
            message(1, 0, false),
        );
        let retry = seq(message(1, 0, false), request.clone());
        return seq(
            request,
            Protocol::Resolved(
                Box::new(approved.clone()),
                Box::new(seq(Protocol::Roll(Box::new(retry)), approved)),
                0,
            ),
        );
    }
    if rank == 27 {
        // 14H's effect owner releases the material only after consuming its
        // formulation prefix. The holder cannot use an older elastic assay
        // to outrun that data boundary after the next material return.
        return seq(
            Protocol::Roll(Box::new(seq(
                message(0, 2, false),
                seq(
                    message(0, 2, true),
                    seq(
                        message(2, 1, true),
                        seq(
                            Protocol::Roll(Box::new(seq(
                                message(3, 2, false),
                                message(2, 1, false),
                            ))),
                            message(1, 0, true),
                        ),
                    ),
                ),
            ))),
            message(0, 2, false),
        );
    }
    if rank == 26 {
        // 14H: returning the material leaves old inner acknowledgements
        // eligible. The next enclosing visit still starts at the parallel
        // grants, rather than requiring those old sequential tails to join.
        let grants = Protocol::Par(
            Box::new(message(0, 1, true)),
            Box::new(message(0, 1, false)),
        );
        let measured = seq(
            Protocol::Roll(Box::new(seq(message(3, 2, false), message(2, 1, false)))),
            message(1, 0, true),
        );
        let independent = Protocol::Roll(Box::new(seq(message(1, 2, false), message(2, 1, false))));
        return seq(
            Protocol::Roll(Box::new(seq(
                grants,
                Protocol::Par(Box::new(measured), Box::new(independent)),
            ))),
            message(0, 1, false),
        );
    }
    if rank == 25 {
        // The same delegated send contract occurs on lane 0 in the inactive
        // sequential arm and lane 1 in the selected parallel arm (14M).
        // After the outer choice and sibling send, the nested resolver runs
        // through a fragment which does not own the inactive arm's I/O port.
        let result = || {
            Protocol::Resolved(
                Box::new(message(3, 2, false)),
                Box::new(message(3, 2, false)),
                0,
            )
        };
        return seq(
            message(0, 1, true),
            seq(
                Protocol::Resolved(
                    Box::new(seq(result(), message(3, 0, false))),
                    Box::new(Protocol::Par(
                        Box::new(message(3, 0, false)),
                        Box::new(result()),
                    )),
                    0,
                ),
                message(1, 0, true),
            ),
        );
    }
    if rank == 24 {
        // The listeners participate in independent input lanes and can also
        // hold resource 3. A route's actor-wide offer lane therefore need not
        // have an I/O port in the ordinary listener view (11M reusable pool).
        let notice = || seq(message(0, 1, false), message(0, 2, false));
        let lease = |worker| {
            seq(
                notice(),
                seq(
                    message(0, worker, true),
                    seq(
                        message(3, 0, false),
                        seq(
                            message(0, 3, false),
                            seq(message(worker, 0, true), notice()),
                        ),
                    ),
                ),
            )
        };
        let leases = Protocol::Roll(Box::new(Protocol::Route(
            Box::new(lease(1)),
            Box::new(lease(2)),
        )));
        return Protocol::Par(
            Box::new(Protocol::Par(
                Box::new(message(1, 0, false)),
                Box::new(message(2, 0, false)),
            )),
            Box::new(Protocol::Route(
                Box::new(seq(leases, notice())),
                Box::new(notice()),
            )),
        );
    }
    if rank == 23 {
        // A fresh child visit can select an unused arm after the containing
        // choice's selected body completed. It retains that outer decision.
        return Protocol::Roll(Box::new(Protocol::Resolved(
            Box::new(seq(
                message(0, 1, false),
                Protocol::Roll(Box::new(Protocol::Resolved(
                    Box::new(message(0, 1, false)),
                    Box::new(seq(message(0, 1, false), message(1, 0, false))),
                    0,
                ))),
            )),
            Box::new(seq(message(0, 1, false), message(1, 0, false))),
            0,
        )));
    }
    if rank == 21 {
        // All three peers participate, but the same public send selects a
        // different first peer after two nested Right decisions (11M).
        let order = |a, b, c| {
            seq(
                message(0, a, false),
                seq(message(0, b, false), message(0, c, false)),
            )
        };
        return Protocol::Resolved(
            Box::new(Protocol::Resolved(
                Box::new(order(1, 2, 3)),
                Box::new(order(2, 1, 3)),
                0,
            )),
            Box::new(Protocol::Resolved(
                Box::new(order(2, 1, 3)),
                Box::new(order(3, 1, 2)),
                0,
            )),
            0,
        );
    }
    if rank == 22 {
        // The same relocation with an actual affine resource as payload.
        // Both workers receive a loan or the other worker's final notice.
        let loan = |worker, other| {
            seq(
                message(0, worker, true),
                seq(
                    message(3, 0, false),
                    seq(message(worker, 0, true), message(0, other, false)),
                ),
            )
        };
        return Protocol::Resolved(
            Box::new(loan(1, 2)),
            Box::new(Protocol::Resolved(
                Box::new(loan(1, 2)),
                Box::new(loan(2, 1)),
                0,
            )),
            0,
        );
    }
    if rank == 20 {
        // An unresolved choice at an old roll entry must use the same
        // custody admission as a typed send. Start the continuation's offer
        // before its peer sends, and keep that Future alive across Pending.
        let returned = || message(0, 1, true);
        let submitted = || seq(message(3, 0, false), message(1, 0, true));
        let ready = || seq(returned(), seq(message(1, 0, false), message(1, 0, true)));
        let rejected = Protocol::Resolved(
            Box::new(message(0, 1, false)),
            Box::new(message(0, 1, false)),
            0,
        );
        return seq(
            returned(),
            seq(
                submitted(),
                seq(
                    Protocol::Roll(Box::new(seq(rejected, seq(returned(), submitted())))),
                    Protocol::Resolved(Box::new(ready()), Box::new(ready()), 0),
                ),
            ),
        );
    }
    if rank == 19 {
        // A preview may keep its next choice while the carrier still needs
        // to receive the real resource back. Unlike assignment 18, this
        // continuation contains an unconsumed return, not only an old one.
        let arm = || seq(message(0, 0, false), message(3, 2, false));
        return seq(
            Protocol::Roll(Box::new(Protocol::Resolved(
                Box::new(arm()),
                Box::new(arm()),
                0,
            ))),
            seq(
                message(0, 1, true),
                seq(message(1, 0, true), message(0, 2, false)),
            ),
        );
    }
    if rank == 18 {
        // A failed result can repeat only while its entry resource is still
        // here. Submission moves that resource onward, but keeps the original
        // carrier live for a subsequent incoming result choice.
        let request = || seq(message(3, 0, false), message(1, 0, true));
        return seq(
            message(0, 1, true),
            seq(
                request(),
                seq(
                    Protocol::Resolved(
                        Box::new(message(0, 1, false)),
                        Box::new(seq(
                            Protocol::Roll(Box::new(seq(
                                message(0, 1, false),
                                seq(message(0, 1, true), request()),
                            ))),
                            message(0, 1, false),
                        )),
                        0,
                    ),
                    seq(
                        message(0, 2, true),
                        seq(
                            Protocol::Route(
                                Box::new(message(2, 0, false)),
                                Box::new(message(2, 0, false)),
                            ),
                            message(2, 0, true),
                        ),
                    ),
                ),
            ),
        );
    }
    if rank == 17 {
        // Each whole-view offer starts a new enclosing choice, while the
        // inner exchange can repeat within that chosen visit. Alternating
        // outer inputs must not remain stuck on the first committed arm.
        let arm = || {
            Protocol::Par(
                Box::new(Protocol::Roll(Box::new(seq(
                    message(0, 1, false),
                    message(1, 0, false),
                )))),
                Box::new(message(0, 2, false)),
            )
        };
        return Protocol::Roll(Box::new(Protocol::Resolved(
            Box::new(arm()),
            Box::new(arm()),
            0,
        )));
    }
    if rank == 16 {
        // Both arms use the delegated resource. Rework is initiated by its
        // holder, which returns the same Endpoint before a new parent visit.
        let checked = || seq(message(3, 2, false), message(2, 3, false));
        let choice = Protocol::Resolved(
            Box::new(seq(
                message(0, 1, false),
                seq(checked(), message(1, 0, false)),
            )),
            Box::new(seq(
                message(0, 1, false),
                seq(
                    Protocol::Roll(Box::new(seq(
                        checked(),
                        seq(message(1, 0, false), message(0, 1, false)),
                    ))),
                    message(0, 1, false),
                ),
            )),
            0,
        );
        let review = seq(
            message(0, 1, true),
            seq(message(1, 0, false), seq(choice, message(1, 0, true))),
        );
        return seq(
            Protocol::Roll(Box::new(Protocol::Par(
                Box::new(review),
                Box::new(message(2, 0, false)),
            ))),
            message(2, 0, false),
        );
    }
    if rank == 15 {
        // Finishing a right-arm suffix leaves its inner roll available while
        // the parallel sibling is absent. Reentering that child must retain
        // the parent's decision, even when its resolver input has changed.
        let review = seq(
            message(1, 0, false),
            Protocol::Resolved(
                Box::new(message(0, 1, false)),
                Box::new(seq(
                    Protocol::Roll(Box::new(seq(message(0, 1, false), message(1, 0, false)))),
                    message(0, 1, false),
                )),
                0,
            ),
        );
        return seq(
            Protocol::Roll(Box::new(Protocol::Par(
                Box::new(review),
                Box::new(message(2, 0, false)),
            ))),
            message(2, 0, false),
        );
    }
    if rank == 14 {
        return Protocol::Resolved(
            Box::new(Protocol::Resolved(
                Box::new(message(0, 0, false)),
                Box::new(message(0, 0, false)),
                0,
            )),
            Box::new(message(0, 0, false)),
            0,
        );
    }
    if rank == 12 {
        let body = seq(
            message(0, 1, true),
            seq(message(3, 1, false), message(1, 0, true)),
        );
        return seq(
            Protocol::Roll(Box::new(body)),
            Protocol::Resolved(
                Box::new(message(0, 2, false)),
                Box::new(message(0, 2, true)),
                0,
            ),
        );
    }
    if rank == 13 {
        let body = seq(
            message(0, 1, true),
            seq(message(3, 1, false), message(1, 0, true)),
        );
        return seq(
            Protocol::Roll(Box::new(body)),
            Protocol::Resolved(
                Box::new(message(2, 0, false)),
                Box::new(seq(message(2, 0, false), message(0, 2, true))),
                0,
            ),
        );
    }
    if rank >= 9 {
        let body = if rank == 11 {
            let arm = || {
                seq(
                    message(0, 1, true),
                    seq(
                        message(3, 0, false),
                        seq(message(0, 3, false), message(1, 0, true)),
                    ),
                )
            };
            Protocol::Resolved(Box::new(arm()), Box::new(arm()), 0)
        } else {
            let arm = || seq(message(0, 1, false), message(0, 1, false));
            Protocol::Resolved(Box::new(arm()), Box::new(arm()), 0)
        };
        return if rank == 10 {
            Protocol::Roll(Box::new(body))
        } else {
            body
        };
    }
    if rank == 8 {
        let resolved = |left, right| Protocol::Resolved(Box::new(left), Box::new(right), 0);
        let inner = resolved(
            resolved(message(0, 0, false), message(0, 0, false)),
            message(0, 0, false),
        );
        return Protocol::Roll(Box::new(resolved(
            seq(
                message(0, 0, false),
                seq(Protocol::Roll(Box::new(inner)), message(0, 0, false)),
            ),
            message(0, 0, false),
        )));
    }
    if rank >= 6 {
        let choice = Protocol::Route(
            Box::new(message(0, 2, false)),
            Box::new(message(0, 2, false)),
        );
        let independent = if rank == 6 {
            message(1, 2, false)
        } else {
            seq(
                message(0, 1, true),
                seq(
                    message(3, 2, false),
                    seq(message(2, 3, false), message(1, 0, true)),
                ),
            )
        };
        return Protocol::Par(Box::new(choice), Box::new(independent));
    }
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
/// The first three assignments keep observable entries distinct. A fourth
/// shares entry contracts in resolved choices, including different peers. A
/// fifth limits that sharing to the same peer, preserving independent sibling
/// sends while reusing a nested contract on different lanes. Identical
/// assignments are deduplicated. This is not every contract partition.
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
    fn heads<'a>(source: &'a Protocol, entries: &mut Vec<&'a Protocol>) {
        match source {
            Protocol::Message { .. } => entries.push(source),
            Protocol::Seq(first, _) | Protocol::Roll(first) => heads(first, entries),
            Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                heads(a, entries);
                heads(b, entries);
            }
        }
    }
    fn shared_entries(source: &Protocol, aliases: &mut BTreeMap<u8, u8>, same_peer: bool) {
        match source {
            Protocol::Message { .. } => {}
            Protocol::Roll(body) => shared_entries(body, aliases, same_peer),
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                if matches!(source, Protocol::Resolved(..)) {
                    let (mut left, mut right) = (Vec::new(), Vec::new());
                    heads(a, &mut left);
                    heads(b, &mut right);
                    for a in left {
                        for b in &right {
                            if let (
                        Protocol::Message {
                            from: af,
                            to: at,
                            contract,
                            owned: ao,
                            ..
                        },
                        Protocol::Message {
                            from: bf,
                            to: bt,
                            event,
                            owned: bo,
                            ..
                        },
                    ) = (a, *b)
                    // send::<M>() names the logical view and message, not
                    // its peer. Different recipients must share this case.
                    && (af, ao) == (bf, bo) && (!same_peer || at == bt)
                            {
                                aliases.insert(*event, *contract);
                            }
                        }
                    }
                }
                shared_entries(a, aliases, same_peer);
                shared_entries(b, aliases, same_peer);
            }
        }
    }
    fn relabel(source: &mut Protocol, aliases: &BTreeMap<u8, u8>) {
        match source {
            Protocol::Message {
                event, contract, ..
            } => {
                let mut label = *event;
                while let Some(next) = aliases.get(&label) {
                    assert!(
                        *next < label,
                        "entry aliases must follow earlier source occurrences"
                    );
                    label = *next;
                }
                if label != *event {
                    *contract = label;
                }
            }
            Protocol::Roll(body) => relabel(body, aliases),
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                relabel(a, aliases);
                relabel(b, aliases);
            }
        }
    }
    for (name, same_peer) in [("shared-entry", false), ("shared-peer-entry", true)] {
        let mut aliases = BTreeMap::new();
        shared_entries(source, &mut aliases, same_peer);
        if !aliases.is_empty() {
            let mut variant = source.clone();
            relabel(&mut variant, &aliases);
            if result.iter().all(|(_, previous)| previous != &variant) {
                result.push((name, variant));
            }
        }
    }
    assert!(result.len() <= VARIANTS);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_lane_reviews_repeat_while_the_other_source_stays_idle() {
        let mut source = protocol(38);
        super::super::syntax::number(&mut source);
        assert!(
            source.executions().any(|execution| {
                let trace = execution.trace();
                let count = |event| trace.messages.iter().filter(|(at, _)| *at == event).count();
                count(1) == 2
                    && count(3) == 2
                    && count(12) == 2
                    && count(15) == 2
                    && count(9) == 0
                    && count(16) == 0
                    && trace
                        .messages
                        .first()
                        .is_some_and(|(event, _)| *event == 15)
            }),
            "the open/commit/rework repeat must exist after the independent source's two baseline visits park"
        );
    }

    #[test]
    fn rejected_requests_reach_both_parallel_continuation_orders() {
        let mut source = protocol(28);
        super::super::syntax::number(&mut source);
        for reverse in [false, true] {
            assert!(source.executions().any(|execution| {
                let trace = execution.trace();
                let count = |event| trace.messages.iter().filter(|(at, _)| *at == event).count();
                count(4) == 2
                    && count(5) == 2
                    && count(6) == 1
                    && count(7) == 1
                    && trace
                        .messages
                        .iter()
                        .position(|(at, _)| *at == 6)
                        .zip(trace.messages.iter().position(|(at, _)| *at == 7))
                        .is_some_and(|(left, right)| (right < left) == reverse)
                    && trace.messages.last().map(|(event, _)| *event) == Some(8)
            }));
        }
    }

    #[test]
    fn service_prefix_and_returned_loan_have_two_enclosing_visits() {
        let mut source = protocol(27);
        super::super::syntax::number(&mut source);
        assert!(source.executions().any(|execution| {
            let trace = execution.trace();
            let count = |event| trace.messages.iter().filter(|(at, _)| *at == event).count();
            count(0) == 2
                && count(1) == 2
                && count(2) == 2
                && count(3) >= 2
                && count(5) == 2
                && trace.messages.last().map(|(event, _)| *event) == Some(6)
        }));
    }

    #[test]
    fn parallel_grants_restart_after_returned_material_and_retained_acknowledgements() {
        let mut source = protocol(26);
        super::super::syntax::number(&mut source);
        assert!(
            source.executions().any(|execution| {
                let trace = execution.trace();
                let count = |event| trace.messages.iter().filter(|(at, _)| *at == event).count();
                count(0) == 2
                    && count(2) >= 2
                    && count(3) >= 2
                    && count(5) >= 2
                    && trace.messages.last().map(|(event, _)| *event) == Some(7)
            }),
            "the source scripts must return the actual Endpoint and restart both grant visits"
        );
    }

    #[test]
    fn a_child_switches_to_an_unused_arm_before_the_parent_restarts() {
        let mut source = protocol(23);
        super::super::syntax::number(&mut source);
        assert!(
            source.executions().any(|execution| {
                execution
                    .trace()
                    .messages
                    .iter()
                    .map(|(event, _)| *event)
                    .eq([0, 2, 3, 1, 4, 5])
            }),
            "the finite scripts must perform the child's rejected/accepted visits before a fresh parent visit"
        );
    }

    #[test]
    fn nested_entry_aliases_cross_peers_and_reach_the_outer_contract() {
        for (rank, entries) in [(21, vec![0, 3, 6, 9]), (22, vec![0, 4, 8])] {
            let mut source = protocol(rank);
            super::super::syntax::number(&mut source);
            let variants = variants(&source);
            let shared = &variants
                .iter()
                .find(|(name, _)| *name == "shared-entry")
                .unwrap()
                .1;
            for &event in &entries {
                assert_eq!(shared.contract_for(event), entries[0]);
            }
            let mut original = Vec::new();
            let mut actual = Vec::new();
            source.messages(&mut original);
            shared.messages(&mut actual);
            assert_eq!(original.len(), actual.len());
            for (original, actual) in original.iter().zip(actual) {
                match (original, actual) {
                    (
                        Protocol::Message {
                            event: a,
                            from: af,
                            to: at,
                            owned: ao,
                            ..
                        },
                        Protocol::Message {
                            event: b,
                            from: bf,
                            to: bt,
                            owned: bo,
                            ..
                        },
                    ) => assert_eq!((a, af, at, ao), (b, bf, bt, bo)),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn delegated_nested_resolver_keeps_shared_contracts_and_sibling_first_schedules() {
        let mut source = protocol(25);
        super::super::syntax::number(&mut source);
        let variants = variants(&source);
        let shared = &variants
            .iter()
            .find(|(name, _)| *name == "shared-peer-entry")
            .unwrap()
            .1;
        assert_eq!(shared.contract_for(1), shared.contract_for(5));
        assert_eq!(shared.contract_for(2), shared.contract_for(6));
        assert_ne!(shared.contract_for(4), shared.contract_for(5));
        assert!(super::super::syntax_violations(shared).is_empty());
        for nested in [5, 6] {
            assert!(shared.executions().any(|execution| {
                let events = &execution.trace().messages;
                let position = |event| events.iter().position(|(id, _)| *id == event);
                matches!((position(0), position(4), position(nested), position(7)),
                    (Some(loan), Some(other), Some(result), Some(returned))
                    if loan < other && other < result && result < returned)
            }));
        }
    }

    #[test]
    fn sparse_listener_views_execute_both_loan_arms_in_one_repeat() {
        let mut source = protocol(24);
        super::super::syntax::number(&mut source);
        assert!(
            source.executions().any(|execution| {
                let events = &execution.trace().messages;
                let position = |event| events.iter().position(|(id, _)| *id == event);
                matches!((position(4), position(7), position(12), position(15)),
                (Some(loan_a), Some(return_a), Some(loan_b), Some(return_b))
                if loan_a < return_a && return_a < loan_b && loan_b < return_b)
            }),
            "the generated scripts must transfer the same resource to both listeners across visits"
        );
    }

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
    fn shared_entries_keep_occurrences_and_different_continuations() {
        for rank in [9, 10, 11] {
            let mut source = protocol(rank);
            super::super::syntax::number(&mut source);
            let variants = variants(&source);
            let shared = &variants
                .iter()
                .find(|(name, _)| *name == "shared-entry")
                .unwrap()
                .1;
            let body = match shared {
                Protocol::Roll(body) => body.as_ref(),
                source => source,
            };
            let Protocol::Resolved(a, b, _) = body else {
                panic!()
            };
            let (a, Some(at)) = localside::split_first(a).unwrap() else {
                panic!()
            };
            let (b, Some(bt)) = localside::split_first(b).unwrap() else {
                panic!()
            };
            assert!(localside::same_message(a, b));
            assert_ne!(a, b, "source occurrences must stay distinct");
            assert_ne!(
                at, bt,
                "the shared entry must not erase either continuation"
            );
            let mut original = Vec::new();
            let mut aliased = Vec::new();
            source.messages(&mut original);
            shared.messages(&mut aliased);
            assert_eq!(original.len(), aliased.len());
            for (original, aliased) in original.into_iter().zip(aliased) {
                let Protocol::Message {
                    event: expected,
                    owned: payload,
                    ..
                } = original
                else {
                    unreachable!()
                };
                let Protocol::Message { event, owned, .. } = aliased else {
                    unreachable!()
                };
                assert_eq!((event, owned), (expected, payload));
            }
        }
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
        let reused = &variants
            .iter()
            .find(|(name, _)| *name == "reuse-last")
            .unwrap()
            .1;
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
