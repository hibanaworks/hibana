//! Assign every generated protocol shard to exactly one CI job and window.
use std::{env, process};
#[path = "protocol_shape_catalog_counts.rs"]
mod catalog_counts;

/// One execution plan for exhaustive catalogs and seeded syntax sampling.
fn ci_matrix(extended: bool) -> serde_json::Value {
    const BUDGET: usize = 2;
    // These full populations also provide the positive side of the required
    // production mutations. The controls assignment executes each once.
    const SENSITIVITY_CONTRACTS: [usize; 5] = [2, 6, 7, 8, 14];
    let plan = Plan::new(BUDGET, 0).expect("the mandatory finite catalog is defined");
    let mut entries = (0..plan.jobs)
        .map(|partition| {
            serde_json::json!({
                "id": format!("constructors-{partition}"),
            "selection": "constructors", "budget": BUDGET,
            "window": plan.window, "partition": partition,
            })
        })
        .collect::<Vec<_>>();
    for first in 0..catalog_counts::CONTRACTS {
        if SENSITIVITY_CONTRACTS.contains(&first) {
            continue;
        }
        entries.push(serde_json::json!({
            "id": format!("contracts-{first}"), "selection": "contracts",
            "first": first, "count": 1,
        }));
    }
    entries.push(serde_json::json!({
        "id": "controls", "selection": "controls", "contracts": SENSITIVITY_CONTRACTS,
    }));
    if extended {
        for first in (0..catalog_counts::FRONTIERS).step_by(4) {
            entries.push(serde_json::json!({
                "id": format!("frontiers-{first}"), "selection": "frontiers",
                "first": first, "count": 4.min(catalog_counts::FRONTIERS - first),
            }));
        }
        for (family, depth, first) in [
            ("interactions", 4, 12439),
            ("interactions", 5, 111955),
            ("spines", 5, 2019),
            ("spines", 5, 15667),
            ("spines", 5, 22409),
            ("spines", 5, 23531),
            ("spines", 3, 630),
            ("spines", 3, 658),
        ] {
            entries.push(serde_json::json!({
                "id": format!("{family}-{depth}-{first}"), "selection": family,
                "depth": depth, "first": first, "count": 1,
            }));
        }
        for depth in 4..=8 {
            entries.push(serde_json::json!({
                "id": format!("sample-{depth}"), "selection": "sample", "depth": depth,
            }));
        }
    }
    serde_json::json!({"include": entries})
}

struct Plan {
    window: usize,
    windows: usize,
    shards: usize,
    jobs: usize,
}

impl Plan {
    fn new(budget: usize, window: usize) -> Result<Self, &'static str> {
        let (windows, shards, jobs) = match budget {
            2 => (1, 32, 32),
            3 => (1, 1024, 128),
            4 => (32, 32768, 128),
            5 => (16, 65536, 128),
            _ => return Err("operator budget must be 2, 3, 4 or 5"),
        };
        if window >= windows {
            return Err("window is outside this operator budget");
        }
        Ok(Self {
            window,
            windows,
            shards,
            jobs,
        })
    }

    fn assigned(&self, job: usize) -> Result<Vec<usize>, &'static str> {
        if job >= self.jobs {
            return Err("job is outside the CI matrix");
        }
        let count = self.shards / (self.jobs * self.windows);
        Ok((0..count)
            .map(|part| job + self.jobs * (part + count * self.window))
            .collect())
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "ci-case") {
        let [_, id] = args.as_slice() else {
            return Err("usage: protocol-shape-partitions ci-case ID".into());
        };
        let plan = ci_matrix(true);
        let entry = plan["include"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"].as_str() == Some(id));
        match entry {
            Some(entry) => println!("{entry}"),
            None => return Err(format!("unknown protocol CI assignment: {id}")),
        }
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "ci-matrix") {
        let extended = match &args[1..] {
            [] => false,
            [suite] if suite == "extended" => true,
            _ => return Err("usage: protocol-shape-partitions ci-matrix [extended]".into()),
        };
        println!("{}", ci_matrix(extended));
        return Ok(());
    }
    if args.len() < 3 {
        return Err(
            "usage: protocol-shape-partitions matrix BUDGET WINDOW | shards BUDGET WINDOW JOB"
                .into(),
        );
    }
    let budget = args[1].parse::<usize>().map_err(|e| e.to_string())?;
    let window = args[2].parse::<usize>().map_err(|e| e.to_string())?;
    let plan = Plan::new(budget, window)?;
    match args[0].as_str() {
        "matrix" if args.len() == 3 => {
            let jobs = (0..plan.jobs)
                .map(|job| job.to_string())
                .collect::<Vec<_>>()
                .join(",");
            println!("{{\"partition\":[{jobs}]}}");
        }
        "shards" if args.len() == 4 => {
            let job = args[3].parse::<usize>().map_err(|e| e.to_string())?;
            for shard in plan.assigned(job)? {
                println!("{shard}\t{}", plan.shards);
            }
        }
        _ => return Err("invalid command or argument count".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mandatory_ci_assigns_the_complete_bounded_population_once() {
        let matrix = ci_matrix(false);
        let entries = matrix["include"].as_array().unwrap();
        let mut partitions = vec![false; Plan::new(2, 0).unwrap().jobs];
        let mut contracts = vec![false; catalog_counts::CONTRACTS];
        let mut controls = 0;
        for entry in entries {
            match entry["selection"].as_str().unwrap() {
                "constructors" => {
                    assert_eq!(entry["budget"], 2);
                    assert_eq!(entry["window"], 0);
                    let partition = entry["partition"].as_u64().unwrap() as usize;
                    assert!(!std::mem::replace(&mut partitions[partition], true));
                }
                "contracts" => {
                    let first = entry["first"].as_u64().unwrap() as usize;
                    assert_eq!(entry["count"], 1);
                    assert!(!std::mem::replace(&mut contracts[first], true));
                }
                "controls" => {
                    controls += 1;
                    let assigned = entry["contracts"].as_array().unwrap();
                    assert!(!assigned.is_empty());
                    for rank in assigned {
                        let rank = rank.as_u64().unwrap() as usize;
                        assert!(!std::mem::replace(&mut contracts[rank], true));
                    }
                }
                other => panic!("unexpected mandatory selection {other}"),
            }
        }
        assert!(partitions.into_iter().all(|assigned| assigned));
        assert!(contracts.into_iter().all(|assigned| assigned));
        assert_eq!(controls, 1);
    }

    #[test]
    fn extended_ci_preserves_every_mandatory_case_and_covers_its_extra_ranges() {
        let basic = ci_matrix(false);
        let extended = ci_matrix(true);
        let basic = basic["include"].as_array().unwrap();
        let extended = extended["include"].as_array().unwrap();
        assert!(extended.starts_with(basic));
        let mut ids = std::collections::BTreeSet::new();
        let mut frontier = vec![false; catalog_counts::FRONTIERS];
        let mut depths = std::collections::BTreeSet::new();
        for entry in extended {
            assert!(ids.insert(entry["id"].as_str().unwrap()));
            match entry["selection"].as_str().unwrap() {
                "frontiers" => {
                    let first = entry["first"].as_u64().unwrap() as usize;
                    let count = entry["count"].as_u64().unwrap() as usize;
                    assert!(count > 0);
                    for seen in &mut frontier[first..first + count] {
                        assert!(!std::mem::replace(seen, true));
                    }
                }
                "sample" => assert!(depths.insert(entry["depth"].as_u64().unwrap())),
                _ => {}
            }
        }
        assert!(frontier.into_iter().all(|assigned| assigned));
        assert_eq!(depths, (4..=8).collect());
    }

    #[test]
    fn every_shard_belongs_to_one_job_and_one_window() {
        for (budget, windows, shards) in [(2, 1, 32), (3, 1, 1024), (4, 32, 32768), (5, 16, 65536)]
        {
            let mut seen = vec![false; shards];
            for window in 0..windows {
                let plan = Plan::new(budget, window).unwrap();
                for job in 0..plan.jobs {
                    let assigned = plan.assigned(job).unwrap();
                    assert!(!assigned.is_empty());
                    for shard in assigned {
                        assert!(shard < shards);
                        assert!(
                            !std::mem::replace(&mut seen[shard], true),
                            "duplicate shard {shard}"
                        );
                    }
                }
                assert!(plan.assigned(plan.jobs).is_err());
            }
            assert!(seen.into_iter().all(|assigned| assigned));
        }
    }

    #[test]
    fn invalid_scope_cannot_create_an_empty_successful_job() {
        for budget in [0, 1, 6, usize::MAX] {
            assert!(Plan::new(budget, 0).is_err());
        }
        for (budget, windows) in [(2, 1), (3, 1), (4, 32), (5, 16)] {
            assert!(Plan::new(budget, windows).is_err());
        }
    }
}
