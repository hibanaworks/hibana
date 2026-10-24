//! Seeded syntax ranks and failure-preserving subtree reduction.
use super::{Protocol, syntax};
#[path = "kani_json.rs"]
mod json;
use num_bigint::BigUint;
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    io::{BufWriter, Write},
    path::Path,
    process::Command,
};

fn below(bound: &BigUint, word: &mut impl FnMut() -> u64) -> BigUint {
    assert!(*bound != BigUint::from(0u8));
    let bits = (bound - 1u8).bits();
    if bits == 0 {
        return 0u8.into();
    }
    loop {
        let mut bytes = Vec::new();
        for _ in 0..bits.div_ceil(64) {
            bytes.extend(word().to_le_bytes());
        }
        bytes.truncate(usize::try_from(bits.div_ceil(8)).unwrap());
        if !bits.is_multiple_of(8) {
            *bytes.last_mut().unwrap() &= (1u8 << (bits % 8)) - 1;
        }
        let rank = BigUint::from_bytes_le(&bytes);
        if rank < *bound {
            return rank;
        }
    }
}

pub(super) fn sample(
    depth: usize,
    seed: u64,
    count: usize,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if depth > 8 || count == 0 {
        return Err("sample depth must be at most eight and count must be positive".into());
    }
    let grammar = syntax::Grammar::new(depth);
    if BigUint::from(count) > *grammar.count(depth) {
        return Err("sample exceeds syntax population".into());
    }
    // SplitMix64 supplies reproducible words; rejection avoids modulo bias.
    let mut state = seed;
    let mut word = || {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    };
    let mut seen = BTreeSet::new();
    let mut writer = BufWriter::new(fs::File::create_new(output)?);
    writeln!(writer, "depth\tseed\tordinal\trank")?;
    for ordinal in 0..count {
        let rank = loop {
            let rank = below(grammar.count(depth), &mut word);
            if seen.insert(rank.clone()) {
                break rank;
            }
        };
        writeln!(writer, "{depth}\t{seed}\t{ordinal}\t{rank}")?;
    }
    writer.flush()?;
    println!("sample depth={depth} seed={seed} cases={count}");
    Ok(())
}

fn reductions(source: &Protocol) -> BTreeSet<Protocol> {
    let mut result = BTreeSet::new();
    match source {
        Protocol::Message { .. } => {}
        Protocol::Roll(body) => {
            result.insert((**body).clone());
            for reduced in reductions(body) {
                result.insert(Protocol::Roll(Box::new(reduced)));
            }
        }
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => {
            result.insert((**a).clone());
            result.insert((**b).clone());
            let join = |a, b| match source {
                Protocol::Seq(..) => Protocol::Seq(a, b),
                Protocol::Par(..) => Protocol::Par(a, b),
                Protocol::Route(..) => Protocol::Route(a, b),
                Protocol::Resolved(_, _, id) => Protocol::Resolved(a, b, *id),
                _ => unreachable!(),
            };
            for child in reductions(a) {
                result.insert(join(Box::new(child), b.clone()));
            }
            for child in reductions(b) {
                result.insert(join(a.clone(), Box::new(child)));
            }
        }
    }
    result
        .into_iter()
        .map(|mut source| {
            syntax::number(&mut source);
            source
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    RuntimeOnly,
    ModelOnly,
    Both,
    Stalled,
    BorrowAdmission,
    CausalAdmission,
}

fn failure(report: &str) -> Option<Failure> {
    for line in report.lines() {
        if let Some(counts) = line.strip_prefix(
            "uncaught exception: generated Endpoint/source conformance failed: runtime-only=",
        ) {
            let (runtime, model) = counts.split_once(" model-only=")?;
            let runtime = runtime.parse::<usize>().ok()?;
            let model = model.parse::<usize>().ok()?;
            return match (runtime > 0, model > 0) {
                (true, false) => Some(Failure::RuntimeOnly),
                (false, true) => Some(Failure::ModelOnly),
                (true, true) => Some(Failure::Both),
                _ => None,
            };
        }
    }
    [
        ("generated localside stalled", Failure::Stalled),
        (
            "operation outside borrowed scope was not rejected:",
            Failure::BorrowAdmission,
        ),
        (
            "send before required receive was not rejected:",
            Failure::CausalAdmission,
        ),
    ]
    .into_iter()
    .find_map(|(message, kind)| report.contains(message).then_some(kind))
}

fn save_source(path: &Path, source: &Protocol) -> Result<(), Box<dyn Error>> {
    let depth = source.syntax_depth();
    let rank = syntax::Grammar::new(depth).rank(source);
    fs::write(
        path,
        format!(
            "depth\trank\toperators\n{depth}\t{rank}\t{}\n",
            source.operator_count()
        ),
    )?;
    Ok(())
}

pub(super) fn shrink(
    repo: &Path,
    depth: usize,
    rank: BigUint,
    output: &Path,
    probe: &[String],
) -> Result<(), Box<dyn Error>> {
    if depth > 8 || probe.is_empty() {
        return Err("invalid shrink depth or missing probe executable".into());
    }
    let grammar = syntax::Grammar::new(depth);
    if rank >= *grammar.count(depth) {
        return Err("shrink rank exceeds population".into());
    }
    let mut best = grammar.protocol(depth, rank).0;
    fs::create_dir(output)?;
    save_source(&output.join("best.tsv"), &best)?;
    let mut trials = BufWriter::new(fs::File::create_new(output.join("trials.tsv"))?);
    writeln!(trials, "trial\tdepth\trank\toperators\tstatus\tfailure")?;
    let mut number = 0usize;
    let mut run = |source: &Protocol| -> Result<Option<Failure>, Box<dyn Error>> {
        let depth = source.syntax_depth();
        let rank = syntax::Grammar::new(depth).rank(source);
        let trial = output.join(format!("trial-{number}"));
        fs::create_dir(&trial)?;
        save_source(&trial.join("source.tsv"), source)?;
        fs::write(
            trial.join("probe.cwd"),
            std::env::current_dir()?.display().to_string(),
        )?;
        let args = probe[1..]
            .iter()
            .cloned()
            .chain([
                repo.display().to_string(),
                depth.to_string(),
                rank.to_string(),
                trial.display().to_string(),
            ])
            .collect::<Vec<_>>();
        let mut argv = fs::File::create_new(trial.join("probe.argv0"))?;
        for arg in std::iter::once(&probe[0]).chain(&args) {
            argv.write_all(arg.as_bytes())?;
            argv.write_all(&[0])?;
        }
        let observed = Command::new(&probe[0]).args(&args).output()?;
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&observed.stdout),
            String::from_utf8_lossy(&observed.stderr)
        );
        fs::write(trial.join("probe.log"), &report)?;
        let kind = if observed.status.success() {
            None
        } else {
            failure(&report)
        };
        writeln!(
            trials,
            "{number}\t{depth}\t{rank}\t{}\t{}\t{kind:?}",
            source.operator_count(),
            observed.status
        )?;
        trials.flush()?;
        number += 1;
        if !observed.status.success() && kind.is_none() {
            return Err(format!("unrecognized failure; retained {}", trial.display()).into());
        }
        Ok(kind)
    };
    let expected = run(&best)?.ok_or("source does not reproduce a recognized failure")?;
    loop {
        let mut next = None;
        for candidate in reductions(&best) {
            assert!(candidate.operator_count() < best.operator_count());
            if run(&candidate)? == Some(expected) {
                next = Some(candidate);
                break;
            }
        }
        let Some(reduced) = next else { break };
        best = reduced;
        save_source(&output.join("best.tsv"), &best)?;
    }
    if run(&best)? != Some(expected) {
        return Err("final reduced source no longer reproduces the original failure".into());
    }
    fs::write(
        output.join("result.txt"),
        format!("failure={expected:?}\nminimal_under=subtree-removal\n"),
    )?;
    println!(
        "shrunk failure={expected:?} depth={} rank={} operators={}",
        best.syntax_depth(),
        syntax::Grammar::new(best.syntax_depth()).rank(&best),
        best.operator_count()
    );
    Ok(())
}

pub(super) fn probe(
    repo: &Path,
    depth: usize,
    rank: BigUint,
    output: &Path,
) -> Result<i32, Box<dyn Error>> {
    if depth > 8 || rank >= *syntax::Grammar::new(depth).count(depth) {
        return Err("invalid probe source rank".into());
    }
    let observed = Command::new("bash")
        .arg(repo.join(".github/scripts/check_protocol_shapes.sh"))
        .args(["--syntax-depth", &depth.to_string(), &rank.to_string(), "1"])
        .output()?;
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&observed.stdout),
        String::from_utf8_lossy(&observed.stderr)
    );
    fs::write(output.join("validation.log"), &report)?;
    if !observed.status.success() {
        eprint!("{report}");
    }
    Ok(observed.status.code().unwrap_or(1))
}

pub(super) fn fuzz(
    repo: &Path,
    depth: usize,
    seed: u64,
    count: usize,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir(output)?;
    let samples = output.join("samples.tsv");
    sample(depth, seed, count, &samples)?;
    let mut checked = 0;
    let mut executed = 0;
    let mut rejected = 0;
    for line in fs::read_to_string(samples)?.lines().skip(1) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [_, _, ordinal, rank] = fields.as_slice() else {
            return Err("invalid sample row".into());
        };
        let rank = rank.parse::<BigUint>()?;
        let trial = output.join(format!("sample-{ordinal}"));
        fs::create_dir(&trial)?;
        let status = probe(repo, depth, rank.clone(), &trial)?;
        if status != 0 {
            let report = fs::read_to_string(trial.join("validation.log"))?;
            if failure(&report).is_some() {
                let executable = std::env::current_exe()?
                    .to_str()
                    .ok_or("invalid tool path")?
                    .to_owned();
                shrink(
                    repo,
                    depth,
                    rank,
                    &output.join(format!("failure-{ordinal}")),
                    &[executable, "syntax-probe".into()],
                )?;
            }
            return Err(format!("source sample failed; retained {}", trial.display()).into());
        }
        let generated = repo
            .join("target/protocol-shapes")
            .join(super::syntax_paths::relative(depth, &rank, 1))
            .join("inputs.json");
        let json::Json::Array(cases) = json::parse(&fs::read_to_string(generated)?)? else {
            return Err("invalid executed source inventory".into());
        };
        match cases.len() {
            0 => rejected += 1,
            1 => executed += 1,
            _ => return Err("one source probe emitted multiple cases".into()),
        }
        checked += 1;
    }
    if checked != count {
        return Err("incomplete syntax sample inventory".into());
    }
    fs::write(
        output.join("result.tsv"),
        format!(
            "depth\tseed\tchecked\texecuted\trejected\n{depth}\t{seed}\t{checked}\t{executed}\t{rejected}\n"
        ),
    )?;
    println!("sampled protocols checked={checked} executed={executed} rejected={rejected}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejection_has_no_modulo_bias_on_a_complete_word_cycle() {
        let mut word = 0u64;
        let mut counts = [0; 3];
        for _ in 0..192 {
            let rank = below(&3u8.into(), &mut || {
                let value = word;
                word += 1;
                value
            });
            counts[usize::try_from(rank).unwrap()] += 1;
        }
        assert_eq!(counts, [64; 3]);
    }

    #[test]
    fn seeds_reproduce_distinct_in_range_bigint_ranks_without_overwriting_evidence() {
        let root = std::env::temp_dir().join(format!(
            "hibana-syntax-seeds-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let a = root.join("a.tsv");
        let b = root.join("b.tsv");
        let c = root.join("c.tsv");
        sample(8, 20261024, 16, &a).unwrap();
        sample(8, 20261024, 16, &b).unwrap();
        sample(8, 20261025, 16, &c).unwrap();
        let text = fs::read_to_string(&a).unwrap();
        assert_eq!(text, fs::read_to_string(&b).unwrap());
        assert_ne!(text, fs::read_to_string(&c).unwrap());
        assert!(sample(8, 0, 1, &a).is_err());
        assert_eq!(text, fs::read_to_string(&a).unwrap());
        let grammar = syntax::Grammar::new(8);
        let mut seen = BTreeSet::new();
        for (ordinal, line) in text.lines().skip(1).enumerate() {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(&fields[..3], &["8", "20261024", &ordinal.to_string()]);
            let rank = fields[3].parse::<BigUint>().unwrap();
            assert!(rank < *grammar.count(8));
            assert!(seen.insert(rank.clone()));
            let source = grammar.protocol(8, rank.clone()).0;
            assert_eq!(source.syntax_depth(), 8);
            assert_eq!(grammar.rank(&source), rank);
        }
        assert_eq!(seen.len(), 16);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reductions_keep_grammar_identity_and_strictly_remove_operators() {
        let msg = || Box::new(super::super::message(&mut 0, 0, 1, false));
        let mut source = Protocol::Seq(
            Box::new(Protocol::Par(
                Box::new(Protocol::Roll(msg())),
                Box::new(Protocol::Route(msg(), msg())),
            )),
            Box::new(Protocol::Resolved(msg(), msg(), 0)),
        );
        syntax::number(&mut source);
        for candidate in reductions(&source) {
            assert!(candidate.operator_count() < source.operator_count());
            let grammar = syntax::Grammar::new(candidate.syntax_depth());
            assert!(
                grammar
                    .protocol(candidate.syntax_depth(), grammar.rank(&candidate))
                    .0
                    == candidate
            );
        }
        let mut tree = super::super::message(&mut 0, 0, 1, false);
        for _ in 0..3 {
            tree = Protocol::Seq(Box::new(tree.clone()), Box::new(tree));
        }
        syntax::number(&mut tree);
        println!(
            "shrink-control depth=3 rank={}",
            syntax::Grammar::new(3).rank(&tree)
        );
    }

    #[test]
    fn only_a_semantic_failure_is_a_reduction_witness() {
        assert_eq!(failure("compiler error E0080"), None);
        assert_eq!(failure("memory limit exceeded"), None);
        assert_eq!(
            failure(
                "uncaught exception: generated Endpoint/source conformance failed: runtime-only=0 model-only=2"
            ),
            Some(Failure::ModelOnly)
        );
        assert_eq!(
            failure(
                "uncaught exception: generated Endpoint/source conformance failed: runtime-only=1 model-only=0"
            ),
            Some(Failure::RuntimeOnly)
        );
    }
}
