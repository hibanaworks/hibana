//! Bounded independent grammar check for compressed route-entry topology.
//!
//! Enumerates every Seq/Par/Choice tree with 1..5 atoms and zero or one Roll at
//! any node. This is one unfolding: Roll preserves body order. Causality is
//! constructed directly from the grammar, without production markers or links.
//! Every atom subset is considered as one logical view; other views cannot
//! affect its chain. The compiler-shaped chain algorithm is compared with an
//! exhaustive greatest-predecessor query and every contiguous-arm lower bound.
//! This host-only checker does not execute or certify the Rust serializer,
//! decoder, source-marker correspondence, or sorted candidate lookup.

const MAX_ATOMS: usize = 5;
const MAX_ROLLS: usize = 1;

#[derive(Clone, Copy, Debug)]
enum Composition {
    Sequential,
    Parallel,
    Choice,
}

#[derive(Clone, Debug)]
enum Source {
    Atom,
    Binary(Composition, Box<Source>, Box<Source>),
    Roll(Box<Source>),
}

fn sources(atoms: usize, rolls: usize) -> Vec<Source> {
    let mut result = Vec::new();
    if atoms == 1 && rolls == 0 {
        result.push(Source::Atom);
    }
    if rolls != 0 {
        result.extend(
            sources(atoms, rolls - 1)
                .into_iter()
                .map(|body| Source::Roll(Box::new(body))),
        );
    }
    for left_atoms in 1..atoms {
        for left_rolls in 0..=rolls {
            let left_sources = sources(left_atoms, left_rolls);
            let right_sources = sources(atoms - left_atoms, rolls - left_rolls);
            for left in &left_sources {
                for right in &right_sources {
                    for kind in [
                        Composition::Sequential,
                        Composition::Parallel,
                        Composition::Choice,
                    ] {
                        result.push(Source::Binary(
                            kind,
                            Box::new(left.clone()),
                            Box::new(right.clone()),
                        ));
                    }
                }
            }
        }
    }
    result
}

/// Bit i in result[j] means i causally precedes j in the source grammar.
fn grammar_predecessors(source: &Source) -> Vec<u32> {
    match source {
        Source::Atom => vec![0],
        Source::Roll(body) => grammar_predecessors(body),
        Source::Binary(kind, left, right) => {
            let mut before = grammar_predecessors(left);
            let split = before.len();
            let all_left = if matches!(kind, Composition::Sequential) {
                (1u32 << split) - 1
            } else {
                0
            };
            before.extend(
                grammar_predecessors(right)
                    .into_iter()
                    .map(|prior| (prior << split) | all_left),
            );
            before
        }
    }
}

fn predecessor_cut(source: &Source, before: usize, after: usize) -> Option<usize> {
    match source {
        Source::Atom => None,
        Source::Roll(body) => predecessor_cut(body, before, after),
        Source::Binary(kind, left, right) => {
            let split = grammar_predecessors(left).len();
            if after < split {
                predecessor_cut(left, before, after)
            } else if before < split {
                (!matches!(kind, Composition::Sequential)).then_some(0)
            } else {
                predecessor_cut(right, before - split, after - split).map(|cut| split + cut)
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    shapes: usize,
    triples: usize,
    views: usize,
    links: usize,
    intervals: usize,
    skipped: usize,
}

fn main() {
    let mut counts = Counts::default();
    for atoms in 1..=MAX_ATOMS {
        for rolls in 0..=MAX_ROLLS {
            for source in sources(atoms, rolls) {
                counts.shapes += 1;
                let before = grammar_predecessors(&source);
                assert_eq!(before.len(), atoms, "grammar width: {source:?}");
                for b in 0..atoms {
                    for q in 0..b {
                        for a in 0..q {
                            counts.triples += 1;
                            if before[b] & (1 << a) != 0 && before[b] & (1 << q) == 0 {
                                assert_ne!(
                                    before[q] & (1 << a),
                                    0,
                                    "source interpolation: source={source:?} a={a} q={q} b={b}"
                                );
                            }
                        }
                    }
                }
                for view in 0..(1u32 << atoms) {
                    counts.views += 1;
                    let mut links: Vec<Option<usize>> = vec![None; atoms];
                    let mut last = None;
                    for step in 0..atoms {
                        if view & (1 << step) == 0 {
                            continue;
                        }
                        let mut prior = last;
                        let mut cut_prior = last;
                        while let Some(index) = cut_prior {
                            let Some(cut) = predecessor_cut(&source, index, step) else { break };
                            assert!(cut <= index);
                            for omitted in cut..=index {
                                assert_eq!(before[step] & (1 << omitted), 0);
                            }
                            cut_prior = (0..cut).rev().find(|&candidate| view & (1 << candidate) != 0);
                        }
                        while let Some(index) = prior {
                            if before[step] & (1u32 << index) != 0 {
                                break;
                            }
                            counts.skipped += 1;
                            prior = links[index];
                        }
                        links[step] = prior;
                        assert_eq!(cut_prior, prior, "interval skip: source={source:?} view={view} step={step}");
                        last = Some(step);
                        let exhaustive = before[step] & view;
                        let greatest = (exhaustive != 0)
                            .then(|| (u32::BITS - 1 - exhaustive.leading_zeros()) as usize);
                        counts.links += 1;
                        assert_eq!(
                            prior, greatest,
                            "greatest predecessor: source={source:?} view={view} step={step}"
                        );
                        // Every upper endpoint containing step is equivalent:
                        // a strict predecessor is already before step < stop.
                        for start in 0..=step {
                            counts.intervals += 1;
                            let expanded = exhaustive >> start != 0;
                            let compressed = prior.is_some_and(|index| index >= start);
                            assert_eq!(
                                expanded, compressed,
                                "compressed entry: source={source:?} view={view} start={start} step={step}"
                            );
                        }
                    }
                }
            }
        }
    }
    // Fix the enumerated domain so a generator change cannot reduce coverage.
    let expected = Counts {
        shapes: 12_542,
        triples: 117_828,
        views: 381_076,
        links: 943_106,
        intervals: 2_810_666,
        skipped: 417_944,
    };
    assert_eq!(counts, expected, "enumeration coverage");
    println!(
        "Route topology finite grammar: atoms=1..{MAX_ATOMS} rolls=0..{MAX_ROLLS} \
         shapes={} triples={} views={} links={} intervals={} skipped={}",
        counts.shapes,
        counts.triples,
        counts.views,
        counts.links,
        counts.intervals,
        counts.skipped,
    );
}
