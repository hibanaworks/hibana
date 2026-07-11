use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn read(path: &str) -> String {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let full = root.join(path);
    read_plain(&full)
}

pub(crate) fn repo_file_exists(path: &str) -> bool {
    PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join(path)
        .exists()
}

pub(crate) fn read_plain(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()))
}

fn is_test_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|part| part == "tests" || part == "test_support")
            .unwrap_or(false)
    })
}

pub(crate) fn read_production_dir_rs(path: &str) -> String {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join(path);
    let mut parts = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", root.display()))
                .path()
        })
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("rs") && !is_test_source(path)
        })
        .collect::<Vec<_>>();
    parts.sort();
    let mut source = String::new();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

fn read_rs_tree_filtered(path: &str, include_tests: bool) -> String {
    fn collect_rs_files(dir: &Path, include_tests: bool, parts: &mut Vec<PathBuf>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_rs_files(&path, include_tests, parts);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                && (include_tests || !is_test_source(&path))
            {
                parts.push(path);
            }
        }
    }

    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join(path);
    let mut parts = Vec::new();
    collect_rs_files(&root, include_tests, &mut parts);
    parts.sort();
    let mut source = String::new();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

pub(crate) fn read_production_rs_tree(path: &str) -> String {
    read_rs_tree_filtered(path, false)
}

pub(crate) fn production_rs_files(path: &str) -> Vec<String> {
    fn collect_rs_files(dir: &Path, parts: &mut Vec<PathBuf>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_rs_files(&path, parts);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                && !is_test_source(&path)
            {
                parts.push(path);
            }
        }
    }

    let manifest =
        PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let root = manifest.join(path);
    let mut parts = Vec::new();
    collect_rs_files(&root, &mut parts);
    parts.sort();
    parts
        .into_iter()
        .map(|part| {
            part.strip_prefix(&manifest)
                .expect("test path must stay under manifest dir")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

pub(crate) fn read_all_rs_tree(path: &str) -> String {
    read_rs_tree_filtered(path, true)
}

pub(crate) fn read_all_rs_tree_except(path: &str, excluded_roots: &[&str]) -> String {
    fn collect_rs_files(dir: &Path, parts: &mut Vec<PathBuf>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_rs_files(&path, parts);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                parts.push(path);
            }
        }
    }

    let manifest =
        PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let root = manifest.join(path);
    let mut parts = Vec::new();
    collect_rs_files(&root, &mut parts);
    parts.sort();
    let mut source = String::new();
    for part in parts {
        let relative = part
            .strip_prefix(&manifest)
            .expect("test path must stay under manifest dir")
            .to_string_lossy()
            .replace('\\', "/");
        if excluded_roots.iter().any(|candidate| {
            *candidate == relative
                || relative
                    .strip_prefix(candidate)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            continue;
        }
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

pub(crate) fn read_tree_except(path: &str, excluded: &[&str]) -> String {
    fn collect_files(dir: &Path, parts: &mut Vec<PathBuf>) {
        let entries =
            fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_files(&path, parts);
            } else {
                parts.push(path);
            }
        }
    }

    let manifest =
        PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let root = manifest.join(path);
    let mut parts = Vec::new();
    collect_files(&root, &mut parts);
    parts.sort();
    let mut source = String::new();
    for part in parts {
        let relative = part
            .strip_prefix(&manifest)
            .expect("test path must stay under manifest dir")
            .to_string_lossy()
            .replace('\\', "/");
        if excluded.iter().any(|candidate| *candidate == relative) {
            continue;
        }
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

pub(crate) fn cluster_core_source() -> String {
    let mut source = read("src/session/cluster/core.rs");
    source.push_str(&read_production_rs_tree("src/session/cluster/core"));
    source
}

pub(crate) fn endpoint_kernel_core_source() -> String {
    let mut source = read("src/endpoint/kernel/core.rs");
    source.push_str(&read_production_rs_tree("src/endpoint/kernel/core"));
    source
}

pub(crate) fn endpoint_kernel_source() -> String {
    let mut source = read("src/endpoint/kernel/mod.rs");
    source.push_str(&read_production_rs_tree("src/endpoint/kernel"));
    source
}

pub(crate) fn lowering_driver_source() -> String {
    let mut source = read("src/global/compiled/lowering/driver.rs");
    source.push_str(&read_production_rs_tree(
        "src/global/compiled/lowering/driver",
    ));
    source
}

pub(crate) fn compiled_image_source() -> String {
    let mut source = read("src/global/compiled/images/image.rs");
    source.push_str(&read_production_rs_tree("src/global/compiled/images/image"));
    source
}

pub(crate) fn runtime_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read_production_dir_rs("src/runtime"));
    source
}

pub(crate) fn endpoint_facade_source() -> String {
    let mut source = read("src/endpoint.rs");
    source.push_str(&read_production_dir_rs("src/endpoint"));
    source
}

pub(crate) fn offer_frontier_source() -> String {
    let mut source = read("src/endpoint/kernel/offer.rs");
    source.push_str(&read_production_dir_rs("src/endpoint/kernel/offer"));
    source
}

pub(crate) fn rendezvous_core_source() -> String {
    let mut source = read("src/rendezvous/core.rs");
    source.push_str(&read_production_dir_rs("src/rendezvous/core"));
    source
}

pub(crate) fn transport_source() -> String {
    let mut source = read("src/transport.rs");
    source.push_str(&read_production_dir_rs("src/transport"));
    source
}
