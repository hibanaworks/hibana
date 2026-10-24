//! Necessary receive-before-send order for unique-label source protocols.
//! A witness stays in the same roll scopes and cannot skip a choice containing
//! the receive. This excludes retained continuations from an earlier visit.
use super::Protocol;

fn required(source: &Protocol, label: u8) -> bool {
    match source {
        Protocol::Message { label: found, .. } => *found == label,
        Protocol::Seq(a, b) | Protocol::Par(a, b) => required(a, label) || required(b, label),
        Protocol::Route(..) | Protocol::Resolved(..) | Protocol::Roll(..) => false,
    }
}

fn without_roll(source: &Protocol, label: u8) -> bool {
    match source {
        Protocol::Message { label: found, .. } => *found == label,
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
            label,
            owned: false,
        } = before
        else {
            continue;
        };
        if from == role {
            continue;
        }
        if messages
            .iter()
            .filter(|m| matches!(m, Protocol::Message{label: other,..} if other == label))
            .count()
            != 1
        {
            continue;
        }
        for target in &messages {
            let Protocol::Message {
                from,
                label: next,
                owned: false,
                ..
            } = target
            else {
                continue;
            };
            if from != role
                || messages
                    .iter()
                    .filter(|m| matches!(m, Protocol::Message{label: other,..} if other == next))
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

#[cfg(test)]
mod tests {
    use super::*;
    fn msg(from: u8, to: u8, label: u8) -> Protocol {
        Protocol::Message {
            from,
            to,
            label,
            owned: false,
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
}
