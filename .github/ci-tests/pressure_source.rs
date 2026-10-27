//! Deterministic benchmark-source construction; no runtime library changes.
mod pressure_templates;
use std::{env, fs};

#[derive(Clone, Copy)]
enum Messages {
    DistinctSchemas,
    CausalHandoff,
    Alternating,
    DisjointPairs,
}

fn sequence(start: usize, end: usize, messages: Messages) -> String {
    if end - start == 1 {
        let idx = start;
        let label = 1 + idx % 46;
        match messages {
            Messages::CausalHandoff => {
                let (source, target) = [(0, 2), (2, 1), (1, 2), (2, 0)][idx % 4];
                format!("g::send::<{source}, {target}, g::Msg<{label}, ()>>()")
            }
            Messages::Alternating => {
                let source = idx % 2;
                let target = 1 - source;
                let label = 1 + source;
                format!("g::send::<{source}, {target}, g::Msg<{label}, ()>>()")
            }
            Messages::DisjointPairs => {
                let source = 2 * (idx % 3);
                let target = source + 1;
                format!("g::send::<{source}, {target}, g::Msg<{label}, ()>>()")
            }
            Messages::DistinctSchemas => {
                format!("g::send::<0, 1, g::Msg<{label}, Payload<{idx}>>>()")
            }
        }
    } else {
        let middle = start + (end - start) / 2;
        format!(
            "g::seq({}, {})",
            sequence(start, middle, messages),
            sequence(middle, end, messages)
        )
    }
}
fn generate(mode: &str, shape: &str, count: usize) -> Result<String, String> {
    if count == 0 || count > 65535 {
        return Err("pressure source count must be in 1..65535".into());
    }
    let (messages, template) = match (mode, shape) {
        ("causal", "capacity") => (Messages::DisjointPairs, pressure_templates::CAPACITY),
        ("causal", "alternating") => (Messages::Alternating, pressure_templates::ALTERNATING),
        ("causal", _) => (Messages::CausalHandoff, pressure_templates::CAUSAL),
        ("route", _) => (Messages::DistinctSchemas, pressure_templates::ROUTE),
        ("message", _) => (Messages::DistinctSchemas, pressure_templates::MESSAGE),
        _ => return Err("unknown pressure source mode".into()),
    };
    let first = sequence(0, count, messages);
    let program = match (mode, shape) {
        ("causal", "linear" | "alternating" | "capacity") | ("message", _) => first,
        ("causal", "route") => format!(
            "g::route({first}, {}).resolve::<7>()",
            sequence(count, count * 2, messages)
        ),
        ("causal", "roll") => format!("({first}).roll()"),
        ("route", _) => format!(
            "g::route({first}, {})",
            sequence(count, count * 2, messages)
        ),
        _ => return Err("unknown causal pressure shape".into()),
    };
    Ok(template.replace("{PROGRAM}", &program))
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err("usage: pressure-source causal|route|message SHAPE COUNT DESTINATION".into());
    }
    let count = args[3].parse().map_err(|_| "invalid pressure count")?;
    fs::write(&args[4], generate(&args[1], &args[2], count)?).map_err(|e| e.to_string())
}
fn main() {
    if let Err(error) = run(&env::args().collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape_and_order() {
        assert_eq!(
            sequence(0, 2, Messages::DistinctSchemas),
            "g::seq(g::send::<0, 1, g::Msg<1, Payload<0>>>(), g::send::<0, 1, g::Msg<2, Payload<1>>>())"
        );
        assert_eq!(
            sequence(3, 4, Messages::CausalHandoff),
            "g::send::<2, 0, g::Msg<4, ()>>()"
        );
        assert!(
            generate("causal", "route", 2)
                .unwrap()
                .contains(".resolve::<7>()")
        );
        assert!(generate("causal", "roll", 2).unwrap().contains(").roll()"));
        assert!(generate("message", "", 2).unwrap().contains("0x4001_0000"));
        assert!(generate("route", "", 2).unwrap().contains("0x5001_0000"));
        assert!(generate("message", "", 0).is_err());
        assert!(generate("causal", "invalid", 2).is_err());
    }
}
