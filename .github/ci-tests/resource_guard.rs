//! Host validation budget. This tool is outside Hibana's runtime and protocol.
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{File, OpenOptions},
    os::unix::{
        fs::OpenOptionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::PathBuf,
    process::{Child, Command},
    thread,
    time::Duration,
};

const STOP_BYTES: u64 = 3_000_000_000;
const INTERVAL: Duration = Duration::from_millis(100);
type Result<T> = std::result::Result<T, String>;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

#[cfg(target_os = "macos")]
mod macos {
    // macOS SDK sys/resource.h: rusage_info_v0, RUSAGE_INFO_V0 = 0.
    #[repr(C)]
    #[derive(Default)]
    struct Usage {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
    }
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Usage) -> i32;
    }
    pub(super) fn memory(pid: i32, rss: u64) -> u64 {
        let mut usage = Usage::default();
        // SAFETY: flavor 0 writes exactly the repr(C) v0 structure above.
        // The initialized output remains exclusively borrowed for this call.
        let result = unsafe { proc_pid_rusage(pid, 0, &mut usage) };
        if result == 0 {
            rss.max(usage.phys_footprint)
        } else {
            rss
        }
    }
}

struct Process {
    parent: i32,
    group: i32,
    rss: u64,
}

fn processes() -> Result<BTreeMap<i32, Process>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,rss="])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("cannot read validation process tree".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let mut table = BTreeMap::new();
    for line in text.lines() {
        let values = line.split_whitespace().collect::<Vec<_>>();
        if values.len() != 4 {
            return Err("invalid validation process table".into());
        }
        let pid = values[0].parse::<i32>().map_err(|e| e.to_string())?;
        let parent = values[1].parse::<i32>().map_err(|e| e.to_string())?;
        let group = values[2].parse::<i32>().map_err(|e| e.to_string())?;
        let rss = values[3]
            .parse::<u64>()
            .map_err(|e| e.to_string())?
            .checked_mul(1024)
            .ok_or("process memory overflow")?;
        if pid <= 0 || table.insert(pid, Process { parent, group, rss }).is_some() {
            return Err("invalid validation process identity".into());
        }
    }
    if table.is_empty() {
        return Err("empty validation process table".into());
    }
    Ok(table)
}

fn owned_processes(table: &BTreeMap<i32, Process>, root: i32) -> BTreeSet<i32> {
    let mut owned = BTreeSet::from([root]);
    loop {
        let before = owned.len();
        for (&pid, process) in table {
            // The private group also retains children reparented after exit.
            if process.group == root || owned.contains(&process.parent) {
                owned.insert(pid);
            }
        }
        if before == owned.len() {
            return owned;
        }
    }
}

fn memory(table: &BTreeMap<i32, Process>, root: i32) -> Result<u64> {
    owned_processes(table, root)
        .into_iter()
        .try_fold(0u64, |total, pid| {
            let Some(process) = table.get(&pid) else {
                return Ok(total);
            };
            #[cfg(target_os = "macos")]
            let bytes = macos::memory(pid, process.rss);
            #[cfg(not(target_os = "macos"))]
            let bytes = process.rss;
            total
                .checked_add(bytes)
                .ok_or_else(|| "aggregate process memory overflow".into())
        })
}

fn stop(child: &mut Child) -> Result<()> {
    let root = i32::try_from(child.id()).map_err(|e| e.to_string())?;
    // SAFETY: Command::process_group(0) created this private child group.
    // Only its members are stopped; the supervisor and other apps are outside it.
    unsafe {
        kill(-root, if cfg!(target_os = "macos") { 17 } else { 19 });
    }
    let table = processes();
    // SAFETY: same private child group; SIGKILL cannot be ignored by a solver.
    unsafe {
        kill(-root, 9);
    }
    if let Ok(table) = table {
        for pid in owned_processes(&table, root)
            .into_iter()
            .filter(|pid| table.get(pid).is_some_and(|process| process.group != root))
        {
            // SAFETY: these process identities belonged to the owned workload
            // in the immediately preceding snapshot, including nested groups.
            unsafe {
                kill(pid, 9);
            }
        }
    }
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

fn run(command: &[String], limit: u64, lock_path: PathBuf) -> Result<i32> {
    let lock: File = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("another local validation is active: {e}"))?;
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .env("CARGO_BUILD_JOBS", "1")
        .env("RUST_TEST_THREADS", "1")
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut maximum = 0;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break Ok(status.code().unwrap_or(128 + status.signal().unwrap_or(0)));
            }
            Ok(None) => (),
            Err(error) => break Err(error.to_string()),
        }
        match processes().and_then(|table| memory(&table, child.id() as i32)) {
            Ok(bytes) => {
                maximum = maximum.max(bytes);
                if bytes >= limit {
                    eprintln!(
                        "validation memory budget exceeded: bytes={bytes} stop_bytes={limit}"
                    );
                    break Ok(137);
                }
            }
            Err(error) => break Err(error),
        }
        thread::sleep(INTERVAL);
    };
    // Also remove any background child after an otherwise normal root exit.
    stop(&mut child)?;
    eprintln!("validation memory: maximum_bytes={maximum} stop_bytes={limit}");
    drop(lock);
    result
}

fn main() {
    let mut command = env::args().skip(1).collect::<Vec<_>>();
    if command.first().is_some_and(|arg| arg == "--") {
        command.remove(0);
    }
    if command.is_empty() {
        eprintln!("usage: resource-guard -- COMMAND [ARG...]");
        std::process::exit(2);
    }
    let lock = env::temp_dir().join("hibana-local-validation.lock");
    match run(&command, STOP_BYTES, lock) {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("validation memory guard failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_lock() -> PathBuf {
        env::temp_dir().join(format!(
            "hibana-memory-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn command_status_and_single_workload_lock_are_preserved() {
        let path = test_lock();
        let command = ["sh".into(), "-c".into(), "exit 31".into()];
        assert_eq!(run(&command, 64 * 1024 * 1024, path.clone()).unwrap(), 31);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        lock.lock().unwrap();
        assert!(
            run(&command, 64 * 1024 * 1024, path.clone())
                .unwrap_err()
                .contains("another local validation is active")
        );
        drop(lock);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn aggregate_children_are_killed_without_stopping_an_unrelated_process() {
        let path = test_lock();
        let mut unrelated = Command::new("sleep").arg("30").spawn().unwrap();
        let exe = env::current_exe()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap();
        let script = "\"$1\" --ignored --exact tests::memory_child --test-threads=1 & \
            \"$1\" --ignored --exact tests::memory_child --test-threads=1 & wait";
        let command = [
            "sh".into(),
            "-c".into(),
            script.into(),
            "children".into(),
            exe,
        ];
        let result = run(&command, 32 * 1024 * 1024, path.clone());
        let unrelated_still_live = unrelated.try_wait().unwrap().is_none();
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(result.unwrap(), 137);
        assert!(unrelated_still_live);
    }

    #[test]
    #[ignore = "allocation fixture, run only as a guarded child"]
    fn memory_child() {
        let mut bytes = vec![0u8; 20 * 1024 * 1024];
        let mut random = 0x1234_5678u32;
        for byte in &mut bytes {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *byte = random as u8;
        }
        std::hint::black_box(&bytes);
        thread::sleep(Duration::from_secs(30));
        std::hint::black_box(bytes);
    }
}
