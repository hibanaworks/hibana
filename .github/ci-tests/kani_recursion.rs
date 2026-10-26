//! Receive proofs use fixed source scopes and arbitrary actor identifiers.
//! Loop bounds exhaust all rows; per-function bounds reflect the source nesting
//! and actor decisions. CBMC exhaustion assertions remain enabled, so an
//! insufficient bound fails rather than truncating a proof.
use super::cargo;
use crate::{
    Audit,
    kani_json::{self, Json},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const PREFIX: &str = "global::const_dsl::receive_lane_causality::actors::kani::";

pub(super) struct Group {
    pub label: &'static str,
    pub harnesses: Vec<String>,
    pub recursion: Option<String>,
}

fn family() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for word in 0..4 {
        names.insert(format!("{PREFIX}owned_linear_seed_word{word}"));
    }
    names.insert(format!(
        "{PREFIX}repeated_inner_choices_are_independent_while_outer_choices_stay_shared"
    ));
    names
}

fn inventory_names(inventory: &Json, total: u64) -> Audit<BTreeSet<String>> {
    let Some(Json::Object(files)) = inventory.get("standard-harnesses") else {
        return Err("Kani inventory is missing compiled harness names".into());
    };
    let mut names = BTreeSet::new();
    for items in files.values() {
        let Json::Array(items) = items else {
            return Err("invalid harness name list".into());
        };
        for item in items {
            let Json::String(name) = item else {
                return Err("invalid compiled harness name".into());
            };
            if !names.insert(name.clone()) {
                return Err("duplicate compiled harness name".into());
            }
        }
    }
    if names.len() as u64 != total {
        return Err("Kani inventory names/count disagree".into());
    }
    Ok(names)
}

fn partition(names: BTreeSet<String>) -> Audit<Vec<Group>> {
    let owned: BTreeSet<_> = names
        .iter()
        .filter(|name| name.starts_with(PREFIX))
        .cloned()
        .collect();
    if !owned.is_empty() && owned != family() {
        return Err(
            "receive proof family must retain every seed partition and the repeated-choice proof"
                .into(),
        );
    }
    let mut groups = ["ordinary", "owned-linear", "parallel-reentry"].map(|label| Group {
        label,
        harnesses: Vec::new(),
        recursion: None,
    });
    for name in names {
        let part = match name.strip_prefix(PREFIX) {
            Some(name) if name.starts_with("owned_linear_seed_word") => 1,
            _ if name.starts_with("global::typestate::cursor::parallel_reentry_tests::") => 2,
            _ => 0,
        };
        groups[part].harnesses.push(name);
    }
    Ok(groups
        .into_iter()
        .flat_map(|group| {
            if group.label == "parallel-reentry" {
                // Keep code generation and its reachable callback set local to
                // each proof, as well as solving its paths separately.
                group
                    .harnesses
                    .into_iter()
                    .map(|name| Group {
                        label: "parallel-reentry",
                        harnesses: vec![name],
                        recursion: None,
                    })
                    .collect::<Vec<_>>()
            } else if group.harnesses.is_empty() {
                Vec::new()
            } else {
                vec![group]
            }
        })
        .collect())
}

fn metadata_files(root: &Path, files: &mut Vec<PathBuf>) -> Audit<()> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            metadata_files(&entry.path(), files)?;
        } else if kind.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .ends_with(".kani-metadata.json")
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn function_names() -> Vec<String> {
    let root = "global::const_dsl::receive_lane_causality";
    vec![
        format!("{root}::CausalFlow::<'_, 3>::advance_as::<0>"),
        format!("{root}::actors::Obligation::<'_, 3, 0>::conflict_from"),
        format!("{root}::actors::ActorMap::<'_, 3, 0>::reference_known"),
        "global::const_dsl::source_arena::ScopeMarkerView::<'_>::offset_lower_bound".into(),
    ]
}

fn collect_functions(map: Json, wanted: &[String]) -> Audit<BTreeMap<String, String>> {
    let Json::Object(map) = map else {
        return Err("invalid compiler function-name map".into());
    };
    let mut functions = BTreeMap::new();
    for (symbol, value) in map {
        let Json::String(name) = value else { continue };
        if wanted.contains(&name) {
            if !symbol.starts_with("_R")
                || !symbol
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err("invalid compiled recursive function identifier".into());
            }
            if functions.insert(name, symbol).is_some() {
                return Err("ambiguous compiled function".into());
            }
        }
    }
    if functions.len() != wanted.len() {
        return Err("missing compiled recursive function".into());
    }
    Ok(functions)
}

fn read_json(path: &Path) -> Audit<Json> {
    kani_json::parse(&fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
}

fn bounds(target: &Path) -> Audit<String> {
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    metadata_files(&target, &mut files)?;
    let wanted_family = family();
    let mut result: Option<String> = None;
    for file in files {
        let metadata = read_json(&file)?;
        let Some(Json::Array(harnesses)) = metadata.get("proof_harnesses") else {
            continue;
        };
        let names: BTreeSet<_> = harnesses
            .iter()
            .filter_map(|h| match h.get("pretty_name") {
                Some(Json::String(name)) => Some(name.clone()),
                _ => None,
            })
            .collect();
        if names != wanted_family || harnesses.len() != wanted_family.len() {
            continue;
        }
        for harness in harnesses {
            let attributes = harness
                .get("attributes")
                .ok_or("missing compiled proof attributes")?;
            if attributes.get("kind") != Some(&Json::String("Proof".into()))
                || attributes.get("should_panic") != Some(&Json::Bool(false))
                || attributes.get("unwind_value") != Some(&Json::Number("5".into()))
            {
                return Err(
                    "receive proofs must retain ordinary assertions and the five-step loop bound"
                        .into(),
                );
            }
            for kind in ["stubs", "verified_stubs"] {
                if !matches!(attributes.get(kind), Some(Json::Array(items)) if items.is_empty()) {
                    return Err(
                        "receive proofs must check the actual unstubbed implementation".into(),
                    );
                }
            }
        }
        let name = format!("{PREFIX}owned_linear_seed_word0");
        let harness = harnesses
            .iter()
            .find(|h| h.get("pretty_name") == Some(&Json::String(name.clone())))
            .ok_or("missing representative compiled harness")?;
        let Some(Json::String(goto)) = harness.get("goto_file") else {
            return Err("missing compiled GOTO path".into());
        };
        let stem = goto
            .strip_suffix(".symtab.out")
            .ok_or("invalid compiled GOTO path")?;
        let map = PathBuf::from(format!("{stem}.pretty_name_map.json"))
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !map.starts_with(&target) {
            return Err("compiled name map outside proof target".into());
        }
        let names = function_names();
        let functions = collect_functions(read_json(&map)?, &names)?;
        // The linear source has neither nested scopes nor actor decisions.
        // A zero recursion bound asserts that those recursive calls are absent.
        // The empty marker search still checks its loop's exhaustion.
        let mut limits = names[..3]
            .iter()
            .map(|name| format!("{}:0", functions[name]))
            .collect::<Vec<_>>();
        limits.push(format!("{}.0:1", functions[&names[3]]));
        let compiled = limits.join(",");
        if result
            .as_ref()
            .is_some_and(|previous| previous != &compiled)
        {
            return Err("inconsistent compiled recursive function identities".into());
        }
        result = Some(compiled);
    }
    result.ok_or_else(|| "missing complete code-generated receive proof family".into())
}

pub(super) fn prepare(
    root: &Path,
    target: &Path,
    inventory: &str,
    total: u64,
) -> Audit<Vec<Group>> {
    let names = inventory_names(&kani_json::parse(inventory)?, total)?;
    let reviewed = fs::read_to_string(root.join("proofs/kani/harness-inventory.json"))
        .map_err(|e| e.to_string())?;
    let version =
        fs::read_to_string(root.join(".github/kani-version")).map_err(|e| e.to_string())?;
    let reviewed_total = kani_json::inventory_total(&reviewed, version.trim())?;
    let expected = inventory_names(&kani_json::parse(&reviewed)?, reviewed_total)?;
    if names != expected {
        return Err("compiled Kani harnesses differ from the reviewed inventory".into());
    }
    let mut groups = partition(names)?;
    if groups.iter().any(|group| group.label.starts_with("owned-")) {
        let status = cargo(root, target)
            .args(["--only-codegen", "--harness", PREFIX.trim_end_matches("::")])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("receive proof code generation failed: {status}"));
        }
        let linear = bounds(target)?;
        for group in &mut groups {
            if group.label == "owned-linear" {
                group.recursion = Some(linear.clone());
            }
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires pinned Kani and actual production code generation"]
    fn production_metadata_retains_the_reviewed_harnesses_and_recursive_bounds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let scratch = super::super::Scratch::create().unwrap();
        let target = scratch.0.join("target");
        assert!(
            cargo(&root, &target)
                .args(["list", "--format", "json"])
                .current_dir(&scratch.0)
                .status()
                .unwrap()
                .success()
        );
        let inventory = fs::read_to_string(scratch.0.join("kani-list.json")).unwrap();
        let version = fs::read_to_string(root.join(".github/kani-version")).unwrap();
        let total = kani_json::inventory_total(&inventory, version.trim()).unwrap();
        let reviewed = fs::read_to_string(root.join("proofs/kani/harness-inventory.json")).unwrap();
        let reviewed_total = kani_json::inventory_total(&reviewed, version.trim()).unwrap();
        let expected =
            inventory_names(&kani_json::parse(&reviewed).unwrap(), reviewed_total).unwrap();
        let groups = prepare(&root, &target, &inventory, total).unwrap();
        let actual: BTreeSet<_> = groups
            .iter()
            .flat_map(|group| group.harnesses.iter().cloned())
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(
            groups
                .iter()
                .map(|group| group.harnesses.len())
                .sum::<usize>() as u64,
            total
        );
        for group in groups {
            assert_eq!(group.recursion.is_some(), group.label.starts_with("owned-"));
        }
    }
    #[test]
    fn partition_retains_all_harnesses_without_duplicating_ordinary_proofs() {
        let mut names = family();
        names.insert("ordinary_proof".into());
        names.insert("global::typestate::cursor::parallel_reentry_tests::proof_a".into());
        names.insert("global::typestate::cursor::parallel_reentry_tests::proof_b".into());
        let groups = partition(names.clone()).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.harnesses.len()).collect::<Vec<_>>(),
            [2, 4, 1, 1]
        );
        assert_eq!(
            groups
                .into_iter()
                .flat_map(|g| g.harnesses)
                .collect::<BTreeSet<_>>(),
            names
        );
        for missing in family() {
            let mut incomplete = family();
            incomplete.remove(&missing);
            assert!(partition(incomplete).is_err());
        }
    }
    #[test]
    fn names_must_match_the_inventory_count_and_be_unique() {
        let value = kani_json::parse(r#"{"standard-harnesses":{"file":["a","b"]}}"#).unwrap();
        assert!(inventory_names(&value, 2).is_ok());
        assert!(inventory_names(&value, 3).is_err());
        let duplicate = kani_json::parse(r#"{"standard-harnesses":{"file":["a","a"]}}"#).unwrap();
        assert!(inventory_names(&duplicate, 2).is_err());
    }
    #[test]
    fn compiled_function_maps_reject_missing_ambiguous_and_invalid_identifiers() {
        let wanted = function_names();
        let valid = || {
            wanted
                .iter()
                .enumerate()
                .map(|(i, name)| (format!("_Rfunction{i}"), Json::String(name.clone())))
                .collect::<BTreeMap<_, _>>()
        };
        assert!(collect_functions(Json::Object(valid()), &wanted).is_ok());
        let mut missing = valid();
        missing.remove("_Rfunction0");
        assert!(collect_functions(Json::Object(missing), &wanted).is_err());
        let mut duplicate = valid();
        duplicate.insert("_Ranother".into(), Json::String(wanted[0].clone()));
        assert!(collect_functions(Json::Object(duplicate), &wanted).is_err());
        let mut invalid = valid();
        invalid.insert("bad:identifier".into(), Json::String(wanted[0].clone()));
        assert!(collect_functions(Json::Object(invalid), &wanted).is_err());
    }
}
