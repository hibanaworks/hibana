//! Source analysis for the unique-label, one-resource grammar through depth two.
//! All values here belong to the host test generator, never to an Endpoint.
use super::Protocol;

#[derive(Clone, Copy)]
struct Cut {
    owner: Option<u8>,
    informed: u8,
    participants: u8,
}

impl Cut {
    fn actor(self, role: u8) -> u8 {
        if role == 3 {
            self.owner.unwrap_or(3)
        } else {
            role
        }
    }
}

pub(super) fn has_transfer(source: &Protocol) -> bool {
    match source {
        Protocol::Message { owned, .. } => *owned,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => has_transfer(a) || has_transfer(b),
        Protocol::Roll(body) => has_transfer(body),
    }
}

fn uses_resource(source: &Protocol) -> bool {
    match source {
        Protocol::Message {
            from, to, owned, ..
        } => *owned || *from == 3 || *to == 3,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => uses_resource(a) || uses_resource(b),
        Protocol::Roll(body) => uses_resource(body),
    }
}

pub(super) fn controller(source: &Protocol) -> Option<u8> {
    match source {
        Protocol::Message { from, .. } => Some(*from),
        Protocol::Seq(a, _) | Protocol::Roll(a) => controller(a),
        Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let left = controller(a)?;
            (controller(b) == Some(left)).then_some(left)
        }
    }
}

fn choice(a: &Protocol, b: &Protocol, owner: Option<u8>) -> bool {
    let Some(logical) = controller(a) else {
        return false;
    };
    if controller(b) != Some(logical) {
        return false;
    }
    let mut entry = Cut {
        owner,
        informed: 0,
        participants: 0,
    };
    let choosing = 1 << entry.actor(logical);
    entry.informed = choosing;
    let Some(left) = walk(a, entry) else {
        return false;
    };
    let Some(right) = walk(b, entry) else {
        return false;
    };
    left.iter().all(|a| {
        right
            .iter()
            .all(|b| a.participants & !choosing == b.participants & !choosing)
    })
}

fn walk(source: &Protocol, entry: Cut) -> Option<Vec<Cut>> {
    match source {
        Protocol::Message {
            from, to, owned, ..
        } => {
            if *owned && (*from >= 3 || *to >= 3 || from == to || entry.owner != Some(*from)) {
                return None;
            }
            let sender = 1 << entry.actor(*from);
            let receiver = 1 << entry.actor(*to);
            if entry.informed & sender == 0 {
                return None;
            }
            Some(vec![Cut {
                owner: if *owned { Some(*to) } else { entry.owner },
                informed: entry.informed | receiver,
                participants: entry.participants | sender | receiver,
            }])
        }
        Protocol::Seq(a, b) => {
            let mut results = Vec::new();
            for next in walk(a, entry)? {
                results.extend(walk(b, next)?);
            }
            Some(results)
        }
        Protocol::Par(a, b) => {
            let transfers_left = has_transfer(a);
            if (transfers_left && uses_resource(b)) || (has_transfer(b) && uses_resource(a)) {
                return None;
            }
            let left = walk(a, entry)?;
            let right = walk(b, entry)?;
            let mut results = Vec::new();
            for left_end in &left {
                for right_end in &right {
                    results.push(Cut {
                        owner: if transfers_left {
                            left_end.owner
                        } else {
                            right_end.owner
                        },
                        informed: left_end.informed | right_end.informed,
                        participants: left_end.participants | right_end.participants,
                    });
                }
            }
            Some(results)
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            if !choice(a, b, entry.owner) {
                return None;
            }
            let mut results = walk(a, entry)?;
            results.extend(walk(b, entry)?);
            Some(results)
        }
        Protocol::Roll(body) => {
            let results = walk(body, entry)?;
            results
                .iter()
                .all(|cut| cut.owner == entry.owner)
                .then_some(results)
        }
    }
}

/// A leaf roll may resume after its sequential continuation. If that
/// continuation permanently moves the resource, its entry owner must control
/// every new visit. Temporary departures cannot obstruct another resource's
/// return here: this bounded grammar has only one resource and a wire-only body.
fn reentry(source: &Protocol, owner: Option<u8>) -> bool {
    let Some(holder) = owner else { return true };
    let Protocol::Seq(a, b) = source else {
        return true;
    };
    let Protocol::Roll(body) = a.as_ref() else {
        return true;
    };
    let Protocol::Message {
        from,
        to,
        owned: false,
        ..
    } = body.as_ref()
    else {
        return false;
    };
    if (*from != 3 && *to != 3)
        || (Cut {
            owner,
            informed: 15,
            participants: 0,
        })
        .actor(*from)
            == holder
    {
        return true;
    }
    walk(
        b,
        Cut {
            owner,
            informed: 15,
            participants: 0,
        },
    )
    .is_some_and(|ends| ends.iter().all(|cut| cut.owner == owner))
}

pub(super) fn in_context(source: &Protocol, owner: Option<u8>) -> bool {
    assert!(source.syntax_depth() <= 2);
    if owner.is_some_and(|owner| owner >= 3) {
        return false;
    }
    walk(
        source,
        Cut {
            owner,
            informed: 15,
            participants: 0,
        },
    )
    .is_some()
        && reentry(source, owner)
}

pub(super) fn valid(source: &Protocol) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let owner = messages.iter().find_map(|message| match message {
        Protocol::Message {
            from, owned: true, ..
        } => Some(*from),
        _ => None,
    });
    in_context(source, owner)
}

pub(super) fn possible_child(source: &Protocol) -> bool {
    [None, Some(0), Some(1), Some(2)]
        .into_iter()
        .any(|owner| in_context(source, owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{syntax, syntax_violations};

    #[test]
    fn shallow_source_rules_and_conversation_analysis_agree_for_every_tree() {
        let grammar = syntax::Grammar::new(1);
        let mut accepted = 0;
        let mut contextual = 0;
        for rank in 0..4128usize {
            let (source, ..) = grammar.protocol(1, rank.into());
            assert_eq!(
                valid(&source),
                syntax_violations(&source).is_empty(),
                "rank={rank}"
            );
            accepted += usize::from(valid(&source));
            contextual += usize::from(possible_child(&source));
        }
        assert_eq!(accepted, 908);
        assert_eq!(contextual, 956);
    }

    #[test]
    fn transfer_prefixes_can_make_a_child_choice_projectable() {
        let grammar = syntax::Grammar::new(2);
        for rank in [17249225usize, 17250249, 17261608, 17262632] {
            let (source, ..) = grammar.protocol(2, rank.into());
            let Protocol::Seq(_, child) = &source else {
                panic!()
            };
            assert!(!valid(child));
            assert!(possible_child(child));
            assert!(valid(&source));
        }
    }

    #[test]
    fn a_service_cannot_restart_after_permanent_resource_departure() {
        let msg = |from, to, owned| Protocol::Message {
            from,
            to,
            label: 0,
            owned,
        };
        for sender in 0..3 {
            let source = Protocol::Seq(
                Box::new(Protocol::Roll(Box::new(msg(sender, 3, false)))),
                Box::new(msg(0, 1, true)),
            );
            assert_eq!(valid(&source), sender == 0);
        }
    }
}
