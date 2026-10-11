//! Private, dependency-free compile-pressure gate utilities.
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, Read},
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
fn number(s: &str) -> Result<u64> {
    s.parse().map_err(|_| format!("invalid integer: {s:?}"))
}
fn budget(text: &str, label: Option<&str>, kind: &str) -> Result<u64> {
    const HEADER: &str =
        "label\tobserved_seconds\tobserved_rss_mib\tseconds_headroom\trss_headroom_mib";
    let mut lines = text
        .lines()
        .filter(|s| !s.starts_with('#') && !s.is_empty());
    if lines.next() != Some(HEADER) {
        return Err("invalid or missing budget columns".into());
    }
    let mut values = Vec::new();
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 5 {
            return Err(format!("invalid budget row: {line}"));
        }
        let nums = fields[1..]
            .iter()
            .map(|s| {
                number(s).and_then(|n| {
                    if n > 0 {
                        Ok(n)
                    } else {
                        Err("nonpositive budget".into())
                    }
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let (a, b) = match kind {
            "seconds" => (0, 2),
            "rss_mib" => (1, 3),
            _ => return Err("unknown limit kind".into()),
        };
        let value = nums[a].checked_add(nums[b]).ok_or("budget overflow")?;
        if label.is_none_or(|l| l == fields[0]) {
            values.push(value);
        }
    }
    if label.is_some() && values.len() != 1 {
        return Err(format!("expected one budget row, found {}", values.len()));
    }
    values
        .into_iter()
        .max()
        .ok_or("empty compile pressure budget".into())
}
#[derive(Debug)]
struct Row {
    parent: u64,
    rss: u64,
    command: String,
}
fn process_number(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid process-table integer".into());
    }
    number(value)
}
fn rows(text: &str, full: bool) -> Result<BTreeMap<u64, Row>> {
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next() else { continue };
        let parent = parts.next().ok_or("truncated process-table row")?;
        let (rss, command) = if full {
            let rss = parts.next().ok_or("truncated process-table RSS row")?;
            let command = parts.collect::<Vec<_>>().join(" ");
            if command.is_empty() {
                return Err("missing process-table command".into());
            }
            (process_number(rss)?, command)
        } else {
            if parts.next().is_some() {
                return Err("extra process-tree field".into());
            }
            (0, String::new())
        };
        let pid = process_number(pid)?;
        if pid == 0 {
            return Err("zero process-table PID".into());
        }
        if rows
            .insert(
                pid,
                Row {
                    parent: process_number(parent)?,
                    rss,
                    command,
                },
            )
            .is_some()
        {
            return Err("duplicate process-table PID".into());
        }
    }
    if rows.is_empty() {
        return Err("empty process table".into());
    }
    Ok(rows)
}
fn require_root(rows: &BTreeMap<u64, Row>, root: u64) -> Result<()> {
    if rows.contains_key(&root) {
        Ok(())
    } else {
        Err("missing requested root process".into())
    }
}

fn descendants(rows: &BTreeMap<u64, Row>, root: u64) -> Vec<u64> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    let mut found = Vec::new();
    while let Some(parent) = stack.pop() {
        for (&pid, row) in rows {
            if row.parent == parent && seen.insert(pid) {
                found.push(pid);
                stack.push(pid);
            }
        }
    }
    found
}
fn tool(command: &str) -> Option<&str> {
    let mut tokens = command.split_whitespace();
    let name = Path::new(tokens.next()?).file_name()?.to_str()?;
    if matches!(name, "cargo" | "rustc" | "rustdoc") {
        return Some(name);
    }
    if name == "rustup"
        && tokens.any(|t| {
            Path::new(t)
                .file_name()
                .is_some_and(|n| n == "cargo" || n == "rustc" || n == "rustdoc")
        })
    {
        return Some("rustup");
    }
    None
}
fn crate_matches(command: &str, name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "--crate-name" {
            return tokens.next() == Some(name);
        }
        if let Some(value) = token.strip_prefix("--crate-name=") {
            return value == name;
        }
    }
    false
}
fn scan(text: &str, root: u64, limit: u64, name: &str) -> Result<(String, i32)> {
    scan_rows(&rows(text, true)?, root, limit, name)
}
fn scan_rows(
    rows: &BTreeMap<u64, Row>,
    root: u64,
    limit: u64,
    name: &str,
) -> Result<(String, i32)> {
    require_root(rows, root)?;
    let mut ids: BTreeSet<_> = descendants(rows, root).into_iter().collect();
    ids.insert(root);
    let mut total = 0u64;
    let mut peak = None;
    let mut matched = false;
    for pid in ids {
        let Some(row) = rows.get(&pid) else { continue };
        let Some(tool) = tool(&row.command) else {
            continue;
        };
        if !crate_matches(&row.command, name) {
            continue;
        }
        matched = true;
        total = total.checked_add(row.rss).ok_or("RSS overflow")?;
        if peak.is_none_or(|(_, rss, _)| row.rss > rss) {
            peak = Some((pid, row.rss, tool));
        }
        if row.rss > limit {
            return Ok((
                format!(
                    "process pid={pid} rss_mib={} total_rss_mib={} comm={tool} matched=1",
                    row.rss.div_ceil(1024),
                    total.div_ceil(1024)
                ),
                0,
            ));
        }
    }
    if total > limit {
        let (pid, rss, tool) = peak.ok_or("missing aggregate peak")?;
        Ok((
            format!(
                "aggregate total_rss_mib={} peak_pid={pid} peak_rss_mib={} peak_comm={tool} matched=1",
                total.div_ceil(1024),
                rss.div_ceil(1024)
            ),
            0,
        ))
    } else {
        Ok((
            format!(
                "ok total_rss_mib={} matched={}",
                total.div_ceil(1024),
                u8::from(matched)
            ),
            7,
        ))
    }
}
fn process_table(full: bool) -> Result<String> {
    let columns = if full {
        "pid=,ppid=,rss=,command="
    } else {
        "pid=,ppid="
    };
    let output = std::process::Command::new("ps")
        .args(["-axo", columns])
        .output()
        .map_err(|e| format!("failed to read process table: {e}"))?;
    if !output.status.success() {
        return Err("failed to read process table".into());
    }
    String::from_utf8(output.stdout).map_err(|e| format!("invalid process table: {e}"))
}
fn stop_tree(root: u64) -> Result<()> {
    // The root may have exited since the violating sample. Still terminate any
    // descendants present in this valid snapshot, then attempt the owned root.
    let tree = process_table(false).and_then(|text| rows(&text, false));
    let mut ids = match &tree {
        Ok(table) => descendants(table, root),
        Err(_) => Vec::new(),
    };
    ids.reverse();
    ids.retain(|&pid| pid != root);
    ids.push(root);
    // Preserve SIGTERM and descendant-before-root ordering. Processes that have
    // already exited are harmless; inability to launch kill is an infrastructure error.
    for pid in ids {
        let _ = std::process::Command::new("kill")
            .args(["--", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("failed to stop process tree: {e}"))?;
    }
    tree.map(|_| ())
}
fn optional_env(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn decimal(value: &str, positive: bool) -> Result<u64> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || positive && value.starts_with('0')
    {
        return Err(format!(
            "invalid {} limit: {value}",
            if positive { "RSS MiB" } else { "time seconds" }
        ));
    }
    number(value)
}
fn limits(root: &Path) -> Result<(u64, u64)> {
    let label = optional_env("HIBANA_COMPILE_PRESSURE_LABEL")?;
    let path = optional_env("HIBANA_COMPILE_PRESSURE_BUDGETS")?
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join(".github/measurement_snapshots/hibana-compile-pressure-budget.tsv")
        });
    let rss = match optional_env("HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB")? {
        Some(value) => decimal(&value, true)?,
        None => budget(
            &fs::read_to_string(&path).map_err(|e| e.to_string())?,
            label.as_deref(),
            "rss_mib",
        )?,
    };
    let seconds = match optional_env("HIBANA_COMPILE_PRESSURE_MAX_SECONDS")? {
        Some(value) => decimal(&value, false)?,
        None => match label {
            None => 0,
            Some(label) => budget(
                &fs::read_to_string(&path).map_err(|e| e.to_string())?,
                Some(&label),
                "seconds",
            )?,
        },
    };
    Ok((rss.checked_mul(1024).ok_or("RSS limit overflow")?, seconds))
}
fn poll_duration(value: &str) -> Result<std::time::Duration> {
    let (value, multiplier) = match value.as_bytes().last() {
        Some(b's') => (&value[..value.len() - 1], 1.0),
        Some(b'm') => (&value[..value.len() - 1], 60.0),
        Some(b'h') => (&value[..value.len() - 1], 3600.0),
        Some(b'd') => (&value[..value.len() - 1], 86400.0),
        _ => (value, 1.0),
    };
    let seconds = value.parse::<f64>().map_err(|e| e.to_string())? * multiplier;
    std::time::Duration::try_from_secs_f64(seconds).map_err(|e| e.to_string())
}
fn epoch_seconds() -> Result<i128> {
    Ok(i128::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs(),
    ))
}
#[derive(Default)]
struct Observation {
    maximum_mib: u64,
    active_start: Option<i128>,
    elapsed: i128,
}
impl Observation {
    fn observe(&mut self, report: &str, now: i128, start: i128, scoped: bool) -> Result<()> {
        let field = report
            .split_whitespace()
            .find_map(|s| s.strip_prefix("total_rss_mib="))
            .ok_or("missing total RSS")?;
        self.maximum_mib = self.maximum_mib.max(number(field)?);
        if scoped {
            if report.split_whitespace().any(|s| s == "matched=1") {
                let active = *self.active_start.get_or_insert(now);
                self.elapsed = self.elapsed.max(now - active);
            } else {
                self.active_start = None;
            }
        } else {
            self.elapsed = now - start;
        }
        Ok(())
    }
    fn finish(&mut self, now: i128, start: i128, scoped: bool) {
        if scoped {
            if let Some(active) = self.active_start {
                self.elapsed = self.elapsed.max(now - active);
            }
        } else {
            self.elapsed = now - start;
        }
    }
}
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}
fn guard(root: &Path, label: &str, command: &[String]) -> Result<i32> {
    let (max_kib, max_seconds) = limits(root)?;
    let name = optional_env("HIBANA_COMPILE_PRESSURE_CRATE_NAME")?.unwrap_or_default();
    let interval = poll_duration(
        &optional_env("HIBANA_COMPILE_PRESSURE_POLL_SECONDS")?.unwrap_or_else(|| "1".into()),
    )?;
    let start = epoch_seconds()?;
    let mut observation = Observation::default();
    let mut child = match std::process::Command::new(&command[0])
        .args(&command[1..])
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("compile pressure guard command failed: {error}");
            return Ok(if error.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            });
        }
    };
    let supervised = (|| -> Result<(i32, bool)> {
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Ok((exit_code(status), true));
            }
            let now = epoch_seconds()?;
            let table = rows(&process_table(true)?, true)?;
            let root = u64::from(child.id());
            if !table.contains_key(&root) {
                // ps can race normal child exit. Only the owned Child handle,
                // never an absent or malformed table alone, proves completion.
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    return Ok((exit_code(status), true));
                }
            }
            let (report, status) = scan_rows(&table, root, max_kib, &name)?;
            observation.observe(&report, now, start, !name.is_empty())?;
            if status == 0 {
                eprintln!(
                    "compile pressure guard violation: {label} exceeded {}MiB RSS: {report}",
                    max_kib / 1024
                );
                return Ok((137, false));
            }
            if max_seconds > 0 && observation.elapsed > i128::from(max_seconds) {
                eprintln!(
                    "compile pressure guard violation: {label} exceeded {max_seconds}s elapsed time"
                );
                return Ok((124, false));
            }
            std::thread::sleep(interval);
        }
    })();
    let status = match supervised {
        Ok((status, true)) => status,
        interrupted => {
            let stopped = stop_tree(u64::from(child.id()));
            // If process-table/kill execution fails, still reap our direct child.
            if stopped.is_err() {
                let _ = child.kill();
            }
            let waited = child.wait().map_err(|e| e.to_string());
            stopped?;
            waited?;
            return interrupted.map(|(status, _)| status);
        }
    };
    observation.finish(epoch_seconds()?, start, !name.is_empty());
    println!(
        "compile pressure observed: {label} elapsed={}s seconds_budget={max_seconds}s max_rss={}MiB rss_budget={}MiB",
        observation.elapsed,
        observation.maximum_mib,
        max_kib / 1024
    );
    Ok(status)
}

fn run(args: &[String]) -> Result<i32> {
    let usage = "usage: compile-pressure guard REPO LABEL COMMAND [ARG...] | limit PATH LABEL seconds|rss_mib | max-rss PATH | scan ROOT LIMIT CRATE | descendants ROOT | offender ROOT LIMIT | live-descendants ROOT";
    match args.get(1).map(String::as_str) {
        Some("guard") if args.len() >= 5 => {
            return guard(Path::new(&args[2]), &args[3], &args[4..]);
        }
        Some("offender") if args.len() == 4 => {
            let name = optional_env("HIBANA_COMPILE_PRESSURE_CRATE_NAME")?.unwrap_or_default();
            let (report, status) = scan(
                &process_table(true)?,
                number(&args[2])?,
                number(&args[3])?,
                &name,
            )?;
            println!("{report}");
            return Ok(if status == 7 { 1 } else { status });
        }
        Some("live-descendants") if args.len() == 3 => {
            let table = rows(&process_table(false)?, false)?;
            let root = number(&args[2])?;
            require_root(&table, root)?;
            for pid in descendants(&table, root).into_iter().rev() {
                println!("{pid}");
            }
        }

        Some("limit") if args.len() == 5 => {
            let text = fs::read_to_string(&args[2]).map_err(|e| e.to_string())?;
            println!("{}", budget(&text, Some(&args[3]), &args[4])?);
        }
        Some("max-rss") if args.len() == 3 => {
            let text = fs::read_to_string(&args[2]).map_err(|e| e.to_string())?;
            println!("{}", budget(&text, None, "rss_mib")?);
        }
        Some("scan") if args.len() == 5 => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            let (output, status) = scan(&text, number(&args[2])?, number(&args[3])?, &args[4])?;
            println!("{output}");
            return Ok(status);
        }
        Some("descendants") if args.len() == 3 => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            let table = rows(&text, false)?;
            let root = number(&args[2])?;
            require_root(&table, root)?;
            for pid in descendants(&table, root).into_iter().rev() {
                println!("{pid}");
            }
        }
        _ => return Err(usage.into()),
    }
    Ok(0)
}
fn main() {
    match run(&env::args().collect::<Vec<_>>()) {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("compile pressure guard failed: {error}");
            std::process::exit(2);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_windows_are_not_accumulated() {
        let mut observation = Observation::default();
        observation
            .observe("ok total_rss_mib=2 matched=1", 10, 0, true)
            .unwrap();
        observation
            .observe("ok total_rss_mib=3 matched=1", 14, 0, true)
            .unwrap();
        observation
            .observe("ok total_rss_mib=0 matched=0", 15, 0, true)
            .unwrap();
        observation
            .observe("ok total_rss_mib=1 matched=1", 30, 0, true)
            .unwrap();
        observation.finish(32, 0, true);
        assert_eq!(observation.elapsed, 4);
        assert_eq!(observation.maximum_mib, 3);
        observation.finish(36, 0, true);
        assert_eq!(observation.elapsed, 6);
    }
    #[test]
    fn unscoped_time_and_strict_limits() {
        let mut observation = Observation::default();
        observation
            .observe("ok total_rss_mib=0 matched=0", 11, 10, false)
            .unwrap();
        assert_eq!(observation.elapsed, 1); // Equal to the budget is admitted.
        observation.finish(12, 10, false);
        assert_eq!(observation.elapsed, 2);
        for invalid in ["", "0", "01", "-1", "+1", "1.0", "18446744073709551616"] {
            assert!(decimal(invalid, true).is_err());
        }
        assert_eq!(decimal("00", false), Ok(0));
        assert_eq!(decimal("1", true), Ok(1));
        for invalid in ["nan", "inf", "-1", "bad"] {
            assert!(poll_duration(invalid).is_err());
        }
        assert_eq!(poll_duration("0.1").unwrap().as_millis(), 100);
        assert_eq!(poll_duration("1m").unwrap().as_secs(), 60);
    }
    #[test]
    fn accounting_includes_nested_tools_but_not_unrelated_trees() {
        let text = "1 0 1 bash\n2 1 1 sh\n3 2 400 rustc --crate-name a\n4 2 500 rustdoc --crate-name a\n5 1 600 cargo build\n6 0 9000 rustc --crate-name a\n";
        assert_eq!(scan(text, 1, 1500, "").unwrap().1, 7);
        assert_eq!(scan(text, 1, 1499, "").unwrap().1, 0);
        assert_eq!(scan(text, 1, 900, "a").unwrap().1, 7);
        assert_eq!(scan(text, 1, 899, "a").unwrap().1, 0);
        assert_eq!(descendants(&rows(text, true).unwrap(), 1).len(), 4);
    }
    #[test]
    fn process_tables_reject_truncation_duplicates_and_empty_input() {
        for bad in [
            "",
            " \n\t",
            "garbage",
            "1",
            "1 0",
            "1 0 7",
            "1 0 7 ",
            "1 x 7 rustc",
            "+1 0 7 rustc",
            "1 0 +7 rustc",
            "0 0 7 rustc",
            "1 0 7 rustc\n1 0 8 cargo",
            "1 0 7 rustc\n2 1",
        ] {
            assert!(rows(bad, true).is_err(), "accepted {bad:?}");
        }
        for bad in ["", "garbage", "1", "1 0 extra", "1 0\n1 0", "1 +0", "0 0"] {
            assert!(rows(bad, false).is_err(), "accepted {bad:?}");
        }
        assert_eq!(rows("\n1 0 7 rustc\n\n", true).unwrap().len(), 1);
        assert_eq!(rows("\n1 0\n\n", false).unwrap().len(), 1);
        assert!(scan("2 0 1 cargo", 1, 100, "").is_err());
        assert!(scan("1 0 1 bash\n2 1", 1, 100, "").is_err());
        assert!(require_root(&rows("2 0", false).unwrap(), 1).is_err());
    }
    #[test]
    fn budgets_fail_closed() {
        let head =
            "label\tobserved_seconds\tobserved_rss_mib\tseconds_headroom\trss_headroom_mib\n";
        let good = format!("{head}gate\t1\t2\t3\t4\n");
        assert_eq!(budget(&good, Some("gate"), "seconds"), Ok(4));
        assert_eq!(budget(&good, None, "rss_mib"), Ok(6));
        assert!(budget(&good, Some("absent"), "seconds").is_err());
        assert!(
            budget(
                &(good.clone() + "gate\t1\t2\t3\t4\n"),
                Some("gate"),
                "seconds"
            )
            .is_err()
        );
        for bad in [
            "",
            head,
            "wrong\n",
            &format!("{head}gate\t0\t2\t3\t4\n"),
            &format!("{head}gate\t1\t2\t3\t4\textra\n"),
        ] {
            assert!(budget(bad, None, "rss_mib").is_err());
        }
    }
    #[test]
    fn process_and_aggregate_limits() {
        let text = "1 0 5 bash\n2 1 700 /bin/rustc --crate-name hibana\n3 2 700 rustdoc --crate-name=hibana\n4 0 9000 rustc --crate-name hibana\n";
        assert_eq!(
            scan(text, 1, 700, "hibana").unwrap().0,
            "aggregate total_rss_mib=2 peak_pid=2 peak_rss_mib=1 peak_comm=rustc matched=1"
        );
        assert!(
            scan(text, 1, 699, "hibana")
                .unwrap()
                .0
                .starts_with("process pid=2")
        );
        assert_eq!(
            scan(text, 1, 1400, "hibana").unwrap(),
            ("ok total_rss_mib=2 matched=1".into(), 7)
        );
        assert_eq!(
            scan(text, 1, 0, "other").unwrap(),
            ("ok total_rss_mib=0 matched=0".into(), 7)
        );
    }
    #[test]
    fn large_table_and_cycles() {
        let text = format!(
            "1 0 1 bash\n2 1 1024 rustc --crate-name hibana {}\n",
            "x".repeat(300_000)
        );
        assert_eq!(
            scan(&text, 1, 1048576, "").unwrap(),
            ("ok total_rss_mib=1 matched=1".into(), 7)
        );
        assert_eq!(descendants(&rows("1 2\n2 1\n", false).unwrap(), 1).len(), 2);
        assert!(scan("2 1 invalid rustc", 1, 1, "").is_err());
        assert_eq!(tool("rustup run stable cargo test"), Some("rustup"));
        assert_eq!(tool("echo rustc"), None);
    }
}
