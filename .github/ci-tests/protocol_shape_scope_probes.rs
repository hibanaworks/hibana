//! Derive scoped public-call trials from source syntax and visit plans.
use super::{Execution, Protocol, causality, localside, probe_data::LocalProbe};

pub(super) fn write_inventory(
    plans: &super::plans::Plans<'_>,
    owner: u8,
    case: usize,
    mut output: impl std::io::Write,
) -> std::io::Result<(usize, usize)> {
    let source = plans.source();
    let script_count = plans.len();
    writeln!(output, r#"{{"name":{case},"scripts":{}}}"#, script_count)?;
    let mut count = 0;
    let causal = causality::pairs(source);
    for (script, execution) in plans.executions().enumerate() {
        let probes = plans_with(source, &execution, owner, &causal);
        count += 2 * probes
            .iter()
            .filter(|probe| !matches!(probe, LocalProbe::Admission { .. }))
            .count();
        writeln!(
            output,
            r#"{{"script":{script},"probes":[{}]}}"#,
            probes
                .iter()
                .copied()
                .map(LocalProbe::json)
                .collect::<Vec<_>>()
                .join(",")
        )?;
    }
    output.flush()?;
    Ok((script_count, count))
}

pub(super) fn first_label(source: &Protocol) -> u8 {
    match source {
        Protocol::Message { event: label, .. } => *label,
        Protocol::Seq(a, _)
        | Protocol::Par(a, _)
        | Protocol::Route(a, _)
        | Protocol::Resolved(a, _, _)
        | Protocol::Roll(a) => first_label(a),
    }
}

/// Enumerate public wire selectors once, rather than multiplying aliases of
/// the same message contract. Source identities remain separate in captures.
pub(super) fn public_queries(
    source: &Protocol,
    role: u8,
    payload: Option<u8>,
) -> Vec<(&Protocol, bool)> {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let mut selectors = std::collections::BTreeMap::new();
    for message in messages {
        if let Protocol::Message {
            from,
            to,
            contract,
            owned,
            ..
        } = message
        {
            for (receive, view) in [(false, *from), (true, *to)] {
                if view == role
                    && !(receive && from == to)
                    && (!owned || receive || payload == Some(3))
                {
                    selectors
                        .entry((*contract, *owned, receive))
                        .or_insert((message, receive));
                }
            }
        }
    }
    selectors.into_values().collect()
}

pub(super) fn outside_messages<'a>(
    global: &'a Protocol,
    part: &Protocol,
    role: u8,
) -> Vec<(&'a Protocol, bool)> {
    let mut all = Vec::new();
    global.messages(&mut all);
    let mut inside = Vec::new();
    part.messages(&mut inside);
    all.into_iter()
        .flat_map(|message| {
            let Protocol::Message {
                from,
                to,
                event: label,
                owned: false,
                ..
            } = message
            else {
                return Vec::new();
            };
            if inside
                .iter()
                .any(|m| matches!(m, Protocol::Message { event: other, .. } if other == label))
            {
                return Vec::new();
            }
            [(false, *from), (true, *to)]
                .into_iter()
                .filter(|(_, participant)| *participant == role)
                .filter(|(receive, _)| {
                    !inside.iter().any(|other| {
                        let Protocol::Message {
                            from,
                            to,
                            contract,
                            owned,
                            ..
                        } = other
                        else {
                            unreachable!()
                        };
                        let Protocol::Message {
                            contract: expected, ..
                        } = message
                        else {
                            unreachable!()
                        };
                        !*owned
                            && contract == expected
                            && (if *receive { *to } else { *from }) == role
                    })
                })
                .map(|(receive, _)| (message, receive))
                .collect()
        })
        .collect()
}

struct Planner<'a> {
    global: &'a Protocol,
    actor: u8,
    probes: Vec<LocalProbe>,
    causal: &'a [(u8, u8, u8)],
    choices: usize,
}

impl Planner<'_> {
    fn walk(
        &mut self,
        source: &Protocol,
        script: &Execution,
        holder: u8,
        primed: Option<(&Protocol, u8)>,
        occurrence: usize,
    ) {
        match (source, script) {
            (
                Protocol::Message {
                    event: label,
                    from,
                    to,
                    owned,
                    ..
                },
                Execution::Message(actual),
            ) => {
                assert_eq!(label, actual);
                for (at_receive, role) in [(false, *from), (true, *to)] {
                    let physical = if role == 3 && localside::mobile(self.global) {
                        holder
                    } else {
                        role
                    };
                    if physical != self.actor || (at_receive && from == to) {
                        continue;
                    }
                    for after in [false, true] {
                        // These cuts already move the real Endpoint value.
                        // Before send and after receive that value is owned
                        // by this localside, with no additional owner flag.
                        let payload = if *owned {
                            after == at_receive
                        } else {
                            holder == self.actor
                                && localside::payload_in_scope(
                                    self.global,
                                    primed
                                        .filter(|(_, event)| *event == *label && !at_receive)
                                        .map_or(source, |(scope, _)| scope),
                                )
                        }
                        .then_some(3);
                        for (query, receive) in public_queries(self.global, role, payload) {
                            let Protocol::Message { event: target, .. } = query else {
                                unreachable!()
                            };
                            for choice in 0..self.choices {
                                self.probes.push(LocalProbe::Admission {
                                    at: *label,
                                    at_receive,
                                    after,
                                    occurrence,
                                    actor: self.actor,
                                    role,
                                    label: *target,
                                    receive,
                                    choice,
                                });
                            }
                        }
                    }
                }
                for &(before, target, role) in self.causal {
                    let receiver = if role == 3 && localside::mobile(self.global) {
                        holder
                    } else {
                        role
                    };
                    if before == *label && receiver == self.actor {
                        self.probes.push(LocalProbe::BeforeReceive {
                            before,
                            occurrence,
                            actor: self.actor,
                            role,
                            label: target,
                        });
                    }
                }
            }
            (Protocol::Seq(a, b), Execution::Seq(left, right)) => {
                self.walk(a, left, holder, primed, occurrence);
                self.walk(
                    b,
                    right,
                    localside::holder_after(a, left, holder),
                    primed,
                    occurrence + left.message_count(),
                );
            }
            (Protocol::Roll(body), Execution::Roll(first, second)) => {
                self.walk(body, first, holder, primed, occurrence);
                self.walk(
                    body,
                    second,
                    localside::holder_after(body, first, holder),
                    None,
                    occurrence + first.message_count(),
                );
            }
            (
                Protocol::Route(a, b) | Protocol::Resolved(a, b, _),
                Execution::Branch(body, right),
            ) => {
                let chosen = if *right { b } else { a };
                let primed = primed
                    .filter(|(_, label)| localside::contains(chosen, *label))
                    .or_else(|| {
                        let label = body.trace().messages[0].0;
                        if matches!(source, Protocol::Resolved(..))
                            && !localside::message_selected_head(chosen, label)
                        {
                            return None;
                        }
                        let mut messages = Vec::new();
                        chosen.messages(&mut messages);
                        let from = messages
                            .iter()
                            .find_map(|m| match m {
                                Protocol::Message {
                                    from,
                                    event: candidate,
                                    ..
                                } if *candidate == label => Some(*from),
                                _ => None,
                            })
                            .unwrap();
                        ((if from == 3 && localside::mobile(self.global) {
                            holder
                        } else {
                            from
                        }) == self.actor)
                            .then_some((source, label))
                    });
                self.walk(chosen, body, holder, primed, occurrence);
            }
            (Protocol::Par(a, b), Execution::Par(..)) => {
                let mut parts = Vec::new();
                localside::parallel_parts(source, &mut parts);
                for role in 0..4 {
                    if (if role == 3 && localside::mobile(self.global) {
                        holder
                    } else {
                        role
                    }) != self.actor
                        || (role == 3 && localside::mobile(source))
                    {
                        continue;
                    }
                    for part in &parts {
                        if !localside::present(part, role, primed.map(|(_, label)| label)) {
                            continue;
                        }
                        for (message, receive) in outside_messages(self.global, part, role) {
                            let Protocol::Message { event: label, .. } = message else {
                                unreachable!()
                            };
                            self.probes.push(LocalProbe::Borrow {
                                group: (first_label(a), first_label(b)),
                                part: first_label(part),
                                occurrence,
                                actor: self.actor,
                                role,
                                label: *label,
                                receive,
                            });
                        }
                    }
                }
                // Adjacent par nodes form one borrow group in the public API.
                self.parallel_children(source, script, holder, primed, occurrence);
            }
            _ => panic!("source and scoped execution disagree"),
        }
    }

    fn parallel_children(
        &mut self,
        source: &Protocol,
        script: &Execution,
        holder: u8,
        primed: Option<(&Protocol, u8)>,
        occurrence: usize,
    ) {
        match (source, script) {
            (Protocol::Par(a, b), Execution::Par(left, right, _)) => {
                self.parallel_children(a, left, holder, primed, occurrence);
                self.parallel_children(b, right, holder, primed, occurrence + left.message_count());
            }
            _ => self.walk(source, script, holder, primed, occurrence),
        }
    }
}

#[cfg(test)]
fn plans(source: &Protocol, script: &Execution, owner: u8) -> Vec<LocalProbe> {
    plans_with(source, script, owner, &causality::pairs(source))
        .into_iter()
        .filter(|probe| !matches!(probe, LocalProbe::Admission { .. }))
        .collect()
}

fn plans_with(
    source: &Protocol,
    script: &Execution,
    owner: u8,
    causal: &[(u8, u8, u8)],
) -> Vec<LocalProbe> {
    let mut probes = Vec::new();
    let mut controllers = Vec::new();
    source.resolver_roles(&mut controllers);
    let choices = super::decisions::patterns(source, controllers.len().try_into().unwrap()).len();
    for actor in 0..4 {
        let mut planner = Planner {
            global: source,
            actor,
            probes: Vec::new(),
            causal,
            choices,
        };
        planner.walk(source, script, owner, None, 0);
        probes.extend(planner.probes);
    }
    let unique = probes.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        probes.len(),
        "duplicate scoped probe coordinates"
    );
    probes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from: u8, to: u8, label: u8) -> Protocol {
        Protocol::Message {
            from,
            to,
            event: label,
            owned: false,
            contract: label,
        }
    }

    #[test]
    fn a_parallel_choice_entry_keeps_the_whole_handles_payload_queries() {
        let catalog = super::super::contract_cases(super::super::template_cases(2, 0, 32));
        let distinct = 416 * super::super::contracts::VARIANTS;
        let source = &catalog
            .cases
            .iter()
            .find(|case| case.0 == distinct)
            .unwrap()
            .4;
        let mut witnessed = false;
        for script in source.executions() {
            if script.trace().messages[0].0 != 4 {
                continue;
            }
            let probes = plans_with(source, &script, 0, &causality::pairs(source));
            for after in [false, true] {
                for (label, receive) in [(0, false), (3, true)] {
                    assert!(probes.contains(&LocalProbe::Admission {
                        at: 4,
                        at_receive: false,
                        after,
                        occurrence: 4,
                        actor: 0,
                        role: 0,
                        label,
                        receive,
                        choice: 0,
                    }));
                }
            }
            witnessed = true;
        }
        assert!(witnessed);
    }

    #[test]
    fn shared_plan_keeps_admissions_and_counts_only_rejection_trials() {
        let mut source = super::super::contracts::protocol(16);
        super::super::syntax::number(&mut source);
        let scripts =
            super::super::plans::Plans::new(&source, super::super::plans::Coverage::ParallelPairs);
        assert!(scripts.executions().any(|execution| {
            plans_with(&source, &execution, 0, &causality::pairs(&source))
                .iter()
                .any(|probe| matches!(probe, LocalProbe::Admission { .. }))
        }));
        let mut output = Vec::new();
        let (script_count, probes) = write_inventory(&scripts, 0, 64, &mut output).unwrap();
        assert_eq!(script_count, scripts.len());
        let text = String::from_utf8(output).unwrap();
        let records = text
            .lines()
            .skip(1)
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), script_count);
        let mut observed = 0;
        let mut admissions = 0;
        for (script, record) in records.iter().enumerate() {
            assert_eq!(record["script"], script);
            let execution = scripts.executions().nth(script).unwrap();
            let expected = plans_with(&source, &execution, 0, &causality::pairs(&source));
            assert_eq!(record["probes"].as_array().unwrap().len(), expected.len());
            for probe in record["probes"].as_array().unwrap() {
                if probe["kind"] == "admission" {
                    admissions += 1;
                    continue;
                }
                assert!(matches!(probe["kind"].as_str(), Some("borrow" | "causal")));
                assert_eq!(probe["accepted"], false);
                observed += 2;
            }
        }
        assert!(observed > 0);
        assert!(admissions > 0);
        assert_eq!(probes, observed);
    }

    #[test]
    fn admission_uses_public_contracts_without_duplicate_aliases_or_self_receives() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 4,
                owned: false,
                contract: 7,
            }),
            Box::new(Protocol::Seq(
                Box::new(Protocol::Message {
                    from: 0,
                    to: 1,
                    event: 5,
                    owned: false,
                    contract: 7,
                }),
                Box::new(Protocol::Message {
                    from: 1,
                    to: 1,
                    event: 6,
                    owned: false,
                    contract: 8,
                }),
            )),
        );
        let selectors = |role| {
            public_queries(&source, role, None)
                .into_iter()
                .map(|(message, receive)| match message {
                    Protocol::Message { event, .. } => (*event, receive),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(selectors(0), [(4, false)]);
        assert_eq!(selectors(1), [(4, true), (6, false)]);
        assert!(selectors(2).is_empty());
    }

    #[test]
    fn identical_labels_preserve_unit_and_endpoint_payload_contracts() {
        let source = Protocol::Seq(
            Box::new(message(0, 1, 7)),
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 8,
                owned: true,
                contract: 7,
            }),
        );
        let selectors = |role, payload| {
            public_queries(&source, role, payload)
                .into_iter()
                .map(|(message, receive)| match message {
                    Protocol::Message { event, .. } => (*event, receive),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(selectors(0, None), [(7, false)]);
        assert_eq!(selectors(0, Some(3)), [(7, false), (8, false)]);
        assert_eq!(selectors(1, None), [(7, true), (8, true)]);
    }

    #[test]
    fn owned_receive_is_probed_before_and_after_later_ordinary_calls() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 0,
                owned: true,
                contract: 0,
            }),
            Box::new(message(1, 2, 1)),
        );
        let execution = source.executions().next().unwrap();
        let probes = plans_with(&source, &execution, 0, &[]);
        for after in [false, true] {
            assert!(probes.contains(&LocalProbe::Admission {
                at: 1,
                at_receive: false,
                after,
                occurrence: 1,
                actor: 1,
                role: 1,
                label: 0,
                receive: true,
                choice: 0,
            }));
        }
    }

    #[test]
    fn owned_transfer_queries_use_the_existing_payload_value() {
        let loan = |from, to, event| Protocol::Message {
            from,
            to,
            event,
            owned: true,
            contract: event,
        };
        let source = Protocol::Seq(Box::new(loan(0, 1, 0)), Box::new(loan(1, 0, 1)));
        let execution = source.executions().next().unwrap();
        let probes = plans_with(&source, &execution, 0, &[]);
        for (at, occurrence, actor, label) in [(0, 0, 0, 0), (1, 1, 1, 1)] {
            for after in [false, true] {
                assert_eq!(
                    probes.contains(&LocalProbe::Admission {
                        at,
                        at_receive: false,
                        after,
                        occurrence,
                        actor,
                        role: actor,
                        label,
                        receive: false,
                        choice: 0,
                    }),
                    !after,
                    "an owned send can consume the original value only before transfer"
                );
            }
        }
        for (at, occurrence, actor, label) in [(0, 0, 1, 1), (1, 1, 0, 0)] {
            for after in [false, true] {
                assert_eq!(
                    probes.contains(&LocalProbe::Admission {
                        at,
                        at_receive: true,
                        after,
                        occurrence,
                        actor,
                        role: actor,
                        label,
                        receive: false,
                        choice: 0,
                    }),
                    after,
                    "a receiver can send the real value only after receiving it"
                );
            }
        }
    }

    #[test]
    fn owned_sends_are_probed_at_ordinary_calls_while_the_value_is_held() {
        let loan = |from, to, event| Protocol::Message {
            from,
            to,
            event,
            contract: event,
            owned: true,
        };
        let source = Protocol::Seq(
            Box::new(message(0, 2, 0)),
            Box::new(Protocol::Seq(
                Box::new(loan(0, 1, 1)),
                Box::new(Protocol::Seq(
                    Box::new(message(1, 0, 2)),
                    Box::new(loan(1, 0, 3)),
                )),
            )),
        );
        let execution = source.executions().next().unwrap();
        let probes = plans_with(&source, &execution, 0, &[]);
        for (at, actor, label) in [(0, 0, 1), (2, 1, 3)] {
            for after in [false, true] {
                assert!(probes.contains(&LocalProbe::Admission {
                    at,
                    at_receive: false,
                    after,
                    occurrence: usize::from(at),
                    actor,
                    role: actor,
                    label,
                    receive: false,
                    choice: 0,
                }));
            }
        }
        assert!(
            !probes.iter().any(|probe| matches!(
                probe,
                LocalProbe::Admission {
                    at: 2,
                    at_receive: true,
                    actor: 0,
                    label: 1,
                    receive: false,
                    ..
                }
            )),
            "the previous owner has no value to move before its return"
        );
    }

    #[test]
    fn a_parallel_payload_query_stays_with_the_child_owning_the_value() {
        let loan = |from, to, event| Protocol::Message {
            from,
            to,
            event,
            contract: event,
            owned: true,
        };
        let source = Protocol::Seq(
            Box::new(loan(0, 1, 0)),
            Box::new(Protocol::Par(
                Box::new(Protocol::Seq(
                    Box::new(message(1, 2, 1)),
                    Box::new(loan(1, 0, 2)),
                )),
                Box::new(message(2, 1, 3)),
            )),
        );
        let execution = source.executions().next().unwrap();
        let probes = plans_with(&source, &execution, 0, &[]);
        for after in [false, true] {
            assert!(probes.contains(&LocalProbe::Admission {
                at: 1,
                at_receive: false,
                after,
                occurrence: 1,
                actor: 1,
                role: 1,
                label: 2,
                receive: false,
                choice: 0,
            }));
        }
        assert!(
            !probes.iter().any(|probe| matches!(
                probe,
                LocalProbe::Admission {
                    at: 3,
                    actor: 1,
                    label: 2,
                    receive: false,
                    ..
                }
            )),
            "the sibling cannot move a value owned by the mobile child"
        );
    }

    #[test]
    fn admission_covers_both_sides_of_each_delegated_operation_and_visit() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 0,
                owned: true,
                contract: 0,
            }),
            Box::new(Protocol::Roll(Box::new(Protocol::Seq(
                Box::new(message(3, 2, 1)),
                Box::new(message(2, 3, 2)),
            )))),
        );
        let execution = source.executions().next().unwrap();
        let measured = plans_with(&source, &execution, 0, &[])
            .into_iter()
            .filter(|probe| matches!(probe, LocalProbe::Admission { .. }))
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected = std::collections::BTreeSet::new();
        // The loan itself has a send query before transfer and receive
        // queries on both sides. Its resource operations have two endpoints,
        // two cuts and two distinct selectors on their actual view.
        for (actor, at_receive, after, receive) in [
            (0, false, false, false),
            (1, true, false, true),
            (1, true, true, true),
        ] {
            expected.insert(LocalProbe::Admission {
                at: 0,
                at_receive,
                after,
                occurrence: 0,
                actor,
                role: actor,
                label: 0,
                receive,
                choice: 0,
            });
        }
        for (at, occurrence, actor, role, at_receive) in [
            (1, 1, 1, 3, false),
            (1, 1, 2, 2, true),
            (2, 2, 2, 2, false),
            (2, 2, 1, 3, true),
            (1, 3, 1, 3, false),
            (1, 3, 2, 2, true),
            (2, 4, 2, 2, false),
            (2, 4, 1, 3, true),
        ] {
            for after in [false, true] {
                for (label, receive) in if role == 3 {
                    [(1, false), (2, true)]
                } else {
                    [(1, true), (2, false)]
                } {
                    expected.insert(LocalProbe::Admission {
                        at,
                        at_receive,
                        after,
                        occurrence,
                        actor,
                        role,
                        label,
                        receive,
                        choice: 0,
                    });
                }
            }
        }
        assert_eq!(measured.len(), 35);
        assert_eq!(measured, expected);
    }

    #[test]
    fn a_delegated_receive_is_probed_in_each_actual_visit() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 0,
                owned: true,
                contract: 0,
            }),
            Box::new(Protocol::Roll(Box::new(Protocol::Seq(
                Box::new(message(2, 3, 1)),
                Box::new(message(3, 2, 2)),
            )))),
        );
        let probes = plans(&source, &source.executions().next().unwrap(), 0);
        assert_eq!(
            probes,
            [
                LocalProbe::BeforeReceive {
                    before: 1,
                    occurrence: 1,
                    actor: 1,
                    role: 3,
                    label: 2
                },
                LocalProbe::BeforeReceive {
                    before: 1,
                    occurrence: 3,
                    actor: 1,
                    role: 3,
                    label: 2
                },
            ]
        );
    }

    #[test]
    fn nested_visits_have_distinct_probe_coordinates() {
        let source = Protocol::Roll(Box::new(Protocol::Roll(Box::new(Protocol::Par(
            Box::new(message(0, 1, 1)),
            Box::new(message(1, 0, 2)),
        )))));
        let execution = source.executions().next().unwrap();
        let probes = plans(&source, &execution, 0);
        assert_eq!(probes.len(), 16);
        assert_eq!(
            probes
                .iter()
                .map(|p| match p {
                    LocalProbe::Borrow { occurrence, .. }
                    | LocalProbe::BeforeReceive { occurrence, .. }
                    | LocalProbe::Admission { occurrence, .. } => *occurrence,
                })
                .collect::<std::collections::BTreeSet<_>>(),
            [0, 2, 4, 6].into_iter().collect()
        );
        assert!(
            probes
                .iter()
                .all(|p| matches!(p,LocalProbe::Borrow{part,label,..} if part != label))
        );
    }

    #[test]
    fn nested_parallel_groups_keep_their_source_boundaries() {
        let source = Protocol::Par(
            Box::new(Protocol::Seq(
                Box::new(message(0, 1, 1)),
                Box::new(Protocol::Par(
                    Box::new(message(0, 1, 2)),
                    Box::new(message(1, 0, 3)),
                )),
            )),
            Box::new(message(0, 1, 4)),
        );
        let execution = source.executions().next().unwrap();
        let probes = plans(&source, &execution, 0);
        assert!(probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                group: (1, 4),
                part: 1,
                label: 4,
                ..
            }
        )));
        assert!(!probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                group: (1, 4),
                part: 1,
                label: 2,
                ..
            }
        )));
        assert!(probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                group: (2, 3),
                part: 2,
                label: 1,
                occurrence: 1,
                ..
            }
        )));
        assert!(probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                group: (2, 3),
                part: 2,
                label: 3,
                ..
            }
        )));
    }

    #[test]
    fn delegation_changes_which_actor_can_probe_the_resource_view() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 1,
                owned: true,
                contract: 1,
            }),
            Box::new(Protocol::Par(
                Box::new(message(3, 2, 2)),
                Box::new(message(2, 3, 3)),
            )),
        );
        let execution = source.executions().next().unwrap();
        let probes = plans(&source, &execution, 0);
        assert!(probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                role: 3,
                actor: 1,
                label: 3,
                ..
            }
        )));
        assert!(!probes.iter().any(|p| matches!(
            p,
            LocalProbe::Borrow {
                role: 3,
                actor: 0,
                ..
            }
        )));
    }

    #[test]
    fn an_undelegated_fourth_role_probes_its_own_borrows() {
        let source = Protocol::Par(Box::new(message(3, 0, 0)), Box::new(message(1, 3, 1)));
        for execution in source.executions() {
            let probes = plans(&source, &execution, 0);
            let fourth = probes
                .iter()
                .filter(|p| matches!(p, LocalProbe::Borrow { role: 3, .. }))
                .collect::<Vec<_>>();
            assert_eq!(fourth.len(), 2);
            assert!(
                fourth
                    .iter()
                    .all(|p| matches!(p, LocalProbe::Borrow { actor: 3, .. }))
            );
        }
    }

    #[test]
    fn a_branch_entry_sent_before_borrowing_is_not_probed_as_a_live_fragment() {
        let arm = |a, b| Protocol::Par(Box::new(message(0, 1, a)), Box::new(message(0, 1, b)));
        let source = Protocol::Route(Box::new(arm(1, 2)), Box::new(arm(3, 4)));
        for execution in source.executions() {
            let first = execution.trace().messages[0].0;
            let probes = plans(&source, &execution, 0);
            assert!(
                !probes
                    .iter()
                    .any(|p| matches!(p,LocalProbe::Borrow{actor:0,part,..} if *part==first))
            );
            assert!(
                probes
                    .iter()
                    .any(|p| matches!(p,LocalProbe::Borrow{actor:1,part,..} if *part==first))
            );
        }
    }

    #[test]
    fn a_sent_head_still_has_an_unconsumed_receive_on_its_other_view() {
        let head = message(0, 3, 1);
        assert!(!localside::present(&head, 0, Some(1)));
        assert!(localside::present(&head, 3, Some(1)));
        let local = message(0, 0, 1);
        assert!(!localside::present(&local, 0, Some(1)));
    }

    #[test]
    fn a_resolved_branch_retains_its_first_operation_inside_the_borrow() {
        let arm = |a, b| Protocol::Par(Box::new(message(0, 1, a)), Box::new(message(0, 1, b)));
        let source = Protocol::Resolved(Box::new(arm(1, 2)), Box::new(arm(3, 4)), 0);
        for execution in source.executions() {
            let first = execution.trace().messages[0].0;
            let probes = plans(&source, &execution, 0);
            assert!(
                probes.iter().any(
                    |p| matches!(p, LocalProbe::Borrow { actor: 0, part, .. } if *part == first)
                )
            );
            assert!(
                probes.iter().any(
                    |p| matches!(p, LocalProbe::Borrow { actor: 1, part, .. } if *part == first)
                )
            );
        }
    }

    #[test]
    fn only_a_message_selected_head_is_sent_before_borrowing_a_resolved_branch() {
        let arm = |a, b| Protocol::Par(Box::new(message(0, 1, a)), Box::new(message(0, 1, b)));
        let source = Protocol::Resolved(
            Box::new(arm(1, 2)),
            Box::new(Protocol::Route(Box::new(arm(3, 4)), Box::new(arm(5, 6)))),
            0,
        );
        for execution in source.executions() {
            let first = execution.trace().messages[0].0;
            let probes = plans(&source, &execution, 0);
            assert_eq!(
                probes.iter().any(
                    |p| matches!(p, LocalProbe::Borrow { actor: 0, part, .. } if *part == first)
                ),
                first < 3
            );
            assert!(
                probes.iter().any(
                    |p| matches!(p, LocalProbe::Borrow { actor: 1, part, .. } if *part == first)
                )
            );
        }
    }
}
