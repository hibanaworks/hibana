//! Exhaustive certified forest model; does not claim arbitrary Rust refinement.
use std::collections::BTreeMap;
type Parent = Option<(usize, usize)>;
type Visits = BTreeMap<(usize, usize), usize>;
fn visit(visits: &mut Visits, node: usize, arm: usize) {
    *visits.entry((node, arm)).or_default() += 1;
}
fn reference(parents: &[Parent], root: usize) -> Visits {
    let mut result = Visits::new();
    visit(&mut result, root, 0);
    visit(&mut result, root, 1);
    for node in 0..parents.len() {
        if node == root {
            continue;
        }
        let mut at = node;
        while let Some((parent, arm)) = parents[at] {
            if parent == root {
                visit(&mut result, node, arm);
                visit(&mut result, node, arm);
                break;
            }
            at = parent;
        }
    }
    result
}
fn walk(parents: &[Parent], root: usize) -> Visits {
    let mut children = vec![[None; 2]; parents.len()];
    for (node, parent) in parents.iter().enumerate() {
        if let Some((parent, arm)) = *parent {
            assert!(children[parent][arm].replace(node).is_none());
        }
    }
    let mut result = Visits::new();
    for root_arm in 0..2 {
        visit(&mut result, root, root_arm);
        let Some(mut current) = children[root][root_arm] else {
            continue;
        };
        let mut next_arm = 0;
        let mut steps = 0;
        loop {
            steps += 1;
            assert!(
                steps <= 4 * parents.len() + 1,
                "stackless traversal failed to terminate"
            );
            if next_arm < 2 {
                let arm = next_arm;
                next_arm += 1;
                visit(&mut result, current, root_arm);
                if let Some(child) = children[current][arm] {
                    current = child;
                    next_arm = 0;
                }
            } else {
                let (parent, arm) = parents[current].expect("non-root must have parent");
                if parent == root {
                    break;
                }
                current = parent;
                next_arm = arm + 1;
            }
        }
    }
    result
}
fn enumerate(
    parents: &mut [Parent],
    node: usize,
    used: &mut [[bool; 2]],
    forests: &mut usize,
    queries: &mut usize,
) {
    if node == parents.len() {
        *forests += 1;
        for root in 0..parents.len() {
            assert_eq!(
                walk(parents, root),
                reference(parents, root),
                "forest {parents:?}, root {root}"
            );
            *queries += 1;
        }
        return;
    }
    parents[node] = None;
    enumerate(parents, node + 1, used, forests, queries);
    for parent in 0..node {
        for arm in 0..2 {
            if !used[parent][arm] {
                used[parent][arm] = true;
                parents[node] = Some((parent, arm));
                enumerate(parents, node + 1, used, forests, queries);
                used[parent][arm] = false;
            }
        }
    }
}
fn verify() -> (usize, usize) {
    let (mut forests, mut queries) = (0, 0);
    for count in 1..8 {
        enumerate(
            &mut vec![None; count],
            0,
            &mut vec![[false; 2]; count],
            &mut forests,
            &mut queries,
        );
    }
    (forests, queries)
}
fn main() {
    let (forests, queries) = verify();
    println!("Exact visitor multisets: {forests} certified forests, {queries} root queries.");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_visitor_multiplicity() {
        let parents = [None, Some((0, 0)), Some((1, 1)), Some((0, 1))];
        let mut expected = Visits::new();
        for (key, count) in [
            ((0, 0), 1),
            ((0, 1), 1),
            ((1, 0), 2),
            ((2, 0), 2),
            ((3, 1), 2),
        ] {
            expected.insert(key, count);
        }
        assert_eq!(reference(&parents, 0), expected);
        assert_eq!(walk(&parents, 0), expected);
        let mut missing = walk(&parents, 0);
        *missing.get_mut(&(2, 0)).unwrap() -= 1;
        assert_ne!(missing, expected, "lost duplicate visits must be rejected");
    }
    #[test]
    fn exhaustive_forests() {
        let (forests, queries) = verify();
        assert_eq!((forests, queries), (42275, 290580));
    }
}
