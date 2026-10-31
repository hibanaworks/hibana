//! Recover nested-roll schedule witnesses from completed public calls.
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Visits = Vec<(usize, usize)>;
type Call = (u8, u8, u8, bool);

#[derive(Deserialize)]
struct Node {
    kind: usize,
    #[serde(default)]
    left: usize,
    #[serde(default)]
    right: usize,
    #[serde(default)]
    sender: u8,
    #[serde(default)]
    receiver: u8,
    #[serde(default)]
    event: u8,
    #[serde(default)]
    owned: bool,
}
#[derive(Deserialize)]
struct Source {
    name: usize,
    nodes: Vec<Node>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    script: usize,
    #[serde(rename = "reversePoll")]
    reverse: bool,
    completed: Vec<Call>,
    probes: Vec<Value>,
}

struct Planned {
    leaf: usize,
    visits: Visits,
}
struct Completed {
    actor: u8,
    leaf: usize,
    visits: Visits,
}

impl Source {
    fn walk(
        &self,
        at: usize,
        script: &Value,
        owner: &mut u8,
        visits: &mut Visits,
        calls: &mut BTreeMap<Call, VecDeque<Planned>>,
    ) -> Result<()> {
        let node = self.nodes.get(at).ok_or("unknown source node")?;
        let fields = script.as_array().ok_or("invalid script")?;
        let field = |at| fields.get(at).ok_or("missing script field");
        let kind = field(0)?.as_u64().ok_or("missing script constructor")?;
        let expected_kind = match node.kind {
            0..=2 => node.kind,
            3 | 4 => 3,
            5 => 4,
            _ => return Err("unknown source constructor".into()),
        };
        if kind != expected_kind as u64 {
            return Err("source and script constructors differ".into());
        }
        match node.kind {
            0 => {
                if field(1)?.as_u64() != Some(u64::from(node.event)) {
                    return Err("source and script labels differ".into());
                }
                let actor = |role| if role == 3 { *owner } else { role };
                let mut record = |role, receive| {
                    calls
                        .entry((actor(role), role, node.event, receive))
                        .or_default()
                        .push_back(Planned {
                            leaf: at,
                            visits: visits.clone(),
                        });
                };
                record(node.sender, false);
                if node.sender != node.receiver {
                    record(node.receiver, true);
                }
                if node.owned {
                    if *owner != node.sender {
                        return Err("source loan is not held by its sender".into());
                    }
                    *owner = node.receiver;
                }
            }
            1 | 2 => {
                self.walk(node.left, field(1)?, owner, visits, calls)?;
                self.walk(node.right, field(2)?, owner, visits, calls)?;
            }
            3 | 4 => {
                let right = field(2)?.as_bool().ok_or("invalid script branch")?;
                self.walk(
                    if right { node.right } else { node.left },
                    field(1)?,
                    owner,
                    visits,
                    calls,
                )?;
            }
            5 => {
                for visit in 0..2 {
                    visits.push((at, visit));
                    self.walk(node.left, field(visit + 1)?, owner, visits, calls)?;
                    visits.pop();
                }
            }
            _ => return Err("unknown source constructor".into()),
        }
        Ok(())
    }

    fn completed(&self, script: &Value, observed: &[Call]) -> Result<Vec<Completed>> {
        let mut owner = self
            .nodes
            .iter()
            .find(|node| node.owned)
            .map_or(3, |node| node.sender);
        let mut planned = BTreeMap::new();
        self.walk(
            self.nodes.len().checked_sub(1).ok_or("empty source")?,
            script,
            &mut owner,
            &mut Vec::new(),
            &mut planned,
        )?;
        let local_messages = self
            .nodes
            .iter()
            .filter(|node| node.kind == 0 && node.sender == node.receiver && !node.owned)
            .map(|node| (node.sender, node.event))
            .collect::<BTreeSet<_>>();
        let mut completed = Vec::new();
        for (position, call) in observed.iter().enumerate() {
            let mut key = *call;
            // send and a preview's recv consume the same self-role operation.
            // There is still only one planned occurrence to remove.
            if local_messages.contains(&(key.1, key.2)) {
                key.3 = false;
            }
            let row = planned
                .get_mut(&key)
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| {
                    format!(
                        "case {} call {position}: no source occurrence for {call:?}",
                        self.name
                    )
                })?;
            completed.push(Completed {
                actor: call.0,
                leaf: row.leaf,
                visits: row.visits,
            });
        }
        if planned.values().any(|rows| !rows.is_empty()) {
            return Err("incomplete source calls in roll schedule coverage".into());
        }
        Ok(completed)
    }

    fn leaves(&self, at: usize) -> BTreeSet<usize> {
        let node = &self.nodes[at];
        match node.kind {
            0 => BTreeSet::from([at]),
            5 => self.leaves(node.left),
            _ => self
                .leaves(node.left)
                .union(&self.leaves(node.right))
                .copied()
                .collect(),
        }
    }

    fn pairs(&self) -> Vec<(usize, usize, usize)> {
        fn walk(
            source: &Source,
            at: usize,
            rolls: &mut Vec<usize>,
            parallels: &mut Vec<(usize, usize)>,
            out: &mut Vec<(usize, usize, usize)>,
        ) {
            let node = &source.nodes[at];
            match node.kind {
                0 => (),
                5 => {
                    for &(parent, sibling) in parallels.iter() {
                        out.push((parent, at, sibling));
                    }
                    rolls.push(at);
                    walk(source, node.left, rolls, parallels, out);
                    rolls.pop();
                }
                2 => {
                    for (part, sibling) in [(node.left, node.right), (node.right, node.left)] {
                        let old = parallels.len();
                        parallels.extend(rolls.iter().map(|&parent| (parent, sibling)));
                        walk(source, part, rolls, parallels, out);
                        parallels.truncate(old);
                    }
                }
                _ => {
                    walk(source, node.left, rolls, parallels, out);
                    walk(source, node.right, rolls, parallels, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(
            self,
            self.nodes.len() - 1,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut out,
        );
        out
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    first: usize,
    last: usize,
}
impl Bounds {
    fn add(slot: &mut Option<Self>, position: usize) {
        if let Some(bounds) = slot {
            bounds.last = position;
        } else {
            *slot = Some(Self {
                first: position,
                last: position,
            });
        }
    }
}

struct Witness {
    actor: u8,
    parent: usize,
    child: usize,
    sibling: usize,
    enclosing: Visits,
    positions: [usize; 4],
}
fn witnesses(source: &Source, calls: &[Completed]) -> Vec<Witness> {
    let mut found = Vec::new();
    for (parent, child, sibling) in source.pairs() {
        let sibling_leaves = source.leaves(sibling);
        let mut ranges = BTreeMap::<(u8, Visits), [[Option<Bounds>; 2]; 2]>::new();
        for (position, call) in calls.iter().enumerate() {
            let Some(parent_at) = call.visits.iter().position(|&(id, _)| id == parent) else {
                continue;
            };
            let side = if call.visits.iter().any(|&(id, _)| id == child) {
                0
            } else if sibling_leaves.contains(&call.leaf) {
                1
            } else {
                continue;
            };
            let visit = call.visits[parent_at].1;
            let key = (call.actor, call.visits[..parent_at].to_vec());
            Bounds::add(
                &mut ranges.entry(key).or_insert([[None; 2]; 2])[side][visit],
                position,
            );
        }
        for ((actor, enclosing), ranges) in ranges {
            let [
                [Some(old_child), Some(new_child)],
                [Some(old_sibling), Some(new_sibling)],
            ] = ranges
            else {
                continue;
            };
            if old_child.last < new_child.first && new_child.first < new_sibling.first {
                found.push(Witness {
                    actor,
                    parent,
                    child,
                    sibling,
                    enclosing,
                    positions: [
                        old_child.last,
                        new_child.first,
                        old_sibling.last,
                        new_sibling.first,
                    ],
                });
            }
        }
    }
    found
}

pub(super) fn audit(output: &Path) -> Result<()> {
    let sources: Vec<Source> = serde_json::from_slice(&fs::read(output.join("inputs.json"))?)?;
    let mut report =
        std::io::BufWriter::new(fs::File::create(output.join("roll-schedule-coverage.tsv"))?);
    writeln!(
        report,
        "case\tscript\treverse_poll\tactor\tparent_node\tchild_node\tsibling_node\tenclosing_visits\told_child_end\tnew_child_start\told_sibling_end\tnew_sibling_start"
    )?;
    let mut seen = BTreeSet::new();
    let mut runs = 0;
    for source in sources {
        let name = format!("{}.jsonl", source.name);
        let mut records = BufReader::new(fs::File::open(
            output.join("scope-observations").join(&name),
        )?)
        .lines();
        let header: Value =
            serde_json::from_str(&records.next().ok_or("missing observation header")??)?;
        let count = header["scripts"].as_u64().ok_or("missing script count")? as usize;
        let mut scripts =
            BufReader::new(fs::File::open(output.join("scripts").join(&name))?).lines();
        for index in 0..count {
            let script: Value =
                serde_json::from_str(&scripts.next().ok_or("missing source script")??)?;
            for reverse in [false, true] {
                let record: Observation =
                    serde_json::from_str(&records.next().ok_or("missing completed script")??)?;
                if record.script != index || record.reverse != reverse {
                    return Err("wrong completed script coordinates".into());
                }
                let _ = record.probes;
                let calls = source.completed(&script, &record.completed)?;
                for witness in witnesses(&source, &calls) {
                    let overlapping = witness.positions[1] < witness.positions[2];
                    if seen.insert((
                        source.name,
                        witness.actor,
                        witness.parent,
                        witness.child,
                        witness.sibling,
                        reverse,
                        overlapping,
                    )) {
                        writeln!(
                            report,
                            "{}\t{index}\t{reverse}\t{}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}",
                            source.name,
                            witness.actor,
                            witness.parent,
                            witness.child,
                            witness.sibling,
                            witness.enclosing,
                            witness.positions[0],
                            witness.positions[1],
                            witness.positions[2],
                            witness.positions[3]
                        )?;
                    }
                }
                runs += 1;
            }
        }
        if records.next().is_some() || scripts.next().is_some() {
            return Err("extra completed script".into());
        }
    }
    report.flush()?;
    println!(
        "Parallel child before a sibling's next parent visit: runs={runs} witnessed_orders={}",
        seen.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_local_preview_receive_consumes_exactly_one_source_occurrence() {
        let mut source = Source {
            name: 0,
            nodes: vec![Node {
                kind: 0,
                left: 0,
                right: 0,
                sender: 0,
                receiver: 0,
                event: 1,
                owned: false,
            }],
        };
        let script = serde_json::json!([0, 1, 0]);
        assert_eq!(
            source
                .completed(&script, &[(0, 0, 1, false)])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            source.completed(&script, &[(0, 0, 1, true)]).unwrap().len(),
            1
        );
        for invalid in [
            vec![(0, 0, 1, false), (0, 0, 1, true)],
            vec![(1, 0, 1, true)],
            vec![(0, 1, 1, true)],
            vec![],
        ] {
            assert!(source.completed(&script, &invalid).is_err());
        }
        source.nodes[0].receiver = 1;
        assert_eq!(
            source
                .completed(&script, &[(0, 0, 1, false), (1, 1, 1, true)])
                .unwrap()
                .len(),
            2
        );
        assert!(
            source
                .completed(&script, &[(0, 0, 1, true), (1, 1, 1, true)])
                .is_err()
        );
    }

    #[test]
    fn syntax_alone_and_a_late_child_do_not_count_as_an_early_child() {
        let source: Source = serde_json::from_str(r#"{"name":0,"nodes":[{"kind":0,"sender":0,"receiver":1,"event":1},{"kind":5,"left":0},{"kind":0,"sender":0,"receiver":1,"event":2},{"kind":2,"left":1,"right":2},{"kind":5,"left":3}]}"#).unwrap();
        assert_eq!(source.pairs(), vec![(4, 1, 2)]);
        let row = |leaf, parent, child| Completed {
            actor: 0,
            leaf,
            visits: if leaf == 0 {
                vec![(4, parent), (1, child)]
            } else {
                vec![(4, parent)]
            },
        };
        let early = vec![
            row(0, 0, 0),
            row(2, 0, 0),
            row(0, 0, 1),
            row(0, 1, 0),
            row(2, 1, 0),
            row(0, 1, 1),
        ];
        assert_eq!(witnesses(&source, &early).len(), 1);
        let late = vec![
            row(0, 0, 0),
            row(2, 0, 0),
            row(0, 0, 1),
            row(2, 1, 0),
            row(0, 1, 0),
            row(0, 1, 1),
        ];
        assert!(witnesses(&source, &late).is_empty());
        assert!(witnesses(&source, &early[..3]).is_empty());
        assert!(witnesses(&source, &[]).is_empty());
    }
}
