#![cfg(feature = "std")]

use std::{collections::BTreeMap, path::PathBuf, process::Command};

const SHAPES: [&str; 3] = ["route_heavy", "linear_heavy", "fanout_heavy"];
const FLASH_BUDGET_BYTES: usize = 768 * 1024;
const STATIC_SRAM_BUDGET_BYTES: usize = 48 * 1024;
const KERNEL_STACK_BUDGET_BYTES: usize = 24 * 1024;
const PEAK_SRAM_BUDGET_BYTES: usize = 96 * 1024;

#[derive(Clone, Copy, Debug, Default)]
struct PicoShapeMetrics {
    flash_bytes: Option<usize>,
    static_sram_bytes: Option<usize>,
    kernel_stack_reserve_bytes: Option<usize>,
    peak_stack_upper_bound_bytes: Option<usize>,
    peak_sram_upper_bound_bytes: Option<usize>,
    measured_peak_stack_bytes: Option<usize>,
    measured_peak_sram_bytes: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ResidentShapeMetrics {
    route_bytes: Option<usize>,
    loop_bytes: Option<usize>,
    endpoint_bytes: Option<usize>,
    compiled_program_header_bytes: Option<usize>,
    compiled_role_header_bytes: Option<usize>,
    compiled_program_persistent_bytes: Option<usize>,
    compiled_role_persistent_bytes: Option<usize>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn command_output(command: &mut Command, label: &str) -> String {
    command.current_dir(repo_root());
    command.env("CARGO_TERM_COLOR", "never");
    command.env("CARGO_TERM_PROGRESS_WHEN", "never");
    command.env("TERM", "dumb");

    let output = command
        .output()
        .unwrap_or_else(|err| panic!("{label} failed to spawn: {err}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut combined = stdout.into_owned();
    if !stderr.is_empty() {
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    assert!(
        output.status.success(),
        "{label} failed with status {:?}\n{}",
        output.status.code(),
        combined
    );
    combined
}

fn parse_usize_field(value: &str, field: &str, shape: &str) -> usize {
    value
        .parse::<usize>()
        .unwrap_or_else(|err| panic!("parse {field} for {shape} failed: {err} ({value})"))
}

fn required_metric(value: Option<usize>, field: &str, shape: &str) -> usize {
    value.unwrap_or_else(|| panic!("missing {field} for {shape}"))
}

fn parse_pico_size_matrix(output: &str) -> BTreeMap<String, PicoShapeMetrics> {
    let mut metrics = BTreeMap::<String, PicoShapeMetrics>::new();
    let mut current_shape: Option<String> = None;

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if let Some(shape) = line
            .strip_prefix("== pico size matrix: ")
            .and_then(|rest| rest.strip_suffix(" =="))
        {
            current_shape = Some(shape.to_owned());
            metrics.entry(shape.to_owned()).or_default();
            continue;
        }

        let Some(shape) = current_shape.as_deref() else {
            continue;
        };
        let Some(shape_metrics) = metrics.get_mut(shape) else {
            continue;
        };

        if let Some(value) = line.strip_prefix("pico smoke flash bytes: ") {
            shape_metrics.flash_bytes = Some(parse_usize_field(value, "flash bytes", shape));
        } else if let Some(value) = line.strip_prefix("pico smoke static sram bytes: ") {
            shape_metrics.static_sram_bytes =
                Some(parse_usize_field(value, "static sram bytes", shape));
        } else if let Some(value) = line.strip_prefix("pico smoke kernel stack reserve bytes: ") {
            shape_metrics.kernel_stack_reserve_bytes = Some(parse_usize_field(
                value,
                "kernel stack reserve bytes",
                shape,
            ));
        } else if let Some(value) = line.strip_prefix("pico smoke peak stack upper-bound bytes: ") {
            shape_metrics.peak_stack_upper_bound_bytes = Some(parse_usize_field(
                value,
                "peak stack upper-bound bytes",
                shape,
            ));
        } else if let Some(value) = line.strip_prefix("pico smoke peak sram upper-bound bytes: ") {
            shape_metrics.peak_sram_upper_bound_bytes = Some(parse_usize_field(
                value,
                "peak sram upper-bound bytes",
                shape,
            ));
        } else if let Some(value) = line.strip_prefix("pico smoke measured peak stack bytes: ") {
            shape_metrics.measured_peak_stack_bytes =
                Some(parse_usize_field(value, "measured peak stack bytes", shape));
        } else if let Some(value) = line.strip_prefix("pico smoke measured peak sram bytes: ") {
            shape_metrics.measured_peak_sram_bytes =
                Some(parse_usize_field(value, "measured peak sram bytes", shape));
        }
    }

    metrics
}

fn parse_resident_shapes(output: &str) -> BTreeMap<String, ResidentShapeMetrics> {
    let mut metrics = BTreeMap::<String, ResidentShapeMetrics>::new();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix("resident-shape ") else {
            continue;
        };
        let mut shape_name = None::<String>;
        let mut shape_metrics = ResidentShapeMetrics::default();

        for field in rest.split_whitespace() {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            match key {
                "name" => shape_name = Some(value.to_owned()),
                "route_bytes" => {
                    shape_metrics.route_bytes =
                        Some(parse_usize_field(value, "route bytes", "resident-shape"))
                }
                "loop_bytes" => {
                    shape_metrics.loop_bytes =
                        Some(parse_usize_field(value, "loop bytes", "resident-shape"))
                }
                "endpoint_bytes" => {
                    shape_metrics.endpoint_bytes =
                        Some(parse_usize_field(value, "endpoint bytes", "resident-shape"))
                }
                "compiled_program_header_bytes" => {
                    shape_metrics.compiled_program_header_bytes = Some(parse_usize_field(
                        value,
                        "compiled program header bytes",
                        "resident-shape",
                    ))
                }
                "compiled_role_header_bytes" => {
                    shape_metrics.compiled_role_header_bytes = Some(parse_usize_field(
                        value,
                        "compiled role header bytes",
                        "resident-shape",
                    ))
                }
                "compiled_program_persistent_bytes" => {
                    shape_metrics.compiled_program_persistent_bytes = Some(parse_usize_field(
                        value,
                        "compiled program persistent bytes",
                        "resident-shape",
                    ))
                }
                "compiled_role_persistent_bytes" => {
                    shape_metrics.compiled_role_persistent_bytes = Some(parse_usize_field(
                        value,
                        "compiled role persistent bytes",
                        "resident-shape",
                    ))
                }
                _ => {}
            }
        }

        let shape_name =
            shape_name.unwrap_or_else(|| panic!("resident-shape line missing name: {line}"));
        metrics.insert(shape_name, shape_metrics);
    }

    metrics
}

fn run_size_matrix_output() -> String {
    command_output(
        Command::new("bash").arg("./.github/scripts/check_pico_size_matrix.sh"),
        "pico size matrix",
    )
}

#[test]
fn huge_choreography_shape_matrix_contracts_are_measured_per_shape() {
    let output = run_size_matrix_output();
    let pico_metrics = parse_pico_size_matrix(&output);
    let resident_metrics = parse_resident_shapes(&output);

    for shape in SHAPES {
        let pico = pico_metrics
            .get(shape)
            .unwrap_or_else(|| panic!("missing pico size matrix block for {shape}"));
        let flash_bytes = required_metric(pico.flash_bytes, "flash bytes", shape);
        let static_sram_bytes = required_metric(pico.static_sram_bytes, "static sram bytes", shape);
        let kernel_stack_reserve_bytes = required_metric(
            pico.kernel_stack_reserve_bytes,
            "kernel stack reserve bytes",
            shape,
        );
        let peak_stack_upper_bound_bytes = required_metric(
            pico.peak_stack_upper_bound_bytes,
            "peak stack upper-bound bytes",
            shape,
        );
        let peak_sram_upper_bound_bytes = required_metric(
            pico.peak_sram_upper_bound_bytes,
            "peak sram upper-bound bytes",
            shape,
        );
        let measured_peak_stack_bytes = required_metric(
            pico.measured_peak_stack_bytes,
            "measured peak stack bytes",
            shape,
        );
        let measured_peak_sram_bytes = required_metric(
            pico.measured_peak_sram_bytes,
            "measured peak sram bytes",
            shape,
        );

        assert!(
            flash_bytes <= FLASH_BUDGET_BYTES,
            "{shape} flash contract regressed: {flash_bytes} > {FLASH_BUDGET_BYTES}"
        );
        assert!(
            static_sram_bytes <= STATIC_SRAM_BUDGET_BYTES,
            "{shape} static SRAM contract regressed: {static_sram_bytes} > {STATIC_SRAM_BUDGET_BYTES}"
        );
        assert!(
            kernel_stack_reserve_bytes <= KERNEL_STACK_BUDGET_BYTES,
            "{shape} kernel stack contract regressed: {kernel_stack_reserve_bytes} > {KERNEL_STACK_BUDGET_BYTES}"
        );
        assert!(
            peak_stack_upper_bound_bytes <= KERNEL_STACK_BUDGET_BYTES,
            "{shape} peak stack upper-bound regressed: {peak_stack_upper_bound_bytes} > {KERNEL_STACK_BUDGET_BYTES}"
        );
        assert!(
            peak_sram_upper_bound_bytes <= PEAK_SRAM_BUDGET_BYTES,
            "{shape} peak SRAM contract regressed: {peak_sram_upper_bound_bytes} > {PEAK_SRAM_BUDGET_BYTES}"
        );
        assert!(
            measured_peak_stack_bytes > 0,
            "{shape} must emit measured peak stack bytes"
        );
        assert!(
            measured_peak_sram_bytes > 0,
            "{shape} must emit measured peak SRAM bytes"
        );

        let resident = resident_metrics
            .get(shape)
            .unwrap_or_else(|| panic!("missing resident-shape measurement for {shape}"));
        assert!(
            required_metric(resident.route_bytes, "route bytes", shape) > 0,
            "{shape} must emit measured route resident bytes"
        );
        required_metric(resident.loop_bytes, "loop bytes", shape);
        assert!(
            required_metric(resident.endpoint_bytes, "endpoint bytes", shape) > 0,
            "{shape} must emit measured endpoint resident bytes"
        );
        assert!(
            required_metric(
                resident.compiled_program_header_bytes,
                "compiled program header bytes",
                shape,
            ) > 0,
            "{shape} must emit measured compiled program header bytes"
        );
        assert!(
            required_metric(
                resident.compiled_role_header_bytes,
                "compiled role header bytes",
                shape,
            ) > 0,
            "{shape} must emit measured compiled role header bytes"
        );
        assert!(
            required_metric(
                resident.compiled_program_persistent_bytes,
                "compiled program persistent bytes",
                shape,
            ) > 0,
            "{shape} must emit measured compiled program persistent bytes"
        );
        assert!(
            required_metric(
                resident.compiled_role_persistent_bytes,
                "compiled role persistent bytes",
                shape,
            ) > 0,
            "{shape} must emit measured compiled role persistent bytes"
        );
    }
}
