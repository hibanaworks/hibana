//! Strict libtest/Miri result validation shared by private repository gates.
use std::{collections::BTreeSet, env, fs, io::Write, path::Path};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counts {
    passed: u64,
    ignored: u64,
}
fn count(text: &str) -> Result<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid count: {text:?}"));
    }
    text.parse().map_err(|_| format!("count overflow: {text}"))
}
fn sum(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or_else(|| "test count overflow".into())
}
fn value(field: &str, suffix: &str) -> Result<u64> {
    count(
        field
            .strip_suffix(suffix)
            .ok_or_else(|| format!("malformed test summary field: {field}"))?,
    )
}
fn parse(log: &str) -> Result<Counts> {
    let running: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with("running "))
        .collect();
    let summaries: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with("test result:"))
        .collect();
    if running.len() != 1 || summaries.len() != 1 {
        return Err("expected exactly one running count and one test summary".into());
    }
    let running = running[0].strip_prefix("running ").unwrap();
    let running = count(
        running
            .strip_suffix(" tests")
            .or_else(|| running.strip_suffix(" test"))
            .ok_or("malformed running count")?,
    )?;
    let fields: Vec<_> = summaries[0]
        .strip_prefix("test result: ok. ")
        .ok_or("unsuccessful or malformed test summary")?
        .split("; ")
        .collect();
    if fields.len() != 6 {
        return Err("malformed test summary field count".into());
    }
    let passed = value(fields[0], " passed")?;
    let failed = value(fields[1], " failed")?;
    let ignored = value(fields[2], " ignored")?;
    let measured = value(fields[3], " measured")?;
    let _filtered = value(fields[4], " filtered out")?;
    let elapsed = fields[5]
        .strip_prefix("finished in ")
        .and_then(|s| s.strip_suffix('s'))
        .ok_or("malformed elapsed time")?
        .parse::<f64>()
        .map_err(|_| "invalid elapsed time")?;
    if !elapsed.is_finite() || elapsed < 0.0 {
        return Err("invalid elapsed time".into());
    }
    if failed != 0 || passed == 0 || running == 0 {
        return Err("requires a nonempty successful test run".into());
    }
    if running != sum(sum(sum(passed, failed)?, ignored)?, measured)? {
        return Err("running count differs from result counts".into());
    }
    Ok(Counts { passed, ignored })
}
fn successful(statuses: &[String]) -> Result<()> {
    if statuses.is_empty() || statuses.iter().any(|s| count(s) != Ok(0)) {
        return Err(format!("test tool or output capture failed: {statuses:?}"));
    }
    Ok(())
}
fn observation(log: &str, label: &str) -> Result<String> {
    let prefix = format!("compile pressure observed: runtime {label} ");
    let lines: Vec<_> = log
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .collect();
    if lines.len() != 1 {
        return Err(format!(
            "expected one aggregate compile pressure observation for {label}"
        ));
    }
    let fields: Vec<_> = lines[0].split_whitespace().collect();
    if fields.len() != 4 {
        return Err("malformed compile pressure observation".into());
    }
    let expected = [
        ("elapsed=", "s"),
        ("seconds_budget=", "s"),
        ("max_rss=", "MiB"),
        ("rss_budget=", "MiB"),
    ];
    for (field, (prefix, suffix)) in fields.iter().zip(expected) {
        count(
            field
                .strip_prefix(prefix)
                .and_then(|v| v.strip_suffix(suffix))
                .ok_or("malformed compile pressure quantity")?,
        )?;
    }
    Ok(format!(
        "runtime compile pressure label={label} {}",
        fields.join(" ")
    ))
}
fn totals(text: &str) -> Result<(BTreeSet<String>, Counts)> {
    let mut labels = BTreeSet::new();
    let mut total = Counts {
        passed: 0,
        ignored: 0,
    };
    for line in text.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 3 || fields[0].is_empty() || !labels.insert(fields[0].to_owned()) {
            return Err("malformed or duplicate Miri total record".into());
        }
        let passed = count(fields[1])?;
        let ignored = count(fields[2])?;
        if passed == 0 {
            return Err("empty Miri total record".into());
        }
        total.passed = sum(total.passed, passed)?;
        total.ignored = sum(total.ignored, ignored)?;
    }
    Ok((labels, total))
}
fn record(path: &Path, label: &str, counts: Counts) -> Result<()> {
    if label.is_empty() || label.contains(['\t', '\n', '\r']) {
        return Err("invalid Miri label".into());
    }
    let previous = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (labels, total) = totals(&previous)?;
    if labels.contains(label) {
        return Err(format!("duplicate Miri suite: {label}"));
    }
    sum(total.passed, counts.passed)?;
    sum(total.ignored, counts.ignored)?;
    writeln!(
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?,
        "{label}\t{}\t{}",
        counts.passed,
        counts.ignored
    )
    .map_err(|e| e.to_string())
}
fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("check") if args.len() == 4 => {
            successful(&args[3..])?; let log = fs::read_to_string(&args[1]).map_err(|e|e.to_string())?;
            parse(&log).map_err(|e|format!("{}: {e}",args[2]))?;
        },
        Some("runtime") if args.len() == 5 => {
            successful(&args[3..])?; let log = fs::read_to_string(&args[1]).map_err(|e|e.to_string())?;
            parse(&log).map_err(|e|format!("{}: {e}",args[2]))?;
            println!("{}",observation(&log,&args[2])?);
        },
        Some("miri") if args.len() == 5 => {
            successful(&args[4..])?; let log = fs::read_to_string(&args[1]).map_err(|e|e.to_string())?;
            let counts = parse(&log).map_err(|e|format!("{}: {e}",args[2]))?;
            record(Path::new(&args[3]),&args[2],counts)?;
        },
        Some("miri-total") if args.len() == 3 => {
            let (labels,counts) = totals(&fs::read_to_string(&args[2]).map_err(|e|e.to_string())?)?;
            if labels.is_empty() { return Err("no completed Miri suites".into()); }
            println!("miri gate passed toolchain={} tests={} ignored={}",args[1],counts.passed,counts.ignored);
        },
        _ => return Err("usage: test-result check LOG LABEL STATUS | runtime LOG LABEL CARGO_STATUS CAPTURE_STATUS | miri LOG LABEL TOTALS STATUS | miri-total TOOLCHAIN TOTALS".into()),
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
    const GOOD: &str = "running 3 tests\ntest alpha ... ok\ntest beta ... ok\ntest gamma ... ignored\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 12 filtered out; finished in 0.02s\n";
    #[test]
    fn successful_fixture_and_singular_run() {
        assert_eq!(
            parse(GOOD).unwrap(),
            Counts {
                passed: 2,
                ignored: 1
            }
        );
        assert_eq!(parse("running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n").unwrap(),Counts {passed:1,ignored:0});
    }
    #[test]
    fn malformed_failed_ambiguous_and_zero_fixtures_rejected() {
        for bad in [
            String::new(),
            format!("{GOOD}{GOOD}"),
            GOOD.replace("running 3 tests", "running 0 tests"),
            GOOD.replace("2 passed", "0 passed"),
            GOOD.replace("0 failed", "1 failed"),
            GOOD.replace("result: ok.", "result: FAILED."),
            GOOD.replace("running 3 tests", "running 9 tests"),
            GOOD.replace("2 passed", "999999999999999999999999 passed"),
            GOOD.replace("1 ignored", "x ignored"),
            GOOD.replace("; 0 measured", ""),
            GOOD.replace("0.02s", "NaNs"),
            GOOD.replace("0.02s", "-1s"),
            format!(
                "{GOOD}test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
            ),
            format!("running 1 test\n{GOOD}"),
        ] {
            assert!(parse(&bad).is_err(), "accepted invalid fixture: {bad}");
        }
    }
    #[test]
    fn errors_from_test_tools_and_output_capture_are_fatal() {
        assert!(successful(&["0".into(), "0".into()]).is_ok());
        for statuses in [
            vec![],
            vec!["1".into()],
            vec!["124".into()],
            vec!["0".into(), "9".into()],
            vec!["garbage".into()],
        ] {
            assert!(successful(&statuses).is_err());
        }
    }
    #[test]
    fn compile_observation_is_unique_complete_and_numeric() {
        let good = "compile pressure observed: runtime fixture elapsed=1s seconds_budget=30s max_rss=2MiB rss_budget=100MiB\n";
        assert_eq!(
            observation(good, "fixture").unwrap(),
            "runtime compile pressure label=fixture elapsed=1s seconds_budget=30s max_rss=2MiB rss_budget=100MiB"
        );
        for bad in [
            "".into(),
            format!("{good}{good}"),
            good.replace("elapsed=1s", "elapsed=oops"),
            good.replace("rss_budget=100MiB", ""),
            good.replace("runtime fixture", "runtime other"),
        ] {
            assert!(observation(&bad, "fixture").is_err());
        }
    }
    #[test]
    fn miri_totals_are_checked_and_suite_labels_unique() {
        assert_eq!(
            totals("one\t2\t1\ntwo\t3\t4\n").unwrap().1,
            Counts {
                passed: 5,
                ignored: 5
            }
        );
        for bad in [
            "one\t0\t0\n",
            "one\t1\t0\none\t2\t0\n",
            "one\t1\n",
            "one\t1\tbad\n",
            "one\t18446744073709551615\t0\ntwo\t1\t0\n",
        ] {
            assert!(totals(bad).is_err());
        }
    }
    #[test]
    fn absent_log_and_bad_cli_fail_closed() {
        assert!(run(&[]).is_err());
        assert!(
            run(&[
                "check".into(),
                "/definitely/missing/test-result.log".into(),
                "fixture".into(),
                "0".into()
            ])
            .is_err()
        );
    }
}
