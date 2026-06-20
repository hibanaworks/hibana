use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("repo test manifest must live at .github/repo-tests");
    println!("cargo:rustc-env=HIBANA_REPO_ROOT={}", root.display());
}
