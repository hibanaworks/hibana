//! Role renaming preserves syntax positions, labels and resolver identities.
use super::{Protocol, syntax::Grammar};
use num_bigint::BigUint;

pub(super) fn canonical(source: &Protocol) -> Protocol {
    fn rename(source: &mut Protocol, names: &mut [Option<u8>; 4], next: &mut u8) {
        match source {
            Protocol::Message { from, to, .. } => {
                for role in [from, to] {
                    *role = *names[usize::from(*role)].get_or_insert_with(|| {
                        let name = *next;
                        *next += 1;
                        name
                    });
                }
            }
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                rename(a, names, next);
                rename(b, names, next);
            }
            Protocol::Roll(body) => rename(body, names, next),
        }
    }
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let resource = messages
        .iter()
        .any(|message| matches!(message, Protocol::Message { owned: true, .. }));
    let mut names = [None; 4];
    if resource {
        names[3] = Some(3);
    }
    let mut result = source.clone();
    rename(&mut result, &mut names, &mut 0);
    result
}

pub(super) fn count(depth: usize) -> BigUint {
    let fixed = |leaves| Grammar::with_leaves(depth, leaves).count(depth).clone();
    // Burnside's lemma: ordinary protocols use all 24 role permutations.
    // When a resource occurs, its role 3 is fixed and the six permutations of
    // the other roles remain. Subtract ordinary trees from that population.
    let ordinary = (fixed(16) + 6u8 * fixed(4) + 8u8 * fixed(1)) / 24u8;
    let resource =
        ((fixed(32) - fixed(16)) + 3u8 * (fixed(8) - fixed(4)) + 2u8 * (fixed(2) - fixed(1))) / 6u8;
    ordinary + resource
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn permute(source: &mut Protocol, names: &[u8; 4]) {
        match source {
            Protocol::Message { from, to, .. } => {
                *from = names[usize::from(*from)];
                *to = names[usize::from(*to)];
            }
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => {
                permute(a, names);
                permute(b, names);
            }
            Protocol::Roll(body) => permute(body, names),
        }
    }

    #[test]
    fn canonical_roles_equal_literal_permutation_orbits() {
        let mut permutations = Vec::new();
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let names = [a, b, c, d];
                        if names.into_iter().collect::<BTreeSet<_>>().len() == 4 {
                            permutations.push(names);
                        }
                    }
                }
            }
        }
        let grammar = Grammar::new(1);
        for depth in 0..=1 {
            let mut classes = BTreeSet::new();
            for rank in 0..usize::try_from(grammar.count(depth)).unwrap() {
                let (source, _, _) = grammar.protocol(depth, rank.into());
                let mut messages = Vec::new();
                source.messages(&mut messages);
                let resource = messages
                    .iter()
                    .any(|m| matches!(m, Protocol::Message { owned: true, .. }));
                let mut orbit = BTreeSet::new();
                for names in &permutations {
                    if resource && names[3] != 3 {
                        continue;
                    }
                    let mut renamed = source.clone();
                    permute(&mut renamed, names);
                    orbit.insert(renamed);
                }
                let representative = canonical(&source);
                assert_eq!(Some(&representative), orbit.first());
                assert!(
                    orbit
                        .iter()
                        .all(|renamed| canonical(renamed) == representative)
                );
                classes.insert(representative);
            }
            assert_eq!(BigUint::from(classes.len()), count(depth));
            println!(
                "role-orbits depth={depth} raw={} representatives={}",
                grammar.count(depth),
                classes.len()
            );
        }
        assert_eq!(count(0), 7u8.into());
    }
}
