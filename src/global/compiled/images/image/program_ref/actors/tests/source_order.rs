use std::{boxed::Box, vec, vec::Vec};

use crate::{
    eff::{EffAtom, EventOrigin, EventPayload},
    global::{
        compiled::{
            images::{ProgramImageBytes, ProgramImagePlan, SourceOrderIndex},
            lowering::CompiledProgramImage,
        },
        const_dsl::{EffList, ReentryMark, ScopeId, ScopeKind},
    },
};

#[derive(Clone, Debug)]
enum Tree {
    Send,
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>),
    Route(Box<Self>, Box<Self>),
    Roll(Box<Self>),
}

impl Tree {
    fn scopes(&self) -> u16 {
        match self {
            Self::Send => 0,
            Self::Seq(a, b) => a.scopes() + b.scopes(),
            Self::Par(a, b) | Self::Route(a, b) => 1 + a.scopes() + b.scopes(),
            Self::Roll(a) => 1 + a.scopes(),
        }
    }

    // Derive membership directly from the grammar, without image ranges or
    // parent links. Results use the same source-preorder identifiers as emit.
    fn rolls_at(&self, event: usize, ordinal: u16) -> Vec<ScopeId> {
        if event >= self.events() {
            return vec![];
        }
        match self {
            Self::Send => vec![],
            Self::Roll(body) => {
                let mut found = body.rolls_at(event, ordinal + 1);
                found.push(ScopeId::roll_scope(ordinal));
                found
            }
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) => {
                let first = ordinal + u16::from(!matches!(self, Self::Seq(..)));
                if event < a.events() {
                    a.rolls_at(event, first)
                } else {
                    b.rolls_at(event - a.events(), first + a.scopes())
                }
            }
        }
    }

    fn events(&self) -> usize {
        match self {
            Self::Send => 1,
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) => a.events() + b.events(),
            Self::Roll(a) => a.events(),
        }
    }

    fn markers(&self) -> usize {
        match self {
            Self::Send => 0,
            Self::Seq(a, b) => a.markers() + b.markers(),
            Self::Par(a, b) => a.markers() + b.markers() + 3,
            Self::Route(a, b) => a.markers() + b.markers() + 4,
            Self::Roll(a) => a.markers() + 2,
        }
    }

    // This comparator uses only the tree, independently of scope identifiers,
    // marker ordering, image serialization and the compiler's order index.
    fn before(&self, first: usize, second: usize) -> bool {
        if first >= second || second >= self.events() {
            return false;
        }
        match self {
            Self::Send => false,
            Self::Roll(a) => a.before(first, second),
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) => {
                let split = a.events();
                if second < split {
                    a.before(first, second)
                } else if first >= split {
                    b.before(first - split, second - split)
                } else {
                    matches!(self, Self::Seq(_, _))
                }
            }
        }
    }

    fn emit<const E: usize>(&self, source: &mut EffList<E>, next: &mut u16) {
        let kind = match self {
            Self::Send => {
                source.push_event_mut(EffAtom {
                    from: 0,
                    to: 1,
                    label: 1,
                    payload: EventPayload::Wire(0),
                    origin: EventOrigin::User,
                    lane: 0,
                });
                return;
            }
            Self::Seq(a, b) => {
                a.emit(source, next);
                b.emit(source, next);
                return;
            }
            Self::Par(..) => ScopeKind::Parallel,
            Self::Route(..) => ScopeKind::Route,
            Self::Roll(..) => ScopeKind::Roll,
        };
        let scope = ScopeId::new(kind, *next);
        *next += 1;
        let start = source.len();
        match self {
            Self::Par(a, b) | Self::Route(a, b) => {
                a.emit(source, next);
                let split = source.len();
                b.emit(source, next);
                let end = source.len();
                match self {
                    Self::Par(..) => source.push_parallel_scope_mut(scope, start, split, end),
                    Self::Route(..) => source.push_route_scope_mut(
                        scope,
                        start,
                        split,
                        end,
                        ReentryMark::SinglePass,
                    ),
                    Self::Send | Self::Seq(..) | Self::Roll(_) => unreachable!(),
                }
            }
            Self::Roll(a) => {
                a.emit(source, next);
                source.push_roll_scope_mut(scope, start, source.len());
            }
            Self::Send | Self::Seq(..) => unreachable!(),
        }
    }
}

fn compare<const E: usize, const B: usize>(tree: &Tree) {
    let events = tree.events();
    let mut source = EffList::<E>::new_partitioned(events, tree.markers(), 0);
    tree.emit(&mut source, &mut 0);
    let plan = ProgramImagePlan::from_program(&source);
    let summary = CompiledProgramImage::scan_const(&source);
    // Compiled references require storage lasting for the test process.
    let bytes = Box::leak(Box::new(ProgramImageBytes::<B>::from_image(
        &source,
        plan.columns(),
    )));
    let image = bytes.compiled_ref(&summary, plan.columns());
    let index = SourceOrderIndex::<E>::from_program(&image).expect("source order index");
    for marker in 0..=source.scope_markers().len() {
        assert_eq!(
            image.source_roll_at(marker),
            image.source_roll_at_scan(marker),
            "roll bounds {tree:?}: {marker}"
        );
        if let Some((child, start, end)) = image.source_roll_at_scan(marker) {
            for event in (0..=events + 1).chain([usize::MAX]) {
                let expected = (0..source.scope_markers().len()).any(|m| {
                    image.source_parallel_at(m).is_some_and(|(a, split, b)| {
                        (a <= start && end <= split && split <= event && event < b)
                            || (a <= event && event < split && split <= start && end <= b)
                    })
                });
                assert_eq!(
                    image.source_roll_has_parallel_entry(child, event),
                    expected,
                    "parallel entry {tree:?}: {child:?} / {event}"
                );
            }
        }
    }
    for event in (0..=events + 1).chain([usize::MAX]) {
        assert_eq!(
            image.source_roll_ancestors(event).collect::<Vec<_>>(),
            tree.rolls_at(event, 0),
            "roll ancestors {tree:?}: {event}"
        );
    }
    for first in (0..=events + 1).chain([usize::MAX]) {
        for second in (0..=events + 1).chain([usize::MAX]) {
            let expected = tree.before(first, second);
            assert_eq!(
                image.source_precedes(first, second),
                expected,
                "{tree:?}: {first}->{second}"
            );
            assert_eq!(
                image.source_precedes_scan(first, second),
                expected,
                "marker oracle {tree:?}: {first}->{second}"
            );
            assert_eq!(
                index.before(first, second),
                expected,
                "{tree:?}: {first}->{second}"
            );
        }
    }
}

fn grammar(depth: usize) -> Vec<Tree> {
    if depth == 0 {
        return vec![Tree::Send];
    }
    let children = grammar(depth - 1);
    let mut trees = vec![Tree::Send];
    for a in &children {
        trees.push(Tree::Roll(Box::new(a.clone())));
        for b in &children {
            trees.push(Tree::Seq(Box::new(a.clone()), Box::new(b.clone())));
            trees.push(Tree::Par(Box::new(a.clone()), Box::new(b.clone())));
            trees.push(Tree::Route(Box::new(a.clone()), Box::new(b.clone())));
        }
    }
    trees
}

#[test]
fn source_order_matches_every_depth_three_scope_tree() {
    let trees = grammar(3);
    assert_eq!(trees.len(), 19765);
    for tree in &trees {
        compare::<64, 512>(tree);
    }
}

fn sampled_tree(state: &mut u64, depth: usize) -> Tree {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    if depth == 0 {
        return Tree::Send;
    }
    match (*state >> 32) % 5 {
        0 => Tree::Send,
        1 => Tree::Roll(Box::new(sampled_tree(state, depth - 1))),
        kind => {
            let a = Box::new(sampled_tree(state, depth - 1));
            let b = Box::new(sampled_tree(state, depth - 1));
            match kind {
                2 => Tree::Seq(a, b),
                3 => Tree::Par(a, b),
                4 => Tree::Route(a, b),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn source_order_matches_deep_mixed_scope_trees() {
    let mut seed = 0x686962616e61;
    for _ in 0..1000 {
        compare::<2048, 16384>(&sampled_tree(&mut seed, 8));
    }
}
