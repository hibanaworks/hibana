//! Derive local rejection trials from source syntax and visit plans.
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
        count += 2 * probes.len();
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
        Protocol::Message { label, .. } => *label,
        Protocol::Seq(a, _)
        | Protocol::Par(a, _)
        | Protocol::Route(a, _)
        | Protocol::Resolved(a, _, _)
        | Protocol::Roll(a) => first_label(a),
    }
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
                label,
                owned: false,
            } = message
            else {
                return Vec::new();
            };
            if inside
                .iter()
                .any(|m| matches!(m, Protocol::Message { label: other, .. } if other == label))
            {
                return Vec::new();
            }
            [(false, *from), (true, *to)]
                .into_iter()
                .filter(|(_, participant)| *participant == role)
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
}

impl Planner<'_> {
    fn walk(
        &mut self,
        source: &Protocol,
        script: &Execution,
        holder: u8,
        primed: Option<u8>,
        occurrence: usize,
    ) {
        match (source, script) {
            (Protocol::Message { label, .. }, Execution::Message(actual)) => {
                assert_eq!(label, actual);
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
                    .filter(|label| localside::contains(chosen, *label))
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
                                    label: candidate,
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
                            .then_some(label)
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
                        if !localside::present(part, role, primed) {
                            continue;
                        }
                        for (message, receive) in outside_messages(self.global, part, role) {
                            let Protocol::Message { label, .. } = message else {
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
        primed: Option<u8>,
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
}

fn plans_with(
    source: &Protocol,
    script: &Execution,
    owner: u8,
    causal: &[(u8, u8, u8)],
) -> Vec<LocalProbe> {
    let mut probes = Vec::new();
    for actor in 0..4 {
        let mut planner = Planner {
            global: source,
            actor,
            probes: Vec::new(),
            causal,
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
            label,
            owned: false,
        }
    }

    #[test]
    fn a_delegated_receive_is_probed_in_each_actual_visit() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                label: 0,
                owned: true,
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
                    | LocalProbe::BeforeReceive { occurrence, .. } => *occurrence,
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
                label: 1,
                owned: true,
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
