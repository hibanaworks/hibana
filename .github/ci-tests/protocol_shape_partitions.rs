//! Assign every generated protocol shard to exactly one CI job and window.
use std::{env, process};

const JOBS: usize = 128;

struct Plan {
    window: usize,
    windows: usize,
    shards: usize,
}

impl Plan {
    fn new(budget: usize, window: usize) -> Result<Self, &'static str> {
        let (windows, shards) = match budget {
            3 => (1, 1024),
            4 => (32, 32768),
            5 => (16, 65536),
            _ => return Err("operator budget must be 3, 4 or 5"),
        };
        if window >= windows {
            return Err("window is outside this operator budget");
        }
        Ok(Self {
            window,
            windows,
            shards,
        })
    }

    fn assigned(&self, job: usize) -> Result<Vec<usize>, &'static str> {
        if job >= JOBS {
            return Err("job is outside the CI matrix");
        }
        let count = self.shards / (JOBS * self.windows);
        Ok((0..count)
            .map(|part| job + JOBS * (part + count * self.window))
            .collect())
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
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
            let jobs = (0..JOBS)
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
    fn every_shard_belongs_to_one_job_and_one_window() {
        for (budget, windows, shards) in [(3, 1, 1024), (4, 32, 32768), (5, 16, 65536)] {
            let mut seen = vec![false; shards];
            for window in 0..windows {
                let plan = Plan::new(budget, window).unwrap();
                for job in 0..JOBS {
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
            }
            assert!(seen.into_iter().all(|assigned| assigned));
        }
    }

    #[test]
    fn invalid_scope_cannot_create_an_empty_successful_job() {
        for budget in [0, 1, 2, 6, usize::MAX] {
            assert!(Plan::new(budget, 0).is_err());
        }
        for (budget, windows) in [(3, 1), (4, 32), (5, 16)] {
            assert!(Plan::new(budget, windows).is_err());
        }
        assert!(Plan::new(5, 0).unwrap().assigned(JOBS).is_err());
    }
}
