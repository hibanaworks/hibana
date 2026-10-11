run_with_compile_pressure_guard() {
  bash "$(dirname "${BASH_SOURCE[0]}")/compile_pressure_tool.sh" guard \
    "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)" "$@"
}

compile_pressure_guard_offender() {
  bash "$(dirname "${BASH_SOURCE[0]}")/compile_pressure_tool.sh" offender "$@"
}

compile_pressure_guard_descendants() {
  bash "$(dirname "${BASH_SOURCE[0]}")/compile_pressure_tool.sh" live-descendants "$@"
}
