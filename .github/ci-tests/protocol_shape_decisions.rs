//! Resolver inputs for every source path and each one-resolver perturbation.
use super::Protocol;
use std::collections::BTreeSet;

fn paths(source: &Protocol) -> BTreeSet<Vec<u16>> {
    match source {
        Protocol::Message { .. } => BTreeSet::from([Vec::new()]),
        Protocol::Seq(a, b) | Protocol::Par(a, b) => {
            let left = paths(a);
            let right = paths(b);
            left.iter()
                .flat_map(|a| {
                    right.iter().map(move |b| {
                        let mut both = a.clone();
                        both.extend(b);
                        both.sort_unstable();
                        both
                    })
                })
                .collect()
        }
        Protocol::Route(a, b) => paths(a).into_iter().chain(paths(b)).collect(),
        Protocol::Resolved(a, b, id) => paths(a)
            .into_iter()
            .chain(paths(b).into_iter().map(|mut choice| {
                let position = choice.binary_search(id).unwrap_err();
                choice.insert(position, *id);
                choice
            }))
            .collect(),
        Protocol::Roll(body) => paths(body),
    }
}

pub(super) fn patterns(source: &Protocol, resolvers: u16) -> BTreeSet<Vec<u16>> {
    let paths = paths(source);
    assert_eq!(
        paths.iter().flatten().copied().collect::<BTreeSet<_>>(),
        (0..resolvers).collect()
    );
    let mut patterns = paths.clone();
    for path in paths {
        for id in 0..resolvers {
            let mut changed = path.clone();
            match changed.binary_search(&id) {
                Ok(position) => {
                    changed.remove(position);
                }
                Err(position) => changed.insert(position, id),
            }
            patterns.insert(changed);
        }
    }
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(depth: usize) -> Protocol {
        if depth == 0 {
            super::super::message(&mut 0, 0, 1, false)
        } else {
            Protocol::Resolved(Box::new(tree(depth - 1)), Box::new(tree(depth - 1)), 0)
        }
    }

    #[test]
    fn three_resolvers_keep_every_boolean_assignment() {
        let mut source = tree(2);
        let (_, count) = super::super::syntax::number(&mut source);
        let expected = (0..8)
            .map(|bits| (0..3).filter(|id| bits & (1 << id) != 0).collect())
            .collect();
        assert_eq!(patterns(&source, count), expected);
    }

    #[test]
    fn independent_choices_keep_their_joint_assignments() {
        let mut source = Protocol::Par(Box::new(tree(2)), Box::new(tree(2)));
        let (_, count) = super::super::syntax::number(&mut source);
        let choices = patterns(&source, count);
        for a in paths(match &source {
            Protocol::Par(a, _) => a,
            _ => unreachable!(),
        }) {
            for b in paths(match &source {
                Protocol::Par(_, b) => b,
                _ => unreachable!(),
            }) {
                let mut both = a.clone();
                both.extend(b);
                assert!(choices.contains(&both));
            }
        }
    }

    #[test]
    fn nested_choices_reduce_unreached_inputs_and_keep_every_selected_message() {
        let mut source = tree(3);
        let (_, count) = super::super::syntax::number(&mut source);
        fn selected(source: &Protocol, choice: &[u16]) -> u8 {
            match source {
                Protocol::Message { event: label, .. } => *label,
                Protocol::Resolved(a, b, id) => {
                    selected(if choice.contains(id) { b } else { a }, choice)
                }
                _ => unreachable!(),
            }
        }
        let choices = patterns(&source, count);
        let full = (0..128)
            .map(|bits| {
                let choice = (0..count)
                    .filter(|id| bits & (1 << id) != 0)
                    .collect::<Vec<_>>();
                selected(&source, &choice)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            choices
                .iter()
                .map(|choice| selected(&source, choice))
                .collect::<BTreeSet<_>>(),
            full
        );
        assert!(choices.len() < 128);
        println!(
            "resolver-case depth=3 rank={} patterns={} full_assignments=128",
            super::super::syntax::Grammar::new(3).rank(&source),
            choices.len()
        );
    }

    #[test]
    fn deep_alternatives_cover_each_path_and_each_resolver_without_a_bitmask() {
        let mut source = tree(6);
        let (_, count) = super::super::syntax::number(&mut source);
        assert_eq!(count, 63);
        let bases = paths(&source);
        assert_eq!(bases.len(), 64);
        let choices = patterns(&source, count);
        assert!(choices.len() <= 64 * 64);
        for path in bases {
            assert!(choices.contains(&path));
            for id in 0..count {
                let mut changed = path.iter().copied().collect::<BTreeSet<_>>();
                if !changed.remove(&id) {
                    changed.insert(id);
                }
                assert!(choices.contains(&changed.into_iter().collect::<Vec<_>>()));
            }
        }
        assert!(choices.iter().any(|choice| choice.contains(&62)));
    }
}
