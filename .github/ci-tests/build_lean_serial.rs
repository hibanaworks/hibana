//! Build every local Lean module in import order, one Lake invocation at a time.
//! The subsequent proof gates still audit the complete theorem surface.
mod lean_audit;

use lean_audit::{AuditResult, erase_non_code, lean_files, read, require};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::{Command, ExitCode},
};

fn visit(
    module: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    active: &mut BTreeSet<String>,
    built: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) -> AuditResult<()> {
    if built.contains(module) {
        return Ok(());
    }
    require(
        active.insert(module.into()),
        format!("cyclic Lean import at {module}"),
    )?;
    for dependency in &graph[module] {
        require(
            graph.contains_key(dependency),
            format!("missing local Lean module {dependency}"),
        )?;
        visit(dependency, graph, active, built, order)?;
    }
    active.remove(module);
    built.insert(module.into());
    order.push(module.into());
    Ok(())
}

fn run(root: &Path, native: Option<&str>) -> AuditResult<()> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let mut files = lean_files(&root.join("Hibana"))?;
    files.push(root.join("Hibana.lean"));
    let mut graph = BTreeMap::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let module = relative
            .with_extension("")
            .components()
            .map(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .ok_or("non-UTF8 Lean module path")
                    .map(str::to_owned)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(".");
        let code = erase_non_code(&read(&file)?)?;
        let mut dependencies = BTreeSet::new();
        for line in code.lines() {
            let mut words = line.split_whitespace();
            let mut first = words.next();
            if matches!(first, Some("public" | "private")) {
                first = words.next();
            }
            if first != Some("import") {
                continue;
            }
            let imports: Vec<_> = words.collect();
            require(
                !imports.is_empty(),
                format!("unsupported multiline import in {}", file.display()),
            )?;
            for import in imports {
                if import == "Hibana" || import.starts_with("Hibana.") {
                    dependencies.insert(import.to_owned());
                }
            }
        }
        require(
            graph.insert(module.clone(), dependencies).is_none(),
            format!("duplicate Lean module {module}"),
        )?;
    }
    let mut order = Vec::new();
    let mut active = BTreeSet::new();
    let mut built = BTreeSet::new();
    if let Some(module) = native {
        require(
            graph.contains_key(module),
            format!("unknown native root {module}"),
        )?;
        visit(module, &graph, &mut active, &mut built, &mut order)?;
    } else {
        for module in graph.keys() {
            visit(module, &graph, &mut active, &mut built, &mut order)?;
        }
        require(order.len() == graph.len(), "incomplete Lean build order")?;
    }
    for (index, module) in order.iter().enumerate() {
        println!("Lean serial build {}/{} {module}", index + 1, order.len());
        let status = Command::new("lake")
            .arg("build")
            .arg(format!(
                "+{module}{}",
                if native.is_some() { ":o" } else { "" }
            ))
            .current_dir(&root)
            .status()
            .map_err(|error| error.to_string())?;
        require(
            status.success(),
            format!("Lean module {module} failed: {status}"),
        )?;
    }
    // Preserve the package's normal default-target check after all module
    // artifacts have been built. It cannot start independent uncached imports.
    if native.is_none() {
        let status = Command::new("lake")
            .arg("build")
            .current_dir(&root)
            .status()
            .map_err(|error| error.to_string())?;
        require(
            status.success(),
            format!("Lean default targets failed: {status}"),
        )?;
    }
    println!(
        "Lean serial module build passed modules={} native={}",
        order.len(),
        native.is_some()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let result = match args.as_slice() {
        [_, root] => run(Path::new(root), None),
        [_, root, flag, module] if flag == "--native" => module
            .to_str()
            .ok_or_else(|| "non-UTF8 native module".into())
            .and_then(|module| run(Path::new(root), Some(module))),
        _ => Err("usage: build_lean_serial PROOF_DIRECTORY [--native MODULE]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Lean serial build failed: {error}");
            ExitCode::FAILURE
        }
    }
}
