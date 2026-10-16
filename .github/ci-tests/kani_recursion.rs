//! Bounded receive proofs have one binary scope and one actor decision.
//! Loop bounds and recursion bounds are separate: the former exhaust four
//! rows, the latter check one nested call. CBMC unwinding assertions remain
//! enabled, so an insufficient bound fails rather than truncating a proof.
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

const ROUTE_PARTITIONS: [(&str, &str); 8] = [
    ("route-direct-0", "owned_route_direct_seed_word0"),
    ("route-direct-1", "owned_route_direct_seed_word1"),
    ("route-direct-2", "owned_route_direct_seed_word2"),
    ("route-direct-3", "owned_route_direct_seed_word3"),
    ("route-swapped-0", "owned_route_swapped_seed_word0"),
    ("route-swapped-1", "owned_route_swapped_seed_word1"),
    ("route-swapped-2", "owned_route_swapped_seed_word2"),
    ("route-swapped-3", "owned_route_swapped_seed_word3"),
];

pub(super) fn validate_selection(selection: Option<&str>) -> Audit<()> {
    match selection {
        None | Some("common") => Ok(()),
        Some(name) if ROUTE_PARTITIONS.iter().any(|(label, _)| *label == name) => Ok(()),
        Some(name) => Err(format!("unknown Kani partition: {name}")),
    }
}

pub(super) struct Group {
    pub label: &'static str,
    pub harnesses: Vec<String>,
    pub recursion: Option<String>,
}

/// Partition execution only after the full compiled inventory and bounds have
/// been checked. Each symbolic query retains its original inputs and checks.
pub(super) fn select(mut groups: Vec<Group>, selection: Option<&str>) -> Audit<Vec<Group>> {
    validate_selection(selection)?;
    let Some(selection) = selection else {
        return Ok(groups);
    };
    let route_names: BTreeSet<_> = ROUTE_PARTITIONS
        .iter()
        .map(|(_, suffix)| format!("{PREFIX}{suffix}"))
        .collect();
    let actual: BTreeSet<_> = groups
        .iter()
        .flat_map(|group| &group.harnesses)
        .filter(|name| route_names.contains(*name))
        .cloned()
        .collect();
    if actual != route_names {
        return Err("partitioned Kani requires the complete compiled route query family".into());
    }
    let selected = ROUTE_PARTITIONS
        .iter()
        .find(|(label, _)| *label == selection)
        .map(|(_, suffix)| format!("{PREFIX}{suffix}"));
    for group in &mut groups {
        group.harnesses.retain(|name| match &selected {
            Some(selected) => name == selected,
            None => !route_names.contains(name),
        });
    }
    groups.retain(|group| !group.harnesses.is_empty());
    if groups.is_empty() {
        return Err("Kani partition cannot be empty".into());
    }
    Ok(groups)
}

fn family() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for kind in ["linear", "parallel", "route_direct", "route_swapped"] {
        for word in 0..4 {
            names.insert(format!("{PREFIX}owned_{kind}_seed_word{word}"));
        }
    }
    for name in [
        "source_scan_route_direct",
        "source_scan_route_swapped",
        "source_scan_parallel_direct",
        "source_scan_parallel_swapped",
        "repeated_inner_choices_are_independent_while_outer_choices_stay_shared",
    ] {
        names.insert(format!("{PREFIX}{name}"));
    }
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
            "receive proof family must retain every seed partition and both source shapes".into(),
        );
    }
    let mut groups =
        ["ordinary", "owned-linear", "owned-parallel", "owned-route"].map(|label| Group {
            label,
            harnesses: Vec::new(),
            recursion: None,
        });
    for name in names {
        let part = match name.strip_prefix(PREFIX) {
            Some(name) if name.starts_with("owned_linear_seed_word") => 1,
            Some(name) if name.starts_with("owned_parallel_seed_word") => 2,
            Some(name) if name.starts_with("owned_route_") || name.starts_with("source_scan_") => 3,
            _ => 0,
        };
        groups[part].harnesses.push(name);
    }
    Ok(groups
        .into_iter()
        .filter(|group| !group.harnesses.is_empty())
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

fn function_names(events: usize, decisions: usize) -> Vec<String> {
    let root = "global::const_dsl::receive_lane_causality";
    let mut names = vec![
        format!("{root}::CausalFlow::<'_, {events}>::advance_as::<{decisions}>"),
        format!("{root}::actors::Obligation::<'_, {events}, {decisions}>::check"),
        format!("{root}::actors::ActorMap::<'_, {events}, {decisions}>::reference_known"),
    ];
    if decisions == 1 {
        names.push(format!(
            "{root}::actors::ActorMap::<'_, {events}, {decisions}>::receiver"
        ));
        names.push("g::source::ownership::queries::Context::<'_>::get".into());
    }
    names
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

fn bounds(root: &Path) -> Audit<[String; 3]> {
    let target = root
        .join("target/kani")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    metadata_files(&target, &mut files)?;
    let wanted_family = family();
    let mut result: Option<[String; 3]> = None;
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
        let mut compiled = Vec::new();
        for (kind, events, decisions) in
            [("linear", 3, 0), ("parallel", 8, 0), ("route_direct", 8, 1)]
        {
            let name = format!("{PREFIX}owned_{kind}_seed_word0");
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
            let names = function_names(events, decisions);
            let functions = collect_functions(read_json(&map)?, &names)?;
            let mut limits = names[..3]
                .iter()
                .map(|name| format!("{}:1", functions[name]))
                .collect::<Vec<_>>();
            if decisions == 1 {
                // One actor decision needs at most one receiver-DAG step.
                // Contexts have an empty base and at most one linked choice.
                // The loop guards and their exhaustion are still checked.
                limits.push(format!("{}.0:2", functions[&names[3]]));
                limits.push(format!("{}.0:2", functions[&names[4]]));
                limits.push(format!("{}.1:2", functions[&names[4]]));
            }
            compiled.push(limits.join(","));
        }
        let compiled: [String; 3] = compiled.try_into().map_err(|_| "recursive proof groups")?;
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

pub(super) fn prepare(root: &Path, inventory: &str, total: u64) -> Audit<Vec<Group>> {
    let mut groups = partition(inventory_names(&kani_json::parse(inventory)?, total)?)?;
    if groups.iter().any(|group| group.label != "ordinary") {
        let status = cargo(root)
            .args(["--only-codegen", "--harness", PREFIX.trim_end_matches("::")])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("receive proof code generation failed: {status}"));
        }
        let [linear, parallel, route] = bounds(root)?;
        for group in &mut groups {
            group.recursion = match group.label {
                "owned-linear" => Some(linear.clone()),
                "owned-parallel" => Some(parallel.clone()),
                "owned-route" => Some(route.clone()),
                _ => None,
            };
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_partitions_cover_the_full_inventory_exactly_once() {
        let mut names = family();
        for index in 0..237 {
            names.insert(format!("ordinary_proof_{index}"));
        }
        assert_eq!(names.len(), 258);
        let mut covered = BTreeSet::new();
        for selection in
            std::iter::once("common").chain(ROUTE_PARTITIONS.iter().map(|(name, _)| *name))
        {
            let mut groups = partition(names.clone()).unwrap();
            for group in &mut groups {
                group.recursion = (group.label != "ordinary").then(|| "original-bounds".into());
            }
            let selected = select(groups, Some(selection)).unwrap();
            assert_eq!(
                selected
                    .iter()
                    .map(|group| group.harnesses.len())
                    .sum::<usize>(),
                if selection == "common" { 250 } else { 1 }
            );
            for group in selected {
                assert_eq!(
                    group.recursion.as_deref(),
                    (group.label != "ordinary").then_some("original-bounds")
                );
                for name in group.harnesses {
                    assert!(covered.insert(name), "duplicate proof across partitions");
                }
            }
        }
        assert_eq!(covered, names);
        let all = select(partition(names.clone()).unwrap(), None).unwrap();
        assert_eq!(
            all.into_iter()
                .flat_map(|group| group.harnesses)
                .collect::<BTreeSet<_>>(),
            names
        );
    }

    #[test]
    fn execution_partition_rejects_unknown_or_missing_compiled_queries() {
        for unknown in [
            "",
            "all",
            "route-direct-4",
            "route-swapped--1",
            "common-extra",
        ] {
            assert!(select(partition(family()).unwrap(), Some(unknown)).is_err());
        }
        for (_, missing) in ROUTE_PARTITIONS {
            let mut groups = partition(family()).unwrap();
            for group in &mut groups {
                group
                    .harnesses
                    .retain(|name| name != &format!("{PREFIX}{missing}"));
            }
            assert!(select(groups, Some("common")).is_err());
        }
        assert!(
            select(
                partition(BTreeSet::from(["ordinary".into()])).unwrap(),
                Some("common")
            )
            .is_err()
        );
    }
    #[test]
    fn partition_retains_all_harnesses_without_duplicating_ordinary_proofs() {
        let mut names = family();
        names.insert("ordinary_proof".into());
        let groups = partition(names.clone()).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.harnesses.len()).collect::<Vec<_>>(),
            [2, 4, 4, 12]
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
        let wanted = function_names(8, 1);
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
