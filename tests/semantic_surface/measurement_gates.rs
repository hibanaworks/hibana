use super::common::*;

#[test]
fn measurement_gates_prevent_recurrent_size_and_stack_regressions() {
    let final_gate = read(".github/scripts/check_final_form_measurements.sh");
    let worktree_gate = read(".github/scripts/check_size_snapshot_regression.sh");
    let performance_gate = read(".github/scripts/check_runtime_performance_hygiene.sh");
    let kernel_monomorphization_gate =
        read(".github/scripts/check_kernel_monomorphization_quarantine.sh");
    let run_final_gate = read(".github/scripts/run_final_form_gates.sh");
    let rust_1_95_gate = read(".github/scripts/check_rust_1_95_stable.sh");
    let warning_free_gate = read(".github/scripts/check_warning_free.sh");
    let direct_projection_gate = read(".github/scripts/check_direct_projection_binary.sh");
    let package_gate = read(".github/scripts/check_package_artifact.sh");
    let huge_gate = read(".github/scripts/check_huge_choreography_budget.sh");
    let thumb_header_gate = read(".github/scripts/check_thumbv6m_frame_header_codegen.sh");
    let thumb_mask_gate = read(".github/scripts/check_thumbv6m_frame_label_mask_codegen.sh");
    let final_gate_with_helpers =
        format!("{final_gate}\n{thumb_header_gate}\n{thumb_mask_gate}\n{run_final_gate}");
    let snapshot = read(".github/measurement_snapshots/hibana-size-snapshot.json");
    let workflow = read(".github/workflows/quality-gates.yml");
    let endpoint_kernel = read("src/endpoint/kernel/core.rs")
        + &read_production_dir_rs("src/endpoint/kernel")
        + &read_production_dir_rs("src/endpoint/kernel/core");

    for required in [
        "if [[ \"${HIBANA_OMIT_FIXED_SNAPSHOT_CHECK:-0}\" != \"1\" ]]; then",
        "fixed snapshot thumb budget check omitted by explicit override; worktree size snapshot still runs",
        "fixed snapshot runtime budget check omitted by explicit override; worktree size snapshot still runs",
        "rustup target add --toolchain \"${TOOLCHAIN}\" thumbv6m-none-eabi",
        "--target thumbv6m-none-eabi",
        "thumb section name=.rodata bytes=%d target=thumbv6m-none-eabi no_default_features=1",
        "values[\"flash_total\"] =",
        "thumb_values[\"flash_total\"] =",
        "bash \"${ROOT_DIR}/.github/scripts/check_thumbv6m_frame_label_mask_codegen.sh\"",
        "bash \"${ROOT_DIR}/.github/scripts/check_thumbv6m_frame_header_codegen.sh\"",
        "__aeabi_(lmul|lcmp|ulcmp|ldivmod|uldivmod|llsl|llsr|lasr)\\\\b",
        "thumbv6m FrameHeader codegen has no aeabi u64 helpers",
        "thumbv6m FrameLabelMask codegen has no aeabi u64 helpers",
        "protocol_artifact_aeabi_metrics",
        "aeabi_u64_helper_count=0",
        "final-form protocol artifact regained aeabi u64 helper calls",
        "resident_prefix_bytes must include the internal tap ring carved before the runtime slab",
        "Rendezvous header, transport T field, alignment padding, and tap ring",
        "actual_sram =",
        "budget_sram =",
        "pico_total_sram_bytes",
        "actual_max_stack = max(metrics[\"peak_stack_bytes\"] for metrics in seen.values())",
        "bash \"${ROOT_DIR}/.github/scripts/check_size_snapshot_regression.sh\"",
        "bash ./.github/scripts/check_no_split_guard_literals.sh",
        "python3 .github/scripts/check_public_api_allowlists.py --self-test",
        "aggregate refactor gate requires ",
        "max_stack/sram/flash all <= snapshot budget and at least one decrease",
    ] {
        assert!(
            final_gate_with_helpers.contains(required),
            "final-form snapshot gate missing required guard: {required}"
        );
    }

    for required in [
        "== final-form projected protocol matrix ==",
        "projected_protocol_matrix_reports_compact_resident_images",
        "PROTOCOL_MATRIX_OUTPUT",
        "protocol-matrix ",
        "minimal_send_recv",
        "nested_par_join",
        "route_with_unselected_nested_par",
        "triple_nested_route",
        "passive_nested_route_observer",
        "alternating_par_route",
        "huge_legal_choreography",
        "program_blob_len",
        "role_blob_len",
        "endpoint_scratch_bytes",
        "largest_section_bytes",
        "== final-form protocol artifact flash matrix ==",
        "FINAL_FORM_PROTOCOL_SOURCE=\"${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_matrix.rs\"",
        "FINAL_FORM_PROTOCOL_BLACK_BOX_SOURCE=\"${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_black_box_roles.rs\"",
        "cp \"${FINAL_FORM_PROTOCOL_SOURCE}\"",
        "cp \"${FINAL_FORM_PROTOCOL_BLACK_BOX_SOURCE}\"",
        "final_form_protocol!(${protocol_name})",
        "final_form_protocol_black_box_roles!(${protocol_name}, &program)",
        "protocol-artifact ",
        "flash_total",
        "rodata_map_bytes",
        "rodata_map_fragments",
        "bucket_symbol_count",
        "map_bucket_symbol_count",
        "selected_program_bucket_count",
        "selected_role_bucket_count",
        "full_bucket_floor_bytes",
        "llvm-nm",
        "-Map=${map}",
        "snapshot-check protocol-artifact",
        "protocol artifact rodata={rodata} exceeds",
        "exceeds selected bucket count",
        "still retains every bucket ladder entry",
        "final-form measurement violation: missing protocol artifact rows",
        "protocol artifact flash_total={actual} exceeds",
        "final-form measurement violation: minimal_send_recv",
    ] {
        assert!(
            final_gate.contains(required),
            "final-form protocol matrix measurement missing required guard: {required}"
        );
    }

    for required in [
        "CURRENT_REF=\"${HIBANA_SIZE_CURRENT_REF:-HEAD}\"",
        "git worktree add --detach \"${CURRENT_WORKTREE}\" \"${CURRENT_REF}\"",
        "measure_tree \"current-${CURRENT_LABEL}\" \"${CURRENT_TREE}\" \"${CURRENT_JSON}\"",
        "hibana-projected-measure",
        "program_import = \"hibana::runtime::program::{project, RoleProgram}\"",
        "pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>)",
        "projected_sections",
        "current runtime snapshot missing shapes",
        "current runtime snapshot shape={shape} missing metrics",
        "\"resident_prefix_bytes\"",
        "\"tap_ring_bytes\"",
        "resident_prefix_bytes must include the internal tap ring carved before the runtime slab",
        "Rendezvous header, transport T field, alignment padding, and tap ring",
        "\"pico_total_sram_bytes\"",
        "SNAPSHOT_FILE=\"${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json\"",
        "budget_snapshot = json.load(f)",
        "worktree-snapshot budget-section {key} actual={actual} budget={maximum}",
        "section {key} exceeds snapshot budget",
        "worktree-snapshot budget-projected-section {key} actual={actual} budget={maximum}",
        "projected section {key} exceeds snapshot budget",
        "worktree-snapshot budget-runtime shape={shape} {key} actual={actual} budget={maximum}",
        "runtime shape {shape} {key} exceeds snapshot budget",
        "worktree-snapshot budget-aggregate {name} actual={actual} budget={maximum}",
        "aggregate snapshot budget gate failed: max_stack/sram/flash must all be <= budget ",
        "and at least one must decrease below budget",
    ] {
        assert!(
            worktree_gate.contains(required),
            "worktree size/stack regression gate missing required guard: {required}"
        );
    }

    for forbidden in [
        "measure_tree \"current-${CURRENT_LABEL}\" \"${CURRENT_TREE}\" \"${CURRENT_JSON}\" 1",
        "allow_probe_patch",
        "text.replace(",
        "path.write_text",
        "failed to inject localside stack probe",
        "refusing to patch current source",
        "HIBANA_OMIT_FIXED_SNAPSHOT_CHECK=0",
        "\"${CI:-false}\" != \"true\"",
        "CI/override",
        "BASE_REF=\"HEAD^\"",
        "BASE_WORKTREE",
        "PUBLISHED_CRATES_IO",
        "HIBANA_SIZE_BASE_REF",
        "hibana::integration",
        "metrics[\"localside_peak_stack_bytes\"] = metrics.get(\"peak_stack_bytes\", 0)",
        "published baseline",
    ] {
        assert!(
            !worktree_gate.contains(forbidden) && !final_gate.contains(forbidden),
            "size gate must not contain current-tree self-patching or CI fixed-snapshot coupling: {forbidden}"
        );
    }

    assert!(
        workflow.contains("fetch-depth: 0")
            && workflow.contains("run: bash ./.github/scripts/run_final_form_gates.sh")
            && run_final_gate.contains("bash ./.github/scripts/check_unsafe_contract_hygiene.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_surface_test_alias_hygiene.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_kernel_monomorphization_quarantine.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_message_monomorphization_hygiene.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_runtime_performance_hygiene.sh")
            && final_gate.contains("HIBANA_OMIT_FIXED_SNAPSHOT_CHECK=1")
            && final_gate
                .contains("if [[ \"${HIBANA_OMIT_WORKTREE_SIZE_SNAPSHOT:-0}\" != \"1\" ]]; then"),
        "CI must run fixed Pico snapshots and the worktree size snapshot unless an explicit local override is set"
    );
    assert!(
        !final_gate_with_helpers.contains("CARGO_BUILD_JOBS")
            && !worktree_gate.contains("CARGO_BUILD_JOBS"),
        "final-form gates must not override Cargo build parallelism"
    );
    assert!(
        !final_gate_with_helpers.contains("RUST_TEST_THREADS")
            && !worktree_gate.contains("RUST_TEST_THREADS"),
        "final-form gates must not override Rust test harness parallelism"
    );
    assert!(
        !format!(
            "{rust_1_95_gate}\n{warning_free_gate}\n{direct_projection_gate}\n{package_gate}\n{huge_gate}"
        )
        .contains("--no-run")
            && !warning_free_gate.contains("check --all-targets")
            && !warning_free_gate.contains("cargo +\"${TOOLCHAIN}\" test -p hibana")
            && rust_1_95_gate.contains("cargo +1.95.0 test -p hibana --test semantic_surface")
            && rust_1_95_gate
                .contains("cargo +1.95.0 test -p hibana --test dynamic_route_scope_resolver"),
        "final-form gates must not use no-run, all-integration, all-target, or all-test Cargo builds"
    );
    let size_gate_pos = run_final_gate
        .find("bash ./.github/scripts/check_final_form_measurements.sh")
        .expect("final gate must include stack/SRAM/flash measurements");
    let unsafe_gate_pos = run_final_gate
        .find("bash ./.github/scripts/check_unsafe_contract_hygiene.sh")
        .expect("final gate must include unsafe contract hygiene");
    let performance_gate_pos = run_final_gate
        .find("bash ./.github/scripts/check_runtime_performance_hygiene.sh")
        .expect("final gate must include runtime performance hygiene");
    assert!(
        unsafe_gate_pos < size_gate_pos,
        "unsafe contract hygiene must run before stack/SRAM/flash measurements"
    );
    assert!(
        size_gate_pos < performance_gate_pos,
        "size/stack/SRAM/flash measurements must run before performance hygiene"
    );
    for required in [
        "pub(crate) fn kernel_recv",
        "pub(crate) fn kernel_branch_recv",
        "pub(crate) fn kernel_send",
        "kernel_(recv|branch_recv|send)",
        "symbol count is ${count}, expected 1",
        "kernel symbol proof passed",
    ] {
        assert!(
            kernel_monomorphization_gate.contains(required),
            "kernel monomorphization gate must prove single send/recv/branch-recv symbols: {required}"
        );
    }
    for forbidden in [
        "CounterClock",
        "Clock +",
        "RuntimeResources<'cfg, C",
        "CursorEndpoint<'r, ROLE, T, C",
        "Rendezvous<'rv, 'cfg, T, C",
        "Port<'lease, T, C",
    ] {
        assert!(
            !endpoint_kernel.contains(forbidden),
            "kernel send/recv/branch-recv paths must not regain a clock monomorphization axis: {forbidden}"
        );
    }

    for required in [
        "\"description\": \"Measured stack, SRAM, and flash values must satisfy",
        "\"localside_peak_stack_bytes\"",
        "\"resident_prefix_bytes\"",
        "\"tap_ring_bytes\"",
        "\"pico_total_sram_bytes\"",
        "\"flash_total_formula\": \".text + .rodata + .data\"",
        "\".text\": 154624",
        "\".rodata\": 15341",
        "\"flash_total\": 169965",
    ] {
        assert!(
            snapshot.contains(required),
            "measurement snapshot must record the fixed Pico budget and localside stack budget: {required}"
        );
    }

    for required in [
        "Size is primary. This gate only blocks structural hot-path regressions",
        "LaneSetView::next_set_from must advance over empty lane runs with bit operations",
        "compiled image hot path ",
        "must not rebuild lane sets by effect-list or full-view scans",
        "endpoint arena must not contain route-scope lane-word caches",
        "cargo test filter matched no tests",
        "offer_requires_framed_receive_evidence_for_branch_demux",
        "branch_recv_transport_consumes_frame_once",
        "forgotten_route_branch_leaves_endpoint_fail_closed",
        "forgotten_route_recv_future_leaves_endpoint_fail_closed",
        "route_inside_parallel_lane_cannot_release_join_before_sibling_lane",
        "alternating_route_parallel_join_uses_only_selected_arms",
        "unselected_route_arm_parallel_events_are_dead_and_not_join_obligations",
        "unselected_route_arm_parallel_events_do_not_block_parallel_join",
        "outer_left_selection_kills_nested_right_route_and_parallel_body",
        "lane_set_view_iterates_set_bits_without_empty_lane_scan",
    ] {
        assert!(
            performance_gate.contains(required),
            "runtime performance hygiene gate missing required operation-count/source guard: {required}"
        );
    }
}

#[test]
fn thumbv6m_frame_header_codegen_has_no_aeabi_u64_helpers() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = root.join(".github/scripts/check_thumbv6m_frame_header_codegen.sh");
    let output = std::process::Command::new("bash")
        .arg(&script)
        .env("TOOLCHAIN", "stable")
        .output()
        .unwrap_or_else(|err| panic!("run {} failed: {err}", script.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "thumbv6m FrameHeader codegen check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("thumbv6m FrameHeader codegen has no aeabi u64 helpers"),
        "thumbv6m FrameHeader codegen check did not report success\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn thumbv6m_mask_codegen_has_no_aeabi_u64_helpers() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = root.join(".github/scripts/check_thumbv6m_frame_label_mask_codegen.sh");
    let output = std::process::Command::new("bash")
        .arg(&script)
        .env("TOOLCHAIN", "stable")
        .output()
        .unwrap_or_else(|err| panic!("run {} failed: {err}", script.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "thumbv6m FrameLabelMask codegen check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("thumbv6m FrameLabelMask codegen has no aeabi u64 helpers"),
        "thumbv6m FrameLabelMask codegen check did not report success\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
