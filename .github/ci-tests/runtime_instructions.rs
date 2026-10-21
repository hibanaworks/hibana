//! Parse actual Callgrind instruction counts. This tool never times a process
//! or treats a missing/empty profile as a zero-cost kernel.
use std::{collections::BTreeMap, env, fs, path::Path};

// Three warmups and one sample: runtime_cost has 128 endpoint operations per
// window; roll_cost has 512. The parameter is tail events or body round trips.
const CASES: [(&str, u32, u64); 7] = [
    ("seq", 8, 512),
    ("seq", 256, 512),
    ("par", 8, 512),
    ("par", 256, 512),
    ("roll", 1, 2048),
    ("roll", 8, 2048),
    ("roll", 32, 2048),
];

fn instructions(text: &str) -> Result<u64, String> {
    let mut events = None;
    let mut total = None;
    let mut own_cost = 0u64;
    let mut call_cost = false;
    for line in text.lines() {
        if total.is_some() && !line.trim().is_empty() {
            return Err("profile data follows the final total".into());
        }
        if let Some(value) = line.strip_prefix("events: ")
            && events.replace(value).is_some()
        {
            return Err("duplicate event declaration".into());
        }
        if let Some(value) = line.strip_prefix("totals: ") {
            let count = value.parse::<u64>().map_err(|error| error.to_string())?;
            total = Some(count);
        } else if line.starts_with("calls=") {
            if call_cost {
                return Err("a call is missing its inclusive cost line".into());
            }
            call_cost = true;
        } else if line.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '*')) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() > 2 {
                return Err("expected one line position and one instruction cost".into());
            }
            let count = match fields.get(1) {
                Some(value) => value.parse::<u64>().map_err(|error| error.to_string())?,
                None => 0,
            };
            if call_cost {
                call_cost = false;
            } else {
                own_cost = own_cost
                    .checked_add(count)
                    .ok_or("instruction sum overflow")?;
            }
        }
    }
    if events != Some("Ir") {
        return Err("profile must contain only the Ir instruction event".into());
    }
    if call_cost {
        return Err("truncated inclusive call cost".into());
    }
    match total {
        Some(count) if count > 0 && count == own_cost => Ok(count),
        Some(_) => Err("instruction total does not match nonempty self costs".into()),
        _ => Err("missing or empty instruction profile".into()),
    }
}

fn budgets(text: &str) -> Result<BTreeMap<(String, u32), u64>, String> {
    let mut result = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        if line.is_empty() || line.starts_with("case\t") {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 {
            return Err(format!("invalid instruction budget row: {line}"));
        }
        let parameter = fields[1]
            .parse::<u32>()
            .map_err(|error| error.to_string())?;
        let operations = fields[2]
            .parse::<u64>()
            .map_err(|error| error.to_string())?;
        let limit = fields[3]
            .parse::<u64>()
            .map_err(|error| error.to_string())?;
        if !CASES.contains(&(fields[0], parameter, operations))
            || limit == 0
            || result
                .insert((fields[0].into(), parameter), limit)
                .is_some()
        {
            return Err(format!("invalid or duplicate instruction budget: {line}"));
        }
    }
    if result.len() != CASES.len() {
        return Err("instruction budgets must cover all seven cases exactly once".into());
    }
    Ok(result)
}

fn main() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: runtime-instructions PROFILE_DIRECTORY BUDGET_FILE".into());
    }
    let root = Path::new(&args[0]);
    let limits = budgets(&fs::read_to_string(&args[1]).map_err(|error| error.to_string())?)?;
    println!("case\tparameter\tendpoint_ops\trun\tinstructions\tinstructions_per_op");
    for (case, parameter, operations) in CASES {
        for run in [1, 2] {
            let file = root.join(format!("{case}-{parameter}-{run}.callgrind"));
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            let count =
                instructions(&text).map_err(|error| format!("{}: {error}", file.display()))?;
            println!(
                "{case}\t{parameter}\t{operations}\t{run}\t{count}\t{:.3}",
                count as f64 / operations as f64,
            );
            let limit = limits[&(case.into(), parameter)];
            if count > limit {
                return Err(format!(
                    "{case}/{parameter} run {run}: {count} instructions exceed {limit}"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{budgets, instructions};

    #[test]
    fn reads_actual_totals_without_double_counting_inclusive_call_costs() {
        assert_eq!(
            instructions(
                "events: Ir\nsummary: 0\nfn=(1) kernel\n1 12\ncalls=1 0\n1 512000\nfn=(2) callee\n0 511988\ntotals: 512000\n"
            ),
            Ok(512_000),
        );
    }

    #[test]
    fn rejects_missing_ambiguous_or_empty_measurements() {
        for text in [
            "",
            "events: Ir\n",
            "events: Ir\nsummary: 0\n",
            "events: Ir\n0 1\ntotals: 0\n",
            "events: Ir\n0 1\ntotals: 2\n",
            "events: Ir\ncalls=1 0\ntotals: 1\n",
            "events: Ir Dr\nsummary: 100 20\n",
            "events: Ir\nsummary: 1\nsummary: 2\n",
            "events: Ir\nevents: Ir\nsummary: 1\n",
        ] {
            assert!(instructions(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn requires_one_finite_budget_for_every_measurement_case() {
        let valid = "seq\t8\t512\t100\nseq\t256\t512\t200\npar\t8\t512\t300\npar\t256\t512\t400\nroll\t1\t2048\t500\nroll\t8\t2048\t600\nroll\t32\t2048\t700\n";
        assert_eq!(budgets(valid).unwrap().len(), 7);
        assert!(budgets("").is_err());
        assert!(budgets(&valid.replace("seq\t8", "par\t8")).is_err());
        assert!(budgets(&valid.replace("512", "128")).is_err());
        assert!(budgets(&valid.replace("400", "0")).is_err());
        assert!(budgets(&valid.replace("2048", "512")).is_err());
        assert!(budgets(&valid.replace("roll\t32", "roll\t64")).is_err());
        assert!(
            budgets(
                valid
                    .lines()
                    .take(6)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .as_str()
            )
            .is_err()
        );
    }
}
