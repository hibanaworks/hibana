//! Constructor ancestry from the source tree, before and after execution.
use super::Protocol;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::Write,
    fs,
    io::{BufWriter, Write as _},
    path::{Path, PathBuf},
};

const MAX_LENGTH: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Edge {
    SeqLeft,
    SeqRight,
    ParLeft,
    ParRight,
    RouteLeft,
    RouteRight,
    ResolvedLeft,
    ResolvedRight,
    Roll,
}

impl Edge {
    fn name(self) -> &'static str {
        match self {
            Self::SeqLeft => "seq.left",
            Self::SeqRight => "seq.right",
            Self::ParLeft => "par.left",
            Self::ParRight => "par.right",
            Self::RouteLeft => "route.left",
            Self::RouteRight => "route.right",
            Self::ResolvedLeft => "resolved.left",
            Self::ResolvedRight => "resolved.right",
            Self::Roll => "roll.body",
        }
    }
    fn operator(self) -> &'static str {
        match self {
            Self::SeqLeft | Self::SeqRight => "seq",
            Self::ParLeft | Self::ParRight => "par",
            Self::RouteLeft | Self::RouteRight => "route",
            Self::ResolvedLeft | Self::ResolvedRight => "resolved",
            Self::Roll => "roll",
        }
    }
    fn sequence(self) -> bool {
        matches!(self, Self::SeqLeft | Self::SeqRight)
    }
}

#[derive(Default)]
pub(super) struct Coverage {
    // Keep one source location per distinct word, not a count of repeated nodes.
    words: BTreeMap<(&'static str, String), (usize, String)>,
}

impl Coverage {
    pub(super) fn add(&mut self, case: usize, source: &Protocol) {
        fn walk(
            out: &mut Coverage,
            case: usize,
            source: &Protocol,
            ancestry: &mut Vec<Edge>,
            path: &mut String,
            delegated: bool,
        ) {
            let children: Vec<_> = match source {
                Protocol::Message { .. } => Vec::new(),
                Protocol::Seq(a, b) => vec![(Edge::SeqLeft, a), (Edge::SeqRight, b)],
                Protocol::Par(a, b) => vec![(Edge::ParLeft, a), (Edge::ParRight, b)],
                Protocol::Route(a, b) => vec![(Edge::RouteLeft, a), (Edge::RouteRight, b)],
                Protocol::Resolved(a, b, _) => {
                    vec![(Edge::ResolvedLeft, a), (Edge::ResolvedRight, b)]
                }
                Protocol::Roll(body) => vec![(Edge::Roll, body)],
            };
            for (edge, body) in children {
                let path_len = path.len();
                path.push(if matches!(edge, Edge::Roll) {
                    'b'
                } else if matches!(
                    edge,
                    Edge::SeqLeft | Edge::ParLeft | Edge::RouteLeft | Edge::ResolvedLeft
                ) {
                    'l'
                } else {
                    'r'
                });
                ancestry.push(edge);
                out.record(case, "syntax", ancestry, path, false);
                if !edge.sequence() {
                    let semantic = ancestry
                        .iter()
                        .copied()
                        .filter(|edge| !edge.sequence())
                        .collect::<Vec<_>>();
                    out.record(case, "nested-sides", &semantic, path, false);
                    out.record(case, "nested-operators", &semantic, path, true);
                    if delegated && super::context::uses_resource(body) {
                        out.record(case, "delegated-sides", &semantic, path, false);
                        out.record(case, "delegated-operators", &semantic, path, true);
                    }
                }
                walk(out, case, body, ancestry, path, delegated);
                ancestry.pop();
                path.truncate(path_len);
            }
        }
        walk(
            self,
            case,
            source,
            &mut Vec::new(),
            &mut String::new(),
            super::context::has_transfer(source),
        );
    }

    fn record(
        &mut self,
        case: usize,
        family: &'static str,
        edges: &[Edge],
        path: &str,
        operators: bool,
    ) {
        for length in 1..=MAX_LENGTH.min(edges.len()) {
            let word = edges[edges.len() - length..]
                .iter()
                .map(|edge| {
                    if operators {
                        edge.operator()
                    } else {
                        edge.name()
                    }
                })
                .collect::<Vec<_>>()
                .join("/");
            self.words
                .entry((family, word))
                .or_insert_with(|| (case, path.to_owned()));
        }
    }

    pub(super) fn witnesses(&self) -> String {
        let mut out = String::from("family\tword\tcase\tpath\n");
        for ((family, word), (case, path)) in &self.words {
            writeln!(out, "{family}\t{word}\t{case}\t{path}").unwrap();
        }
        out
    }

    pub(super) fn counts(&self) -> String {
        let mut out = String::from("family\tlength\tcovered\tpossible_words\n");
        for (family, alphabet) in [
            ("syntax", 9usize),
            ("nested-sides", 7),
            ("nested-operators", 4),
            ("delegated-sides", 7),
            ("delegated-operators", 4),
        ] {
            for length in 1..=MAX_LENGTH {
                let count = self
                    .words
                    .keys()
                    .filter(|(name, word)| *name == family && word.split('/').count() == length)
                    .count();
                writeln!(
                    out,
                    "{family}\t{length}\t{count}\t{}",
                    alphabet.pow(length as u32)
                )
                .unwrap();
            }
        }
        out
    }
}

const NESTED_EDGES: [&str; 7] = [
    "par.left",
    "par.right",
    "route.left",
    "route.right",
    "resolved.left",
    "resolved.right",
    "roll.body",
];

fn read_witnesses(text: &str, length: usize) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut lines = text.lines();
    if lines.next() != Some("family\tword\tcase\tpath") {
        return Err("invalid executed coverage header".into());
    }
    let mut seen = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [family, word, case, path] = fields.as_slice() else {
            return Err("invalid executed coverage record".into());
        };
        case.parse::<usize>()?;
        if path.is_empty() || !path.bytes().all(|b| matches!(b, b'l' | b'r' | b'b')) {
            return Err("invalid coverage source path".into());
        }
        let edges = word.split('/').collect::<Vec<_>>();
        if edges.is_empty()
            || edges.len() > MAX_LENGTH
            || !edges.iter().all(|edge| match *family {
                "syntax" => NESTED_EDGES.contains(edge) || ["seq.left", "seq.right"].contains(edge),
                "nested-sides" | "delegated-sides" => NESTED_EDGES.contains(edge),
                "nested-operators" | "delegated-operators" => {
                    ["par", "route", "resolved", "roll"].contains(edge)
                }
                _ => false,
            })
        {
            return Err("unknown coverage family or constructor word".into());
        }
        if !seen.insert((*family, *word)) {
            return Err("duplicate executed coverage word".into());
        }
        if *family == "delegated-sides" && edges.len() == length {
            covered.insert((*word).to_owned());
        }
    }
    Ok(covered)
}

pub(super) fn uncovered(
    length: usize,
    output: &Path,
    runs: &[PathBuf],
) -> Result<(), Box<dyn Error>> {
    if !(1..=MAX_LENGTH).contains(&length) {
        return Err("coverage word length must be one through five".into());
    }
    let mut covered = BTreeSet::new();
    for directory in runs {
        // The validation script publishes these only after Endpoint execution,
        // source comparison, mutation controls and input hash checks succeed.
        let executed = fs::read_to_string(directory.join("executed-coverage.tsv"))?;
        if executed != fs::read_to_string(directory.join("source-coverage.tsv"))? {
            return Err("executed coverage differs from its source inventory".into());
        }
        covered.extend(read_witnesses(&executed, length)?);
    }
    let mut writer = BufWriter::new(fs::File::create_new(output)?);
    writeln!(writer, "length\trank\tword")?;
    let population = 7usize.pow(length as u32);
    for word_rank in 0..population {
        let mut remainder = word_rank;
        let mut edges = vec![""; length];
        for edge in edges.iter_mut().rev() {
            *edge = NESTED_EDGES[remainder % 7];
            remainder /= 7;
        }
        let word = edges.join("/");
        if !covered.contains(&word) {
            // Alternate the extra Endpoint round trip. This plan covers each
            // word once; it does not claim both resource variants per word.
            writeln!(
                writer,
                "{length}\t{}\t{word}",
                2 * word_rank + word_rank % 2
            )?;
        }
    }
    writer.flush()?;
    println!(
        "delegated-side coverage length={length} executed_words={} missing={} total={population}",
        covered.len(),
        population - covered.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_distinct_well_formed_executed_words_can_remove_coverage_targets() {
        let header = "family\tword\tcase\tpath\n";
        let row = "delegated-sides\tpar.left/route.right\t3\tlrr\n";
        assert_eq!(
            read_witnesses(&format!("{header}{row}"), 2).unwrap(),
            BTreeSet::from(["par.left/route.right".into()])
        );
        assert!(read_witnesses(&format!("{header}{row}{row}"), 2).is_err());
        for row in [
            "nested-sides\tseq.left\t0\tl",
            "unknown\tpar.left\t0\tl",
            "nested-sides\tpar.left\t0\t../x",
            "nested-sides\tpar.left\tx\tl",
        ] {
            assert!(read_witnesses(&format!("{header}{row}\n"), 1).is_err());
        }
    }

    fn leaf() -> Protocol {
        Protocol::Message {
            from: 0,
            to: 1,
            label: 0,
            owned: false,
        }
    }

    #[test]
    fn sequential_scaffolding_is_transparent_only_in_the_named_nested_families() {
        let source = Protocol::Par(
            Box::new(leaf()),
            Box::new(Protocol::Seq(
                Box::new(leaf()),
                Box::new(Protocol::Route(
                    Box::new(Protocol::Roll(Box::new(leaf()))),
                    Box::new(leaf()),
                )),
            )),
        );
        let mut coverage = Coverage::default();
        coverage.add(11, &source);
        assert_eq!(
            coverage
                .words
                .get(&("nested-sides", "par.right/route.left/roll.body".into())),
            Some(&(11, "rrlb".into()))
        );
        assert!(
            coverage
                .words
                .contains_key(&("syntax", "par.right/seq.right/route.left/roll.body".into()))
        );
        assert!(
            !coverage
                .words
                .contains_key(&("syntax", "par.right/route.left/roll.body".into()))
        );
        assert!(
            coverage
                .words
                .contains_key(&("nested-operators", "par/route/roll".into()))
        );
    }

    #[test]
    fn branches_do_not_create_sibling_ancestry_and_repeated_shapes_do_not_inflate_counts() {
        let source = Protocol::Par(
            Box::new(Protocol::Roll(Box::new(leaf()))),
            Box::new(Protocol::Resolved(Box::new(leaf()), Box::new(leaf()), 0)),
        );
        let mut coverage = Coverage::default();
        coverage.add(0, &source);
        let before = coverage.witnesses();
        coverage.add(1, &source);
        assert_eq!(coverage.witnesses(), before);
        assert!(
            !coverage
                .words
                .contains_key(&("nested-operators", "roll/resolved".into()))
        );
        assert!(coverage.counts().contains("nested-sides\t1\t5\t7\n"));
    }

    #[test]
    fn an_ordinary_sibling_cannot_cover_a_delegated_path() {
        let source = Protocol::Seq(
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                label: 0,
                owned: true,
            }),
            Box::new(Protocol::Par(
                Box::new(Protocol::Roll(Box::new(Protocol::Message {
                    from: 3,
                    to: 2,
                    label: 1,
                    owned: false,
                }))),
                Box::new(Protocol::Resolved(Box::new(leaf()), Box::new(leaf()), 0)),
            )),
        );
        let mut coverage = Coverage::default();
        coverage.add(0, &source);
        assert!(
            coverage
                .words
                .contains_key(&("nested-sides", "par.right/resolved.left".into()))
        );
        assert!(
            !coverage
                .words
                .contains_key(&("delegated-sides", "par.right/resolved.left".into()))
        );
        assert!(
            coverage
                .words
                .contains_key(&("delegated-sides", "par.left/roll.body".into()))
        );
    }
}
