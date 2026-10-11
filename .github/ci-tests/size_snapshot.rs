//! Private size-measurement parser and fixed-budget regression gate.
#[allow(dead_code)]
mod kani_json;
use kani_json::Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};
type Result<T> = std::result::Result<T, String>;
type Metrics = BTreeMap<String, u64>;
const SECTIONS: &[&str] = &[".text", ".rodata", ".data", ".bss", "flash_total"];
const SHAPES: &[&str] = &["fanout_heavy", "linear_heavy", "route_heavy"];
const RUNTIME_METRICS: &[&str] = &[
    "session_kit_storage_bytes",
    "resident_prefix_bytes",
    "tap_ring_bytes",
    "sidecar_scratch_high_water_bytes",
    "live_endpoint_bytes",
    "peak_live_slab_bytes",
    "localside_peak_stack_bytes",
    "peak_stack_bytes",
    "modeled_runtime_sram_bytes",
];
fn read(path: &str) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or("measurement sum overflow".into())
}
fn sum(mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0, add)
}
fn integer(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| format!("invalid nonnegative measurement: {text}"))
}
fn metric(metrics: &Metrics, key: &str) -> u64 {
    metrics.get(key).copied().unwrap_or(0)
}
fn sections(raw: &str, prefix: &str) -> Result<String> {
    let mut values = [0_u64; 4];
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let index = if name.starts_with(".text") || name == "__text" {
            Some(0)
        } else if name.starts_with(".rodata") || matches!(name, "__const" | "__cstring") {
            Some(1)
        } else if name.starts_with(".data") || name == "__data" {
            Some(2)
        } else if name.starts_with(".bss") || matches!(name, "__bss" | "__common" | "__thread_bss")
        {
            Some(3)
        } else {
            None
        };
        if let Some(i) = index {
            let size = integer(
                fields
                    .next()
                    .ok_or_else(|| format!("missing size for section {name}"))?,
            )?;
            values[i] = add(values[i], size)?;
        }
    }
    Ok(SECTIONS[..4]
        .iter()
        .zip(values)
        .map(|(name, value)| format!("{prefix} {name} {value}\n"))
        .collect())
}
fn parse_sections(output: &str, prefix: &str) -> Result<Metrics> {
    let mut result = Metrics::new();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix(prefix).and_then(|s| s.strip_prefix(' ')) else {
            continue;
        };
        let Some((name, value)) = rest.split_once(' ') else {
            continue;
        };
        if !name.starts_with('.') || name.len() == 1 || !name[1..].bytes().all(key_byte) {
            continue;
        }
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        result.insert(name.into(), integer(&value[..digits])?);
    }
    let flash = sum([".text", ".rodata", ".data"]
        .iter()
        .map(|k| metric(&result, k)))?;
    result.insert("flash_total".into(), flash);
    Ok(result)
}
fn key_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
fn shape_name(line: &str) -> Option<&str> {
    let mut offset = 0;
    while let Some(i) = line[offset..].find("shape=") {
        let start = offset + i + 6;
        let len = line[start..].bytes().take_while(|c| key_byte(*c)).count();
        if len > 0 {
            return Some(&line[start..start + len]);
        }
        offset = start;
    }
    None
}
fn numeric_metrics(line: &str) -> Result<Metrics> {
    let mut result = Metrics::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !key_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && key_byte(bytes[i]) {
            i += 1;
        }
        let end = i;
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > value_start {
                result.insert(line[start..end].into(), integer(&line[value_start..i])?);
            }
        }
    }
    Ok(result)
}
fn capture(
    label: &str,
    section_output: &str,
    projected_output: &str,
    stack_output: &str,
) -> Result<Json> {
    let sections = parse_sections(section_output, "section")?;
    let projected = parse_sections(projected_output, "projected section")?;
    let mut shapes: BTreeMap<String, Metrics> = BTreeMap::new();
    for line in stack_output.lines() {
        if line.contains("large-choreography-runtime ")
            && let Some(shape) = shape_name(line)
        {
            shapes.insert(shape.into(), numeric_metrics(line)?);
        }
    }
    let data_bss = add(metric(&sections, ".data"), metric(&sections, ".bss"))?;
    for metrics in shapes.values_mut() {
        if metric(metrics, "resident_prefix_bytes") < metric(metrics, "tap_ring_bytes") {
            return Err("resident_prefix_bytes must include the internal tap ring carved before the runtime slab".into());
        }
        let modeled = sum([
            data_bss,
            metric(metrics, "session_kit_storage_bytes"),
            metric(metrics, "resident_prefix_bytes"),
            metric(metrics, "peak_live_slab_bytes"),
            metric(metrics, "localside_peak_stack_bytes"),
        ]
        .into_iter())?;
        metrics.insert("modeled_runtime_sram_bytes".into(), modeled);
    }
    let mut maximum = Metrics::new();
    for metrics in shapes.values() {
        for (key, value) in metrics {
            if key != "slab_bytes" {
                let current = maximum.entry(key.clone()).or_default();
                *current = (*current).max(*value);
            }
        }
    }
    Ok(Json::Object(BTreeMap::from([
        ("label".into(), Json::String(label.into())),
        ("sections".into(), metrics_json(sections)),
        ("projected_sections".into(), metrics_json(projected)),
        (
            "runtime_shapes".into(),
            Json::Object(
                shapes
                    .into_iter()
                    .map(|(name, metrics)| (name, metrics_json(metrics)))
                    .collect(),
            ),
        ),
        ("runtime_max".into(), metrics_json(maximum)),
    ])))
}
fn metrics_json(metrics: Metrics) -> Json {
    Json::Object(
        metrics
            .into_iter()
            .map(|(key, value)| (key, Json::Number(value.to_string())))
            .collect(),
    )
}
fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn encode(json: &Json, depth: usize) -> String {
    match json {
        Json::Object(map) => {
            if map.is_empty() {
                return "{}".into();
            }
            let indent = "  ".repeat(depth);
            let child = "  ".repeat(depth + 1);
            format!(
                "{{\n{}\n{indent}}}",
                map.iter()
                    .map(|(key, value)| format!(
                        "{child}{}: {}",
                        json_string(key),
                        encode(value, depth + 1)
                    ))
                    .collect::<Vec<_>>()
                    .join(",\n")
            )
        }
        Json::String(s) => json_string(s),
        Json::Number(n) => n.clone(),
        _ => unreachable!("measurement snapshots only contain objects, strings and numbers"),
    }
}
fn object<'a>(value: &'a Json, label: &str) -> Result<&'a BTreeMap<String, Json>> {
    if let Json::Object(map) = value {
        Ok(map)
    } else {
        Err(format!("expected object: {label}"))
    }
}
fn field<'a>(value: &'a Json, key: &str) -> Result<&'a Json> {
    value
        .get(key)
        .ok_or_else(|| format!("missing snapshot field: {key}"))
}
fn number(value: &Json) -> Result<u64> {
    if let Json::Number(n) = value {
        integer(n)
    } else {
        Err("measurement must be a nonnegative integer".into())
    }
}
fn number_or_zero(value: Option<&Json>) -> Result<u64> {
    value.map(number).transpose().map(|n| n.unwrap_or(0))
}
fn compare(current: &Json, snapshot: &Json, min_headroom: i128) -> Result<(String, Vec<String>)> {
    let mut failures = Vec::new();
    let mut output = String::new();
    let empty = Json::Object(BTreeMap::new());
    let shapes = object(
        current.get("runtime_shapes").unwrap_or(&empty),
        "runtime_shapes",
    )?;
    let missing = SHAPES
        .iter()
        .filter(|shape| !shapes.contains_key(**shape))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        failures.push(format!(
            "current runtime snapshot missing shapes: {}",
            missing.join(", ")
        ));
    }
    for shape in SHAPES {
        if let Some(metrics) = shapes.get(*shape) {
            let metrics = object(metrics, shape)?;
            let missing = RUNTIME_METRICS
                .iter()
                .filter(|key| !metrics.contains_key(**key))
                .copied()
                .collect::<BTreeSet<_>>();
            if !missing.is_empty() {
                failures.push(format!(
                    "current runtime snapshot shape={shape} missing metrics: {}",
                    missing.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    let budget = field(snapshot, "budget")?;
    let section_budget = field(
        field(budget, "thumbv6m_none_eabi_no_std_release_lib")?,
        "sections",
    )?;
    let sections = field(current, "sections")?;
    object(sections, "sections")?;
    object(section_budget, "section budget")?;
    let projected = current.get("projected_sections").unwrap_or(&empty);
    object(projected, "projected_sections")?;
    for (label, values, prefix) in [
        ("section", sections, ""),
        ("projected-section", projected, "projected "),
    ] {
        for key in SECTIONS {
            let actual = number_or_zero(values.get(key))?;
            let maximum = number_or_zero(section_budget.get(key))?;
            output += &format!(
                "worktree-snapshot budget-{label} {key} actual={actual} budget={maximum}\n"
            );
            if actual > maximum {
                failures.push(format!("{prefix}section {key} exceeds snapshot budget: actual={actual} budget={maximum}"));
            }
        }
    }
    let runtime_budget = object(field(budget, "runtime_shapes")?, "runtime budget")?;
    for shape in SHAPES {
        let Some(current_shape) = shapes.get(*shape) else {
            continue;
        };
        if let Some(maxima) = runtime_budget.get(*shape) {
            for (key, maximum) in object(maxima, shape)? {
                let Some(actual) = current_shape.get(key) else {
                    continue;
                };
                let actual = number(actual)?;
                let maximum = number(maximum)?;
                output += &format!(
                    "worktree-snapshot budget-runtime shape={shape} {key} actual={actual} budget={maximum}\n"
                );
                if actual > maximum {
                    failures.push(format!("runtime shape {shape} {key} exceeds snapshot budget: actual={actual} budget={maximum}"));
                }
            }
        }
    }
    let runtime_max = field(current, "runtime_max")?;
    object(runtime_max, "runtime_max")?;
    let current_stack = number_or_zero(runtime_max.get("peak_stack_bytes"))?;
    let current_sram = number_or_zero(runtime_max.get("modeled_runtime_sram_bytes"))?;
    let budget_max = |key: &str| -> Result<u64> {
        runtime_budget
            .values()
            .map(|v| number(field(v, key)?))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max()
            .ok_or("runtime budget is empty".into())
    };
    let budget_stack = budget_max("peak_stack_bytes")?;
    let budget_sram = budget_max("modeled_runtime_sram_bytes")?;
    let current_flash = number_or_zero(sections.get("flash_total"))?;
    let budget_flash = number(field(section_budget, "flash_total")?)?;
    let mut non_growing = 0;
    let mut decreased = 0;
    for (name, actual, maximum) in [
        ("max_stack", current_stack, budget_stack),
        ("sram", current_sram, budget_sram),
        ("flash", current_flash, budget_flash),
    ] {
        output += &format!(
            "worktree-snapshot budget-aggregate {name} actual={actual} budget={maximum}\n"
        );
        if actual > maximum {
            failures.push(format!(
                "aggregate {name} exceeds snapshot budget: actual={actual} budget={maximum}"
            ));
        }
        if actual <= maximum {
            non_growing += 1;
        }
        if actual < maximum {
            decreased += 1;
        }
    }
    if non_growing < 3 || decreased < 1 {
        failures.push("aggregate snapshot budget gate failed: max_stack/sram/flash must all be <= budget and at least one must decrease below budget".into());
    }
    let headroom = i128::from(budget_sram) - i128::from(current_sram);
    output += &format!(
        "worktree-snapshot aggregate sram_headroom current={headroom} minimum={min_headroom}\n"
    );
    if headroom < min_headroom {
        failures.push(format!("aggregate modeled runtime SRAM headroom is too small: headroom={headroom} minimum={min_headroom}"));
    }
    Ok((output, failures))
}
fn run(args: &[String]) -> Result<()> {
    match args{
        [mode,raw,prefix]if mode=="sections"=>print!("{}",sections(&read(raw)?,prefix)?),
        [mode,label,sections,projected,stack,out]if mode=="capture"=>{let value=capture(label,&read(sections)?,&read(projected)?,&read(stack)?)?;fs::write(Path::new(out),encode(&value,0)+"\n").map_err(|e|e.to_string())?;},
        [mode,current,snapshot,headroom]if mode=="compare"=>{let current=kani_json::parse(&read(current)?)?;let snapshot=kani_json::parse(&read(snapshot)?)?;let (output,failures)=compare(&current,&snapshot,headroom.parse().map_err(|_|"invalid SRAM headroom")?)?;print!("{output}");if !failures.is_empty(){return Err(format!("size snapshot regression detected:\n{}",failures.iter().map(|s|format!("  - {s}")).collect::<Vec<_>>().join("\n")));}},
        _=>return Err("usage: size-snapshot sections RAW PREFIX | capture LABEL SECTIONS PROJECTED STACK OUT | compare CURRENT SNAPSHOT MIN_SRAM_HEADROOM".into())
    }
    Ok(())
}
fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn llvm_sections() {
        let raw = "file.o:\nsection size addr\n.text 10 0\n.text.foo 12 0\n.rodata 3 0\n__const 4 0\n.data 5 0\n__data 6 0\n.bss 7 0\n__common 8 0\n.debug 999 0\n";
        assert_eq!(
            sections(raw, "section").unwrap(),
            "section .text 22\nsection .rodata 7\nsection .data 11\nsection .bss 15\n"
        );
        assert!(sections(".text not-a-number 0\n", "section").is_err());
        assert!(sections(".text 18446744073709551615 0\n.text 1 0\n", "section").is_err());
    }
    #[test]
    fn runtime_capture() {
        let stack = "noise\nlarge-choreography-runtime shape=route_heavy session_kit_storage_bytes=32 resident_prefix_bytes=952 tap_ring_bytes=512 peak_live_slab_bytes=4000 localside_peak_stack_bytes=3000 peak_stack_bytes=3500 slab_bytes=9000\n";
        let captured = capture(
            "label \"quoted\"",
            "section .data 2\nsection .bss 3\n",
            "",
            stack,
        )
        .unwrap();
        let metrics = captured
            .get("runtime_shapes")
            .unwrap()
            .get("route_heavy")
            .unwrap();
        assert_eq!(
            number(metrics.get("modeled_runtime_sram_bytes").unwrap()).unwrap(),
            7989
        );
        assert!(
            captured
                .get("runtime_max")
                .unwrap()
                .get("slab_bytes")
                .is_none()
        );
        assert_eq!(kani_json::parse(&encode(&captured, 0)).unwrap(), captured);
        assert!(
            capture(
                "x",
                "",
                "",
                &stack.replace("resident_prefix_bytes=952", "resident_prefix_bytes=1")
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    fn fixture() -> (Json, Json) {
        let shape = || {
            metrics_json(
                RUNTIME_METRICS
                    .iter()
                    .map(|key| {
                        (
                            (*key).into(),
                            if *key == "modeled_runtime_sram_bytes" {
                                1000
                            } else {
                                10
                            },
                        )
                    })
                    .collect(),
            )
        };
        let budget_shape = || {
            metrics_json(
                RUNTIME_METRICS
                    .iter()
                    .map(|key| {
                        (
                            (*key).into(),
                            if *key == "modeled_runtime_sram_bytes" {
                                1064
                            } else {
                                10
                            },
                        )
                    })
                    .collect(),
            )
        };
        let sections = || metrics_json(SECTIONS.iter().map(|key| ((*key).into(), 100)).collect());
        let current = Json::Object(BTreeMap::from([
            ("sections".into(), sections()),
            ("projected_sections".into(), sections()),
            (
                "runtime_shapes".into(),
                Json::Object(SHAPES.iter().map(|s| ((*s).into(), shape())).collect()),
            ),
            ("runtime_max".into(), shape()),
        ]));
        let budget = Json::Object(BTreeMap::from([(
            "budget".into(),
            Json::Object(BTreeMap::from([
                (
                    "thumbv6m_none_eabi_no_std_release_lib".into(),
                    Json::Object(BTreeMap::from([("sections".into(), sections())])),
                ),
                (
                    "runtime_shapes".into(),
                    Json::Object(
                        SHAPES
                            .iter()
                            .map(|s| ((*s).into(), budget_shape()))
                            .collect(),
                    ),
                ),
            ])),
        )]));
        (current, budget)
    }
    fn change(value: &mut Json, path: &[&str], replacement: Option<Json>) {
        let Json::Object(map) = value else {
            panic!("expected fixture object")
        };
        if path.len() == 1 {
            if let Some(replacement) = replacement {
                map.insert(path[0].into(), replacement);
            } else {
                map.remove(path[0]);
            }
        } else {
            change(map.get_mut(path[0]).unwrap(), &path[1..], replacement);
        }
    }
    #[test]
    fn fixed_budget_and_sram_headroom_boundaries() {
        let (mut current, budget) = fixture();
        assert!(compare(&current, &budget, 64).unwrap().1.is_empty());
        change(
            &mut current,
            &["runtime_max", "modeled_runtime_sram_bytes"],
            Some(Json::Number("1001".into())),
        );
        assert!(
            compare(&current, &budget, 64)
                .unwrap()
                .1
                .iter()
                .any(|e| e.contains("headroom=63 minimum=64"))
        );
        assert!(compare(&current, &budget, 63).unwrap().1.is_empty());
        change(
            &mut current,
            &["runtime_max", "modeled_runtime_sram_bytes"],
            Some(Json::Number("1064".into())),
        );
        assert!(
            compare(&current, &budget, 0)
                .unwrap()
                .1
                .iter()
                .any(|e| e.contains("at least one must decrease"))
        );
    }
    #[test]
    fn missing_and_over_budget_measurements_fail() {
        for path in [
            &["runtime_shapes", "route_heavy"][..],
            &["runtime_shapes", "linear_heavy", "resident_prefix_bytes"][..],
        ] {
            let (mut current, budget) = fixture();
            change(&mut current, path, None);
            assert!(!compare(&current, &budget, 64).unwrap().1.is_empty());
        }
        for path in [
            &["sections", ".text"][..],
            &["projected_sections", ".rodata"][..],
            &[
                "runtime_shapes",
                "fanout_heavy",
                "sidecar_scratch_high_water_bytes",
            ][..],
            &["runtime_max", "peak_stack_bytes"][..],
            &["runtime_max", "modeled_runtime_sram_bytes"][..],
        ] {
            let (mut current, budget) = fixture();
            change(&mut current, path, Some(Json::Number("9999".into())));
            assert!(!compare(&current, &budget, 64).unwrap().1.is_empty());
        }
        for bad in [
            Json::String("100".into()),
            Json::Bool(true),
            Json::Number("-1".into()),
            Json::Number("1.5".into()),
            Json::Number("18446744073709551616".into()),
        ] {
            let (mut current, budget) = fixture();
            change(
                &mut current,
                &["runtime_max", "peak_stack_bytes"],
                Some(bad),
            );
            assert!(compare(&current, &budget, 64).is_err());
        }
    }
}
