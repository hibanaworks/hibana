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
    let manifest_test_gate = read(".github/scripts/check_manifest_tests.sh");
    let miri_gate = read(".github/scripts/check_miri.sh");
    let miri_toolchain = read(".github/miri-toolchain");
    let compile_pressure_guard = read(".github/scripts/lib/compile_pressure_guard.sh");
    let compile_pressure_budget_helper = read(".github/scripts/lib/compile_pressure_budget.py");
    let compile_pressure_budget =
        read(".github/measurement_snapshots/hibana-compile-pressure-budget.tsv");
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
        "compile_pressure_guard.sh",
        "run_with_compile_pressure_guard",
        "HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE",
        "HIBANA_COMPILE_PRESSURE_LABEL=final_form_gate",
        "HIBANA_COMPILE_PRESSURE_CRATE_NAME=hibana",
        "aggregate refactor gate requires ",
        "max_stack/sram/flash all <= snapshot budget and at least one decrease",
    ] {
        assert!(
            final_gate_with_helpers.contains(required),
            "final-form snapshot gate missing required guard: {required}"
        );
    }

    for required in [
        "local max_mib=\"${HIBANA_COMPILE_PRESSURE_MAX_RSS_MIB:-}\"",
        "HIBANA_COMPILE_PRESSURE_BUDGETS:-$(cd \"$(dirname \"${BASH_SOURCE[0]}\")/../..\" && pwd)/measurement_snapshots/hibana-compile-pressure-budget.tsv",
        "budget_label=\"${HIBANA_COMPILE_PRESSURE_LABEL:-}\"",
        "local crate_name=\"${HIBANA_COMPILE_PRESSURE_CRATE_NAME:-}\"",
        "compile_pressure_budget.py",
        "limit \"${budget_path}\" \"${budget_label}\" rss_mib",
        "compile_pressure_guard_limit_seconds",
        "HIBANA_COMPILE_PRESSURE_MAX_SECONDS",
        "limit \"${budget_path}\" \"${budget_label}\" seconds",
        "max-rss",
        "{\"cargo\", \"rustc\", \"rustdoc\"}",
        "if name == \"rustup\":",
        "def crate_arg_matches(command: str) -> bool:",
        "if token == \"--crate-name\"",
        "if token.startswith(\"--crate-name=\"):",
        "if crate_name and not crate_arg_matches(command):",
        "matched_process = True",
        "total_rss += rss",
        "descendants = {root}",
        "aggregate total_rss_mib=",
        "matched=1",
        "matched={matched}",
        "ok total_rss_mib=",
        "active_window_start_seconds=\"\"",
        "active_window_seconds=\"$((now_seconds - active_window_start_seconds))\"",
        "if (( active_window_seconds > elapsed_seconds )); then",
        "if [[ -n \"${HIBANA_COMPILE_PRESSURE_CRATE_NAME:-}\" ]]; then",
        "max_observed_mib",
        "elapsed=${elapsed_seconds}s seconds_budget=${max_seconds}s max_rss=${max_observed_mib}MiB rss_budget=$((max_kib / 1024))MiB",
        "HIBANA_COMPILE_PRESSURE_POLL_SECONDS:-1",
        "sys.exit(7)",
        "if [[ \"${status}\" -eq 7 ]]; then",
        "compile_pressure_guard_stop_tree",
        "return 137",
        "return 124",
    ] {
        assert!(
            compile_pressure_guard.contains(required),
            "compile pressure guard must enforce a snapshot-derived aggregate rust-tool emergency stop: {required}"
        );
    }

    for forbidden in ["5242880", "10485760", "5120", "10240"] {
        assert!(
            !format!("{run_final_gate}\n{compile_pressure_guard}").contains(forbidden),
            "final-form compile pressure guard must not drift back to 5GiB/10GiB ceilings: {forbidden}"
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
        "name = \"hibana-final-form-measure\"",
        "[workspace]\n\n[dependencies]\nhibana = { path = \"../..\", default-features = false }",
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
            && run_final_gate.contains("bash ./.github/scripts/check_manifest_tests.sh")
            && run_final_gate.contains("bash ./.github/scripts/check_miri.sh")
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
        manifest_test_gate.contains("import tomllib")
            && manifest_test_gate.contains("get(\"workspace\", {}).get(\"members\")")
            && manifest_test_gate.contains("data.get(\"test\", [])")
            && manifest_test_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path")
            && manifest_test_gate.contains("if running == 0 or passed != running")
            && manifest_test_gate.contains("manifest test gate count mismatch")
            && miri_toolchain.trim() == "nightly-2026-05-28"
            && !miri_gate.contains("MIRI_TOOLCHAIN:-")
            && miri_gate.contains("export MIRIFLAGS=\"-Zmiri-strict-provenance\"")
            && miri_gate.contains("cargo +\"${MIRI_TOOLCHAIN}\" miri test")
            && miri_gate.contains(
                "public-runtime-owner \\\n  18 \\\n  18 \\\n  0 \\\n  -p hibana \\\n  --test miri_runtime_owner"
            )
            && miri_gate.contains(
                "endpoint-waiter-owner \\\n  2 \\\n  2 \\\n  0 \\\n  -p hibana \\\n  --lib \\\n  rendezvous::core::endpoint_waiter::tests"
            )
            && miri_gate.contains(
                "affine-send-owner \\\n  4 \\\n  4 \\\n  0 \\\n  -p hibana \\\n  --test affine_progression"
            )
            && miri_gate.contains(
                "direct-recv-owner \\\n  10 \\\n  10 \\\n  0 \\\n  -p hibana \\\n  --test cursor_send_recv_direct_recv"
            )
            && miri_gate.contains(
                "forgotten-recv-owner \\\n  1 \\\n  1 \\\n  0 \\\n  -p hibana \\\n  --test cursor_send_recv_session_forget_recv"
            )
            && miri_gate.contains(
                "forgotten-send-owner \\\n  1 \\\n  1 \\\n  0 \\\n  -p hibana \\\n  --test cursor_send_recv_session_forget_send"
            )
            && miri_gate.contains(
                "endpoint-drop-wake-owner \\\n  2 \\\n  2 \\\n  0 \\\n  -p hibana \\\n  --test cursor_send_recv_session_drop_wake"
            )
            && miri_gate.contains(
                "session-fault-cancel-owner \\\n  1 \\\n  1 \\\n  0 \\\n  -p hibana \\\n  --test cursor_send_recv_session_fault_cancel"
            )
            && miri_gate.contains(
                "local-action-owner \\\n  3 \\\n  3 \\\n  0 \\\n  -p hibana \\\n  --test local_action"
            )
            && miri_gate.contains(
                "route-branch-send-owner \\\n  3 \\\n  3 \\\n  0 \\\n  -p hibana \\\n  --test route_branch_send"
            )
            && miri_gate.contains(
                "resolved-send-owner \\\n  2 \\\n  2 \\\n  0 \\\n  -p hibana \\\n  --test send_route_authority"
            )
            && miri_gate.contains(
                "offer-branch-owner \\\n  11 \\\n  11 \\\n  0 \\\n  -p hibana \\\n  --test offer_branch_recv_evidence"
            )
            && miri_gate.contains(
                "resident-sidecar-owner \\\n  20 \\\n  19 \\\n  1 \\\n  -p hibana \\\n  --lib \\\n  storage_layout::capacity::tests"
            )
            && miri_gate
                .contains("miri gate passed toolchain=${MIRI_TOOLCHAIN} tests=77 ignored=1")
            && miri_gate.contains("local expected_listed=\"$2\"")
            && miri_gate.contains("local expected_passed=\"$3\"")
            && miri_gate.contains("local expected_ignored=\"$4\"")
            && !miri_gate.contains("--exact")
            && miri_gate.contains("storage_layout::capacity::tests")
            && miri_gate.contains("miri gate test-count mismatch")
            && workflow.contains("--profile minimal --component miri --component rust-src"),
        "final-form validation must execute every manifest target and the pinned nonzero Miri owner suite"
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
        !format!("{rust_1_95_gate}\n{warning_free_gate}\n{direct_projection_gate}\n{package_gate}")
            .contains("--no-run")
            && !warning_free_gate.contains("check --all-targets")
            && !warning_free_gate.contains("cargo +\"${TOOLCHAIN}\" test -p hibana")
            && rust_1_95_gate.contains(
                "cargo +1.95.0 test --manifest-path \"${ROOT_DIR}/.github/repo-tests/Cargo.toml\" --test semantic_surface"
            )
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
        "assert_runtime_test_targets_are_unique",
        "cargo test target must be run once per script",
        "hibana-compile-pressure-budget.tsv",
        "compile_pressure_guard.sh",
        "run_with_compile_pressure_guard",
        "missing aggregate compile pressure observation",
        "runtime compile pressure label=",
        "max_rss=",
        "seconds_budget=",
        "rss_budget=",
        "HIBANA_RUNTIME_TEST_TARGET_DIR",
        "CARGO_TARGET_DIR",
        "== runtime cold compile-pressure test ==",
        "cold_parallel_route_nesting",
        "--test offer_branch_recv_evidence",
        "--test parallel_route_nesting",
        "--test parallel_route_alternating",
        "--test huge_choreography_runtime",
        "lane_set_view_iterates_set_bits_without_empty_lane_scan",
    ] {
        assert!(
            performance_gate.contains(required),
            "runtime performance hygiene gate missing required operation-count/source guard: {required}"
        );
    }

    for required in [
        "FIELDS = (",
        "\"observed_seconds\"",
        "\"observed_rss_mib\"",
        "\"seconds_headroom\"",
        "\"rss_headroom_mib\"",
        "def limit_for",
        "observed_seconds",
        "seconds_headroom",
        "observed_rss_mib",
        "rss_headroom_mib",
        "max-rss",
    ] {
        assert!(
            compile_pressure_budget_helper.contains(required),
            "compile pressure budget helper must be the single parser for snapshot-derived limits: {required}"
        );
    }

    let compile_pressure_scripts =
        format!("{performance_gate}\n{compile_pressure_guard}\n{compile_pressure_budget_helper}");
    for forbidden in ["9216", "8704", "8448", "5632", "  420", "  300"] {
        assert!(
            !compile_pressure_scripts.contains(forbidden),
            "compile-pressure scripts must not keep rough inline budgets: {forbidden}"
        );
    }
    for forbidden in ["max_seconds", "max_rss_mib"] {
        assert!(
            !compile_pressure_budget.contains(forbidden),
            "compile-pressure snapshot must store observations and headroom, not direct max budgets: {forbidden}"
        );
    }

    for required in [
        "Limit = observed + headroom",
        "label\tobserved_seconds\tobserved_rss_mib\tseconds_headroom\trss_headroom_mib",
    ] {
        assert!(
            compile_pressure_budget.contains(required),
            "compile pressure budget snapshot missing required header: {required}"
        );
    }

    for label in [
        "final_form_gate",
        "cold_parallel_route_nesting",
        "offer_branch_recv_evidence",
        "parallel_route_nesting",
        "parallel_route_alternating",
        "lane_set_view_iterates_set_bits_without_empty_lane_scan",
        "huge_choreography_runtime",
    ] {
        assert_eq!(
            compile_pressure_budget
                .lines()
                .filter(|line| line.starts_with(&format!("{label}\t")))
                .count(),
            1,
            "compile pressure budget snapshot must contain exactly one row for {label}"
        );
    }

    for line in compile_pressure_budget
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("label\t") && !line.is_empty())
    {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            5,
            "compile pressure budget rows must be label/observed/headroom fields: {line}"
        );
        for value in &columns[1..] {
            let parsed = value.parse::<u32>().unwrap_or_else(|err| {
                panic!("compile pressure budget must be numeric: {line}: {err}")
            });
            assert!(
                parsed > 0,
                "compile pressure budget must be positive: {line}"
            );
        }
        let observed_seconds = columns[1]
            .parse::<u32>()
            .expect("observed seconds checked numeric");
        let observed_rss = columns[2]
            .parse::<u32>()
            .expect("observed rss checked numeric");
        let seconds_headroom = columns[3]
            .parse::<u32>()
            .expect("seconds headroom checked numeric");
        let rss_headroom = columns[4]
            .parse::<u32>()
            .expect("rss headroom checked numeric");
        assert!(
            seconds_headroom <= 60 || seconds_headroom <= observed_seconds.saturating_mul(2),
            "compile pressure seconds headroom must stay close to observation: {line}"
        );
        assert!(
            rss_headroom <= 512 || rss_headroom <= observed_rss / 2,
            "compile pressure RSS headroom must stay close to observation: {line}"
        );
    }

    assert!(
        !run_final_gate.contains("check_huge_choreography_budget.sh")
            && !performance_gate.contains("huge_choreography_compile")
            && !compile_pressure_budget.contains("huge_choreography_compile"),
        "huge choreography compile proof must stay in the runtime integration target, not a second target"
    );

    let hot_runtime_section = performance_gate
        .rsplit("echo \"== runtime performance operation-count tests ==\"")
        .next()
        .expect("runtime performance hot section start")
        .split("echo \"== runtime cold compile-pressure test ==\"")
        .next()
        .expect("runtime performance hot section end");
    let cold_runtime_section = performance_gate
        .rsplit("echo \"== runtime cold compile-pressure test ==\"")
        .next()
        .expect("runtime performance cold section start")
        .split("echo \"runtime performance hygiene check passed\"")
        .next()
        .expect("runtime performance cold section end");

    for target in [
        "--test offer_branch_recv_evidence",
        "--test parallel_route_nesting",
        "--test parallel_route_alternating",
        "--test huge_choreography_runtime",
    ] {
        assert_eq!(
            hot_runtime_section.matches(target).count(),
            1,
            "runtime performance hygiene gate must run each cargo test target once: {target}"
        );
    }
    assert_eq!(
        cold_runtime_section
            .matches("--test parallel_route_nesting")
            .count(),
        1,
        "runtime cold compile-pressure gate must run the representative heavy target once"
    );
    assert!(
        cold_runtime_section.contains("mktemp -d")
            && cold_runtime_section.contains("cleanup_cold_target_dir")
            && cold_runtime_section
                .contains("HIBANA_RUNTIME_TEST_TARGET_DIR=\"${cold_target_dir}\""),
        "runtime cold compile-pressure gate must use and clean a fresh target dir"
    );

    for stale_filter in [
        "offer_requires_framed_receive_evidence_for_branch_demux",
        "branch_recv_transport_consumes_frame_once",
        "forgotten_route_branch_leaves_endpoint_fail_closed",
        "forgotten_route_recv_future_leaves_endpoint_fail_closed",
        "route_inside_parallel_lane_cannot_release_join_before_sibling_lane",
        "alternating_route_parallel_join_uses_only_selected_arms",
        "unselected_route_arm_parallel_events_are_dead_and_not_join_obligations",
        "unselected_route_arm_parallel_events_do_not_block_parallel_join",
        "outer_left_selection_kills_nested_right_route_and_parallel_body",
    ] {
        assert!(
            !performance_gate.contains(stale_filter),
            "runtime performance hygiene gate must not reintroduce filter-by-filter cargo runs: {stale_filter}"
        );
    }
}
