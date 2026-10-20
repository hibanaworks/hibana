//! Independent finite wire-color models. Runtime Covers/SameClassUnique and
//! well-formed-marker correspondence remain explicit premises, not conclusions.
#[allow(dead_code)]
mod kani_json;
use std::{cmp::Reverse, collections::BTreeSet, env, path::Path, process::Command};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    sender: usize,
    receiver: usize,
    lane: usize,
    color: usize,
}
impl Row {
    fn key(self) -> (usize, usize, usize) {
        (self.sender, self.receiver, self.lane)
    }
    fn color(self, color: usize) -> Self {
        Self { color, ..self }
    }
}
#[derive(Clone, Copy, Debug)]
struct Marker {
    start: usize,
    end: usize,
    ordinal: usize,
    kind: usize,
    enter: bool,
}
fn marker(start: usize, end: usize, ordinal: usize) -> Marker {
    Marker {
        start,
        end,
        ordinal,
        kind: 1,
        enter: true,
    }
}
fn rust_owners(rows: &[Row], markers: &[Marker]) -> Vec<usize> {
    (0..rows.len())
        .map(|event| {
            let mut best = usize::MAX;
            let mut owner = 0;
            for m in markers {
                if m.enter && m.kind == 1 && m.start <= event && event < m.end {
                    let span = m.end - m.start;
                    if span < best || (span == best && m.ordinal + 1 > owner) {
                        best = span;
                        owner = m.ordinal + 1;
                    }
                }
            }
            owner
        })
        .collect()
}
fn list_owners(rows: &[Row], markers: &[Marker]) -> Vec<usize> {
    (0..rows.len())
        .map(|event| {
            markers
                .iter()
                .filter(|m| m.enter && m.kind == 1 && (m.start..m.end).contains(&event))
                .map(|m| (m.end - m.start, Reverse(m.ordinal + 1)))
                .min()
                .map_or(0, |(_, Reverse(owner))| owner)
        })
        .collect()
}
fn rust_functional(rows: &[Row], markers: &[Marker]) -> (Vec<Row>, Option<usize>) {
    if !markers.iter().any(|m| m.enter && m.kind == 1) {
        return (rows.to_vec(), None);
    }
    let original: Vec<_> = rows.iter().map(|r| r.color).collect();
    let owners = rust_owners(rows, markers);
    let mut source = rows.to_vec();
    for event in 0..source.len() {
        let current = source[event];
        if current.sender == current.receiver {
            continue;
        }
        let mut used = [0u8; 32];
        for prior in 0..event {
            let previous = source[prior];
            if previous.key() == current.key()
                && (original[prior] != original[event] || owners[prior] != owners[event])
            {
                let value = previous.color;
                assert!(value < 256);
                used[value >> 3] |= 1 << (value & 7);
            }
        }
        let Some(color) = (0..256).find(|v| used[v >> 3] & (1 << (v & 7)) == 0) else {
            source.truncate(event);
            return (source, Some(event));
        };
        source[event] = current.color(color);
    }
    (source, None)
}
fn list_reference(rows: &[Row], markers: &[Marker]) -> Vec<Row> {
    if !markers.iter().any(|m| m.enter && m.kind == 1) {
        return rows.to_vec();
    }
    let owners = list_owners(rows, markers);
    let mut assigned: Vec<Row> = Vec::new();
    for (j, current) in rows.iter().enumerate() {
        let blocked: Vec<_> = rows[..j]
            .iter()
            .enumerate()
            .filter(|(i, old)| {
                old.key() == current.key() && (old.color, owners[*i]) != (current.color, owners[j])
            })
            .map(|(i, _)| assigned[i].color)
            .collect();
        let selected = (0..256).find(|c| !blocked.contains(c)).unwrap_or(256);
        assigned.push(if current.sender == current.receiver {
            *current
        } else {
            current.color(selected)
        });
    }
    assigned
}
fn compare(rows: &[Row], markers: &[Marker]) -> (Vec<Row>, Option<usize>) {
    assert!(rows.iter().all(|r| r.color < 256));
    assert!(
        markers
            .iter()
            .all(|m| m.ordinal <= 8191 && m.start <= m.end && m.end <= rows.len())
    );
    assert_eq!(rust_owners(rows, markers), list_owners(rows, markers));
    let (actual, failed) = rust_functional(rows, markers);
    let expected = list_reference(rows, markers);
    assert_eq!(failed, expected.iter().position(|r| r.color == 256));
    assert_eq!(actual, expected[..failed.unwrap_or(expected.len())]);
    assert_eq!(expected.len(), rows.len());
    for (before, after) in rows.iter().zip(&expected) {
        assert_eq!(before.key(), after.key());
        if before.sender == before.receiver {
            assert_eq!(before, after);
        }
    }
    if !markers.iter().any(|m| m.kind == 1 && m.enter) {
        assert_eq!(expected, rows);
    } else if failed.is_none() {
        let owners = list_owners(rows, markers);
        for (j, current) in rows.iter().enumerate() {
            if current.sender == current.receiver {
                continue;
            }
            for (i, prior) in rows[..j].iter().enumerate() {
                if prior.key() == current.key()
                    && (prior.color, owners[i]) != (current.color, owners[j])
                {
                    assert_ne!(expected[i].color, expected[j].color);
                }
            }
        }
    }
    (expected, failed)
}
#[derive(Clone, Copy, Debug)]
struct RawMarker {
    offset: usize,
    scope: usize,
    tag: usize,
}
impl RawMarker {
    fn enter(self) -> bool {
        self.scope / 8192 == 1 && self.tag.is_multiple_of(4)
    }
}
fn raw(offset: usize, scope: usize, tag: usize) -> RawMarker {
    RawMarker { offset, scope, tag }
}
fn closing(a: RawMarker, b: RawMarker) -> bool {
    a.scope == b.scope && b.tag % 4 == 2
}
fn raw_markers(markers: &[Marker]) -> Vec<RawMarker> {
    let mut out = Vec::new();
    for m in markers {
        let scope = m.kind * 8192 + m.ordinal;
        if m.enter {
            out.push(raw(m.start, scope, 0));
        }
        out.push(raw(m.end, scope, 2));
    }
    out
}
fn external_owners(rows: &[Row], markers: &[RawMarker]) -> Vec<usize> {
    (0..rows.len())
        .map(|index| {
            let mut best: Option<(usize, usize)> = None;
            for &m in markers {
                if m.enter()
                    && m.offset <= index
                    && let Some(exit) = markers
                        .iter()
                        .find(|&&other| closing(m, other))
                        .filter(|exit| index < exit.offset)
                {
                    let span = exit.offset.saturating_sub(m.offset);
                    let owner = m.scope % 8192 + 1;
                    if best.is_none_or(|(old_span, old_owner)| {
                        span < old_span || (span == old_span && old_owner < owner)
                    }) {
                        best = Some((span, owner));
                    }
                }
            }
            best.map_or(0, |(_, owner)| owner)
        })
        .collect()
}
fn former_owners(rows: &[Row], markers: &[RawMarker]) -> Vec<usize> {
    (0..rows.len())
        .map(|event| {
            markers
                .iter()
                .enumerate()
                .filter_map(|(pos, &m)| {
                    let stop = markers[pos + 1..]
                        .iter()
                        .find(|&&other| closing(m, other))
                        .map_or(rows.len(), |m| m.offset);
                    (m.enter() && m.offset <= event && event < stop)
                        .then(|| (stop - m.offset, Reverse(m.scope % 8192 + 1)))
                })
                .min()
                .map_or(0, |(_, Reverse(owner))| owner)
        })
        .collect()
}
fn well_formed(markers: &[RawMarker]) -> bool {
    markers
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enter())
        .all(|(i, &m)| {
            let matches: Vec<_> = markers
                .iter()
                .enumerate()
                .filter(|(_, other)| closing(m, **other))
                .map(|(j, _)| j)
                .collect();
            matches.len() == 1 && matches[0] > i
        })
}
fn allocate(rows: &[Row], markers: &[RawMarker], owners: &[usize], reverse: bool) -> Vec<Row> {
    if !markers.iter().any(|m| m.enter()) {
        return rows.to_vec();
    }
    let mut prior: Vec<(Row, usize, usize)> = Vec::new();
    let mut result = Vec::new();
    for (&atom, &owner) in rows.iter().zip(owners) {
        let used: Vec<_> = prior
            .iter()
            .filter(|(old, old_owner, _)| {
                old.key() == atom.key() && (old.color != atom.color || *old_owner != owner)
            })
            .map(|(_, _, color)| *color)
            .collect();
        let color = if atom.sender == atom.receiver {
            atom.color
        } else {
            (0..256).find(|c| !used.contains(c)).unwrap_or(256)
        };
        result.push(atom.color(color));
        let entry = (atom, owner, color);
        if reverse {
            prior.insert(0, entry);
        } else {
            prior.push(entry);
        }
    }
    result
}
fn compare_valid(rows: &[Row], markers: &[Marker]) -> (Vec<Row>, Option<usize>) {
    assert!(rows.iter().all(|r| r.color < 256));
    assert!(
        markers
            .iter()
            .all(|m| m.ordinal <= 8191 && m.start <= m.end && m.end <= rows.len())
    );

    let raw = raw_markers(markers);
    assert!(well_formed(&raw));
    let expected = rust_owners(rows, markers);
    let external = external_owners(rows, &raw);
    let old = former_owners(rows, &raw);
    assert_eq!(external, old);
    assert_eq!(external, expected);
    let result = allocate(rows, &raw, &external, false);
    assert_eq!(result, allocate(rows, &raw, &old, true));
    let (expected, failed) = compare(rows, markers);
    assert_eq!(result, expected);
    (result, failed)
}
fn base() -> Row {
    Row {
        sender: 0,
        receiver: 1,
        lane: 0,
        color: 0,
    }
}
fn fixtures() -> Vec<(&'static str, Vec<Row>, Vec<Marker>)> {
    let b = base();
    vec![
        (
            "ordinary_257_no_roll",
            (0..257).map(|i| b.color(i % 256)).collect(),
            vec![],
        ),
        (
            "separate_rolls",
            vec![b; 4],
            (0..4).map(|i| marker(i, i + 1, i)).collect(),
        ),
        (
            "nested_and_coextensive",
            vec![b; 3],
            vec![marker(1, 2, 2), marker(1, 2, 1), marker(0, 3, 0)],
        ),
        (
            "reverse_nested",
            vec![b; 4],
            vec![marker(1, 3, 1), marker(0, 4, 0)],
        ),
        (
            "domains_and_original_labels",
            vec![
                b,
                b.color(1),
                Row { sender: 2, ..b },
                Row { receiver: 2, ..b },
                Row { lane: 1, ..b },
            ],
            vec![marker(0, 5, 0)],
        ),
        (
            "self_sends_preserve_labels",
            vec![
                Row {
                    receiver: 0,
                    color: 255,
                    ..b
                },
                b,
                Row {
                    receiver: 0,
                    color: 19,
                    ..b
                },
                b,
            ],
            vec![marker(0, 2, 0), marker(2, 4, 1)],
        ),
        (
            "half_open_boundary_and_nonroll_markers",
            vec![b; 5],
            vec![
                marker(1, 3, 1),
                Marker {
                    kind: 0,
                    ..marker(0, 5, 2)
                },
                marker(3, 3, 3),
                Marker {
                    enter: false,
                    ..marker(0, 5, 4)
                },
            ],
        ),
        (
            "full_256_palette",
            vec![b; 256],
            (0..256).map(|i| marker(i, i + 1, i)).collect(),
        ),
        (
            "257th_reports_first_failure",
            vec![b; 257],
            (0..257).map(|i| marker(i, i + 1, i)).collect(),
        ),
        (
            "total_reference_continues_after_invalid",
            [
                vec![b; 257],
                vec![Row {
                    sender: 1,
                    receiver: 2,
                    ..b
                }],
            ]
            .concat(),
            (0..258).map(|i| marker(i, i + 1, i)).collect(),
        ),
    ]
}
fn check_fixtures() {
    for (name, rows, markers) in fixtures() {
        let (expected, failed) = compare_valid(&rows, &markers);
        let colors: Vec<_> = expected.iter().map(|r| r.color).collect();
        match name {
            "full_256_palette" => assert_eq!(colors, (0..256).collect::<Vec<_>>()),
            "257th_reports_first_failure" => {
                assert_eq!(failed, Some(256));
                assert_eq!(colors.last(), Some(&256));
            }
            "total_reference_continues_after_invalid" => {
                assert_eq!(failed, Some(256));
                assert_eq!(colors.last(), Some(&0));
            }
            _ => {
                assert!(failed.is_none());
            }
        }
    }
}
fn permutations(markers: &mut [Marker], start: usize, out: &mut Vec<Vec<Marker>>) {
    if start == markers.len() {
        out.push(markers.to_vec());
        return;
    }
    for i in start..markers.len() {
        markers.swap(start, i);
        permutations(markers, start + 1, out);
        markers.swap(start, i);
    }
}
fn exhaustive() -> usize {
    let b = base();
    let variants = [
        b,
        b.color(1),
        b.color(255),
        Row { lane: 1, ..b },
        Row { receiver: 2, ..b },
        Row { sender: 2, ..b },
        Row {
            receiver: 0,
            color: 255,
            ..b
        },
    ];
    let mut count = 0;
    for n in 0..5u32 {
        let n = n as usize;
        let mut layouts = vec![
            vec![],
            vec![marker(0, n, 0)],
            vec![marker(0, n, 0), marker(0, n, 1)],
        ];
        if n >= 2 {
            layouts.push(vec![marker(0, n, 0), marker(1, n, 1)]);
            layouts.push(vec![marker(0, 1, 0), marker(1, n, 1)]);
        }
        if n >= 3 {
            layouts.push(vec![
                marker(0, n, 0),
                marker(1, n - 1, 1),
                marker(1, n - 1, 2),
            ]);
        }
        for mut markers in layouts {
            let mut orders = Vec::new();
            permutations(&mut markers, 0, &mut orders);
            for choice in 0..7usize.pow(n as u32) {
                let mut value = choice;
                let mut rows = vec![b; n];
                for row in rows.iter_mut().rev() {
                    *row = variants[value % 7];
                    value /= 7;
                }
                for order in &orders {
                    compare_valid(&rows, order);
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 38840);
    count
}
fn mutant(rows: &[Row], markers: &[Marker], defect: &str) -> Vec<usize> {
    let owners = list_owners(rows, markers);
    let mut assigned = Vec::new();
    for (j, row) in rows.iter().enumerate() {
        let mut blocked = Vec::new();
        for (i, old) in rows[..j].iter().enumerate() {
            let old_color = if defect == "mutated_baseline" {
                assigned[i]
            } else {
                old.color
            };
            let old_diff = old_color != row.color && defect != "omit_original";
            let owner_diff = owners[i] != owners[j] && defect != "omit_owner";
            if old.key() == row.key() && (old_diff || owner_diff) {
                blocked.push(assigned[i]);
            }
        }
        assigned.push((0..256).find(|c| !blocked.contains(c)).unwrap_or(256));
    }
    assigned
}
fn mutation_controls() {
    let b = base();
    for (rows, markers, defect, correct, bad) in [
        (
            vec![b.color(7); 2],
            vec![marker(0, 2, 0)],
            "mutated_baseline",
            vec![0, 0],
            vec![0, 1],
        ),
        (
            vec![b, b.color(1)],
            vec![marker(0, 2, 0)],
            "omit_original",
            vec![0, 1],
            vec![0, 0],
        ),
        (
            vec![b; 2],
            vec![marker(0, 1, 0), marker(1, 2, 1)],
            "omit_owner",
            vec![0, 1],
            vec![0, 0],
        ),
    ] {
        assert_eq!(
            compare(&rows, &markers)
                .0
                .iter()
                .map(|r| r.color)
                .collect::<Vec<_>>(),
            correct
        );
        assert_eq!(mutant(&rows, &markers, defect), bad);
        assert_ne!(correct, bad);
    }
}
fn raw_controls() {
    let b = base();
    let malformed = [
        ("missing_exit", vec![b; 3], vec![raw(1, 8192, 0)]),
        (
            "duplicate_exit_before_enter",
            vec![b; 3],
            vec![raw(0, 8192, 2), raw(0, 8192, 0), raw(3, 8192, 2)],
        ),
        (
            "only_exit_before_enter",
            vec![b; 3],
            vec![raw(0, 8192, 2), raw(0, 8192, 0)],
        ),
        (
            "orphan_exit_without_enter",
            vec![b.color(7); 2],
            vec![raw(2, 8192, 2)],
        ),
        (
            "duplicate_later_exits_choose_first",
            vec![b; 3],
            vec![raw(0, 8192, 0), raw(1, 8192, 2), raw(3, 8192, 2)],
        ),
    ];
    for (name, rows, markers) in &malformed {
        let ext = external_owners(rows, markers);
        let old = former_owners(rows, markers);
        match *name {
            "orphan_exit_without_enter" => {
                assert_eq!(ext, vec![0, 0]);
                assert_eq!(ext, old);
                assert_eq!(allocate(rows, markers, &ext, false), *rows);
            }
            "duplicate_later_exits_choose_first" => {
                assert!(!well_formed(markers));
                assert_eq!(ext, vec![1, 0, 0]);
                assert_eq!(ext, old);
            }
            _ => {
                assert!(!well_formed(markers));
                assert_ne!(ext, old);
            }
        }
        // Both allocation paths are executed for malformed bridge witnesses too.
        let _ = allocate(rows, markers, &ext, false);
        let _ = allocate(rows, markers, &old, true);
    }
    let m = &malformed[4].2;
    assert_eq!(external_owners(&[b; 3], &[m[0], m[2], m[1]]), vec![1, 1, 1]);
    let filtered = vec![2, 7, 2, 9];
    let reversed: Vec<_> = filtered.iter().copied().rev().collect();
    assert_ne!(filtered, reversed);
    assert_eq!(
        filtered.iter().collect::<BTreeSet<_>>(),
        reversed.iter().collect()
    );
    let fixtures = fixtures();
    let (_, rows, markers) = fixtures
        .iter()
        .find(|f| f.0 == "257th_reports_first_failure")
        .unwrap();
    let (overflow, failed) = compare_valid(rows, markers);
    assert_eq!(failed, Some(256));
    let labels = |role| {
        overflow
            .iter()
            .filter(|r| role == r.sender || role == r.receiver)
            .map(|r| r.color)
            .collect::<Vec<_>>()
    };
    assert!(labels(0).contains(&256) && labels(1).contains(&256));
    assert!(labels(2).is_empty());
    assert!(labels(2).iter().all(|&c| c < 256));
    assert!(!labels(0).iter().all(|&c| c < 256));
}
#[derive(Default)]
struct Graph {
    name: String,
    rows: Vec<Row>,
    markers: Vec<Marker>,
    colors: Vec<usize>,
    owners: Vec<usize>,
}
fn object(value: &kani_json::Json) -> &std::collections::BTreeMap<String, kani_json::Json> {
    match value {
        kani_json::Json::Object(map) => map,
        _ => panic!("expected JSON object"),
    }
}
fn array(value: &kani_json::Json) -> &[kani_json::Json] {
    match value {
        kani_json::Json::Array(values) => values,
        _ => panic!("expected JSON array"),
    }
}
fn integer(value: &kani_json::Json) -> usize {
    match value {
        kani_json::Json::Number(number) => number.parse().expect("nonnegative graph integer"),
        _ => panic!("expected graph integer"),
    }
}
fn fixture_graphs(repo: &Path) -> usize {
    let fixture =
        repo.join("proofs/checks/projection/elastic-roll-colors/fixtures/roll-graphs.json.gz");
    let output = Command::new("gzip")
        .arg("-dc")
        .arg(fixture)
        .output()
        .expect("gzip must be installed");
    assert!(
        output.status.success(),
        "gzip failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = kani_json::parse(std::str::from_utf8(&output.stdout).expect("UTF-8 graph JSON"))
        .expect("valid graph JSON");
    let mut graphs = Vec::new();
    for (name, value) in object(&object(&json)["graphs"]) {
        let fields = object(value);
        let mut graph = Graph {
            name: name.clone(),
            ..Graph::default()
        };
        for event in array(&fields["events"]) {
            let event = object(event);
            let key = array(&event["key"]);
            assert_eq!(key.len(), 3);
            graph.rows.push(Row {
                sender: integer(&key[0]),
                receiver: integer(&key[1]),
                lane: integer(&key[2]),
                color: integer(&event["baseline_color"]),
            });
            graph.colors.push(integer(&event["proposed_color"]));
            graph
                .owners
                .push(if event["innermost_roll"] == kani_json::Json::Null {
                    0
                } else {
                    integer(&event["innermost_roll"])
                        .checked_add(1)
                        .expect("owner ordinal overflow")
                });
        }
        for scope in array(&fields["scopes"]) {
            let scope = object(scope);
            let kind = match &scope["kind"] {
                kani_json::Json::String(kind) => match kind.as_str() {
                    "Roll" => 1,
                    "Route" => 0,
                    "Parallel" | "Par" => 2,
                    _ => panic!("unknown graph scope kind"),
                },
                _ => panic!("invalid scope kind"),
            };
            graph.markers.push(Marker {
                kind,
                ..marker(
                    integer(&scope["start"]),
                    integer(&scope["end"]),
                    integer(&scope["id"]),
                )
            });
        }
        graphs.push(graph);
    }
    assert_eq!(graphs.len(), 11, "graph fixture inventory changed");
    for g in &graphs {
        let (expected, failed) = compare_valid(&g.rows, &g.markers);
        assert!(failed.is_none(), "{}", g.name);
        assert_eq!(
            expected.iter().map(|r| r.color).collect::<Vec<_>>(),
            g.colors,
            "{}",
            g.name
        );
        let owners = list_owners(&g.rows, &g.markers);
        assert_eq!(owners, g.owners, "{}", g.name);
        let memberships: Vec<BTreeSet<_>> = (0..g.rows.len())
            .map(|event| {
                g.markers
                    .iter()
                    .filter(|m| m.kind == 1 && m.start <= event && event < m.end)
                    .map(|m| m.ordinal)
                    .collect()
            })
            .collect();
        for (i, a) in owners.iter().enumerate() {
            for (j, b) in owners.iter().enumerate() {
                assert_eq!(
                    a == b,
                    memberships[i] == memberships[j],
                    "{} {i} {j}",
                    g.name
                );
            }
        }
    }
    graphs.len()
}

fn main() {
    let repo = env::args().nth(1).expect("usage: wire-model REPO");
    check_fixtures();
    let count = exhaustive();
    mutation_controls();
    raw_controls();
    let models = fixture_graphs(Path::new(&repo));
    println!(
        "Wire correspondence: {count} bounded cases; 9 named fixtures; {models} graph fixtures; 3 executed mutants."
    );
    println!(
        "External allocator: {count} bounded cases; 10 named fixtures; {models} graph fixtures; 5 raw-marker controls."
    );
    println!(
        "LIMIT: independent finite transcriptions; runtime coverage and well-formed-marker bridge remain explicit premises."
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_finite_enumeration() {
        assert_eq!(exhaustive(), 38840);
    }
    #[test]
    fn palette_first_failure_and_self_sends() {
        check_fixtures();
    }
    #[test]
    fn executable_mutant_controls() {
        mutation_controls();
        raw_controls();
    }
}
