//! Private, dependency-free replay of finite elastic graph evidence.
//! gzip only decompresses fixtures; JSON decoding and proof algorithms live in Rust.
#[path = "kani_json.rs"]
mod kani_json;
use kani_json::Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
fn command(program: &str, args: &[&str], input: Option<&[u8]>) -> String {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect(program);
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input)
            .expect("write subprocess input");
    }
    let result = child.wait_with_output().expect("wait subprocess");
    assert!(
        result.status.success(),
        "{program} failed: {}",
        result.status
    );
    String::from_utf8(result.stdout).expect("UTF-8 output")
}

fn number(s: &str) -> usize {
    s.parse().expect("unsigned integer")
}
fn field(line: &str, key: &str) -> usize {
    let rest = line.split_once(key).expect("inventory field").1;
    number(rest.split(|c: char| !c.is_ascii_digit()).next().unwrap())
}
#[derive(Debug, Clone)]
struct Event {
    id: usize,
    key: Vec<usize>,
    baseline: usize,
    proposed: usize,
    members: BTreeSet<usize>,
    inner: String,
}
#[derive(Default, Clone)]
struct Graph {
    events: Vec<Event>,
    rolls: Vec<(usize, usize, usize)>,
    edges: BTreeSet<(usize, usize)>,
}
#[cfg(test)]
fn list(s: &str) -> Vec<usize> {
    if s.is_empty() {
        vec![]
    } else {
        s.split(',').map(number).collect()
    }
}
fn main() {
    run(&std::env::args().skip(1).collect::<Vec<_>>());
}
pub fn run(args: &[String]) {
    assert!(
        args.len() == 1 || (args.len() == 3 && args[1] == "--output"),
        "usage: check_roll_membership ROOT [--output NEW.json]"
    );
    let root = Path::new(&args[0]);
    if args.len() == 3 {
        assert!(
            !Path::new(&args[2]).exists(),
            "Replay output must be a new file"
        );
    }
    let inventory = fs::read_to_string(root.join("evidence/run-inventory.log")).unwrap();
    validate_inventory(&inventory);
    let raw = command(
        "gzip",
        &[
            "-dc",
            root.join("roll-membership-capacity-model.json.gz")
                .to_str()
                .unwrap(),
        ],
        None,
    );
    let graphs = decode_graphs(&kani_json::parse(&raw).expect("fixture JSON"));
    validate_graphs(&graphs);
    let recorded =
        kani_json::parse(&fs::read_to_string(root.join("roll-membership-z3.json")).unwrap())
            .expect("historical JSON");
    let model = fs::read_to_string(root.join("RollMembership.smt2")).unwrap();
    let names: Vec<_> = model
        .lines()
        .filter_map(|l| {
            l.strip_prefix("; Query ")
                .map(|l| l.split_once(": ").unwrap().1)
        })
        .collect();
    let outcomes: Vec<_> = model
        .lines()
        .filter_map(|l| l.strip_prefix("; EXPECT: "))
        .collect();
    let historical: Vec<Vec<_>> = array(get(&recorded, "checks"))
        .iter()
        .map(|c| {
            ["name", "expected", "actual"]
                .iter()
                .map(|key| string(get(c, key)))
                .collect()
        })
        .collect();
    assert_eq!(names.len(), 31);
    assert_eq!(outcomes.len(), 31);
    assert_eq!(historical.len(), 31);
    let graph_queries: BTreeSet<_> = names
        .iter()
        .filter_map(|n| n.strip_suffix("_concrete_coloring_all_edges"))
        .collect();
    assert_eq!(graph_queries, graphs.keys().map(String::as_str).collect());
    for i in 0..31 {
        assert_eq!(historical[i], vec![names[i], outcomes[i], outcomes[i]]);
    }
    let actual = command(
        "z3",
        &["-T:300", root.join("RollMembership.smt2").to_str().unwrap()],
        None,
    );
    let passed = solver_outcomes_match(&actual, &outcomes);
    let actual: Vec<_> = actual.lines().collect();
    assert_eq!(actual.len(), 31, "extra or missing solver output");
    for i in 0..31 {
        println!("{}: {} (expected {})", names[i], actual[i], outcomes[i]);
    }
    if args.len() == 3 {
        let version = command("z3", &["--version"], None);
        let version = version.split_whitespace().nth(2).expect("Z3 version");
        let mut checks = String::from("[");
        for i in 0..31 {
            if i != 0 {
                checks.push(',');
            }
            checks.push_str(&format!(
                "{{\"name\":{},\"expected\":{},\"actual\":{}}}",
                quote(names[i]),
                quote(outcomes[i]),
                quote(actual[i])
            ));
        }
        checks.push(']');
        let report = format!(
            "{{\"passed\":{passed},\"z3_version\":{},\"claim_limit\":{},\"checks\":{checks}}}\n",
            quote(version),
            quote(string(get(&recorded, "claim_limit")))
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])
            .expect("Replay output must be a new file");
        file.write_all(report.as_bytes()).unwrap();
    }
    assert!(
        passed,
        "solver result differs from historical gate (unknown fails closed)"
    );
    println!("PASS exact 31-query replay; universal Rust Covers/SameClassUnique remains unproved");
}

#[cfg(test)]
fn parse_graphs(rows: &str) -> BTreeMap<String, Graph> {
    let mut graphs = BTreeMap::<String, Graph>::new();
    for row in rows.lines() {
        let p: Vec<_> = row.split('\t').collect();
        let g = graphs.entry(p[1].to_owned()).or_default();
        match p[0] {
            "G" => assert_eq!(p.len(), 2),
            "S" => {
                assert_eq!(p.len(), 6);
                if p[5] == "Roll" {
                    g.rolls.push((number(p[2]), number(p[3]), number(p[4])));
                }
            }
            "V" => {
                assert_eq!(p.len(), 8);
                g.events.push(Event {
                    id: number(p[2]),
                    key: list(p[3]),
                    baseline: number(p[4]),
                    proposed: number(p[5]),
                    members: list(p[6]).into_iter().collect(),
                    inner: p[7].into(),
                });
            }
            "E" => {
                assert_eq!(p.len(), 4);
                g.edges.insert((number(p[2]), number(p[3])));
            }
            _ => panic!("unknown fixture record"),
        }
    }
    graphs
}
fn validate_graphs(graphs: &BTreeMap<String, Graph>) {
    assert!(!graphs.is_empty());
    for (name, g) in graphs {
        let members: Vec<BTreeSet<usize>> = (0..g.events.len())
            .map(|i| {
                g.rolls
                    .iter()
                    .filter(|&&(_, start, end)| start <= i && i < end)
                    .map(|&(id, _, _)| id)
                    .collect()
            })
            .collect();
        let mut edges = BTreeSet::new();
        let mut assigned = Vec::new();
        for (j, b) in g.events.iter().enumerate() {
            assert_eq!(b.id, j, "{name}: event id");
            assert!(b.key.len() >= 2);
            assert_eq!(members[j], b.members, "{name}: membership");
            let mut forbidden = BTreeSet::new();
            for (i, a) in g.events[..j].iter().enumerate() {
                if a.key == b.key
                    && a.key[0] != a.key[1]
                    && (a.baseline != b.baseline || members[i] != members[j])
                {
                    edges.insert((i, j));
                    forbidden.insert(assigned[i]);
                }
            }
            let color = (0..256)
                .find(|k| !forbidden.contains(k))
                .expect("prefix exhausted");
            assert_eq!(color, b.proposed, "{name}/{j}: greedy color");
            assigned.push(color);
        }
        assert_eq!(edges, g.edges, "{name}: reconstructed edges");
        for (i, a) in g.events.iter().enumerate() {
            for (j, b) in g.events.iter().enumerate() {
                assert_eq!(
                    members[i] == members[j],
                    a.inner == b.inner,
                    "{name}: innermost/full membership equivalence"
                );
            }
        }
        for &(i, j) in &edges {
            assert_ne!(assigned[i], assigned[j], "{name}: edge coloring");
        }
        if name == "minimal" {
            assert!(edges.contains(&(2, 6)));
        }
    }
}
fn validate_inventory(inventory: &str) {
    let mut observed = BTreeMap::new();
    for line in inventory
        .lines()
        .filter(|s| s.contains("INVENTORY recv idx="))
    {
        let idx = field(line, "INVENTORY recv idx=");
        let enabled = line
            .split_once("enabled=")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        assert!(enabled == "true" || enabled == "false");
        observed.insert(
            idx,
            (
                enabled == "true",
                field(line, "peer: "),
                field(line, "label: "),
                field(line, "frame_label: "),
                field(line, "lane: "),
            ),
        );
    }
    assert_eq!(
        observed.keys().copied().collect::<Vec<_>>(),
        [0, 2, 3, 5, 6, 7, 9]
    );
    assert_eq!(observed[&2].2, 52);
    assert_eq!(observed[&6].2, 55);
    let mut pairs = BTreeSet::new();
    for (&a, av) in &observed {
        for (&b, bv) in &observed {
            if av.0 && bv.0 && av.1 == bv.1 && av.3 == bv.3 && av.4 == bv.4 {
                pairs.insert((a, b));
            }
        }
    }
    // Bind the exported SMT collision disjunction to the live inventory.
    assert_eq!(
        pairs,
        [
            (2, 2),
            (2, 6),
            (3, 3),
            (3, 7),
            (6, 2),
            (6, 6),
            (7, 3),
            (7, 7)
        ]
        .into()
    );
}
fn solver_outcomes_match(actual: &str, expected: &[&str]) -> bool {
    actual.lines().collect::<Vec<_>>() == expected
        && expected.iter().all(|s| *s == "sat" || *s == "unsat")
}
fn get<'a>(value: &'a Json, key: &str) -> &'a Json {
    value.get(key).expect(key)
}
fn array(value: &Json) -> &[Json] {
    if let Json::Array(v) = value {
        v
    } else {
        panic!("expected JSON array")
    }
}
fn string(value: &Json) -> &str {
    if let Json::String(v) = value {
        v
    } else {
        panic!("expected JSON string")
    }
}
fn integer(value: &Json) -> usize {
    if let Json::Number(v) = value {
        number(v)
    } else {
        panic!("expected JSON integer")
    }
}
fn decode_graphs(value: &Json) -> BTreeMap<String, Graph> {
    let Json::Object(entries) = get(value, "graphs") else {
        panic!("expected graph object")
    };
    entries
        .iter()
        .map(|(name, source)| {
            let events = array(get(source, "events"))
                .iter()
                .map(|e| Event {
                    id: integer(get(e, "id")),
                    key: array(get(e, "key")).iter().map(integer).collect(),
                    baseline: integer(get(e, "baseline_color")),
                    proposed: integer(get(e, "proposed_color")),
                    members: array(get(e, "roll_membership"))
                        .iter()
                        .map(integer)
                        .collect(),
                    inner: match get(e, "innermost_roll") {
                        Json::Null => "null".into(),
                        value => integer(value).to_string(),
                    },
                })
                .collect();
            let rolls = array(get(source, "scopes"))
                .iter()
                .filter(|s| string(get(s, "kind")) == "Roll")
                .map(|s| {
                    (
                        integer(get(s, "id")),
                        integer(get(s, "start")),
                        integer(get(s, "end")),
                    )
                })
                .collect();
            let edges = array(get(source, "edges"))
                .iter()
                .map(|edge| {
                    let edge = array(edge);
                    assert_eq!(edge.len(), 2);
                    (integer(&edge[0]), integer(&edge[1]))
                })
                .collect();
            (
                name.clone(),
                Graph {
                    events,
                    rolls,
                    edges,
                },
            )
        })
        .collect()
}
fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0}'..='\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> BTreeMap<String, Graph> {
        // Two same-domain events separated only by Roll membership.
        parse_graphs(
            "G\ttest\nS\ttest\t0\t1\t2\tRoll\nV\ttest\t0\t0,1,0\t0\t0\t\tnull\nV\ttest\t1\t0,1,0\t0\t1\t0\t0\nE\ttest\t0\t1\n",
        )
    }
    #[test]
    fn all_finite_mutations_fail_closed() {
        validate_graphs(&fixture());
        for mutation in 0..6 {
            let mut graphs = fixture();
            let g = graphs.get_mut("test").unwrap();
            match mutation {
                0 => g.edges.clear(),
                1 => g.events[1].members.clear(),
                2 => g.events[1].proposed = 255,
                3 => g.events[1].id = 99,
                4 => g.events[1].inner = "null".into(),
                5 => g.rolls[0].2 = 1,
                _ => unreachable!(),
            }
            assert!(
                std::panic::catch_unwind(|| validate_graphs(&graphs)).is_err(),
                "mutation {mutation}"
            );
        }
    }
    #[test]
    fn solver_output_is_exact_and_known() {
        assert!(solver_outcomes_match("sat\nunsat\n", &["sat", "unsat"]));
        for bad in [
            "",
            "sat\n",
            "sat\nunsat\nsat\n",
            "unknown\nunsat\n",
            "unsat\nsat\n",
            "sat\n(error bad)\n",
        ] {
            assert!(!solver_outcomes_match(bad, &["sat", "unsat"]));
        }
    }
    #[test]
    fn subprocess_failures_are_fatal() {
        assert!(std::panic::catch_unwind(|| command("false", &[], None)).is_err());
    }
    #[test]
    fn existing_output_is_never_replaced() {
        let path =
            std::env::temp_dir().join(format!("elastic-no-overwrite-{}", std::process::id()));
        fs::write(&path, "preserved").unwrap();
        assert!(
            std::panic::catch_unwind(|| run(&[
                "unused".into(),
                "--output".into(),
                path.to_str().unwrap().into()
            ]))
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "preserved");
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn inventory_mutations_fail_closed() {
        let original = include_str!("../../proofs/elastic-roll-colors/evidence/run-inventory.log");
        validate_inventory(original);
        for bad in [
            original.replace("idx=6 enabled=true", "idx=6 enabled=false"),
            original.replace("label: 52,", "label: 99,"),
        ] {
            assert!(std::panic::catch_unwind(|| validate_inventory(&bad)).is_err());
        }
    }
}
