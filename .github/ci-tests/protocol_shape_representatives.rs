//! Enumerate role-normalized source candidates without pruning a child by its
//! standalone verdict. Each child is retained if some incoming owner permits it.
use super::{Protocol, context, syntax::Grammar};

struct Child {
    source: Protocol,
    rank: usize,
    roles: Vec<u8>,
    transfer: bool,
    head: Option<u8>,
}

impl Child {
    fn new(source: Protocol, rank: usize) -> Self {
        let mut messages = Vec::new();
        source.messages(&mut messages);
        let roles = messages
            .iter()
            .flat_map(|message| match message {
                Protocol::Message { from, to, .. } => [*from, *to],
                _ => unreachable!(),
            })
            .collect();
        let transfer = context::has_transfer(&source);
        let head = context::controller(&source);
        Self {
            source,
            rank,
            roles,
            transfer,
            head,
        }
    }
}

fn canonical_roles(roles: impl IntoIterator<Item = u8>, resource: bool) -> bool {
    let mut seen = if resource { 1 << 3 } else { 0 };
    let mut next = 0;
    for role in roles {
        if seen & (1 << role) == 0 {
            if role != next {
                return false;
            }
            seen |= 1 << role;
            next += 1;
        }
    }
    true
}

fn children(grammar: &Grammar, depth: usize) -> Vec<Child> {
    let count = usize::try_from(grammar.count(depth)).expect("bounded child population");
    (0..count)
        .filter_map(|rank| {
            let (source, ..) = grammar.protocol(depth, rank.into());
            context::possible_child(&source).then(|| Child::new(source, rank))
        })
        .collect()
}

fn append(ranks: &mut Vec<usize>, source: Protocol, rank: usize, grammar: &Grammar) {
    if context::valid(&source) {
        debug_assert_eq!(grammar.rank(&source), rank.into());
        ranks.push(rank);
    }
}

pub(super) fn catalog(depth: usize) -> Result<Vec<usize>, &'static str> {
    if depth > 2 {
        return Err("source representative depth must be zero, one or two");
    }
    let grammar = Grammar::new(depth);
    if depth < 2 {
        let count = usize::try_from(grammar.count(depth)).expect("bounded shallow population");
        return Ok((0..count)
            .filter(|rank| {
                let (source, ..) = grammar.protocol(depth, (*rank).into());
                let child = Child::new(source, *rank);
                canonical_roles(child.roles.iter().copied(), child.transfer)
                    && context::valid(&child.source)
            })
            .collect());
    }
    let leaves = children(&grammar, 0);
    let trees = children(&grammar, 1);
    let mut ranks = Vec::new();
    for child in &trees {
        if canonical_roles(child.roles.iter().copied(), child.transfer) {
            append(
                &mut ranks,
                Protocol::Roll(Box::new(child.source.clone())),
                child.rank,
                &grammar,
            );
        }
    }
    let leaf_count = usize::try_from(grammar.count(0)).unwrap();
    let tree_count = usize::try_from(grammar.count(1)).unwrap();
    let through = leaf_count + tree_count;
    let left_tall = tree_count * through;
    let pairs = left_tall + leaf_count * tree_count;
    for kind in 0..4 {
        for (lefts, right_tall) in [(&trees, false), (&leaves, true)] {
            for left in lefts {
                let right_leaves = if right_tall {
                    &[][..]
                } else {
                    leaves.as_slice()
                };
                for right in right_leaves.iter().chain(&trees) {
                    if kind >= 2 && (left.head.is_none() || left.head != right.head) {
                        continue;
                    }
                    if !canonical_roles(
                        left.roles.iter().chain(&right.roles).copied(),
                        left.transfer || right.transfer,
                    ) {
                        continue;
                    }
                    let a = Box::new(left.source.clone());
                    let b = Box::new(right.source.clone());
                    let source = match kind {
                        0 => Protocol::Seq(a, b),
                        1 => Protocol::Par(a, b),
                        2 => Protocol::Route(a, b),
                        3 => Protocol::Resolved(a, b, 0),
                        _ => unreachable!(),
                    };
                    let position = if right_tall {
                        left_tall + left.rank * tree_count + right.rank
                    } else {
                        left.rank * through
                            + right.rank
                            + if right.source.syntax_depth() == 1 {
                                leaf_count
                            } else {
                                0
                            }
                    };
                    append(
                        &mut ranks,
                        source,
                        tree_count + kind * pairs + position,
                        &grammar,
                    );
                }
            }
        }
    }
    assert!(
        ranks.windows(2).all(|pair| pair[0] < pair[1]),
        "duplicate or out-of-order representative"
    );
    Ok(ranks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_role_order_matches_role_permutation_normalization() {
        let grammar = Grammar::new(1);
        for depth in 0..=1 {
            let count = usize::try_from(grammar.count(depth)).unwrap();
            for rank in 0..count {
                let (source, ..) = grammar.protocol(depth, rank.into());
                let child = Child::new(source, rank);
                assert_eq!(
                    canonical_roles(child.roles, child.transfer),
                    super::super::symmetry::canonical(&child.source) == child.source
                );
            }
        }
    }

    #[test]
    fn resource_contexts_keep_children_rejected_as_standalone_protocols() {
        let grammar = Grammar::new(1);
        let trees = children(&grammar, 1);
        assert_eq!(trees.len(), 956);
        let route_rank = 2080 + 11 * 32 + 9;
        let child = trees.iter().find(|child| child.rank == route_rank).unwrap();
        assert!(!context::valid(&child.source));
        assert!(context::in_context(&child.source, Some(1)));
    }

    #[test]
    fn depth_two_ranks_reconstruct_the_same_canonical_source_candidates() {
        let ranks = catalog(2).unwrap();
        let grammar = Grammar::new(2);
        for rank in &ranks {
            let (source, ..) = grammar.protocol(2, (*rank).into());
            assert_eq!(source, super::super::symmetry::canonical(&source));
            assert!(context::valid(&source));
        }
        for raw in [17249225usize, 17250249, 17261608, 17262632] {
            let (source, ..) = grammar.protocol(2, raw.into());
            let canonical = super::super::symmetry::canonical(&source);
            let rank = usize::try_from(grammar.rank(&canonical)).unwrap();
            assert!(
                ranks.binary_search(&rank).is_ok(),
                "lost contextual choice {raw}"
            );
        }
        // Both parallel children are compound. Include independent resource
        // transfers as well as each ordinary nested constructor.
        for raw in [17315339usize, 17316363, 17592014, 19788164, 21851467] {
            let (source, ..) = grammar.protocol(2, raw.into());
            let Protocol::Par(left, right) = source else {
                panic!("missing parallel parent")
            };
            assert_eq!(left.syntax_depth(), 1);
            assert_eq!(right.syntax_depth(), 1);
            assert!(ranks.binary_search(&raw).is_ok());
        }
        println!("canonical source candidates depth=2 count={}", ranks.len());
    }
}
