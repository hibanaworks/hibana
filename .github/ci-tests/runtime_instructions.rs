//! Parse actual Callgrind instruction counts. This tool never times a process
//! or treats a missing/empty profile as a zero-cost kernel.
use std::{env, fs, path::Path};

fn instructions(text: &str) -> Result<u64, String> {
    let mut events = None;
    let mut summary = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("events: ")
            && events.replace(value).is_some()
        {
            return Err("duplicate event declaration".into());
        }
        if let Some(value) = line.strip_prefix("summary: ") {
            let count = value.parse::<u64>().map_err(|error| error.to_string())?;
            if summary.replace(count).is_some() {
                return Err("duplicate profile summary".into());
            }
        }
    }
    if events != Some("Ir") {
        return Err("profile must contain only the Ir instruction event".into());
    }
    match summary {
        Some(count) if count > 0 => Ok(count),
        _ => Err("missing or empty instruction profile".into()),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: runtime-instructions PROFILE_DIRECTORY".into());
    }
    let root = Path::new(&args[0]);
    // runtime_cost performs three warmups plus one measured sample, all within
    // the same explicitly instrumented operation windows: 4 * 128 operations.
    const OPERATIONS: u64 = 512;
    println!("case\ttail_events\tendpoint_ops\trun\tinstructions\tinstructions_per_op");
    for case in ["seq", "par"] {
        for tail in [8, 256] {
            for run in [1, 2] {
                let file = root.join(format!("{case}-{tail}-{run}.callgrind"));
                let text = fs::read_to_string(&file)
                    .map_err(|error| format!("{}: {error}", file.display()))?;
                let count =
                    instructions(&text).map_err(|error| format!("{}: {error}", file.display()))?;
                println!(
                    "{case}\t{tail}\t{OPERATIONS}\t{run}\t{count}\t{:.3}",
                    count as f64 / OPERATIONS as f64,
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::instructions;

    #[test]
    fn reads_the_inclusive_instruction_summary() {
        assert_eq!(
            instructions("events: Ir\nsummary: 512000\nfn=(1) kernel\n1 4\n"),
            Ok(512_000),
        );
    }

    #[test]
    fn rejects_missing_ambiguous_or_empty_measurements() {
        for text in [
            "",
            "events: Ir\n",
            "events: Ir\nsummary: 0\n",
            "events: Ir Dr\nsummary: 100 20\n",
            "events: Ir\nsummary: 1\nsummary: 2\n",
            "events: Ir\nevents: Ir\nsummary: 1\n",
        ] {
            assert!(instructions(text).is_err(), "{text:?}");
        }
    }
}
