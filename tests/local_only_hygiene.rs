use std::fs;
use std::path::{Path, PathBuf};

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()))
}

fn walk_rs_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(root).unwrap_or_else(|err| panic!("read_dir {} failed: {err}", root.display()))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("dir entry under {} failed: {err}", root.display()));
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn local_cell_helper_stays_forbidden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("tests/support/local_only.rs").exists(),
        "tests must not regrow a generic LocalCell helper"
    );
}

#[test]
fn tests_do_not_define_static_local_cell_state() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tests_root = root.join("tests");
    let mut files = Vec::new();
    walk_rs_files(&tests_root, &mut files);

    let static_kw = "static ";
    let cell_kw = "LocalCell<";
    for path in files {
        let source = read(&path);
        for line in source.lines() {
            assert!(
                !(line.contains(static_kw) && line.contains(cell_kw)),
                "tests must not keep shared static LocalCell state: {}: {}",
                path.display(),
                line.trim()
            );
        }
    }
}

#[test]
fn huge_runtime_helpers_do_not_use_generic_sync_cells() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    {
        let relative = "tests/huge_choreography_runtime.rs";
        let source = read(&root.join(relative));
        assert!(
            !source.contains("unsafe impl<T> Sync"),
            "huge choreography helpers must not contain blanket generic Sync impls: {relative}"
        );
        assert!(
            !source.contains("struct StaticCell"),
            "huge choreography helpers must not contain the generic StaticCell pattern: {relative}"
        );
    }
}
