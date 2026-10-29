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
    if mode == "orchestration" {
        return orchestration(shape, count);
    }
    if count == 0 || count > 65535 {
        return Err("pressure source count must be in 1..65535".into());
    }
    if (mode, shape) == ("causal", "delegation") {
        return Ok(delegation(count));
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

fn delegation(transfers: usize) -> String {
    let mut program = String::new();
    for event in 0..transfers {
        let (from, to) = if event % 2 == 0 { (1, 2) } else { (2, 1) };
        let send = format!("g::send::<{from}, {to}, Loan>()");
        if event == 0 {
            program.push_str(&format!("    let program = {send};\n"));
        } else {
            program.push_str(&format!("    let program = g::seq(program, {send});\n"));
        }
    }
    // The type is deliberately left-associated; allow its structural trait
    // depth while retaining the const-evaluation work limit.
    let recursion = 4 * transfers + 128;
    format!(
        "#![no_std]\n#![deny(warnings)]\n#![forbid(long_running_const_eval)]\n#![recursion_limit=\"{recursion}\"]\n\nuse hibana::{{Endpoint, g, runtime::program::{{project, RoleProgram}}}};\ntype Loan = g::Msg<1, Endpoint<'static, 9>>;\n\n#[inline(never)]\npub fn projected_roles() -> (RoleProgram<1>, RoleProgram<2>) {{\n{program}    (project(&program), project(&program))\n}}\n"
    )
}

fn combine(parts: &[String], operator: &str) -> String {
    match parts {
        [] => panic!("empty pressure protocol"),
        [part] => part.clone(),
        _ => {
            let middle = parts.len() / 2;
            format!(
                "g::{operator}({}, {})",
                combine(&parts[..middle], operator),
                combine(&parts[middle..], operator)
            )
        }
    }
}

fn exchange(from: usize, peer: usize, label: usize, rounds: usize) -> String {
    let pair = format!(
        "g::seq(g::send::<{from}, {peer}, g::Msg<{label}, [u8; {peer}]>>(), g::send::<{peer}, {from}, g::Msg<{label}, [u8; {peer}]>>())"
    );
    combine(&vec![pair; rounds], "seq")
}

fn orchestration(shape: &str, roles: usize) -> Result<String, String> {
    if !matches!(shape, "mixed" | "hierarchical") || roles != 100 {
        return Err("orchestration pressure expects mixed|hierarchical 100".into());
    }
    let worker = |from, peer, rounds| {
        format!(
            "g::route(g::par({}, {}), g::par({}, {})).resolve::<{peer}>().roll()",
            exchange(from, peer, 1, rounds),
            exchange(from, peer, 2, rounds),
            exchange(from, peer, 3, rounds),
            exchange(from, peer, 4, rounds),
        )
    };
    let program = if shape == "mixed" {
        // 40 prefix events + 99 workers * 2 arms * 2 lanes * 5 round trips.
        let workers = (1..roles)
            .map(|peer| worker(0, peer, 5))
            .collect::<Vec<_>>();
        format!(
            "g::seq({}, {})",
            exchange(0, 99, 5, 20),
            combine(&workers, "par")
        )
    } else {
        // 25 groups of one coordinator and three workers: 25 * (56 + 56 + 40)
        // body events, preceded by 25 independent four-round exchanges (200).
        let starts = (0..25)
            .map(|group| exchange(0, if group == 0 { 1 } else { group * 4 }, 5, 4))
            .collect::<Vec<_>>();
        let groups = (0..25)
            .map(|group| {
                let from = group * 4;
                let workers = (1..=3)
                    .map(|offset| worker(from, from + offset, if offset == 3 { 5 } else { 7 }))
                    .collect::<Vec<_>>();
                combine(&workers, "par")
            })
            .collect::<Vec<_>>();
        format!(
            "g::seq({}, {})",
            combine(&starts, "par"),
            combine(&groups, "par")
        )
    };
    let projections = (0..roles)
        .map(|role| format!("    core::hint::black_box(project::<{role}>(&program));\n"))
        .collect::<String>();
    Ok(format!(
        "#![no_std]\n#![deny(warnings)]\n#![forbid(long_running_const_eval)]\n\nuse hibana::{{g, runtime::program::project}};\n\n#[inline(never)]\npub fn projected_roles() {{\n    let program = {program};\n{projections}}}\n"
    ))
}
fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 5 {
        return Err(
            "usage: pressure-source causal|route|message|orchestration SHAPE COUNT DESTINATION"
                .into(),
        );
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
