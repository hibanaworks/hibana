//! Derive borrowed-scope rejection probes from source syntax and visit plans.
use super::{Execution, Protocol, localside};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Probe {
    group: (u8, u8),
    part: u8,
    occurrence: usize,
    actor: u8,
    role: u8,
    label: u8,
    receive: bool,
}

impl Probe {
    fn json(&self) -> String {
        format!(
            r#"{{"group":[{},{}],"part":{},"occurrence":{},"actor":{},"role":{},"label":{},"receive":{},"accepted":false}}"#,
            self.group.0,
            self.group.1,
            self.part,
            self.occurrence,
            self.actor,
            self.role,
            self.label,
            self.receive
        )
    }
}

pub(super) fn write_inventory(
    source: &Protocol,
    owner: u8,
    case: usize,
    mut output: impl std::io::Write,
) -> std::io::Result<(usize, usize)> {
    let executions = source.executions();
    writeln!(
        output,
        r#"{{"name":{case},"scripts":{}}}"#,
        executions.len()
    )?;
    let mut count = 0;
    for (script, execution) in executions.iter().enumerate() {
        let probes = plans(source, execution, owner);
        count += 2 * probes.len();
        writeln!(
            output,
            r#"{{"script":{script},"probes":[{}]}}"#,
            probes.iter().map(Probe::json).collect::<Vec<_>>().join(",")
        )?;
    }
    output.flush()?;
    Ok((executions.len(), count))
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
    probes: Vec<Probe>,
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
                assert_eq!(label, actual)
            }
            (Protocol::Seq(a, b), Execution::Seq(left, right)) => {
                self.walk(a, left, holder, primed, occurrence);
                self.walk(
                    b,
                    right,
                    localside::final_holder(a, holder),
                    primed,
                    occurrence + left.message_count(),
                );
            }
            (Protocol::Roll(body), Execution::Roll(first, second)) => {
                self.walk(body, first, holder, primed, occurrence);
                self.walk(
                    body,
                    second,
                    holder,
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
                        ((if from == 3 { holder } else { from }) == self.actor).then_some(label)
                    });
                self.walk(chosen, body, holder, primed, occurrence);
            }
            (Protocol::Par(a, b), Execution::Par(..)) => {
                let mut parts = Vec::new();
                localside::parallel_parts(source, &mut parts);
                for role in 0..4 {
                    if (if role == 3 { holder } else { role }) != self.actor
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
                            self.probes.push(Probe {
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

pub(super) fn plans(source: &Protocol, script: &Execution, owner: u8) -> Vec<Probe> {
    let mut probes = Vec::new();
    for actor in 0..3 {
        let mut planner = Planner {
            global: source,
            actor,
            probes: Vec::new(),
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
    fn nested_visits_have_distinct_probe_coordinates() {
        let source = Protocol::Roll(Box::new(Protocol::Roll(Box::new(Protocol::Par(
            Box::new(message(0, 1, 1)),
            Box::new(message(1, 0, 2)),
        )))));
        let execution = source.executions().remove(0);
        let probes = plans(&source, &execution, 0);
        assert_eq!(probes.len(), 16);
        assert_eq!(
            probes
                .iter()
                .map(|p| p.occurrence)
                .collect::<std::collections::BTreeSet<_>>(),
            [0, 2, 4, 6].into_iter().collect()
        );
        assert!(probes.iter().all(|p| p.part != p.label));
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
        let execution = source.executions().remove(0);
        let probes = plans(&source, &execution, 0);
        assert!(
            probes
                .iter()
                .any(|p| p.group == (1, 4) && p.part == 1 && p.label == 4)
        );
        assert!(
            !probes
                .iter()
                .any(|p| p.group == (1, 4) && p.part == 1 && p.label == 2)
        );
        assert!(
            probes
                .iter()
                .any(|p| p.group == (2, 3) && p.part == 2 && p.label == 1 && p.occurrence == 1)
        );
        assert!(
            probes
                .iter()
                .any(|p| p.group == (2, 3) && p.part == 2 && p.label == 3)
        );
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
        let execution = source.executions().remove(0);
        let probes = plans(&source, &execution, 0);
        assert!(
            probes
                .iter()
                .any(|p| p.role == 3 && p.actor == 1 && p.label == 3)
        );
        assert!(!probes.iter().any(|p| p.role == 3 && p.actor == 0));
    }

    #[test]
    fn a_branch_entry_sent_before_borrowing_is_not_probed_as_a_live_fragment() {
        let arm = |a, b| Protocol::Par(Box::new(message(0, 1, a)), Box::new(message(0, 1, b)));
        let source = Protocol::Route(Box::new(arm(1, 2)), Box::new(arm(3, 4)));
        for execution in source.executions() {
            let first = execution.trace().messages[0].0;
            let probes = plans(&source, &execution, 0);
            assert!(!probes.iter().any(|p| p.actor == 0 && p.part == first));
            assert!(probes.iter().any(|p| p.actor == 1 && p.part == first));
        }
    }

    #[test]
    fn a_resolved_branch_retains_its_first_operation_inside_the_borrow() {
        let arm = |a, b| Protocol::Par(Box::new(message(0, 1, a)), Box::new(message(0, 1, b)));
        let source = Protocol::Resolved(Box::new(arm(1, 2)), Box::new(arm(3, 4)), 0);
        for execution in source.executions() {
            let first = execution.trace().messages[0].0;
            let probes = plans(&source, &execution, 0);
            assert!(probes.iter().any(|p| p.actor == 0 && p.part == first));
            assert!(probes.iter().any(|p| p.actor == 1 && p.part == first));
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
                probes.iter().any(|p| p.actor == 0 && p.part == first),
                first < 3
            );
            assert!(probes.iter().any(|p| p.actor == 1 && p.part == first));
        }
    }
}
