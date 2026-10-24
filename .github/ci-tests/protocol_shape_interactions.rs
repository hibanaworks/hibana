//! Construct source protocols for nested operator interactions.
use super::{Protocol, ownership};

const CONTEXTS: usize = 9;

pub(super) fn count(length: usize) -> usize {
    2 * CONTEXTS.pow(u32::try_from(length).expect("interaction length"))
}

fn message(from: u8, to: u8, owned: bool) -> Protocol {
    Protocol::Message {
        from,
        to,
        label: 0,
        owned,
    }
}
fn seq(a: Protocol, b: Protocol) -> Protocol {
    Protocol::Seq(Box::new(a), Box::new(b))
}

pub(super) struct Interaction {
    pub source: Protocol,
    pub labels: u8,
    pub resolvers: u16,
    pub identity: String,
    pub unpermitted_reentry: Option<Protocol>,
}

// In this catalog, service 2 talks only to the resource view. Independent
// work uses roles 0 and 1. Inspect all first service actions, including par
// siblings and both choice arms, without consulting projection diagnostics.
fn service_initiates(source: &Protocol) -> Option<bool> {
    match source {
        Protocol::Message { from, to, .. } => (*from == 2 || *to == 2).then_some(*from == 2),
        Protocol::Seq(a, b) => service_initiates(a).or_else(|| service_initiates(b)),
        Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            match (service_initiates(a), service_initiates(b)) {
                (None, other) | (other, None) => other,
                (Some(a), Some(b)) => Some(a || b),
            }
        }
        Protocol::Roll(body) => service_initiates(body),
    }
}

fn permit_reentry(source: &mut Protocol) -> usize {
    match source {
        Protocol::Message { .. } => 0,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => permit_reentry(a) + permit_reentry(b),
        Protocol::Roll(body) => {
            let nested = permit_reentry(body);
            if service_initiates(body) == Some(true) {
                **body = seq(message(3, 2, false), (**body).clone());
                nested + 1
            } else {
                nested
            }
        }
    }
}

pub(super) fn protocol(length: usize, rank: usize) -> Interaction {
    assert!(length <= 5 && rank < count(length));
    let nested_loan = !rank.is_multiple_of(2);
    let mut word_rank = rank / 2;
    let mut contexts = vec![0; length];
    for context in contexts.iter_mut().rev() {
        *context = word_rank % CONTEXTS;
        word_rank /= CONTEXTS;
    }
    let mut body = seq(message(3, 2, false), message(2, 3, false));
    if nested_loan {
        body = seq(message(1, 0, true), seq(body, message(0, 1, true)));
    }
    for context in contexts.iter().rev() {
        body = match context {
            0 => seq(message(2, 3, false), body),
            1 => seq(message(3, 2, false), body),
            2 => seq(body, message(2, 3, false)),
            3 => seq(body, message(3, 2, false)),
            4 | 5 => {
                let independent = if nested_loan {
                    Protocol::Par(
                        Box::new(message(0, 1, false)),
                        Box::new(Protocol::Roll(Box::new(message(1, 0, false)))),
                    )
                } else {
                    seq(message(0, 1, false), message(1, 0, false))
                };
                let (left, right) = if *context == 4 {
                    (body, independent)
                } else {
                    (independent, body)
                };
                Protocol::Par(Box::new(left), Box::new(right))
            }
            6 | 7 => {
                let mut messages = Vec::new();
                body.messages(&mut messages);
                let inform_owner = messages.iter().any(|m| {
                    matches!(
                        m,
                        Protocol::Message { from: 0, .. } | Protocol::Message { to: 0, .. }
                    )
                });
                let arm = || {
                    let rest = if inform_owner {
                        seq(message(3, 0, false), body.clone())
                    } else {
                        body.clone()
                    };
                    seq(message(3, 2, false), rest)
                };
                if *context == 6 {
                    Protocol::Route(Box::new(arm()), Box::new(arm()))
                } else {
                    Protocol::Resolved(Box::new(arm()), Box::new(arm()), 0)
                }
            }
            8 => Protocol::Roll(Box::new(body)),
            _ => unreachable!(),
        };
    }
    let mut global = seq(message(0, 1, true), seq(body, message(1, 0, true)));
    let mut unpermitted = global.clone();
    let permits = permit_reentry(&mut global);
    let unpermitted_reentry = (permits != 0).then(|| {
        super::syntax::number(&mut unpermitted);
        unpermitted
    });
    let (labels, resolvers) = super::syntax::number(&mut global);
    assert!(
        ownership::violations(&global).is_empty(),
        "constructed ownership violation"
    );
    let names = [
        "seq-recv-before",
        "seq-send-before",
        "seq-recv-after",
        "seq-send-after",
        "par-left",
        "par-right",
        "route",
        "resolved",
        "roll",
    ];
    let word = contexts
        .iter()
        .map(|context| names[*context])
        .collect::<Vec<_>>()
        .join("/");
    Interaction {
        source: global,
        labels,
        resolvers,
        identity: format!("contexts={word}/nested-loan={nested_loan}/reentry-permits={permits}"),
        unpermitted_reentry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn constructed_contexts_preserve_ownership_and_distinct_syntax() {
        for length in 0..=3 {
            let mut representatives = BTreeSet::new();
            for rank in 0..count(length) {
                let Interaction { source, labels, .. } = protocol(length, rank);
                let mut messages = Vec::new();
                source.messages(&mut messages);
                assert_eq!(messages.len(), usize::from(labels));
                assert!(representatives.insert(super::super::symmetry::canonical(&source)));
            }
            assert_eq!(representatives.len(), count(length));
        }
        for context in 0..CONTEXTS {
            let word = (0..5).fold(0, |rank, _| CONTEXTS * rank + context);
            for nested in 0..2 {
                let Interaction { source, labels, .. } = protocol(5, 2 * word + nested);
                assert!(labels <= 254);
                assert!(source.syntax_depth() >= 5);
            }
        }
    }

    #[test]
    fn mixed_context_words_include_both_parallel_sides_and_delegation() {
        for word in [[8, 4, 7, 0, 6], [0, 7, 5, 8, 6]] {
            let rank = 2 * word
                .into_iter()
                .fold(0, |rank, context| CONTEXTS * rank + context)
                + 1;
            let Interaction {
                source, identity, ..
            } = protocol(5, rank);
            assert!(source.syntax_depth() > 5);
            println!("mixed-interaction length=5 rank={rank} {identity}");
        }
    }

    #[test]
    fn service_initiated_reentry_gets_permission_inside_each_visit() {
        for rank in [144, 145] {
            let Interaction {
                source,
                unpermitted_reentry,
                ..
            } = protocol(2, rank);
            let unpermitted = unpermitted_reentry.unwrap();
            let Protocol::Seq(_, tail) = &source else {
                panic!()
            };
            let Protocol::Seq(body, _) = tail.as_ref() else {
                panic!()
            };
            let Protocol::Roll(body) = body.as_ref() else {
                panic!()
            };
            assert_eq!(service_initiates(body), Some(false));
            assert_eq!(source.operator_count(), unpermitted.operator_count() + 1);
            assert!(ownership::violations(&unpermitted).is_empty());
        }
        assert!(protocol(2, 146).unpermitted_reentry.is_none());
    }
}
