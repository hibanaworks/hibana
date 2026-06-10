use super::common::*;

fn collect_rs_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
    {
        let path = entry
            .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

#[test]
fn repo_test_support_modules_are_not_orphaned() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tests_root = root.join("tests");
    let support_root = tests_root.join("support");
    let tests_source = read_all_rs_tree("tests");
    let mut support_files = Vec::new();
    collect_rs_files(&support_root, &mut support_files);
    support_files.sort();

    for path in support_files {
        let relative = path
            .strip_prefix(&tests_root)
            .expect("support file must be under tests")
            .to_string_lossy()
            .replace('\\', "/");
        let marker = format!("#[path = \"{relative}\"]");
        assert!(
            tests_source.contains(&marker),
            "repo test support module must be referenced by #[path] or deleted: {relative}"
        );
    }
}

#[test]
fn public_surface_and_gates_do_not_retain_legacy_role_token_api() {
    let legacy_role_token = concat!("g::", "Role<");
    for (name, source) in [
        ("production source", read_production_rs_tree("src")),
        ("README", read("README.md")),
        (
            "size snapshot gate",
            read(".github/scripts/check_size_snapshot_regression.sh"),
        ),
        (
            "surface hygiene gate",
            read(".github/scripts/check_surface_hygiene.sh"),
        ),
        (
            "final form measurement gate",
            read(".github/scripts/check_final_form_measurements.sh"),
        ),
    ] {
        assert!(
            !source.contains(legacy_role_token),
            "{name} must not retain legacy public role-token API residue"
        );
    }
}
