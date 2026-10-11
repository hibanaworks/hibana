//! Private measurement parsing and resource-budget checks. No dependencies.
#[allow(dead_code)]
mod kani_json;
use kani_json::Json;
use std::{collections::BTreeMap, env, fs, process::Command};
type Result<T> = std::result::Result<T, String>;
type Metrics = BTreeMap<String, u64>;
const PROTOCOLS: [&str; 7] = [
    "minimal_send_recv",
    "nested_par_join",
    "route_with_unselected_nested_par",
    "triple_nested_route",
    "passive_nested_route_observer",
    "alternating_par_route",
    "huge_legal_choreography",
];
const MATRIX_KEYS: [&str; 4] = [
    "program_blob_len",
    "role_blob_len",
    "endpoint_scratch_bytes",
    "largest_section_bytes",
];
fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn total(values: impl IntoIterator<Item = u64>) -> Result<u64> {
    values.into_iter().try_fold(0u64, |sum, value| {
        sum.checked_add(value)
            .ok_or_else(|| "measurement total overflow".into())
    })
}
fn size_total(values: impl IntoIterator<Item = usize>) -> Result<usize> {
    values.into_iter().try_fold(0usize, |sum, value| {
        sum.checked_add(value)
            .ok_or_else(|| "symbol count overflow".into())
    })
}
fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=");
    let (_, tail) = line.split_once(&needle)?;
    let n = tail.bytes().take_while(|c| word(*c)).count();
    (n > 0).then_some(&tail[..n])
}
fn metrics(line: &str) -> Result<Metrics> {
    let bytes = line.as_bytes();
    let mut out = Metrics::new();
    let mut at = 0;
    while at < bytes.len() {
        if !word(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && word(bytes[at]) {
            at += 1;
        }
        if at >= bytes.len() || bytes[at] != b'=' {
            continue;
        }
        let end = at;
        at += 1;
        let nstart = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at > nstart {
            out.insert(
                line[start..end].into(),
                line[nstart..at].parse().map_err(|_| "metric exceeds u64")?,
            );
        }
    }
    Ok(out)
}
fn rows(text: &str, marker: &str, key: &str) -> Result<BTreeMap<String, Metrics>> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if let Some((_, tail)) = line.split_once(marker)
            && let Some(name) = field(tail, key)
        {
            out.insert(name.into(), metrics(tail)?);
        }
    }
    Ok(out)
}
fn sections(text: &str) -> Result<Metrics> {
    let mut out = Metrics::new();
    for line in text.lines() {
        if let Some((_, tail)) = line.split_once("thumb section name=")
            && let Some((name, bytes)) = tail.split_once(" bytes=")
            && name.starts_with('.')
            && name[1..].bytes().all(word)
        {
            let digits = bytes.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                out.insert(
                    name.into(),
                    bytes[..digits]
                        .parse()
                        .map_err(|_| "section size exceeds u64")?,
                );
            }
        }
    }
    let total = total(
        [".text", ".rodata", ".data"]
            .iter()
            .map(|k| out.get(*k).copied().unwrap_or(0)),
    )?;
    out.insert("flash_total".into(), total);
    Ok(out)
}
fn get(row: &Metrics, key: &str) -> Result<u64> {
    row.get(key)
        .copied()
        .ok_or_else(|| format!("missing metric {key}"))
}
fn object(value: &Json) -> Result<&BTreeMap<String, Json>> {
    if let Json::Object(map) = value {
        Ok(map)
    } else {
        Err("expected JSON object".into())
    }
}
fn number(value: &Json) -> Result<u64> {
    if let Json::Number(n) = value {
        n.parse().map_err(|_| "expected nonnegative integer".into())
    } else {
        Err("expected JSON number".into())
    }
}
fn at<'a>(mut value: &'a Json, path: &[&str]) -> Result<&'a Json> {
    for key in path {
        value = object(value)?
            .get(*key)
            .ok_or_else(|| format!("missing snapshot key {key}"))?;
    }
    Ok(value)
}
fn json_metrics(value: &Json) -> Result<Metrics> {
    object(value)?
        .iter()
        .map(|(key, value)| Ok((key.clone(), number(value)?)))
        .collect()
}
fn budget(row: &Metrics, key: &str, maximum: u64, label: &str) -> Result<()> {
    let actual = get(row, key)?;
    println!("snapshot-check {label} {key} actual={actual} budget={maximum}");
    require(
        actual <= maximum,
        format!("{label} {key}={actual} exceeds {maximum}"),
    )
}
fn thumb(snapshot: &Json, text: &str) -> Result<()> {
    let values = sections(text)?;
    for (key, maximum) in json_metrics(at(
        snapshot,
        &[
            "budget",
            "thumbv6m_none_eabi_no_std_release_lib",
            "sections",
        ],
    )?)? {
        budget(&values, &key, maximum, "thumb")?;
    }
    Ok(())
}
fn layout(text: &str) -> Result<()> {
    let line = text
        .lines()
        .find_map(|l| l.split_once("future-layout ").map(|(_, tail)| tail))
        .ok_or("missing future-layout report")?;
    let values = metrics(line)?;
    let word = std::mem::size_of::<usize>() as u64;
    for (name, maximum) in [
        ("Endpoint", 3 * word),
        ("RouteBranch", 2 * word),
        ("OfferFuture", 3 * word),
        ("RecvFuture", 3 * word),
        ("BranchRecvFuture", 3 * word),
        ("SendFuture", 5 * word),
        ("RawOfferFuture", 2 * word),
        ("RawRecvFuture", 2 * word),
        ("OfferFutureLease", 1),
        ("RecvFutureLease", 1),
    ] {
        require(
            get(&values, name)? <= maximum,
            format!("{name} exceeds {maximum}"),
        )?;
    }
    for base in ["RecvFuture", "BranchRecvFuture"] {
        for suffix in ["U8", "U64", "Bytes"] {
            require(
                get(&values, base)? == get(&values, &format!("{base}{suffix}"))?,
                "future size depends on message payload type",
            )?;
        }
    }
    Ok(())
}
fn matrix(text: &str) -> Result<()> {
    let rows = rows(text, "protocol-matrix ", "name")?;
    let minimal = rows
        .get("minimal_send_recv")
        .ok_or("missing minimal protocol")?;
    for (key, maximum) in MATRIX_KEYS.into_iter().zip([16, 32, 512, 192]) {
        budget(minimal, key, maximum, "protocol-matrix minimal")?;
    }
    for name in PROTOCOLS {
        let row = rows
            .get(name)
            .ok_or_else(|| format!("missing protocol {name}"))?;
        for (key, maximum) in MATRIX_KEYS.into_iter().zip([256, 512, 1024, 384]) {
            budget(row, key, maximum, &format!("protocol-matrix name={name}"))?;
        }
    }
    Ok(())
}
fn protocol_metrics(text: &str, name: &str) -> Result<()> {
    for line in text.lines() {
        if let Some((_, tail)) = line.split_once("protocol-matrix ")
            && field(tail, "name") == Some(name)
        {
            let row = metrics(tail)?;
            println!(
                "program_blob_len={} role_blob_len={} endpoint_scratch_bytes={} largest_section_bytes={}",
                get(&row, MATRIX_KEYS[0])?,
                get(&row, MATRIX_KEYS[1])?,
                get(&row, MATRIX_KEYS[2])?,
                get(&row, MATRIX_KEYS[3])?
            );
            return Ok(());
        }
    }
    Err(format!("missing protocol matrix metrics for {name}"))
}
fn map_report(map_path: &str, bin_path: &str, llvm_nm: &str) -> Result<()> {
    let bytes = fs::read(map_path).map_err(|e| format!("cannot read link map: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let fragments: Vec<u64> = text
        .lines()
        .filter(|l| l.contains(":(.rodata"))
        .filter_map(|l| {
            l.split_whitespace()
                .nth(2)
                .and_then(|n| u64::from_str_radix(n, 16).ok())
        })
        .collect();
    let nm = Command::new(llvm_nm)
        .args(["--defined-only", bin_path])
        .output()
        .map_err(|e| e.to_string())?;
    require(
        nm.status.success(),
        format!("llvm-nm failed: {}", String::from_utf8_lossy(&nm.stderr)),
    )?;
    let symbols = String::from_utf8_lossy(&nm.stdout);
    let names = [
        "ProgramProjectionBlob",
        "RoleProjectionBlob",
        "ProgramImageBytes",
        "RoleImageBytes",
    ];
    println!(
        "rodata_map_bytes={} rodata_map_fragments={} bucket_symbol_count={} map_bucket_symbol_count={} full_bucket_floor_bytes={}",
        total(fragments.iter().copied())?,
        fragments.len(),
        size_total(names.iter().map(|n| symbols.matches(n).count()))?,
        size_total(names.iter().map(|n| text.matches(n).count()))?,
        [32, 64, 96, 128, 192, 256, 384, 512, 1024, 2048, 4096, 8192]
            .iter()
            .sum::<u64>()
    );
    Ok(())
}
fn artifacts(text: &str) -> Result<()> {
    let rows = rows(text, "protocol-artifact ", "name")?;
    for name in PROTOCOLS {
        let row = rows
            .get(name)
            .ok_or_else(|| format!("missing protocol artifact {name}"))?;
        for key in [
            "flash_total",
            "rodata",
            "rodata_map_bytes",
            "rodata_map_fragments",
            "program_blob_len",
            "role_blob_len",
            "endpoint_scratch_bytes",
            "largest_section_bytes",
            "selected_program_bucket_count",
            "selected_role_bucket_count",
            "bucket_symbol_count",
            "map_bucket_symbol_count",
            "full_bucket_floor_bytes",
        ] {
            get(row, key)?;
        }
        let label = format!("protocol-artifact name={name}");
        let minimal = name == "minimal_send_recv";
        budget(
            row,
            "flash_total",
            if minimal { 2048 } else { 16384 },
            &label,
        )?;
        budget(row, "rodata", if minimal { 1024 } else { 4096 }, &label)?;
        let rodata = get(row, "rodata")?;
        let mapped = get(row, "rodata_map_bytes")?;
        require(mapped <= rodata, "map rodata exceeds section rodata")?;
        let padding = rodata - mapped;
        println!("snapshot-check {label} rodata_section_padding_bytes actual={padding} budget=64");
        require(padding <= 64, "rodata padding exceeds 64")?;
        require(
            get(row, "rodata_map_fragments")? <= 32,
            "too many rodata map fragments",
        )?;
        let selected = total([
            get(row, "selected_program_bucket_count")?,
            get(row, "selected_role_bucket_count")?,
        ])?;
        for key in ["bucket_symbol_count", "map_bucket_symbol_count"] {
            budget(row, key, selected, &label)?;
        }
        require(
            rodata
                < get(row, "full_bucket_floor_bytes")?
                    .checked_mul(selected)
                    .ok_or("bucket floor overflow")?,
            "artifact retains every bucket ladder entry",
        )?;
    }
    Ok(())
}
fn add_sram(rows: &mut BTreeMap<String, Metrics>, data_bss: u64, strict: bool) -> Result<()> {
    for row in rows.values_mut() {
        require(
            row.get("resident_prefix_bytes").copied().unwrap_or(0)
                >= row.get("tap_ring_bytes").copied().unwrap_or(0),
            "resident_prefix_bytes must include tap ring",
        )?;
        let mut total = data_bss;
        for key in [
            "session_kit_storage_bytes",
            "resident_prefix_bytes",
            "peak_live_slab_bytes",
            "localside_peak_stack_bytes",
        ] {
            let value = if strict {
                get(row, key)?
            } else {
                row.get(key).copied().unwrap_or(0)
            };
            total = total.checked_add(value).ok_or("SRAM metric overflow")?;
        }
        row.insert("modeled_runtime_sram_bytes".into(), total);
    }
    Ok(())
}
fn max_metric<'a>(rows: impl Iterator<Item = &'a Metrics>, key: &str) -> Result<u64> {
    rows.map(|r| get(r, key))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .ok_or("empty metric inventory".into())
}
fn runtime(snapshot: &Json, thumb: &str, stack: &str, headroom: u64) -> Result<()> {
    let expected = object(at(snapshot, &["budget", "runtime_shapes"])?)?
        .iter()
        .map(|(k, v)| Ok((k.clone(), json_metrics(v)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut seen = rows(stack, "large-choreography-runtime ", "shape")?;
    let sections = sections(thumb)?;
    let data_bss = total([
        sections.get(".data").copied().unwrap_or(0),
        sections.get(".bss").copied().unwrap_or(0),
    ])?;
    add_sram(&mut seen, data_bss, false)?;
    for (shape, limits) in &expected {
        let row = seen
            .get(shape)
            .ok_or_else(|| format!("missing runtime shape {shape}"))?;
        for (key, &maximum) in limits {
            budget(row, key, maximum, &format!("runtime shape={shape}"))?;
        }
    }
    let actual_stack = max_metric(seen.values(), "peak_stack_bytes")?;
    let budget_stack = max_metric(expected.values(), "peak_stack_bytes")?;
    println!(
        "snapshot-check runtime max_peak_stack_bytes actual={actual_stack} budget={budget_stack}"
    );
    require(actual_stack <= budget_stack, "max peak stack exceeded")?;
    let actual_sram = max_metric(seen.values(), "modeled_runtime_sram_bytes")?;
    let budget_sram = max_metric(expected.values(), "modeled_runtime_sram_bytes")?;
    let flash = number(at(
        snapshot,
        &[
            "budget",
            "thumbv6m_none_eabi_no_std_release_lib",
            "sections",
            "flash_total",
        ],
    )?)?;
    let mut decreased = false;
    for (name, maximum, actual) in [
        ("max_stack", budget_stack, actual_stack),
        ("sram", budget_sram, actual_sram),
        ("flash", flash, get(&sections, "flash_total")?),
    ] {
        println!("snapshot-check aggregate {name} actual={actual} budget={maximum}");
        require(
            actual <= maximum,
            format!("aggregate {name} exceeds snapshot"),
        )?;
        decreased |= actual < maximum;
    }
    require(
        decreased,
        "aggregate max_stack/sram/flash require at least one decrease",
    )?;
    let actual_headroom = budget_sram - actual_sram;
    println!("snapshot-check aggregate sram_headroom actual={actual_headroom} minimum={headroom}");
    require(
        actual_headroom >= headroom,
        "modeled runtime SRAM headroom too small",
    )
}
fn commas(value: u64) -> String {
    let text = value.to_string();
    let mut result = String::new();
    for (i, c) in text.chars().enumerate() {
        if i > 0 && (text.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}
fn live(thumb: &str, artifact: &str, stack: &str, host: &str) -> Result<()> {
    let thumb = sections(thumb)?;
    let artifacts = artifact
        .lines()
        .filter(|l| l.starts_with("protocol-artifact "))
        .map(metrics)
        .collect::<Result<Vec<_>>>()?;
    let mut runtime = BTreeMap::new();
    for (i, line) in stack
        .lines()
        .filter_map(|l| l.split_once("large-choreography-runtime ").map(|(_, l)| l))
        .enumerate()
    {
        runtime.insert(i.to_string(), metrics(line)?);
    }
    require(
        !artifacts.is_empty() && !runtime.is_empty(),
        "live footprint report missing protocol or runtime measurements",
    )?;
    let data_bss = total([
        thumb.get(".data").copied().unwrap_or(0),
        thumb.get(".bss").copied().unwrap_or(0),
    ])?;
    add_sram(&mut runtime, data_bss, true)?;
    let tap = max_metric(runtime.values(), "tap_ring_bytes")?;
    println!("Live footprint report: host={host}");
    for (label, key) in [
        ("`SessionKitStorage`".into(), "session_kit_storage_bytes"),
        (
            format!(
                "Fixed per-rendezvous storage, including the {} B tap records",
                commas(tap)
            ),
            "resident_prefix_bytes",
        ),
        (
            "Peak live runtime slab across tracked heavy shapes".into(),
            "peak_live_slab_bytes",
        ),
        (
            "Runtime operation stack high-water".into(),
            "peak_stack_bytes",
        ),
        (
            "Modeled runtime SRAM envelope".into(),
            "modeled_runtime_sram_bytes",
        ),
    ] {
        println!("{label}: {} B", commas(max_metric(runtime.values(), key)?));
    }
    let artifact_flash = artifacts
        .iter()
        .map(|r| get(r, "flash_total"))
        .collect::<Result<Vec<_>>>()?;
    for (label, value) in [
        (
            "Minimal linked protocol artifact",
            *artifact_flash.iter().min().unwrap(),
        ),
        (
            "Largest linked artifact in the tracked protocol matrix",
            *artifact_flash.iter().max().unwrap(),
        ),
        (
            "Complete no-default `libhibana.rlib` sections",
            get(&thumb, "flash_total")?,
        ),
        ("Library `.data + .bss`", data_bss),
    ] {
        println!("{label}: {} B", commas(value));
    }
    Ok(())
}
fn var(name: &str) -> Result<String> {
    env::var(name).map_err(|e| format!("{name}: {e}"))
}
fn snapshot() -> Result<Json> {
    kani_json::parse(&fs::read_to_string(var("SNAPSHOT_FILE")?).map_err(|e| e.to_string())?)
}
fn stdin_text() -> Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}
fn llvm_sections(text: &str, mode: &str) -> Result<()> {
    let mut sums = [0u64; 4];
    for line in text.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0];
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
        if let Some(index) = index {
            let size: u64 = parts[1].parse().map_err(|_| "invalid LLVM section size")?;
            sums[index] = sums[index]
                .checked_add(size)
                .ok_or("section sum overflow")?;
        }
    }
    match mode {
        "host" | "thumb" => {
            for (name, size) in [".text", ".rodata", ".data", ".bss"].iter().zip(sums) {
                if mode == "thumb" {
                    println!(
                        "thumb section name={name} bytes={size} target=thumbv6m-none-eabi no_default_features=1"
                    );
                } else {
                    println!("section name={name} bytes={size}");
                }
            }
        }
        "artifact" => print!(
            "flash_total={} text={} rodata={} data={} bss={}",
            total([sums[0], sums[1], sums[2]])?,
            sums[0],
            sums[1],
            sums[2],
            sums[3]
        ),
        _ => return Err("unknown section output mode".into()),
    }
    Ok(())
}
fn nonempty_test(text: &str) -> Result<()> {
    require(
        text.lines().any(|line| {
            line.split("running ").skip(1).any(|tail| {
                let Some((n, tail)) = tail.split_once(' ') else {
                    return false;
                };
                n.parse::<u64>().is_ok_and(|n| n > 0) && tail.starts_with("test")
            })
        }),
        "cargo test filter matched no tests",
    )
}
fn aeabi(bin: &str, llvm_nm: &str) -> Result<()> {
    let output = Command::new(llvm_nm)
        .arg(bin)
        .output()
        .map_err(|e| e.to_string())?;
    require(
        output.status.success(),
        "protocol artifact symbol scan failed",
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    require(
        !text
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|token| {
                matches!(
                    token,
                    "__aeabi_lmul"
                        | "__aeabi_lcmp"
                        | "__aeabi_ulcmp"
                        | "__aeabi_ldivmod"
                        | "__aeabi_uldivmod"
                        | "__aeabi_llsl"
                        | "__aeabi_llsr"
                        | "__aeabi_lasr"
                )
            }),
        "protocol artifact regained aeabi u64 helper calls",
    )?;
    print!("aeabi_u64_helper_count=0");
    Ok(())
}
fn role_count(name: &str) -> Result<u64> {
    match name {
        "minimal_send_recv" | "triple_nested_route" | "huge_legal_choreography" => Ok(2),
        "nested_par_join"
        | "route_with_unselected_nested_par"
        | "passive_nested_route_observer"
        | "alternating_par_route" => Ok(4),
        _ => Err("unknown protocol artifact name".into()),
    }
}
fn write_project(root: &str, destination: &str, name: Option<&str>) -> Result<()> {
    let dir = std::path::Path::new(destination);
    fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    let root_toml = root.replace('\\', "\\\\").replace('"', "\\\"");
    let (package, workspace) = match name {
        Some(name) => {
            role_count(name)?;
            (format!("hibana-protocol-{}", name.replace('_', "-")), "")
        }
        None => ("hibana-final-form-measure".into(), "[workspace]\n\n"),
    };
    let manifest = format!(
        "[package]\nname = \"{package}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n{workspace}[dependencies]\nhibana = {{ path = \"{root_toml}\", default-features = false }}\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    let source = if let Some(name) = name {
        for file in [
            "final_form_protocol_matrix.rs",
            "final_form_protocol_black_box_roles.rs",
        ] {
            fs::copy(
                std::path::Path::new(root)
                    .join("src/global/role_program/tests")
                    .join(file),
                dir.join("src").join(file),
            )
            .map_err(|e| e.to_string())?;
        }
        format!(
            "#![no_std]\n#![no_main]\n\nuse core::panic::PanicInfo;\nuse hibana::{{g, runtime::program::{{project, RoleProgram}}}};\nuse hibana::g::Msg;\n\ninclude!(\"final_form_protocol_matrix.rs\");\ninclude!(\"final_form_protocol_black_box_roles.rs\");\n\n#[panic_handler]\nfn panic(_: &PanicInfo) -> ! {{ loop {{}} }}\n\n#[unsafe(no_mangle)]\npub extern \"C\" fn _start() -> ! {{\n    let program = final_form_protocol!({name});\n    final_form_protocol_black_box_roles!({name}, &program);\n    loop {{}}\n}}\n"
        )
    } else {
        "fn main() {\n    std::hint::black_box(hibana::g::send::<\n        0,\n        1,\n        hibana::g::Msg<7, ()>\n    >());\n}\n".into()
    };
    fs::write(dir.join("src/main.rs"), source).map_err(|e| e.to_string())
}
fn argument(index: usize) -> Result<String> {
    env::args()
        .nth(index)
        .ok_or_else(|| format!("missing argument {index}"))
}
fn observation(text: &str, prefix: &str) -> Result<(u64, u64)> {
    let line = text
        .lines()
        .rfind(|line| line.starts_with(prefix))
        .ok_or("missing compile-pressure observation")?;
    let tail = line.split_once("elapsed=").ok_or("missing elapsed time")?.1;
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    require(fields.len() >= 3, "incomplete compile-pressure observation")?;
    let seconds = fields[0]
        .strip_suffix('s')
        .ok_or("invalid elapsed time unit")?
        .parse()
        .map_err(|_| "invalid elapsed time")?;
    let limit = fields[1]
        .strip_prefix("seconds_budget=")
        .and_then(|v| v.strip_suffix('s'))
        .ok_or("invalid time budget field")?;
    let _: u64 = limit.parse().map_err(|_| "invalid time budget")?;
    let rss = fields[2]
        .strip_prefix("max_rss=")
        .and_then(|v| v.strip_suffix("MiB"))
        .ok_or("invalid RSS field")?
        .parse()
        .map_err(|_| "invalid RSS")?;
    Ok((seconds, rss))
}
fn llvm_image(text: &str) -> Result<u64> {
    let mut total = 0u64;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        if name.starts_with(".text")
            || name.starts_with(".rodata")
            || matches!(name, "__text" | "__const" | "__cstring")
        {
            let value: u64 = parts
                .next()
                .ok_or("missing section size")?
                .parse()
                .map_err(|_| "invalid section size")?;
            total = total.checked_add(value).ok_or("section sum overflow")?;
        }
    }
    Ok(total)
}
fn message_budget(image: [u64; 3], rlib: [u64; 3]) -> Result<()> {
    let ig64 = image[1].saturating_sub(image[0]);
    let ig256 = image[2].saturating_sub(image[1]);
    let rg64 = rlib[1].saturating_sub(rlib[0]);
    let rg256 = rlib[2].saturating_sub(rlib[1]);
    println!(
        "message-heavy thumb growth image_1_to_64={ig64} image_64_to_256={ig256} rlib_1_to_64={rg64} rlib_64_to_256={rg256}"
    );
    require(
        image[2].saturating_sub(image[0]) <= 512 * 1024,
        "message-heavy thumb image exceeded absolute growth budget",
    )?;
    require(
        rlib[2].saturating_sub(rlib[0]) <= 2 * 1024 * 1024,
        "message-heavy thumb rlib exceeded absolute growth budget",
    )?;
    require(
        ig256
            <= ig64
                .checked_mul(6)
                .and_then(|n| n.checked_add(16384))
                .ok_or("image growth limit overflow")?,
        "message-heavy thumb image growth became superlinear",
    )?;
    require(
        rg256
            <= rg64
                .checked_mul(6)
                .and_then(|n| n.checked_add(65536))
                .ok_or("rlib growth limit overflow")?,
        "message-heavy thumb rlib growth became superlinear",
    )
}
fn run() -> Result<()> {
    match argument(1)?.as_str() {
        "observation" => {
            let text = fs::read_to_string(argument(2)?).map_err(|e| e.to_string())?;
            let (seconds, rss) = observation(&text, &argument(3)?)?;
            println!("{seconds} {rss}");
            Ok(())
        }
        "llvm-image" => {
            println!("{}", llvm_image(&stdin_text()?)?);
            Ok(())
        }
        "file-size" => {
            println!(
                "{}",
                fs::metadata(argument(2)?).map_err(|e| e.to_string())?.len()
            );
            Ok(())
        }
        "message-budget" => {
            let mut values = [0u64; 6];
            for (i, value) in values.iter_mut().enumerate() {
                *value = argument(i + 2)?.parse().map_err(|_| "invalid byte count")?;
            }
            message_budget(
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            )
        }
        "llvm-sections" => llvm_sections(&stdin_text()?, &argument(2)?),
        "nonempty-test" => nonempty_test(&stdin_text()?),
        "aeabi" => aeabi(&var("BIN_PATH")?, &var("LLVM_NM")?),
        "role-count" => {
            print!("{}", role_count(&argument(2)?)?);
            Ok(())
        }
        "host-project" => write_project(&argument(2)?, &argument(3)?, None),
        "artifact-project" => write_project(&argument(2)?, &argument(3)?, Some(&argument(4)?)),
        "thumb" => thumb(&snapshot()?, &var("THUMB_SECTION_OUTPUT")?),
        "layout" => layout(&var("FUTURE_LAYOUT_OUTPUT")?),
        "matrix" => matrix(&var("PROTOCOL_MATRIX_OUTPUT")?),
        "protocol-metrics" => {
            protocol_metrics(&var("PROTOCOL_MATRIX_OUTPUT")?, &var("PROTOCOL_NAME")?)
        }
        "map" => map_report(&var("MAP_PATH")?, &var("BIN_PATH")?, &var("LLVM_NM")?),
        "artifacts" => artifacts(&var("PROTOCOL_ARTIFACT_OUTPUT")?),
        "runtime" => runtime(
            &snapshot()?,
            &var("THUMB_SECTION_OUTPUT")?,
            &var("STACK_HIGH_WATER_OUTPUT")?,
            env::var("HIBANA_MODELED_RUNTIME_SRAM_MIN_HEADROOM_BYTES")
                .unwrap_or_else(|_| "64".into())
                .parse()
                .map_err(|_| "invalid SRAM headroom")?,
        ),
        "live" => live(
            &var("THUMB_SECTION_OUTPUT")?,
            &var("PROTOCOL_ARTIFACT_OUTPUT")?,
            &var("STACK_HIGH_WATER_OUTPUT")?,
            &var("MEASUREMENT_HOST")?,
        ),
        _ => Err("unknown final-measurements operation".into()),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("final-form measurement violation: {error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_measurements_never_wrap() {
        assert!(total([u64::MAX, 1]).is_err());
        assert!(sections("thumb section name=.text bytes=18446744073709551615\nthumb section name=.rodata bytes=1").is_err());
        assert!(llvm_sections(".text 18446744073709551615\n.rodata 1\n", "artifact").is_err());
        assert!(llvm_image(".text 18446744073709551615\n.rodata 1\n").is_err());
        assert!(metrics("bytes=18446744073709551616").is_err());
        assert!(message_budget([0, u64::MAX, 0], [0, 0, 0]).is_err());
        let mut rows = BTreeMap::from([(
            "huge".into(),
            BTreeMap::from([
                ("session_kit_storage_bytes".into(), u64::MAX),
                ("resident_prefix_bytes".into(), 1),
            ]),
        )]);
        assert!(add_sram(&mut rows, 0, false).is_err());
    }

    #[test]
    fn observations_and_growth_fail_closed() {
        let good = "compile pressure observed: case elapsed=2s seconds_budget=10s max_rss=6MiB";
        assert_eq!(
            observation(good, "compile pressure observed: case ").unwrap(),
            (2, 6)
        );
        for bad in [
            "",
            "compile pressure observed: case elapsed=2s max_rss=6MiB",
            "compile pressure observed: case elapsed=bad seconds_budget=10s max_rss=6MiB",
        ] {
            assert!(observation(bad, "compile pressure observed: case ").is_err());
        }
        assert_eq!(
            llvm_image(".text.one 12\n.rodata 4\n.data 999\n__const 3").unwrap(),
            19
        );
        assert!(llvm_image(".text invalid").is_err());
        assert!(message_budget([100, 200, 300], [100, 200, 300]).is_ok());
        assert!(message_budget([300, 200, 100], [300, 200, 100]).is_ok());
        assert!(message_budget([0, 0, 16385], [0, 0, 0]).is_err());
        assert!(message_budget([0, 0, 0], [0, 0, 65537]).is_err());
        assert!(message_budget([0, 100000, 524289], [0, 0, 0]).is_err());
        assert!(message_budget([0, 0, 0], [0, 1000000, 2097153]).is_err());
        assert!(nonempty_test("running 2 tests").is_ok());
        assert!(nonempty_test("running 0 tests").is_err());
        assert!(role_count("unknown").is_err());
    }

    #[test]
    fn parsing_and_missing_values() {
        assert_eq!(
            metrics("hello k=12 n_2=4 tail").unwrap(),
            BTreeMap::from([("k".into(), 12), ("n_2".into(), 4)])
        );
        assert!(layout("missing").is_err());
        assert!(matrix("protocol-matrix name=minimal_send_recv").is_err());
        assert!(artifacts("").is_err());
        assert_eq!(
            sections("thumb section name=.text bytes=20\nthumb section name=.data bytes=5")
                .unwrap()["flash_total"],
            25
        );
        assert_eq!(commas(1234567), "1,234,567");
    }
    #[test]
    fn protocol_budget_and_mutants() {
        let good=PROTOCOLS.iter().map(|name|format!("protocol-matrix name={name} program_blob_len=16 role_blob_len=32 endpoint_scratch_bytes=512 largest_section_bytes=192\n")).collect::<String>();
        assert!(matrix(&good).is_ok());
        assert!(matrix(&good.replace("program_blob_len=16", "program_blob_len=257")).is_err());
        assert!(matrix(&good.replace("role_blob_len=32", "missing=32")).is_err());
        assert!(matrix(&good.lines().skip(1).collect::<Vec<_>>().join("\n")).is_err());
    }
    #[test]
    fn future_layout_payload_independence() {
        let w = std::mem::size_of::<usize>();
        let mut line = format!(
            "future-layout Endpoint={} RouteBranch={} OfferFuture={} RecvFuture={} BranchRecvFuture={} SendFuture={} RawOfferFuture={} RawRecvFuture={} OfferFutureLease=1 RecvFutureLease=1",
            3 * w,
            2 * w,
            3 * w,
            3 * w,
            3 * w,
            5 * w,
            2 * w,
            2 * w
        );
        for base in ["RecvFuture", "BranchRecvFuture"] {
            for suffix in ["U8", "U64", "Bytes"] {
                line.push_str(&format!(" {base}{suffix}={}", 3 * w));
            }
        }
        assert!(layout(&line).is_ok());
        assert!(
            layout(&line.replace(&format!("RecvFutureU64={}", 3 * w), "RecvFutureU64=999"))
                .is_err()
        );
    }
}
