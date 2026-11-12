//! Pass actual generated Endpoint captures to EPF without linking a second core.
//! This host codec stores evidence; EPF owns capture and execution validation.
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[path = "protocol_shape_witness.rs"]
mod witness;
use witness::SourcePlan;
#[path = "protocol_shape_capture_queries.rs"]
mod queries;
#[path = "protocol_shape_state_data.rs"]
mod state_data;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    name: usize,
    scripts: usize,
    first: usize,
    count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedRun {
    script: usize,
    #[serde(rename = "reversePoll")]
    reverse_poll: bool,
    checkpoints: [u32; 4],
    events: Vec<[u64; 6]>,
    borrows: Vec<[u64; 8]>,
}

fn create(path: &Path) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

fn records(path: &Path, rows: impl IntoIterator<Item = [u64; 5]>) -> Result<()> {
    let mut file = BufWriter::new(create(path)?);
    for row in rows {
        file.write_all(&u32::try_from(row[0])?.to_be_bytes())?;
        file.write_all(&u16::try_from(row[1])?.to_be_bytes())?;
        file.write_all(&u16::try_from(row[2])?.to_be_bytes())?;
        file.write_all(&u32::try_from(row[3])?.to_be_bytes())?;
        file.write_all(&u32::try_from(row[4])?.to_be_bytes())?;
    }
    file.flush()?;
    Ok(())
}

fn candidate(plan: &SourcePlan, run: &CapturedRun, snapshot: &[[u64; 5]]) -> Result<Vec<u8>> {
    let events = &run.events;
    const SEND: u64 = 0x0202;
    const RECV: u64 = 0x0203;
    const SESSION: u64 = 0x0204;
    const POLL: u64 = 0x0208;
    const CHOICE: u64 = 0x0221;
    const REENTRY: u64 = 0x0222;
    const RESOLVER: u64 = 0x0407;
    let committed = |row: &[u64; 6]| matches!(row[2], SEND | RECV | SESSION) && row[4] == 2;
    let mut occurrences = vec![None; events.len()];
    let mut used = BTreeSet::new();
    for (position, row) in events.iter().enumerate().filter(|(_, row)| committed(row)) {
        let actor = row[3] >> 8;
        let view = row[5] >> 24;
        let origin = row[5] & 65535;
        let receive = u64::from(row[3] & 255 == 2);
        let (index, call) = plan
            .calls
            .iter()
            .enumerate()
            .find(|(index, call)| {
                !used.contains(index) && call[..4] == [actor, view, origin, receive]
            })
            .ok_or_else(|| {
                format!("no planned occurrence for committed record {position}: {row:?}")
            })?;
        used.insert(index);
        occurrences[position] = Some(call[4]);
    }
    let mut scopes = BTreeMap::<u64, [u64; 3]>::new();
    for &[_, id, key, boundary, kind] in snapshot {
        if id == 0x0313 {
            let index = usize::try_from(kind & 3)?;
            if index > 2 {
                return Err("invalid source scope boundary".into());
            }
            let bounds = scopes.entry(key).or_insert([u64::MAX; 3]);
            // A route has enter/exit markers for each arm under the same
            // scope ID; a par instead has one explicit split marker.
            if key >> 13 == 0 {
                match index {
                    0 => bounds[0] = bounds[0].min(boundary),
                    2 if bounds[1] == u64::MAX => bounds[1] = boundary,
                    2 => bounds[2] = boundary,
                    _ => return Err("invalid route scope boundary".into()),
                }
            } else {
                bounds[index] = boundary;
            }
        }
    }
    let mut observed_borrows = BTreeSet::new();
    let mut previous_cut = 0;
    for &[
        actor,
        view,
        start,
        stop,
        part_start,
        part_stop,
        occurrence,
        cut,
    ] in &run.borrows
    {
        if actor > 3
            || view > 3
            || !(start <= part_start && part_start < part_stop && part_stop <= stop)
            || !scopes.iter().any(|(&scope, &bounds)| {
                scope >> 13 == 2 && bounds[0] == start && bounds[2] == stop
            })
            || cut < previous_cut
            || cut > plan.calls.len() as u64
            || occurrence > plan.calls.len() as u64
            || !plan.calls.iter().any(|call| {
                call[0] == actor && call[1] == view && part_start <= call[2] && call[2] < part_stop
            })
            || !observed_borrows
                .insert([actor, view, start, stop, part_start, part_stop, occurrence])
        {
            return Err(
                "public borrow record disagrees with the compiled source, actor or script cut"
                    .into(),
            );
        }
        previous_cut = cut;
    }
    let mut first_polls = BTreeSet::new();
    let mut acceptances = Vec::new();
    for (position, row) in events.iter().enumerate() {
        if row[4] != 2 || committed(row) {
            continue;
        }
        let actor = row[3] >> 8;
        let scope = row[5] >> 16;
        let origin = row[5] & 65535;
        let related = |next: &[u64; 6]| {
            if !committed(next) || next[3] >> 8 != actor {
                return false;
            }
            let source = next[5] & 65535;
            if row[2] == POLL || (row[2] == REENTRY && scope >> 13 == 1) {
                next[5] >> 24 == row[3] & 255
                    && source == origin
                    && (row[2] != POLL || next[3] & 255 == 1)
            } else {
                scopes.get(&scope).is_some_and(|&[start, split, end]| {
                    let arm = if row[2] == RESOLVER {
                        row[3] & 255
                    } else {
                        origin
                    };
                    let (start, end) = match arm {
                        0 => (start, split),
                        1 => (split, end),
                        255 => (start, end),
                        _ => return false,
                    };
                    start <= source && source < end
                })
            }
        };
        if matches!(row[2], POLL | CHOICE | REENTRY | RESOLVER) {
            let next = events
                .iter()
                .enumerate()
                .skip(position + 1)
                .find(|(_, next)| related(next))
                .map(|(index, _)| index);
            occurrences[position] = match next {
                Some(next) => occurrences[next],
                None => plan.calls.iter().enumerate().find_map(|(index, call)| {
                    if used.contains(&index) {
                        return None;
                    }
                    // An uncommitted callback/control still needs an explicit
                    // candidate occurrence. EPF checks its admission against
                    // the complete supplied prefix; this grants no progress.
                    let [actor, view, origin, receive, _] = *call;
                    let id = if receive == 1 { RECV } else { SEND };
                    related(&[
                        0,
                        0,
                        id,
                        (actor << 8) | (receive + 1),
                        2,
                        view << 24 | origin,
                    ])
                    .then_some(call[4])
                }),
            };
            if occurrences[position].is_none() {
                return Err(format!(
                    "no source candidate supports control record {position}: {row:?}"
                )
                .into());
            }
            if row[2] == POLL
                && next.is_some()
                && first_polls.insert((row[0], occurrences[position]))
            {
                let source = snapshot
                    .iter()
                    .find(|source| source[1] == 0x0312 && source[0] == origin)
                    .ok_or("poll names a missing compiled source row")?;
                // The fixture's poll_send always accepts atomically and returns
                // Ready. This is only an untrusted candidate interval; EPF also
                // requires its real publication/receipt and checks readiness.
                if source[3] >> 24 != (source[3] >> 16) & 255 {
                    acceptances.push([row[0], row[1], (position + 1) as u64]);
                }
            }
        }
    }
    let mut bytes = b"HEPC\0\0\0\x01".to_vec();
    bytes.extend_from_slice(&2u32.to_be_bytes());
    for count in [
        plan.unfolding.len(),
        plan.choices.len(),
        events.len(),
        acceptances.len(),
    ] {
        bytes.extend_from_slice(&u64::try_from(count)?.to_be_bytes());
    }
    for row in &plan.unfolding {
        for word in row {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
    }
    for row in &plan.choices {
        for word in row {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
    }
    for (row, occurrence) in events.iter().zip(occurrences) {
        for word in [row[0], row[1], occurrence.unwrap_or(u64::MAX)] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
    }
    for row in acceptances {
        for word in row {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
    }
    Ok(bytes)
}

// Immutable evidence and file locations for one capture batch. This owns no
// protocol progress, actor state or authority, and never drives an Endpoint.
struct CaptureCheck<'a> {
    epf: Option<&'a Path>,
    source_checker: &'a Path,
    output: &'a Path,
    snapshot: &'a [[u64; 5]],
    header: &'a Header,
    baselines: &'a [CapturedRun],
    plans: &'a [SourcePlan],
    plan_input: &'a Path,
}

impl CaptureCheck<'_> {
    fn audit(&self, label: &str, run: &CapturedRun, plan: &SourcePlan) -> Result<usize> {
        // Reject an unsupported proposal before creating files. A rejected
        // candidate is not evidence that no other finite source witness exists.
        let proposed = candidate(plan, run, self.snapshot)?;
        let directory = self.output.join(label);
        fs::create_dir(&directory)?;
        let mut producers: [Vec<[u64; 5]>; 4] = std::array::from_fn(|_| Vec::new());
        for [producer, ordinal, id, key, arg0, arg1] in &run.events {
            producers
                .get_mut(usize::try_from(*producer)?)
                .ok_or("capture names an undeclared producer")?
                .push([*ordinal, *id, *key, *arg0, *arg1]);
        }
        let witness = directory.join("witness.bin");
        create(&witness)?.write_all(&proposed)?;
        let mut command = Command::new(self.epf.ok_or("EPF audit was not requested")?);
        command
            .arg("check")
            .arg(self.output.join("protocol.bin"))
            .arg(witness);
        for (producer, (rows, checkpoint)) in producers.into_iter().zip(run.checkpoints).enumerate()
        {
            let path = directory.join(format!("capture-{producer}.bin"));
            records(&path, rows)?;
            // This checkpoint came from the producer, never from the supplied rows.
            command.arg(path).arg(checkpoint.to_string());
        }
        let certificate = directory.join("audit.json");
        let status = command
            .stdin(Stdio::null())
            .stdout(create(&certificate)?)
            .stderr(create(&directory.join("audit.log"))?)
            .status()?;
        if !status.success() {
            return Err(format!(
                "EPF audit did not succeed for script {} reverse={}: {status}; evidence={}",
                run.script,
                run.reverse_poll,
                directory.display()
            )
            .into());
        }
        let checked: serde_json::Value = serde_json::from_reader(File::open(certificate)?)?;
        if checked["kind"] != "source_interpretation"
            || checked["session"] != 2
            || checked["order"].as_array().map(Vec::len) != Some(run.events.len())
        {
            return Err("EPF returned an incomplete or unrelated execution certificate".into());
        }
        Ok(run.events.len())
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let (required, epf) = match args.as_slice() {
        [source_checker, input, plan_input, query_input, output] => (
            (source_checker, input, plan_input, query_input, output),
            None,
        ),
        [
            source_checker,
            input,
            plan_input,
            query_input,
            output,
            flag,
            epf,
        ] if flag.as_os_str() == "--epf" => (
            (source_checker, input, plan_input, query_input, output),
            Some(fs::canonicalize(epf)?),
        ),
        _ => {
            return Err(
            "usage: protocol-shape-capture SOURCE_CHECKER CAPTURE.jsonl SOURCE_PLANS.jsonl QUERY_CAPTURE.jsonl NEW_OUTPUT [--epf EPF_BINARY]"
                .into(),
        );
        }
    };
    let (source_checker, input, plan_input, query_input, output) = required;
    let source_checker = fs::canonicalize(source_checker)?;
    let mut lines = BufReader::new(File::open(input)?).lines();
    let header: Header = serde_json::from_str(&lines.next().ok_or("missing capture inventory")??)?;
    let end = header
        .first
        .checked_add(header.count)
        .ok_or("capture interval overflow")?;
    if header.count == 0 || end > header.scripts {
        return Err("empty or out-of-domain capture inventory".into());
    }
    let runs = header
        .count
        .checked_mul(2)
        .ok_or("capture run count overflow")?;
    let snapshot: Vec<[u64; 5]> =
        serde_json::from_reader(File::open(input.with_extension("projection.json"))?)?;
    fs::create_dir(output)?;
    let protocol = output.join("protocol.bin");
    records(&protocol, snapshot.iter().copied())?;
    let plans: Vec<SourcePlan> = BufReader::new(File::open(plan_input)?)
        .lines()
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect::<Result<_>>()?;
    if plans.len() != header.scripts
        || plans
            .iter()
            .enumerate()
            .any(|(script, plan)| plan.script != script)
    {
        return Err("missing, duplicate or reordered source plan".into());
    }
    let mut baselines = Vec::new();
    for script in 0..header.scripts {
        if !(header.first..end).contains(&script) {
            continue;
        }
        for reverse in [false, true] {
            let run: CapturedRun =
                serde_json::from_str(&lines.next().ok_or("missing capture execution")??)?;
            if run.script != script || run.reverse_poll != reverse {
                return Err("missing, duplicate or reordered capture execution".into());
            }
            baselines.push(run);
        }
    }
    if lines.next().is_some() {
        return Err("unexpected capture suffix".into());
    }
    let checker = CaptureCheck {
        epf: epf.as_deref(),
        source_checker: &source_checker,
        output,
        snapshot: &snapshot,
        header: &header,
        baselines: &baselines,
        plans: &plans,
        plan_input,
    };
    let mut events = 0usize;
    let mut borrow_calls = 0usize;
    for run in &baselines {
        let plan = &plans[run.script];
        let calls = run
            .events
            .iter()
            .filter(|row| matches!(row[2], 0x0202..=0x0204) && row[4] == 2)
            .count();
        if calls != plan.calls.len() {
            return Err("completed capture is missing planned Endpoint operations".into());
        }
        if plan.unfolding.iter().any(|step| step[0] == 2) && run.borrows.is_empty() {
            return Err("parallel source execution has no recorded public borrow boundary".into());
        }
        if epf.is_some() {
            checker.audit(
                &format!("script-{}-reverse-{}", run.script, run.reverse_poll),
                run,
                plan,
            )?;
        }
        events = events
            .checked_add(run.events.len())
            .ok_or("capture record count overflow")?;
        borrow_calls = borrow_calls
            .checked_add(run.borrows.len())
            .ok_or("borrow observation count overflow")?;
        if epf.is_some() {
            println!(
                "EPF checked source={} script={} reverse={}",
                header.name, run.script, run.reverse_poll
            );
        }
    }
    let queries = queries::check(&checker, query_input)?;
    let summary = serde_json::json!({
        "source":header.name, "scripts":header.scripts, "first":header.first,
        "count":header.count, "runs":runs, "records":events,
        "recorded_borrow_calls":borrow_calls,
        "baseline_protocol_eligibility_checked":epf.is_some(),
        "public_cut_conformance_checked":true,
        "completed_query_execution":queries,
        "source_conformance_checked":false,
        "application_effects_certified":false,
        "session_completion_certified":false,
    });
    serde_json::to_writer_pretty(create(&output.join("summary.json"))?, &summary)?;
    println!("{summary}");
    if summary["completed_query_execution"]["unresolved"] != 0 {
        return Err("some completed queries have no checked source candidate in the supplied finite plan family; this is not an illegality verdict".into());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
