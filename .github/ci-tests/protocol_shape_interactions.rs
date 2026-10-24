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

pub(super) fn protocol(length: usize, rank: usize) -> (Protocol, u8, u16, String) {
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
    (
        global,
        labels,
        resolvers,
        format!("contexts={word}/nested-loan={nested_loan}"),
    )
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
                let (source, labels, _, _) = protocol(length, rank);
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
                let (source, labels, _, _) = protocol(5, 2 * word + nested);
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
            let (source, _, _, name) = protocol(5, rank);
            assert!(source.syntax_depth() > 5);
            println!("mixed-interaction length=5 rank={rank} {name}");
        }
    }
}
