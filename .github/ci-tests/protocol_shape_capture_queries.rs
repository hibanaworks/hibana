//! Preserve the exact measured public-call cuts while checking their evidence.
//! Completion eligibility comes from the source-cut checker and the actual
//! fixture FIFO. Positive histories additionally need an EPF source witness;
//! a failed witness search is never interpreted as proof of an illegal call.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct QueryHeader {
    name: usize,
    scripts: usize,
    first: usize,
    count: usize,
    source_conformance_checked: bool,
    resolver_inputs: Vec<Vec<u16>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Inventory {
    kind: String,
    script: usize,
    reverse_poll: bool,
    queries: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Measurement {
    script: usize,
    reverse_poll: bool,
    probe: serde_json::Value,
    prefix_records: usize,
    prefix_borrows: usize,
    prefix_calls: usize,
    outcome: String,
    error_utf8: Vec<u8>,
    checkpoints: [u32; 4],
    suffix: Vec<[u64; 6]>,
    state: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAnswers {
    source: usize,
    answers: Vec<(usize, Vec<usize>)>,
    readiness: Vec<(usize, bool)>,
}

impl SourceAnswers {
    fn ready(&self, query: usize) -> Result<bool> {
        let (id, candidates) = self
            .answers
            .get(query)
            .ok_or("missing source query answer")?;
        let (ready_id, ready) = self
            .readiness
            .get(query)
            .ok_or("missing source completion answer")?;
        if *id != query
            || *ready_id != query
            || candidates.iter().collect::<BTreeSet<_>>().len() != candidates.len()
            || (*ready && candidates.is_empty())
        {
            return Err("missing, duplicate or reordered source query answer".into());
        }
        Ok(*ready)
    }
}

fn source_answers(
    checker: &CaptureCheck<'_>,
    input: &Path,
    header: &QueryHeader,
) -> Result<SourceAnswers> {
    let root = input
        .parent()
        .and_then(Path::parent)
        .ok_or("query capture has no observation root")?;
    let mut lines = BufReader::new(File::open(input)?).lines();
    let _: QueryHeader = read(&mut lines)?;
    let mut questions = Vec::new();
    for line in lines {
        let value: serde_json::Value = serde_json::from_str(&line?)?;
        if value["kind"] == "inventory" {
            let _: Inventory = serde_json::from_value(value)?;
            continue;
        }
        let sample: Measurement = serde_json::from_value(value)?;
        let choice = sample.probe["choice"]
            .as_u64()
            .and_then(|choice| usize::try_from(choice).ok())
            .and_then(|choice| header.resolver_inputs.get(choice))
            .ok_or("source query has an unplanned resolver input")?;
        let baseline = checker
            .baselines
            .iter()
            .find(|baseline| {
                baseline.script == sample.script && baseline.reverse_poll == sample.reverse_poll
            })
            .ok_or("source query has no actual baseline capture")?;
        let commits = baseline
            .events
            .get(..sample.prefix_records)
            .ok_or("source query prefix exceeds its actual capture")?
            .iter()
            .filter(|record| matches!(record[2], 0x0202 | 0x0203) && record[4] == 2)
            .map(|record| [record[2], record[3], record[4], record[5]])
            .collect::<Vec<_>>();
        questions.push(serde_json::json!({
            "id":questions.len(),"state":sample.state,"origin":sample.probe["label"],
            "receive":sample.probe["receive"],"input":choice,"session":2,"commits":commits,
        }));
    }
    if questions.is_empty() {
        return Err("empty source query inventory".into());
    }
    let questions_path = checker.output.join("source-questions.json");
    serde_json::to_writer(create(&questions_path)?, &questions)?;
    let mut images = Vec::<serde_json::Value>::new();
    let prefix = format!("{}-", header.name);
    for entry in fs::read_dir(root.join("operation-images"))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("non-Unicode operation image")?;
        if name.starts_with(&prefix) && name.ends_with(".json") {
            images.push(serde_json::from_reader(File::open(entry.path())?)?);
        }
    }
    let images_path = checker.output.join("resident-images.json");
    serde_json::to_writer(create(&images_path)?, &images)?;
    let answers_path = checker.output.join("source-answers.json");
    let catalog = checker
        .plan_input
        .parent()
        .and_then(Path::parent)
        .ok_or("source plans have no catalog root")?
        .join("catalog.json");
    let status = Command::new(checker.source_checker)
        .arg("--public-cuts")
        .arg(header.name.to_string())
        .arg(catalog)
        .arg(images_path)
        .arg(
            root.join("operation-states")
                .join(format!("{}.jsonl", header.name)),
        )
        .arg(questions_path)
        .arg(&answers_path)
        .status()?;
    if !status.success() {
        return Err(format!("source-cut checker failed: {status}").into());
    }
    let answers: SourceAnswers = serde_json::from_reader(File::open(answers_path)?)?;
    if answers.source != header.name
        || answers.answers.len() != questions.len()
        || answers.readiness.len() != questions.len()
    {
        return Err("source answers do not cover the actual query inventory".into());
    }
    Ok(answers)
}

fn read<T: serde::de::DeserializeOwned>(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> Result<T> {
    Ok(serde_json::from_str(
        &lines.next().ok_or("missing scoped-query record")??,
    )?)
}

fn prefix(baseline: &CapturedRun, measurement: &Measurement, calls: usize) -> Result<CapturedRun> {
    if measurement.prefix_borrows > baseline.borrows.len() || measurement.prefix_calls > calls {
        return Err("query cut extends beyond the completed baseline".into());
    }
    let mut events = baseline
        .events
        .get(..measurement.prefix_records)
        .ok_or("query capture prefix exceeds its baseline")?
        .to_vec();
    let completed = events
        .iter()
        .filter(|row| matches!(row[2], 0x0202..=0x0204) && row[4] == 2)
        .count();
    if completed != measurement.prefix_calls {
        return Err("query completed-call cut disagrees with the actual committed prefix".into());
    }
    events.extend_from_slice(&measurement.suffix);
    let mut ordinals = [0u32; 4];
    for &[producer, ordinal, ..] in &events {
        let next = ordinals
            .get_mut(usize::try_from(producer)?)
            .ok_or("query capture names an unknown producer")?;
        if ordinal != u64::from(*next) {
            return Err("query capture contains a gap, duplicate or reordered ordinal".into());
        }
        *next = next
            .checked_add(1)
            .ok_or("query capture counter overflow")?;
    }
    if ordinals != measurement.checkpoints {
        return Err("query capture is missing its producer checkpoint suffix".into());
    }
    if !matches!(
        measurement.outcome.as_str(),
        "completed" | "pending" | "rejected"
    ) || measurement.error_utf8.is_empty() != (measurement.outcome != "rejected")
        || std::str::from_utf8(&measurement.error_utf8).is_err()
    {
        return Err("query outcome and error evidence disagree".into());
    }
    Ok(CapturedRun {
        script: measurement.script,
        reverse_poll: measurement.reverse_poll,
        checkpoints: measurement.checkpoints,
        events,
        borrows: baseline.borrows[..measurement.prefix_borrows].to_vec(),
    })
}

fn matches_call(snapshot: &[[u64; 5]], measurement: &Measurement, input: &[u16]) -> Result<()> {
    let number = |field: &str| {
        measurement.probe[field]
            .as_u64()
            .ok_or("invalid query call field")
    };
    let receive = measurement.probe["receive"]
        .as_bool()
        .ok_or("invalid query direction")?;
    let view = number("role")?;
    let actor = number("actor")?;
    let target = number("label")?;
    let source = |event| {
        snapshot
            .iter()
            .find(|row| row[1] == 0x0312 && row[0] == event)
            .ok_or("query call names a missing compiled source event")
    };
    let requested = source(target)?;
    let shift = if receive { 16 } else { 24 };
    if (requested[3] >> shift) & 255 != view {
        return Err("query selector does not belong to the requested logical view".into());
    }
    for row in measurement
        .suffix
        .iter()
        .filter(|row| row[2] == 0x0407 && row[4] == 2)
    {
        let resolver = u16::try_from(row[5] & 65535)?;
        let scope = row[5] >> 16;
        if row[3] >> 8 != actor
            || row[3] & 255 != u64::from(input.contains(&resolver))
            || !snapshot.iter().any(|authority| {
                authority[1] == 0x0314
                    && authority[2] == scope
                    && authority[3] == view
                    && authority[4] == u64::from(resolver) + 1
            })
        {
            return Err(
                "query resolver observation disagrees with its actual callback input or authority"
                    .into(),
            );
        }
    }
    let committed: Vec<_> = measurement
        .suffix
        .iter()
        .filter(|row| matches!(row[2], 0x0202..=0x0204) && row[4] == 2)
        .collect();
    if measurement.outcome != "completed" {
        if !committed.is_empty() {
            return Err("a pending or rejected query published a completed operation".into());
        }
        return Ok(());
    }
    let [record] = committed.as_slice() else {
        return Err("a completed query needs exactly one actual operation commit".into());
    };
    let actual = source(record[5] & 65535)?;
    if record[3] >> 8 != actor
        || record[3] & 255 != u64::from(receive) + 1
        || record[5] >> 24 != view
        || (actual[3] >> shift) & 255 != view
        || actual[3] & 255 != requested[3] & 255
        || actual[2] >> 8 != requested[2] >> 8
        || actual[4] != requested[4]
    {
        return Err("completed query does not match its typed public message contract".into());
    }
    Ok(())
}

pub(super) fn check(checker: &CaptureCheck<'_>, input: &Path) -> Result<serde_json::Value> {
    let CaptureCheck {
        output,
        plan_input,
        header: expected,
        baselines,
        plans,
        ..
    } = *checker;
    let mut lines = BufReader::new(File::open(input)?).lines();
    let header: QueryHeader = read(&mut lines)?;
    if (header.name, header.scripts, header.first, header.count)
        != (
            expected.name,
            expected.scripts,
            expected.first,
            expected.count,
        )
        || header.source_conformance_checked
        || header.resolver_inputs.is_empty()
    {
        return Err("query inventory does not match the actual capture interval".into());
    }
    let states = state_data::read(
        &input
            .parent()
            .and_then(Path::parent)
            .ok_or("query observation has no population root")?
            .join("operation-states")
            .join(format!("{}.jsonl", header.name)),
    )?;
    let mut used_states = BTreeSet::new();
    let scope_input = plan_input
        .parent()
        .and_then(Path::parent)
        .ok_or("source plans have no source inventory root")?
        .join("scope-plans")
        .join(format!("{}.jsonl", header.name));
    let mut scope_lines = BufReader::new(File::open(scope_input)?).lines();
    let source_header: serde_json::Value = read(&mut scope_lines)?;
    if source_header != serde_json::json!({"name":header.name,"scripts":header.scripts}) {
        return Err("query source inventory differs from the compiled case".into());
    }
    let mut planned = Vec::new();
    for script in 0..header.scripts {
        let record: serde_json::Value = read(&mut scope_lines)?;
        if record["script"] != script || record.as_object().map(|row| row.len()) != Some(2) {
            return Err("missing, duplicate or reordered query source plan".into());
        }
        let probes = record["probes"]
            .as_array()
            .ok_or("missing query source probes")?;
        let source_probes: Vec<_> = probes
            .iter()
            .filter(|probe| probe["kind"] == "admission")
            .map(serde_json::to_string)
            .collect::<std::result::Result<_, _>>()?;
        let unique: BTreeSet<_> = source_probes.iter().cloned().collect();
        if unique.len() != source_probes.len() {
            return Err("duplicate scoped-query source coordinate".into());
        }
        planned.push(unique);
    }
    if scope_lines.next().is_some() {
        return Err("unexpected query source-plan suffix".into());
    }
    let candidates: Vec<SourcePlan> = if checker.epf.is_some() {
        BufReader::new(File::open(
            plan_input.with_file_name(format!("{}.query-witness.jsonl", header.name)),
        )?)
        .lines()
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect::<Result<_>>()?
    } else {
        Vec::new()
    };
    if (checker.epf.is_some() && candidates.is_empty())
        || candidates
            .iter()
            .enumerate()
            .any(|(id, plan)| id != plan.script)
    {
        return Err("empty, missing, duplicate or reordered query candidate family".into());
    }
    let answers = source_answers(checker, input, &header)?;
    let mut query_index = 0;
    let mut completed = 0usize;
    let mut pending = 0usize;
    let mut rejected = 0usize;
    let mut unresolved = 0usize;
    let mut epf_checked = 0usize;
    for baseline in baselines {
        let inventory: Inventory = read(&mut lines)?;
        let mut missing = planned[baseline.script].clone();
        if inventory.kind != "inventory"
            || inventory.script != baseline.script
            || inventory.reverse_poll != baseline.reverse_poll
            || inventory.queries != missing.len()
        {
            return Err("query matrix does not match its source plan and poll order".into());
        }
        for query in 0..inventory.queries {
            let measurement: Measurement = read(&mut lines)?;
            let state = states
                .get(measurement.state)
                .ok_or("query names a missing operation state")?;
            if state.progress["actor"] != measurement.probe["actor"]
                || state.cut["target"]["view"] != measurement.probe["role"]
                || !state.progress["fault"].is_null()
            {
                return Err(
                    "query operation state belongs to a different or faulted target".into(),
                );
            }
            used_states.insert(measurement.state);
            if measurement.script != baseline.script
                || measurement.reverse_poll != baseline.reverse_poll
                || !missing.remove(&serde_json::to_string(&measurement.probe)?)
                || measurement.probe["choice"]
                    .as_u64()
                    .and_then(|id| usize::try_from(id).ok())
                    .and_then(|id| header.resolver_inputs.get(id))
                    .is_none()
            {
                return Err("missing, duplicate, foreign or unplanned scoped query".into());
            }
            if !matches!(
                measurement.outcome.as_str(),
                "completed" | "pending" | "rejected"
            ) {
                return Err("unknown scoped-query outcome".into());
            }
            let run = prefix(baseline, &measurement, plans[baseline.script].calls.len())?;
            let input =
                &header.resolver_inputs[measurement.probe["choice"].as_u64().unwrap() as usize];
            matches_call(checker.snapshot, &measurement, input)?;
            let ready = answers.ready(query_index)?;
            query_index += 1;
            if ready != (measurement.outcome == "completed") {
                return Err(format!(
                    "source/runtime completion mismatch: source={} script={} reverse={} probe={} state={} source_ready={ready} outcome={} error={}",
                    header.name, measurement.script, measurement.reverse_poll, measurement.probe,
                    measurement.state, measurement.outcome,
                    String::from_utf8_lossy(&measurement.error_utf8),
                ).into());
            }
            if measurement.outcome == "pending" {
                pending += 1;
                continue;
            }
            if measurement.outcome == "rejected" {
                rejected += 1;
                continue;
            }
            completed += 1;
            if checker.epf.is_none() {
                continue;
            }
            let mut errors = Vec::new();
            let mut checked = false;
            for plan in std::iter::once(&plans[baseline.script]).chain(candidates.iter()) {
                let label = format!(
                    "query-script-{}-reverse-{}-index-{query}-attempt-{}",
                    baseline.script,
                    baseline.reverse_poll,
                    errors.len()
                );
                match checker.audit(&label, &run, plan) {
                    Ok(_) => {
                        checked = true;
                        break;
                    }
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if checked {
                epf_checked += 1;
            } else {
                unresolved += 1;
                serde_json::to_writer_pretty(
                    create(&output.join(format!(
                        "unresolved-script-{}-reverse-{}-query-{query}.json",
                        baseline.script, baseline.reverse_poll
                    )))?,
                    &serde_json::json!({"probe":measurement.probe,"errors":errors}),
                )?;
            }
        }
        if !missing.is_empty() {
            return Err("incomplete scoped-query matrix".into());
        }
    }
    if lines.next().is_some()
        || used_states.len() != states.len()
        || query_index != answers.answers.len()
    {
        return Err("unexpected scoped-query suffix or unreferenced operation state".into());
    }
    Ok(serde_json::json!({
        "checked":completed,"epf_checked":epf_checked,"epf_requested":checker.epf.is_some(),"unresolved":unresolved,
        "pending":pending,"rejected":rejected,
        "negative_eligibility_checked":true,
        "eligibility_scope":"measured public calls at actual scoped cuts with the fixture FIFO; completion only, not the Pending/error distinction",
        "candidate_family_complete":false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_answers_bind_each_completion_to_its_source_candidate_inventory() {
        let mut answers = SourceAnswers {
            source: 0,
            answers: vec![(0, vec![0])],
            readiness: vec![(0, true)],
        };
        assert!(answers.ready(0).unwrap());
        answers.readiness[0].1 = false;
        assert!(!answers.ready(0).unwrap());
        answers.answers[0].1.push(0);
        assert!(answers.ready(0).is_err());
        answers.answers[0].1.clear();
        answers.readiness[0].1 = true;
        assert!(answers.ready(0).is_err());
        answers.answers[0] = (1, vec![0]);
        assert!(answers.ready(0).is_err());
        answers.answers[0] = (0, vec![0]);
        answers.readiness[0].0 = 1;
        assert!(answers.ready(0).is_err());
        answers.readiness.clear();
        assert!(answers.ready(0).is_err());
    }

    fn measurement() -> Measurement {
        Measurement {
            script: 0,
            reverse_poll: false,
            probe: serde_json::json!({"role":0,"actor":0,"label":0,"receive":false}),
            prefix_records: 1,
            prefix_borrows: 0,
            prefix_calls: 0,
            outcome: "completed".into(),
            error_utf8: vec![],
            checkpoints: [2, 0, 0, 0],
            suffix: vec![[0, 1, 0x0202, 1, 2, 1]],
            state: 0,
        }
    }

    #[test]
    fn query_commit_keeps_the_public_contract_without_selecting_a_peer() {
        let snapshot = [
            [0, 0x0312, 0, 1 << 16 | 7, 0],
            [1, 0x0312, 0, 2 << 16 | 7, 0],
        ];
        let mut sample = measurement();
        // send::<M> selects a view and contract, not the remote peer or the
        // original event named by the generator's alias.
        assert!(matches_call(&snapshot, &sample, &[]).is_ok());
        sample.suffix.clear();
        assert!(matches_call(&snapshot, &sample, &[]).is_err());
        sample = measurement();
        sample.outcome = "pending".into();
        assert!(matches_call(&snapshot, &sample, &[]).is_err());
        for field in [2, 3, 5] {
            sample = measurement();
            sample.suffix[0][field] = match field {
                2 => 0x0206,
                3 => 2,
                _ => 1 << 24 | 1,
            };
            assert!(matches_call(&snapshot, &sample, &[]).is_err());
        }
        let mut different = snapshot;
        different[1][3] += 1;
        assert!(matches_call(&different, &measurement(), &[]).is_err());
        different = snapshot;
        different[1][4] = 1;
        assert!(matches_call(&different, &measurement(), &[]).is_err());
    }

    #[test]
    fn query_resolver_evidence_has_the_recorded_input_and_logical_authority() {
        let snapshot = [
            [0, 0x0312, 0, 1 << 16 | 7, 0],
            [1, 0x0312, 0, 2 << 16 | 7, 0],
            [0, 0x0314, 0, 0, 18],
        ];
        let mut sample = measurement();
        sample.suffix[0][1] = 2;
        sample.suffix.insert(0, [0, 1, 0x0407, 1, 2, 17]);
        assert!(matches_call(&snapshot, &sample, &[17]).is_ok());
        assert!(matches_call(&snapshot, &sample, &[]).is_err());
        sample.suffix[0][3] = 257;
        assert!(matches_call(&snapshot, &sample, &[17]).is_err());
        sample.suffix[0][3] = 1;
        sample.suffix[0][5] = 2 << 16 | 17;
        assert!(matches_call(&snapshot, &sample, &[17]).is_err());
    }

    #[test]
    fn query_capture_requires_its_own_complete_producer_checkpoint() {
        let baseline = CapturedRun {
            script: 0,
            reverse_poll: false,
            checkpoints: [1, 0, 0, 0],
            events: vec![[0, 0, 0, 0, 0, 0]],
            borrows: vec![],
        };
        let mut sample = measurement();
        assert!(prefix(&baseline, &sample, 0).is_ok());
        sample.checkpoints[0] = 3;
        assert!(prefix(&baseline, &sample, 0).is_err());
        sample = measurement();
        sample.suffix[0][1] = 0;
        assert!(prefix(&baseline, &sample, 0).is_err());
        sample = measurement();
        sample.suffix[0][0] = 4;
        assert!(prefix(&baseline, &sample, 0).is_err());
        sample = measurement();
        sample.prefix_records = 2;
        assert!(prefix(&baseline, &sample, 0).is_err());
        sample = measurement();
        sample.prefix_calls = 1;
        assert!(prefix(&baseline, &sample, 0).is_err());
    }
}
