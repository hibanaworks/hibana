use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    read_plain(&full)
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
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
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

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
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

pub(crate) fn read_all_rs_tree(path: &str) -> String {
    read_rs_tree_filtered(path, true)
}

pub(crate) fn capability_token_source() -> String {
    let mut source = read("src/control/cap/mint.rs");
    source.push_str(&read_production_dir_rs("src/control/cap/mint"));
    source
}

pub(crate) fn cluster_core_source() -> String {
    let mut source = read("src/control/cluster/core.rs");
    source.push_str(&read_production_rs_tree("src/control/cluster/core"));
    source
}

pub(crate) fn endpoint_kernel_core_source() -> String {
    let mut source = read("src/endpoint/kernel/core.rs");
    source.push_str(&read_production_rs_tree("src/endpoint/kernel/core"));
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

pub(crate) fn integration_source() -> String {
    let mut source = read("src/integration.rs");
    source.push_str(&read_production_dir_rs("src/integration"));
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

fn read_test_with_modules(root_path: &str, module_dir: &str) -> String {
    let mut source = read(root_path);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(module_dir);
    let mut parts = Vec::new();
    collect_test_rs_files(&root, &mut parts);
    parts.sort();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

fn collect_test_rs_files(dir: &Path, parts: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_test_rs_files(&path, parts);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            parts.push(path);
        }
    }
}

pub(crate) fn cursor_send_recv_tests_source() -> String {
    read_test_with_modules("tests/cursor_send_recv.rs", "tests/cursor_send_recv")
}

pub(crate) fn read_offer_tests() -> String {
    let mut source = read("src/endpoint/kernel/test_support/core_offer_tests.rs");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/endpoint/kernel/test_support/core_offer_tests");
    let mut parts = Vec::new();
    collect_test_rs_files(&root, &mut parts);
    parts.retain(|path| path.file_name().and_then(|name| name.to_str()) != Some("mod.rs"));
    parts.sort();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
}

pub(crate) fn lines(path: &str) -> Vec<String> {
    read(path)
        .lines()
        .map(normalize_ws)
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn normalize_ws(input: impl AsRef<str>) -> String {
    let mut normalized = String::new();
    let mut first = true;
    for part in input.as_ref().split_whitespace() {
        if !first {
            normalized.push(' ');
        }
        first = false;
        normalized.push_str(part);
    }
    normalized
}

pub(crate) fn control_op_variants() -> Vec<String> {
    let mint = capability_token_source();
    let body = mint
        .split_once("pub enum ControlOp {")
        .and_then(|(_, tail)| tail.split_once("\n}").map(|(body, _)| body))
        .expect("ControlOp enum must stay in mint.rs");

    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("///") || line.starts_with("#[") {
                return None;
            }
            let variant = line
                .split_once('=')
                .map_or(line, |(name, _)| name)
                .trim()
                .trim_end_matches(',');
            variant
                .chars()
                .next()
                .filter(|ch| ch.is_ascii_uppercase())
                .map(|_| variant.to_string())
        })
        .collect()
}
