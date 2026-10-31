//! Check native observations of Hibana's no_std runtime query counters.
use std::{collections::BTreeMap, env, fmt::Write as _, fs, io::Write as _, path::Path};

const CASES: [(&str, u32, u32); 11] = [
    ("seq", 8, 128),
    ("seq", 256, 128),
    ("par", 8, 128),
    ("par", 256, 128),
    ("roll", 1, 512),
    ("roll", 8, 512),
    ("roll", 32, 512),
    ("route", 0, 128),
    ("route", 1, 128),
    ("delegation", 1, 384),
    ("nested-roll", 2, 384),
];
const METRICS: [&str; 8] = [
    "operation_identity_queries",
    "conflict_queries",
    "outbound_candidate_rows",
    "inbound_candidate_rows",
    "frame_candidate_queries",
    "admission_checks",
    "predecessor_rows",
    "route_row_queries",
];
type Key = (String, u32, String);

fn coordinates(case: &str, parameter: &str, operations: &str, metric: &str) -> Result<Key, String> {
    let parameter = parameter.parse::<u32>().map_err(|e| e.to_string())?;
    let operations = operations.parse::<u32>().map_err(|e| e.to_string())?;
    if !CASES.contains(&(case, parameter, operations)) || !METRICS.contains(&metric) {
        return Err("unknown work-count case, operation count or metric".into());
    }
    Ok((case.into(), parameter, metric.into()))
}

fn measured(text: &str) -> Result<BTreeMap<Key, u64>, String> {
    let mut rows: BTreeMap<Key, [Option<u64>; 2]> = BTreeMap::new();
    for line in text.lines().filter(|line| line.starts_with("runtime-work")) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [_, case, parameter, operations, run, metric, count] = fields.as_slice() else {
            return Err("invalid runtime work record".into());
        };
        let key = coordinates(case, parameter, operations, metric)?;
        let run = run.parse::<usize>().map_err(|e| e.to_string())?;
        let count = count.parse::<u64>().map_err(|e| e.to_string())?;
        let entry = rows.entry(key).or_insert([None; 2]);
        let Some(slot) = entry.get_mut(run) else {
            return Err("unknown work-count run".into());
        };
        if slot.replace(count).is_some() {
            return Err("duplicate work-count observation".into());
        }
    }
    if rows.len() != CASES.len() * METRICS.len() {
        return Err("missing runtime work observations".into());
    }
    let mut result = BTreeMap::new();
    for (key, pair) in rows {
        let [Some(first), Some(second)] = pair else {
            return Err("missing repeated work-count run".into());
        };
        if first != second {
            return Err("work counts changed between identical executions".into());
        }
        if ["operation_identity_queries", "admission_checks"].contains(&key.2.as_str())
            && first == 0
        {
            return Err("an executed kernel has no observed runtime work".into());
        }
        result.insert(key, first);
    }
    for metric in METRICS {
        if !result
            .iter()
            .any(|(key, count)| key.2 == metric && *count != 0)
        {
            return Err(format!("runtime work suite never exercised {metric}"));
        }
    }
    Ok(result)
}

fn budgets(text: &str) -> Result<BTreeMap<Key, u64>, String> {
    let mut result = BTreeMap::new();
    let mut header = false;
    for line in text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        if !header {
            if line != "case\tparameter\tendpoint_ops\tmetric\tlimit" {
                return Err("invalid runtime work budget header".into());
            }
            header = true;
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        let [case, parameter, operations, metric, limit] = fields.as_slice() else {
            return Err("invalid runtime work budget row".into());
        };
        let key = coordinates(case, parameter, operations, metric)?;
        let limit = limit.parse::<u64>().map_err(|e| e.to_string())?;
        if result.insert(key, limit).is_some() {
            return Err("duplicate runtime work budget".into());
        }
    }
    if result.len() != CASES.len() * METRICS.len() {
        return Err("incomplete runtime work budgets".into());
    }
    Ok(result)
}

fn check(observed: &BTreeMap<Key, u64>, limits: &BTreeMap<Key, u64>) -> Result<(), String> {
    if observed.keys().ne(limits.keys()) {
        return Err("work observations and budgets have different inventories".into());
    }
    for (key, count) in observed {
        if *count > limits[key] {
            return Err(format!(
                "runtime work increased for {key:?}: {count} > {}",
                limits[key]
            ));
        }
    }
    Ok(())
}

fn initial_budget(observed: &BTreeMap<Key, u64>) -> Result<String, String> {
    let mut text = String::from(
        "# Logical Rust query counts, not CPU instructions or elapsed time.\n# Initial limits add 1% rounded up. Changes require explicit review.\ncase\tparameter\tendpoint_ops\tmetric\tlimit\n",
    );
    for ((case, parameter, metric), count) in observed {
        let operations = CASES
            .iter()
            .find(|row| row.0 == case && row.1 == *parameter)
            .ok_or("unknown case")?
            .2;
        let limit = count
            .checked_add(count.div_ceil(100))
            .ok_or("work budget overflow")?;
        writeln!(text, "{case}\t{parameter}\t{operations}\t{metric}\t{limit}")
            .map_err(|e| e.to_string())?;
    }
    Ok(text)
}

fn main() -> Result<(), String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if let [command, observations] = args.as_slice()
        && command == "artifact"
    {
        let text = fs::read_to_string(observations).map_err(|e| e.to_string())?;
        println!("{}", test_artifact(&text)?);
        return Ok(());
    }
    let [command, observations, budget] = args.as_slice() else {
        return Err("usage: runtime-work check|record OBSERVATIONS BUDGET".into());
    };
    let text = fs::read_to_string(observations).map_err(|e| e.to_string())?;
    let observed = measured(&text)?;
    if command == "record" {
        let text = initial_budget(&observed)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(Path::new(budget))
            .map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    } else if command == "check" {
        let limits = budgets(&fs::read_to_string(budget).map_err(|e| e.to_string())?)?;
        check(&observed, &limits)?;
        println!(
            "runtime work: {} cases, {} metrics, two identical executions; all within reviewed limits",
            CASES.len(),
            METRICS.len()
        );
    } else {
        return Err("unknown runtime work command".into());
    }
    Ok(())
}

fn test_artifact(text: &str) -> Result<String, String> {
    let mut artifact = None;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if value["reason"] == "compiler-artifact"
            && value["target"]["name"] == "hibana"
            && value["target"]["kind"] == serde_json::json!(["lib"])
            && value["profile"]["test"] == true
        {
            let path = value["executable"]
                .as_str()
                .ok_or("missing test executable")?;
            if artifact.replace(path.to_owned()).is_some() {
                return Err("more than one Hibana test artifact".into());
            }
        }
    }
    artifact.ok_or_else(|| "missing Hibana test artifact".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        let mut text = String::new();
        for (case, parameter, operations) in CASES {
            for run in 0..2 {
                for metric in METRICS {
                    writeln!(
                        text,
                        "runtime-work\t{case}\t{parameter}\t{operations}\t{run}\t{metric}\t100"
                    )
                    .unwrap();
                }
            }
        }
        text
    }

    #[test]
    fn every_metric_and_both_runs_are_required() {
        let text = fixture();
        let observed = measured(&text).unwrap();
        let limits = budgets(&initial_budget(&observed).unwrap()).unwrap();
        check(&observed, &limits).unwrap();
        let first = text.lines().next().unwrap();
        assert!(measured("").is_err());
        assert!(measured("test result: ok. 0 passed").is_err());
        assert!(measured(&text.lines().skip(1).collect::<Vec<_>>().join("\n")).is_err());
        assert!(measured(&format!("{text}{first}\n")).is_err());
        for changed in [
            first.replace("\t0\t", "\t2\t"),
            first.replace("\t100", "\t101"),
            first.replace("\t128\t", "\t127\t"),
            first.replace("operation_identity_queries", "unknown"),
        ] {
            let mutant = text.replacen(first, &changed, 1);
            assert!(measured(&mutant).is_err(), "accepted {changed}");
        }
    }

    #[test]
    fn extra_work_fails_without_rewriting_the_budget() {
        let observed = measured(&fixture()).unwrap();
        let text = initial_budget(&observed).unwrap();
        let limits = budgets(&text).unwrap();
        let mut extra = observed.clone();
        *extra.values_mut().next().unwrap() = 102;
        assert!(check(&extra, &limits).is_err());
        assert!(
            budgets(
                &text
                    .lines()
                    .filter(|line| !line.starts_with("par\t8\t128\tadmission_checks\t"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
            .is_err()
        );
    }

    #[test]
    fn a_zero_cost_profile_cannot_replace_an_executed_kernel() {
        assert!(measured(&fixture().replace("\t100\n", "\t0\n")).is_err());
    }

    #[test]
    fn every_instrumented_path_must_be_exercised_by_the_suite() {
        let text = fixture();
        for metric in METRICS {
            let mutant = text.replace(&format!("\t{metric}\t100\n"), &format!("\t{metric}\t0\n"));
            assert!(measured(&mutant).is_err(), "unmeasured path: {metric}");
        }
    }

    #[test]
    fn only_one_current_test_artifact_can_supply_the_measurement() {
        let value = serde_json::json!({
            "reason": "compiler-artifact", "target": {"name": "hibana", "kind": ["lib"]},
            "profile": {"test": true}, "executable": "/target/hibana-current"
        })
        .to_string();
        assert_eq!(test_artifact(&value).unwrap(), "/target/hibana-current");
        assert!(test_artifact("").is_err());
        assert!(test_artifact("invalid JSON").is_err());
        assert!(test_artifact(&format!("{value}\n{value}")).is_err());
        assert!(test_artifact(&value.replace("true", "false")).is_err());
        assert!(test_artifact(&value.replace("[\"lib\"]", "[\"example\"]")).is_err());
    }
}
