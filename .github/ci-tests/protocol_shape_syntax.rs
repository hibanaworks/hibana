//! Rank the complete bounded-height grammar without materializing its population.
use super::{Protocol, message};
use num_bigint::BigUint;

// Four logical roles, with role 3 also available as an Endpoint payload.
// Invalid self-transfers and self-carriers remain candidates for rejection.
const LEAVES: usize = 32;

pub(super) struct Grammar {
    exact: Vec<BigUint>,
    through: Vec<BigUint>,
}

impl Grammar {
    pub(super) fn new(depth: usize) -> Self {
        Self::with_leaves(depth, LEAVES)
    }

    pub(super) fn with_leaves(depth: usize, leaves: usize) -> Self {
        let mut exact = vec![BigUint::from(leaves)];
        let mut through = exact.clone();
        for height in 1..=depth {
            // Disjoint pairs: the left child reaches height-1, or only the
            // right does. Both children otherwise range independently.
            let mut pairs = &exact[height - 1] * &through[height - 1];
            if height > 1 {
                pairs += &through[height - 2] * &exact[height - 1];
            }
            let count = &exact[height - 1] + 4u8 * pairs;
            through.push(&through[height - 1] + &count);
            exact.push(count);
        }
        Self { exact, through }
    }

    pub(super) fn count(&self, depth: usize) -> &BigUint {
        &self.exact[depth]
    }

    pub(super) fn rank(&self, source: &Protocol) -> BigUint {
        let depth = source.syntax_depth();
        let below = |child: &Protocol| {
            let height = child.syntax_depth();
            self.rank(child)
                + if height == 0 {
                    BigUint::from(0u8)
                } else {
                    self.through[height - 1].clone()
                }
        };
        let (kind, left, right) = match source {
            Protocol::Message {
                from, to, owned, ..
            } => {
                return BigUint::from(
                    4 * usize::from(*from) + usize::from(*to) + if *owned { 16 } else { 0 },
                );
            }
            Protocol::Roll(body) => return self.rank(body),
            Protocol::Seq(a, b) => (0u8, a, b),
            Protocol::Par(a, b) => (1, a, b),
            Protocol::Route(a, b) => (2, a, b),
            Protocol::Resolved(a, b, _) => (3, a, b),
        };
        let left_pairs = &self.exact[depth - 1] * &self.through[depth - 1];
        let pairs = &left_pairs
            + if depth > 1 {
                &self.through[depth - 2] * &self.exact[depth - 1]
            } else {
                BigUint::from(0u8)
            };
        let position = if left.syntax_depth() == depth - 1 {
            self.rank(left) * &self.through[depth - 1] + below(right)
        } else {
            left_pairs + below(left) * &self.exact[depth - 1] + self.rank(right)
        };
        &self.exact[depth - 1] + kind * pairs + position
    }

    fn through(&self, depth: usize, mut rank: BigUint) -> Protocol {
        for height in 0..=depth {
            if rank < self.exact[height] {
                return self.exact(height, rank);
            }
            rank -= &self.exact[height];
        }
        panic!("syntax rank outside cumulative population")
    }

    fn exact(&self, depth: usize, mut rank: BigUint) -> Protocol {
        assert!(rank < self.exact[depth]);
        if depth == 0 {
            let rank = usize::try_from(rank).expect("leaf rank fits usize");
            return message(
                &mut 0,
                ((rank % 16) / 4) as u8,
                (rank % 4) as u8,
                rank >= 16,
            );
        }
        if rank < self.exact[depth - 1] {
            return Protocol::Roll(Box::new(self.exact(depth - 1, rank)));
        }
        rank -= &self.exact[depth - 1];
        let pairs_left = &self.exact[depth - 1] * &self.through[depth - 1];
        let pairs_right = if depth > 1 {
            &self.through[depth - 2] * &self.exact[depth - 1]
        } else {
            BigUint::from(0u8)
        };
        let pairs = &pairs_left + &pairs_right;
        let constructor = usize::try_from(&rank / &pairs).expect("four binary constructors");
        rank %= &pairs;
        let (left, right) = if rank < pairs_left {
            let count = &self.through[depth - 1];
            (
                self.exact(depth - 1, &rank / count),
                self.through(depth - 1, &rank % count),
            )
        } else {
            rank -= pairs_left;
            let count = &self.exact[depth - 1];
            (
                self.through(depth - 2, &rank / count),
                self.exact(depth - 1, &rank % count),
            )
        };
        let (left, right) = (Box::new(left), Box::new(right));
        match constructor {
            0 => Protocol::Seq(left, right),
            1 => Protocol::Par(left, right),
            2 => Protocol::Route(left, right),
            3 => Protocol::Resolved(left, right, 0),
            _ => unreachable!(),
        }
    }

    pub(super) fn protocol(&self, depth: usize, rank: BigUint) -> (Protocol, u16, u16) {
        let mut source = self.exact(depth, rank.clone());
        assert_eq!(self.rank(&source), rank, "syntax rank/unrank mismatch");
        let (labels, resolvers) = number(&mut source);
        (source, labels, resolvers)
    }
}

pub(super) fn number(source: &mut Protocol) -> (u16, u16) {
    fn visit(source: &mut Protocol, label: &mut u16, resolver: &mut u16) {
        match source {
            Protocol::Message { label: value, .. } => {
                *value = u8::try_from(*label).expect("syntax label domain");
                *label = label.checked_add(1).expect("syntax message count");
            }
            Protocol::Resolved(left, right, id) => {
                *id = *resolver;
                *resolver = resolver.checked_add(1).expect("syntax resolver domain");
                visit(left, label, resolver);
                visit(right, label, resolver);
            }
            Protocol::Seq(left, right)
            | Protocol::Par(left, right)
            | Protocol::Route(left, right) => {
                visit(left, label, resolver);
                visit(right, label, resolver);
            }
            Protocol::Roll(body) => visit(body, label, resolver),
        }
    }
    let (mut label, mut resolver) = (0, 0);
    visit(source, &mut label, &mut resolver);
    (label, resolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // Construct the small population directly, without rank arithmetic.
    fn naive(depth: usize, leaf_count: usize) -> BTreeSet<Protocol> {
        let leaves = (0..leaf_count)
            .map(|i| message(&mut 0, ((i % 16) / 4) as u8, (i % 4) as u8, i >= 16))
            .collect::<BTreeSet<_>>();
        if depth == 0 {
            return leaves;
        }
        let previous = naive(depth - 1, leaf_count);
        let mut all = leaves;
        for left in &previous {
            all.insert(Protocol::Roll(Box::new(left.clone())));
            for right in &previous {
                let pair = || (Box::new(left.clone()), Box::new(right.clone()));
                let (a, b) = pair();
                all.insert(Protocol::Seq(a, b));
                let (a, b) = pair();
                all.insert(Protocol::Par(a, b));
                let (a, b) = pair();
                all.insert(Protocol::Route(a, b));
                let (a, b) = pair();
                all.insert(Protocol::Resolved(a, b, 0));
            }
        }
        all
    }

    #[test]
    fn every_small_tree_has_exactly_one_rank() {
        let grammar = Grammar::new(1);
        for depth in 0..=1 {
            let count = usize::try_from(grammar.count(depth)).unwrap();
            let ranked = (0..count)
                .map(|rank| {
                    let tree = grammar.exact(depth, BigUint::from(rank));
                    assert_eq!(grammar.rank(&tree), BigUint::from(rank));
                    tree
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(ranked.len(), count);
            assert_eq!(
                ranked,
                naive(depth, LEAVES)
                    .into_iter()
                    .filter(|p| p.syntax_depth() == depth)
                    .collect()
            );
        }
        // The second level includes both-composite binary children and
        // every pairing of constructors, checked against literal enumeration.
        let grammar = Grammar::with_leaves(2, 2);
        let count = usize::try_from(grammar.count(2)).unwrap();
        let ranked = (0..count)
            .map(|rank| grammar.exact(2, BigUint::from(rank)))
            .collect::<BTreeSet<_>>();
        assert_eq!(ranked.len(), count);
        assert_eq!(
            ranked,
            naive(2, 2)
                .into_iter()
                .filter(|p| p.syntax_depth() == 2)
                .collect()
        );
    }

    #[test]
    fn tall_populations_do_not_overflow_machine_integers() {
        let grammar = Grammar::new(8);
        assert!(grammar.count(8).bits() > 128);
        for depth in 0..=8 {
            for rank in [BigUint::from(0u8), grammar.count(depth) - 1u8] {
                let (source, labels, resolvers) = grammar.protocol(depth, rank);
                assert_eq!(source.syntax_depth(), depth);
                assert!(usize::from(labels) <= 1usize << depth);
                assert!(usize::from(resolvers) < (1usize << depth));
            }
        }
    }

    #[test]
    fn height_eight_uses_all_256_labels_without_overflowing_the_count() {
        fn balanced(depth: usize) -> Protocol {
            if depth == 0 {
                message(&mut 0, 0, 1, false)
            } else {
                Protocol::Seq(Box::new(balanced(depth - 1)), Box::new(balanced(depth - 1)))
            }
        }
        let grammar = Grammar::new(8);
        let source = balanced(8);
        let (source, count, _) = grammar.protocol(8, grammar.rank(&source));
        let mut messages = Vec::new();
        source.messages(&mut messages);
        let labels = messages
            .into_iter()
            .map(|message| match message {
                Protocol::Message { label, .. } => *label,
                _ => unreachable!(),
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(count, 256);
        assert_eq!(labels, (0..=u8::MAX).collect());
    }

    #[test]
    fn binary_children_can_both_contain_every_constructor() {
        let grammar = Grammar::new(3);
        for constructor in 0..4u8 {
            let pairs = (&grammar.exact[2] * &grammar.through[2])
                + (&grammar.through[1] * &grammar.exact[2]);
            let rank = &grammar.exact[2]
                + BigUint::from(constructor) * pairs
                + (&grammar.exact[2] - 1u8) * &grammar.through[2]
                + &grammar.through[2]
                - 1u8;
            let (source, _, _) = grammar.protocol(3, rank);
            assert_eq!(source.syntax_depth(), 3);
            let (a, b) = match source {
                Protocol::Seq(a, b)
                | Protocol::Par(a, b)
                | Protocol::Route(a, b)
                | Protocol::Resolved(a, b, _) => (a, b),
                _ => panic!("binary constructor missing"),
            };
            assert_eq!(a.syntax_depth(), 2);
            assert_eq!(b.syntax_depth(), 2);
        }
    }

    #[test]
    fn mixed_delegation_and_both_composite_children_have_grammar_ranks() {
        let mut next = 0;
        let loan = message(&mut next, 0, 1, true);
        let choose = Protocol::Resolved(
            Box::new(message(&mut next, 3, 2, false)),
            Box::new(message(&mut next, 3, 2, false)),
            0,
        );
        let exchange = Protocol::Roll(Box::new(Protocol::Seq(
            Box::new(message(&mut next, 1, 2, false)),
            Box::new(message(&mut next, 2, 1, false)),
        )));
        let tail = message(&mut next, 1, 0, true);
        let source = Protocol::Seq(
            Box::new(loan),
            Box::new(Protocol::Seq(
                Box::new(Protocol::Par(Box::new(choose), Box::new(exchange))),
                Box::new(tail),
            )),
        );
        let depth = source.syntax_depth();
        let grammar = Grammar::new(depth);
        let rank = grammar.rank(&source);
        assert_eq!(grammar.protocol(depth, rank.clone()).0, source);
        println!("mixed-delegation depth={depth} rank={rank}");
    }

    #[test]
    fn conditional_owners_continue_through_parallel_operations() {
        for inside_parallel in [false, true] {
            let mut next = 0;
            let choice = Protocol::Route(
                Box::new(message(&mut next, 0, 1, true)),
                Box::new(message(&mut next, 0, 1, false)),
            );
            let source = if inside_parallel {
                Protocol::Seq(
                    Box::new(Protocol::Par(
                        Box::new(choice),
                        Box::new(message(&mut next, 2, 0, false)),
                    )),
                    Box::new(message(&mut next, 3, 2, false)),
                )
            } else {
                Protocol::Seq(
                    Box::new(choice),
                    Box::new(Protocol::Seq(
                        Box::new(Protocol::Par(
                            Box::new(message(&mut next, 3, 2, false)),
                            Box::new(message(&mut next, 0, 2, false)),
                        )),
                        Box::new(message(&mut next, 2, 3, false)),
                    )),
                )
            };
            let depth = source.syntax_depth();
            let grammar = Grammar::new(depth);
            let rank = grammar.rank(&source);
            assert_eq!(grammar.protocol(depth, rank.clone()).0, source);
            let owners = source
                .executions()
                .map(|execution| super::super::localside::holder_after(&source, &execution, 0))
                .collect::<BTreeSet<_>>();
            assert_eq!(owners, BTreeSet::from([0, 1]));
            println!("conditional-parallel inside={inside_parallel} depth={depth} rank={rank}");
        }
    }

    #[test]
    fn one_actor_can_observe_a_choice_through_different_endpoint_views() {
        for resolved in [false, true] {
            for parallel in [false, true] {
                let mut next = 0;
                let loan = message(&mut next, 0, 1, true);
                let a = Box::new(if parallel {
                    Protocol::Par(
                        Box::new(message(&mut next, 0, 3, false)),
                        Box::new(message(&mut next, 0, 1, false)),
                    )
                } else {
                    message(&mut next, 0, 3, false)
                });
                let b = Box::new(if parallel {
                    Protocol::Par(
                        Box::new(message(&mut next, 0, 3, false)),
                        Box::new(message(&mut next, 0, 1, false)),
                    )
                } else {
                    message(&mut next, 0, 1, false)
                });
                let choice = if resolved {
                    Protocol::Resolved(a, b, 0)
                } else {
                    Protocol::Route(a, b)
                };
                let source = Protocol::Seq(
                    Box::new(loan),
                    Box::new(Protocol::Seq(
                        Box::new(choice),
                        Box::new(message(&mut next, 1, 0, true)),
                    )),
                );
                let outer = Protocol::Par(
                    Box::new(source.clone()),
                    Box::new(Protocol::Seq(
                        Box::new(message(&mut next, 2, 0, false)),
                        Box::new(message(&mut next, 0, 2, false)),
                    )),
                );
                for (context, source) in [
                    ("base", source.clone()),
                    ("roll", Protocol::Roll(Box::new(source))),
                    ("outer-par", outer),
                ] {
                    let depth = source.syntax_depth();
                    let grammar = Grammar::new(depth);
                    let rank = grammar.rank(&source);
                    assert_eq!(grammar.protocol(depth, rank.clone()).0, source);
                    println!(
                        "multi-view-choice resolved={resolved} parallel={parallel} context={context} depth={depth} rank={rank}"
                    );
                }
            }
        }
    }
}
