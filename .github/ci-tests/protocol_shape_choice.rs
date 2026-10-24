//! Source rules for a root choice between two single messages.
//!
//! This deliberately does not classify compound arms: an earlier transfer or
//! notification can change which physical participant knows their choice.
use super::Protocol;

pub(super) fn violations(source: &Protocol) -> Vec<(&'static str, &'static str)> {
    let mut result = Vec::new();
    let (a, b) = match source {
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => (a, b),
        _ => return result,
    };
    let (
        Protocol::Message {
            from: af, to: at, ..
        },
        Protocol::Message {
            from: bf, to: bt, ..
        },
    ) = (a.as_ref(), b.as_ref())
    else {
        return result;
    };
    if af != bf {
        result.push(("RouteControllerMismatch", "route arms use different first visible controllers; help: give both route arms the same first-visible controller"));
    }
    // Ownership is checked first by the caller. A transfer in a root arm
    // identifies the resource's initial holder; there is no earlier transfer.
    let initial = [a, b]
        .into_iter()
        .find_map(|message| match message.as_ref() {
            Protocol::Message {
                from, owned: true, ..
            } => Some(*from),
            _ => None,
        });
    let actor = |role| {
        if role == 3 {
            initial.unwrap_or(3)
        } else {
            role
        }
    };
    let controller = actor(*af);
    let left = [actor(*af), actor(*at)];
    let right = [actor(*bf), actor(*bt)];
    for role in 0..4 {
        if role == controller || (left.contains(&role) == right.contains(&role) && role != right[0])
        {
            continue;
        }
        // This participant is absent in one arm, or sends before receiving
        // any notification in the right arm. Either prevents it learning the
        // selected branch. Retain every witness instead of fixing error order.
        let reason = match role {
            0 => {
                "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation; role=0; help: notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"
            }
            1 => {
                "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation; role=1; help: notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"
            }
            2 => {
                "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation; role=2; help: notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"
            }
            3 => {
                "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation; role=3; help: notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"
            }
            _ => unreachable!("catalog role domain"),
        };
        result.push(("MissingBranchKnowledge", reason));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_single_message_choice_has_one_shared_sender_and_receiver() {
        for sender in 0..4 {
            for receiver in 0..4 {
                for other_sender in 0..4 {
                    for other_receiver in 0..4 {
                        let msg = |from, to, label| {
                            Box::new(Protocol::Message {
                                from,
                                to,
                                label,
                                owned: false,
                            })
                        };
                        for resolved in [false, true] {
                            let a = msg(sender, receiver, 0);
                            let b = msg(other_sender, other_receiver, 1);
                            let choice = if resolved {
                                Protocol::Resolved(a, b, 0)
                            } else {
                                Protocol::Route(a, b)
                            };
                            let problems = violations(&choice);
                            assert_eq!(
                                problems.is_empty(),
                                sender == other_sender && receiver == other_receiver
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_notification_in_a_compound_arm_requires_a_separate_analysis() {
        let source = Protocol::Route(
            Box::new(Protocol::Seq(
                Box::new(Protocol::Message {
                    from: 0,
                    to: 1,
                    label: 0,
                    owned: false,
                }),
                Box::new(Protocol::Message {
                    from: 0,
                    to: 2,
                    label: 1,
                    owned: false,
                }),
            )),
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                label: 2,
                owned: false,
            }),
        );
        assert!(violations(&source).is_empty());
    }
}
