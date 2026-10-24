//! Source-only ownership rules for the catalog's single Endpoint<3> resource.
use super::Protocol;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rejection {
    NonUnitSelfMessage,
    EndpointIsItsOwnCarrier,
    ConflictingInitialOwners,
    ParallelOwnershipConflict,
    RollChangesOwner,
}

impl Rejection {
    pub(super) fn problem(self) -> &'static str {
        match self {
            Self::NonUnitSelfMessage => "NonUnitSelfMessage",
            Self::EndpointIsItsOwnCarrier => "EndpointIsItsOwnCarrier",
            Self::ConflictingInitialOwners => "ConflictingInitialOwners",
            Self::ParallelOwnershipConflict => "ParallelOwnershipConflict",
            Self::RollChangesOwner => "RollChangesOwner",
        }
    }
    pub(super) fn diagnostic(self) -> &'static str {
        match self {
            Self::NonUnitSelfMessage => "self-role actions require the unit payload",
            Self::EndpointIsItsOwnCarrier => {
                "an owned endpoint cannot carry its own delegation; role=3; help: transfer the Endpoint through distinct control roles"
            }
            Self::ConflictingInitialOwners => {
                "owned endpoint has conflicting initial actors; role=3; help: give each transferred Endpoint one initial holder"
            }
            Self::ParallelOwnershipConflict => {
                "parallel endpoint delegation conflicts with another use; role=3; help: keep the transfer and dependent resource operations in sequence"
            }
            Self::RollChangesOwner => {
                "rolled endpoint delegation must restore its incoming ownership; role=3; help: return each transferred Endpoint to its incoming holder inside the roll"
            }
        }
    }
}

// Row i contains all possible final owners when i is the initial owner.
// A zero row means that some path cannot execute from that initial owner.
#[derive(Debug, PartialEq, Eq)]
struct Flow {
    ends: [u8; 3],
    transfers: bool,
    uses: bool,
}

impl Flow {
    fn input_mask(&self) -> u8 {
        self.ends.iter().enumerate().fold(0, |mask, (owner, ends)| {
            mask | (u8::from(*ends != 0) << owner)
        })
    }
    fn finish(self) -> Result<Self, Rejection> {
        if self.input_mask() == 0 {
            Err(Rejection::ConflictingInitialOwners)
        } else {
            Ok(self)
        }
    }
}

fn flow(source: &Protocol) -> Result<Flow, Rejection> {
    match source {
        Protocol::Message {
            from, to, owned, ..
        } => {
            let mut ends = [1, 2, 4];
            if *owned {
                // Invalid carriers and non-unit self operations are classified
                // before ownership flow is checked.
                assert!(*from < 3 && *to < 3 && from != to);
                ends = [0; 3];
                ends[usize::from(*from)] = 1 << *to;
            }
            Ok(Flow {
                ends,
                transfers: *owned,
                uses: *owned || *from == 3 || *to == 3,
            })
        }
        Protocol::Roll(body) => {
            let body = flow(body)?;
            let ends = core::array::from_fn(|owner| {
                if body.ends[owner] == 1 << owner {
                    1 << owner
                } else {
                    0
                }
            });
            if ends == [0; 3] {
                return Err(Rejection::RollChangesOwner);
            }
            Ok(Flow { ends, ..body })
        }
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => {
            let a = flow(a)?;
            let b = flow(b)?;
            let ends = match source {
                Protocol::Seq(..) => core::array::from_fn(|owner| {
                    let middle = a.ends[owner];
                    if middle == 0 || middle & !b.input_mask() != 0 {
                        0
                    } else {
                        (0..3)
                            .filter(|next| middle & (1 << next) != 0)
                            .fold(0, |result, next| result | b.ends[next])
                    }
                }),
                Protocol::Route(..) | Protocol::Resolved(..) => core::array::from_fn(|owner| {
                    if a.ends[owner] == 0 || b.ends[owner] == 0 {
                        0
                    } else {
                        a.ends[owner] | b.ends[owner]
                    }
                }),
                Protocol::Par(..) => {
                    if a.input_mask() & b.input_mask() == 0 {
                        return Err(Rejection::ConflictingInitialOwners);
                    }
                    if (a.transfers && b.uses) || (b.transfers && a.uses) {
                        return Err(Rejection::ParallelOwnershipConflict);
                    }
                    core::array::from_fn(|owner| {
                        if a.ends[owner] == 0 || b.ends[owner] == 0 {
                            0
                        } else if a.transfers {
                            a.ends[owner]
                        } else {
                            b.ends[owner]
                        }
                    })
                }
                _ => unreachable!(),
            };
            Flow {
                ends,
                transfers: a.transfers || b.transfers,
                uses: a.uses || b.uses,
            }
            .finish()
        }
    }
}

fn rejection(source: &Protocol) -> Option<Rejection> {
    flow(source).err()
}

pub(super) fn violations(source: &Protocol) -> BTreeMap<&'static str, &'static str> {
    fn walk(source: &Protocol, found: &mut BTreeSet<Rejection>) -> (bool, bool, bool) {
        let (valid_carriers, transfers, uses) = match source {
            Protocol::Message {
                from, to, owned, ..
            } => {
                let valid = !owned || (*from < 3 && *to < 3);
                if !valid {
                    found.insert(Rejection::EndpointIsItsOwnCarrier);
                }
                (valid, *owned, *owned || *from == 3 || *to == 3)
            }
            Protocol::Roll(body) => walk(body, found),
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                let (av, at, au) = walk(a, found);
                let (bv, bt, bu) = walk(b, found);
                if matches!(source, Protocol::Par(..)) && ((at && bu) || (bt && au)) {
                    found.insert(Rejection::ParallelOwnershipConflict);
                }
                (av && bv, at || bt, au || bu)
            }
        };
        if valid_carriers && let Some(problem) = rejection(source) {
            found.insert(problem);
        }
        (valid_carriers, transfers, uses)
    }
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let mut found = BTreeSet::new();
    if messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { from,to,owned:true,.. } if from==to))
    {
        // Construction itself rejects the non-unit local operation.
        found.insert(Rejection::NonUnitSelfMessage);
    } else {
        walk(source, &mut found);
    }
    found
        .into_iter()
        .map(|p| (p.problem(), p.diagnostic()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    fn loan(from: u8, to: u8, label: u8) -> Protocol {
        Protocol::Message {
            from,
            to,
            label,
            owned: true,
        }
    }

    #[test]
    fn sequential_handoffs_require_the_value_actually_received() {
        for next in 0..3 {
            if next == 2 {
                continue;
            }
            let valid = Protocol::Seq(Box::new(loan(0, 2, 0)), Box::new(loan(2, next, 1)));
            assert_eq!(flow(&valid).unwrap().ends, [1 << next, 0, 0]);
            for wrong in [0, 1] {
                if wrong == next {
                    continue;
                }
                let invalid =
                    Protocol::Seq(Box::new(loan(0, 2, 0)), Box::new(loan(wrong, next, 1)));
                assert_eq!(
                    rejection(&invalid),
                    Some(Rejection::ConflictingInitialOwners)
                );
            }
        }
    }

    #[test]
    fn conditional_ownership_must_work_for_every_selected_arm() {
        let choice = Protocol::Route(
            Box::new(loan(0, 1, 0)),
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                label: 1,
                owned: false,
            }),
        );
        assert_eq!(flow(&choice).unwrap().ends, [3, 0, 0]);
        assert_eq!(
            rejection(&Protocol::Seq(
                Box::new(choice.clone()),
                Box::new(loan(0, 2, 2))
            )),
            Some(Rejection::ConflictingInitialOwners)
        );
        assert_eq!(
            rejection(&Protocol::Roll(Box::new(choice))),
            Some(Rejection::RollChangesOwner)
        );
    }

    #[test]
    fn returning_the_value_inside_a_choice_makes_repetition_possible() {
        let source = Protocol::Roll(Box::new(Protocol::Route(
            Box::new(Protocol::Seq(
                Box::new(loan(0, 1, 0)),
                Box::new(loan(1, 0, 1)),
            )),
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                label: 2,
                owned: false,
            }),
        )));
        assert_eq!(flow(&source).unwrap().ends, [1, 0, 0]);
        let depth = source.syntax_depth();
        let grammar = super::super::syntax::Grammar::new(depth);
        let rank = grammar.rank(&source);
        assert_eq!(grammar.protocol(depth, rank.clone()).0, source);
        println!("returned-choice depth={depth} rank={rank}");
    }

    #[test]
    fn simultaneous_source_violations_have_separate_witnesses() {
        let source = Protocol::Par(Box::new(loan(0, 1, 0)), Box::new(loan(0, 3, 1)));
        let actual = violations(&source);
        assert_eq!(
            actual.keys().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from(["EndpointIsItsOwnCarrier", "ParallelOwnershipConflict",])
        );
        let self_transfer = Protocol::Par(Box::new(loan(0, 1, 0)), Box::new(loan(0, 0, 1)));
        assert_eq!(
            violations(&self_transfer)
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            ["NonUnitSelfMessage"]
        );
    }

    // Independently execute the owned messages of every finite word. The
    // parallel check examines the two source subtrees, not the flow rows.
    fn parallel_conflict(source: &Protocol) -> bool {
        let mentions = |source: &Protocol| {
            let mut messages = Vec::new();
            source.messages(&mut messages);
            messages.iter().any(|m| matches!(m, Protocol::Message {from,to,owned,..} if *owned || *from==3 || *to==3))
        };
        match source {
            Protocol::Message { .. } => false,
            Protocol::Roll(body) => parallel_conflict(body),
            Protocol::Seq(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                parallel_conflict(a) || parallel_conflict(b)
            }
            Protocol::Par(a, b) => {
                (super::super::localside::mobile(a) && mentions(b))
                    || (super::super::localside::mobile(b) && mentions(a))
                    || parallel_conflict(a)
                    || parallel_conflict(b)
            }
        }
    }

    #[test]
    fn all_height_one_flows_match_literal_owned_value_execution() {
        let grammar = super::super::syntax::Grammar::new(1);
        let mut checked = 0;
        for depth in 0..=1 {
            for rank in 0..usize::try_from(grammar.count(depth)).unwrap() {
                let source = grammar.protocol(depth, BigUint::from(rank)).0;
                let mut messages = Vec::new();
                source.messages(&mut messages);
                if messages.iter().any(|m| matches!(m, Protocol::Message { from,to,owned:true,.. } if from==to || *from==3 || *to==3)) { continue; }
                let mut expected = [0; 3];
                if !parallel_conflict(&source) {
                    for initial in 0..3u8 {
                        let mut ends = 0;
                        let all = source.executions().all(|execution| {
                            let mut owner = initial;
                            for (label,_) in execution.trace().messages {
                                match messages.iter().find(|m| matches!(m, Protocol::Message { label: value,.. } if *value == label)).unwrap() {
                                    Protocol::Message { from,to,owned:true,.. } => {
                                        if *from != owner { return false; }
                                        owner = *to;
                                    }
                                    Protocol::Message { .. } => (),
                                    _ => unreachable!(),
                                }
                            }
                            ends |= 1 << owner;
                            true
                        });
                        if all {
                            expected[usize::from(initial)] = ends;
                        }
                    }
                }
                match flow(&source) {
                    Ok(actual) => {
                        assert_eq!(actual.ends, expected, "accepted depth={depth} rank={rank}")
                    }
                    Err(_) => assert_eq!(expected, [0; 3], "rejected depth={depth} rank={rank}"),
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 1980);
    }
}
