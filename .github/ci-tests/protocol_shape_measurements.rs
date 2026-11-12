//! Assemble every recorded interval without reconstructing Endpoint progress.
use super::*;

fn next(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Result<(String, Value)> {
    let line = lines.next().ok_or("incomplete measurement stream")??;
    let value = serde_json::from_str(&line)?;
    Ok((line, value))
}

fn coordinates(value: &Value, script: usize, reverse: bool) -> Result<()> {
    if number(value, "script")? != script || value["reversePoll"] != reverse {
        return Err("missing, duplicated or reordered measurement interval".into());
    }
    Ok(())
}

pub(super) fn assemble(
    output: &Path,
    case: &Case,
    parts: &[(core::ops::Range<usize>, PathBuf)],
) -> Result<()> {
    let state_directory = output.join("operation-states");
    fs::create_dir_all(&state_directory)?;
    let mut state_writer = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(state_directory.join(format!("{}.jsonl", case.name)))?,
    );
    let mut state_ids = BTreeMap::new();
    for kind in ["captures", "admission-observations"] {
        let directory = output.join(kind);
        fs::create_dir_all(&directory)?;
        let mut writer = BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{}.jsonl", case.name)))?,
        );
        let mut common = None;
        let mut completed = 0;
        for (range, part) in parts {
            let mut remapping = Vec::new();
            let mut used = BTreeSet::new();
            if kind == "admission-observations" {
                for mut state in state_data::read(
                    &part
                        .join("operation-states")
                        .join(format!("{}.jsonl", case.name)),
                )? {
                    let key = state.key()?;
                    let id = if let Some(&id) = state_ids.get(&key) {
                        id
                    } else {
                        let id = state_ids.len();
                        state.id = id;
                        serde_json::to_writer(&mut state_writer, &state)?;
                        writeln!(state_writer)?;
                        state_ids.insert(key, id);
                        id
                    };
                    remapping.push(id);
                }
            }
            let mut lines = BufReader::new(fs::File::open(
                part.join(kind).join(format!("{}.jsonl", case.name)),
            )?)
            .lines();
            let (_, mut header) = next(&mut lines)?;
            if number(&header, "name")? != case.name
                || number(&header, "scripts")? != case.scripts
                || number(&header, "first")? != range.start
                || number(&header, "count")? != range.len()
                || range.start != completed
            {
                return Err("measurement header differs from its assigned source interval".into());
            }
            header["first"] = Value::from(0);
            header["count"] = Value::from(case.scripts);
            if let Some(previous) = &common {
                if previous != &header {
                    return Err("measurement contract changed between partitions".into());
                }
            } else {
                writeln!(writer, "{header}")?;
                common = Some(header);
            }
            for script in range.clone() {
                for reverse in [false, true] {
                    let (line, value) = next(&mut lines)?;
                    coordinates(&value, script, reverse)?;
                    writeln!(writer, "{line}")?;
                    if kind == "admission-observations" {
                        if value["kind"] != "inventory" {
                            return Err("missing scoped-query inventory".into());
                        }
                        for _ in 0..number(&value, "queries")? {
                            let (_, mut value) = next(&mut lines)?;
                            coordinates(&value, script, reverse)?;
                            if !value["probe"].is_object() {
                                return Err("query is missing its actual target observation".into());
                            }
                            let original = number(&value, "state")?;
                            let id = remapping
                                .get(original)
                                .ok_or("query references a missing operation state")?;
                            used.insert(original);
                            value["state"] = Value::from(*id);
                            writeln!(writer, "{value}")?;
                        }
                    } else if !value["events"].is_array()
                        || !value["checkpoints"].is_array()
                        || !value["borrows"].is_array()
                    {
                        return Err("incomplete runtime capture".into());
                    }
                }
            }
            if lines.next().is_some() || used.len() != remapping.len() {
                return Err("unexpected measurement suffix or unreferenced operation state".into());
            }
            completed = range.end;
        }
        if completed != case.scripts {
            return Err("missing measurement partitions".into());
        }
        writer.flush()?;
    }
    state_writer.flush()?;
    let images = output.join("operation-images");
    fs::create_dir_all(&images)?;
    let mut inventory = None;
    for (_, part) in parts {
        let projected = output
            .join("captures")
            .join(format!("{}.projection.json", case.name));
        let source = part
            .join("captures")
            .join(format!("{}.projection.json", case.name));
        copy_same(&source, &projected)?;
        let mut names = BTreeSet::new();
        for entry in fs::read_dir(part.join("operation-images"))? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err("unexpected operation image entry".into());
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or("non-Unicode resident image name")?;
            let suffix = name
                .strip_prefix(&format!("{}-", case.name))
                .and_then(|name| name.strip_suffix(".json"))
                .ok_or("resident image belongs to a different source case")?;
            if !matches!(suffix, "0" | "1" | "2" | "3") || !names.insert(name.to_owned()) {
                return Err("duplicate or unknown resident actor image".into());
            }
            copy_same(&entry.path(), &images.join(entry.file_name()))?;
        }
        if names.is_empty() {
            return Err("missing actual resident images".into());
        }
        if inventory
            .as_ref()
            .is_some_and(|previous| previous != &names)
        {
            return Err("resident image inventory changed between partitions".into());
        }
        inventory = Some(names);
    }
    Ok(())
}

fn copy_same(source: &Path, destination: &Path) -> Result<()> {
    if destination.try_exists()? {
        if !same_bytes(source, destination)? {
            return Err("immutable projected data changed between partitions".into());
        }
    } else {
        fs::copy(source, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> (PathBuf, Case, Vec<(core::ops::Range<usize>, PathBuf)>) {
        let root = env::temp_dir().join(format!(
            "hibana-measurements-{}-{label}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let case = Case {
            name: 7,
            scripts: 2,
        };
        let mut parts = Vec::new();
        for script in 0..2 {
            let directory = root.join(format!("part-{script}"));
            fs::create_dir_all(directory.join("captures")).unwrap();
            fs::create_dir_all(directory.join("admission-observations")).unwrap();
            fs::create_dir_all(directory.join("operation-images")).unwrap();
            fs::create_dir_all(directory.join("operation-states")).unwrap();
            let header = serde_json::json!({"name":7,"scripts":2,"first":script,"count":1});
            let mut captures = format!("{header}\n");
            let mut queries = format!("{header}\n");
            for reverse in [false, true] {
                captures += &format!(
                    "{}\n",
                    serde_json::json!({"script":script,"reversePoll":reverse,"events":[[0]],"checkpoints":[1],"borrows":[]})
                );
                queries += &format!(
                    "{}\n{}\n",
                    serde_json::json!({"kind":"inventory","script":script,"reversePoll":reverse,"queries":1}),
                    serde_json::json!({"script":script,"reversePoll":reverse,"probe":{"example":script},"state":0})
                );
            }
            fs::write(directory.join("captures/7.jsonl"), captures).unwrap();
            fs::write(directory.join("admission-observations/7.jsonl"), queries).unwrap();
            fs::write(
                directory.join("captures/7.projection.json"),
                "actual immutable projection",
            )
            .unwrap();
            fs::write(
                directory.join("operation-images/7-0.json"),
                "actual resident image",
            )
            .unwrap();
            fs::write(directory.join("operation-states/7.jsonl"), format!("{}\n",
                serde_json::json!({"id":0,"cut":{"target":script},"progress":{"done":true},"borrowReentry":null,"retainedRolls":[],"creditedRolls":[]}))).unwrap();
            parts.push((script..script + 1, directory));
        }
        (root, case, parts)
    }

    #[test]
    fn all_script_intervals_and_both_poll_orders_are_retained() {
        let (root, case, parts) = fixture("complete");
        assemble(&root, &case, &parts).unwrap();
        let captures = fs::read_to_string(root.join("captures/7.jsonl")).unwrap();
        let rows: Vec<Value> = captures
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            rows[0],
            serde_json::json!({"name":7,"scripts":2,"first":0,"count":2})
        );
        assert_eq!(rows.len(), 5);
        for (index, row) in rows[1..].iter().enumerate() {
            coordinates(row, index / 2, index % 2 == 1).unwrap();
        }
        assert_eq!(
            fs::read_to_string(root.join("admission-observations/7.jsonl"))
                .unwrap()
                .lines()
                .count(),
            9
        );
        let states = state_data::read(&root.join("operation-states/7.jsonl")).unwrap();
        assert_eq!(states.len(), 2);
        let queries = fs::read_to_string(root.join("admission-observations/7.jsonl")).unwrap();
        for row in queries
            .lines()
            .skip(1)
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
        {
            if row["probe"].is_object() {
                assert_eq!(row["state"], row["script"]);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_query_cannot_be_hidden_by_a_complete_capture() {
        let (root, case, parts) = fixture("missing-query");
        let file = parts[1].1.join("admission-observations/7.jsonl");
        let text = fs::read_to_string(&file).unwrap();
        fs::write(
            &file,
            text.lines().take(4).collect::<Vec<_>>().join("\n") + "\n",
        )
        .unwrap();
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_intervals_and_changed_projection_data_are_rejected() {
        let (root, case, mut parts) = fixture("duplicate-interval");
        parts[1].0 = 0..1;
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
        let (root, case, parts) = fixture("changed-image");
        fs::write(
            parts[1].1.join("operation-images/7-0.json"),
            "different image",
        )
        .unwrap();
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_later_partition_cannot_omit_a_resident_actor_image() {
        let (root, case, parts) = fixture("missing-image");
        fs::write(
            parts[0].1.join("operation-images/7-1.json"),
            "second actual resident image",
        )
        .unwrap();
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_query_requires_the_passive_actor_state_from_the_same_cut() {
        let (root, case, parts) = fixture("missing-progress");
        let file = parts[1].1.join("admission-observations/7.jsonl");
        let text = fs::read_to_string(&file).unwrap();
        let mut rows: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        rows[2].as_object_mut().unwrap().remove("state");
        fs::write(
            file,
            rows.into_iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn equal_state_values_merge_without_removing_any_query() {
        let (root, case, parts) = fixture("equal-states");
        let state = fs::read(parts[0].1.join("operation-states/7.jsonl")).unwrap();
        fs::write(parts[1].1.join("operation-states/7.jsonl"), state).unwrap();
        assemble(&root, &case, &parts).unwrap();
        assert_eq!(
            state_data::read(&root.join("operation-states/7.jsonl"))
                .unwrap()
                .len(),
            1
        );
        let rows = fs::read_to_string(root.join("admission-observations/7.jsonl")).unwrap();
        let queries: Vec<Value> = rows
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|row| row["probe"].is_object())
            .collect();
        assert_eq!(queries.len(), 4);
        assert!(queries.iter().all(|row| row["state"] == 0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unreferenced_state_cannot_hide_in_the_evidence_inventory() {
        let (root, case, parts) = fixture("unused-state");
        let file = parts[1].1.join("operation-states/7.jsonl");
        let mut text = fs::read_to_string(&file).unwrap();
        text += &format!(
            "{}\n",
            serde_json::json!({"id":1,"cut":{"target":99},"progress":{"done":false},"borrowReentry":null,"retainedRolls":[],"creditedRolls":[]})
        );
        fs::write(file, text).unwrap();
        assert!(assemble(&root, &case, &parts).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
