use super::common::*;
use std::{fs, path::PathBuf, process::Command};

fn named_struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name} {{");
    let tail = source
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("{name} struct must stay visible"));
    tail.split("\n}")
        .next()
        .unwrap_or_else(|| panic!("{name} struct body must stay visible"))
}

fn frame_header_impl(source: &str) -> &str {
    let impl_start = source
        .find("impl FrameHeader {")
        .expect("FrameHeader impl block");
    let tail = &source[impl_start..];
    let open = tail.find('{').expect("FrameHeader impl open");
    let mut depth = 0usize;
    for (idx, byte) in tail[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..open + idx + 1];
                }
            }
            _ => {}
        }
    }
    panic!("FrameHeader impl close");
}

fn run_script(script: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(root.join(script))
        .env("CARGO_BUILD_JOBS", "1")
        .env("RUST_TEST_THREADS", "1")
        .output()
        .unwrap_or_else(|err| panic!("run {script} failed: {err}"));
    assert!(
        output.status.success(),
        "{script} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn production_sources_do_not_reintroduce_static_hygiene_residue() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        concat!("#[", "allow(dead_code)]"),
        "legacy",
        "heuristic",
        "fallback",
        "infer",
        "absorbed",
        "keeps_waiting",
        "DiscardedAndPending",
        "mismatch must stay pending",
        "mismatch discard",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not re-grow static hygiene residue: {forbidden}"
        );
    }

    let mut inline_in_attr_group = false;
    for line in production.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            if trimmed == "#[inline]" {
                assert!(
                    !inline_in_attr_group,
                    "production source must not duplicate #[inline] in one attribute group"
                );
                inline_in_attr_group = true;
            }
        } else if !trimmed.is_empty() {
            inline_in_attr_group = false;
        }
    }

    assert!(
        !production.contains("wake_by_ref();\n        return Poll::Pending;"),
        "transport mismatch paths must not wake and return Pending"
    );
    assert!(
        !production.contains("wake_by_ref();\n            Poll::Pending"),
        "transport mismatch paths must not wake and stay Pending"
    );
}

#[test]
fn frame_header_has_no_u64_storage() {
    let transport = read("src/transport.rs");
    let header_impl = frame_header_impl(&transport);
    assert!(
        transport.contains("pub struct FrameHeader([u8; 8]);"),
        "FrameHeader must stay an eight-byte carrier observation"
    );
    assert!(
        !header_impl.contains("u64") && !header_impl.contains("1u64"),
        "FrameHeader impl must not re-grow u64 packing or extraction"
    );
}

#[test]
fn runtime_public_api_has_no_frame_header_u64_raw() {
    let transport = read("src/transport.rs");
    let allowlist = read(".github/allowlists/runtime-public-api.txt");
    let header_impl = frame_header_impl(&transport);
    assert!(
        header_impl.contains("pub const fn from_bytes(bytes: [u8; 8]) -> Self")
            && header_impl.contains("pub const fn bytes(self) -> [u8; 8]"),
        "FrameHeader public surface must stay byte-based"
    );
    for forbidden in [
        "FrameHeader::from_raw",
        "FrameHeader::raw",
        "pub const fn from_raw(",
        "pub const fn raw(",
        "raw_header",
    ] {
        assert!(
            !header_impl.contains(forbidden) && !allowlist.contains(forbidden),
            "runtime public API must not expose FrameHeader u64 raw surface: {forbidden}"
        );
    }
}

#[test]
fn transport_docs_do_not_teach_u64_header() {
    let docs = read("README.md");
    for forbidden in [
        "FrameHeader::from_raw",
        "raw_header",
        "u64 observation",
        "carrier-owned `u64`",
    ] {
        assert!(
            !docs.contains(forbidden),
            "transport docs must not teach u64 FrameHeader construction: {forbidden}"
        );
    }
    assert!(
        docs.contains("FrameHeader::from_bytes(header_bytes)"),
        "transport docs must teach byte-based FrameHeader construction"
    );
}

fn assert_frame_label_mask_byte_limb_storage(labels: &str) {
    let body = named_struct_body(labels, "FrameLabelMask");
    assert!(
        labels.contains("#[repr(transparent)]"),
        "FrameLabelMask must stay a transparent byte-limb wrapper"
    );
    assert_eq!(
        body.trim(),
        "limbs: [u8; 32],",
        "FrameLabelMask must stay a fixed [u8; 32] mask"
    );
    assert!(
        labels.contains("limbs[(frame_label >> 3) as usize]"),
        "FrameLabelMask indexing must stay byte-limb based"
    );
    assert!(
        labels.contains("1u8 << (frame_label & 7)"),
        "FrameLabelMask bit construction must stay u8 based"
    );
}

fn assert_frame_label_mask_ops_use_no_u64(labels: &str) {
    for forbidden in [
        "u64", "1u64", "[u64;", "word0", "word1", "word2", "word3", ">> 6", "<< 6", "* 64", "/ 64",
    ] {
        assert!(
            !labels.contains(forbidden),
            "FrameLabelMask must not re-grow wide integer storage or helpers: {forbidden}"
        );
    }
}

#[test]
fn frame_label_mask_has_no_u64_storage() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_byte_limb_storage(&labels);
}

#[test]
fn frame_label_mask_ops_do_not_use_u64() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_ops_use_no_u64(&labels);
}

#[test]
fn scope_frame_label_meta_does_not_own_frame_label_masks() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let meta = named_struct_body(&evidence, "ScopeFrameLabelMeta");
    let masks = named_struct_body(&evidence, "ScopeFrameLabelMasks");
    let scratch = named_struct_body(&evidence, "ScopeFrameLabelScratch");
    let view = named_struct_body(&evidence, "ScopeFrameLabelView<'a>");

    for required in [
        "recv_frame_label: u8",
        "recv_arm: u8",
        "controller_frame_labels: [u8; 2]",
        "flags: u8",
    ] {
        assert!(
            meta.contains(required),
            "ScopeFrameLabelMeta must keep only scalar route facts: {required}"
        );
    }
    for forbidden in [
        "FrameLabelMask",
        "ScopeFrameLabelMasks",
        "limbs",
        "[u8; 32]",
    ] {
        assert!(
            !meta.contains(forbidden),
            "ScopeFrameLabelMeta must not own frame-label masks: {forbidden}"
        );
    }
    assert!(
        scratch.contains("masks: ScopeFrameLabelMasks")
            && view.contains("masks: &'a ScopeFrameLabelMasks"),
        "mask ownership must stay confined to scratch and borrowed through ScopeFrameLabelView"
    );
    assert_eq!(
        masks.trim(),
        "pub(super) arm_frame_label_masks: [FrameLabelMask; 2],",
        "ScopeFrameLabelMasks must not re-grow duplicate per-arm mask sets"
    );
}

#[test]
fn scope_frame_label_scratch_is_not_copy() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let scratch_prefix = evidence
        .split("pub(super) struct ScopeFrameLabelScratch")
        .next()
        .expect("evidence source");
    let scratch_attrs = scratch_prefix
        .rsplit("\n\n")
        .next()
        .expect("ScopeFrameLabelScratch attrs");
    assert!(
        !scratch_attrs.contains("Copy") && !scratch_attrs.contains("Clone"),
        "ScopeFrameLabelScratch must not be Clone/Copy"
    );
}

#[test]
fn scope_frame_label_masks_is_not_copy() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let masks_prefix = evidence
        .split("pub(super) struct ScopeFrameLabelMasks")
        .next()
        .expect("evidence source");
    let masks_attrs = masks_prefix
        .rsplit("\n\n")
        .next()
        .expect("ScopeFrameLabelMasks attrs");
    assert!(
        !masks_attrs.contains("Copy") && !masks_attrs.contains("Clone"),
        "ScopeFrameLabelMasks must not be Clone/Copy"
    );
}

#[test]
fn no_by_value_scope_frame_label_meta_in_hot_paths() {
    let hot_path = [
        read("src/endpoint/kernel/core/frontier_helpers.rs"),
        read("src/endpoint/kernel/core/scope_evidence_logic.rs"),
        read("src/endpoint/kernel/offer.rs"),
        read("src/endpoint/kernel/offer/facts.rs"),
        read("src/endpoint/kernel/offer/passive.rs"),
        read("src/endpoint/kernel/offer/select.rs"),
    ]
    .join("\n");
    for forbidden in [
        "ScopeFrameLabelMasks::EMPTY",
        "frame_label_masks",
        "frame_label_meta: &ScopeFrameLabelMeta",
        ") -> ScopeFrameLabelMeta",
        ".frame_hint_mask(&",
        "fn selection_frame_label_meta(",
        "fn offer_scope_frame_label_meta(",
        "fn scope_frame_label_meta(",
        "fn scope_frame_label_meta_at(",
    ] {
        assert!(
            !hot_path.contains(forbidden),
            "scope frame-label hot path must use ScopeFrameLabelScratch/View, not by-value mask plumbing: {forbidden}"
        );
    }
    assert!(
        hot_path.contains("ScopeFrameLabelScratch::EMPTY")
            && hot_path.contains("ScopeFrameLabelView<'_>"),
        "offer hot paths must build scratch locally and pass ScopeFrameLabelView"
    );
}

#[test]
fn no_by_value_scope_frame_label_scratch_in_hot_paths() {
    let hot_path = [
        read("src/endpoint/kernel/core/frontier_helpers.rs"),
        read("src/endpoint/kernel/core/scope_evidence_logic.rs"),
        read("src/endpoint/kernel/offer.rs"),
        read("src/endpoint/kernel/offer/facts.rs"),
        read("src/endpoint/kernel/offer/passive.rs"),
        read("src/endpoint/kernel/offer/select.rs"),
    ]
    .join("\n");
    for forbidden in [
        ") -> ScopeFrameLabelScratch",
        "frame_label_scratch: ScopeFrameLabelScratch",
        "scratch: ScopeFrameLabelScratch",
        "ScopeFrameLabelMasks,",
    ] {
        assert!(
            !hot_path.contains(forbidden),
            "scope frame-label scratch must not flow by value through hot path signatures: {forbidden}"
        );
    }
    assert!(
        hot_path.contains("&mut ScopeFrameLabelScratch")
            && hot_path.contains("ScopeFrameLabelView<'_>"),
        "hot paths must use mutable scratch and borrowed frame-label views"
    );
}

#[test]
fn semantic_surface_sources_under_file_budget() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in ["tests/semantic_surface", "src/endpoint/kernel/evidence"] {
        let full = root.join(dir);
        if !full.exists() {
            continue;
        }
        for entry in fs::read_dir(&full).unwrap_or_else(|err| panic!("read {dir} failed: {err}")) {
            let path = entry
                .unwrap_or_else(|err| panic!("read entry in {dir} failed: {err}"))
                .path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let lines = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()))
                .lines()
                .count();
            assert!(
                lines <= 800,
                "semantic/source test file must stay under 800 lines: {} has {lines}",
                path.display()
            );
        }
    }
    assert!(
        read("src/endpoint/kernel/evidence.rs").lines().count() <= 300,
        "evidence.rs production owner must stay under 300 lines"
    );
}

#[test]
fn evidence_file_budget_under_300() {
    assert!(
        read("src/endpoint/kernel/evidence.rs").lines().count() <= 300,
        "evidence.rs production owner must stay under 300 lines"
    );
}

#[test]
fn source_residue_forbidden_literals_are_not_raw_script_hits() {
    let semantic_tests = read_all_rs_tree("tests/semantic_surface");
    let forbidden_attr = concat!("#[", "allow(dead_code)]");
    assert!(
        !semantic_tests.contains(forbidden_attr),
        "semantic tests must split forbidden dead-code attribute literals"
    );
}

#[test]
fn resolver_surface_hygiene_allows_split_test_literal_only() {
    run_script(".github/scripts/check_resolver_surface_hygiene.sh");
}

#[test]
fn kernel_quarantine_extracts_poll_public_recv_by_braces() {
    let script = read(".github/scripts/check_kernel_monomorphization_quarantine.sh");
    assert!(
        script.contains("def extract_rust_function(")
            && script.contains("extract_rust_function(public_runtime, \"poll_public_recv\")"),
        "kernel quarantine must use brace-matched poll_public_recv extraction"
    );
}

#[test]
fn final_form_static_gates_run_without_budget_or_regex_false_failure() {
    run_script(".github/scripts/check_maintainability_budgets.sh");
    run_script(".github/scripts/check_resolver_surface_hygiene.sh");
    run_script(".github/scripts/check_kernel_monomorphization_quarantine.sh");
}
