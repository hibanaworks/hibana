#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

check_absent "EndpointOpFrontier|EndpointOpKey|ProjectedInboundKey|OutboundOpMask|RouteFrontierSummary|route_frontier_summaries|push_route_frontier|route_frontier_summary\\(" \
  "endpoint-op frontier storage or route frontier summary residue" \
  src/g.rs src/g src/global
check_absent "struct[[:space:]]+EndpointOp\\(|validate_parallel_endpoint_ops|first_visible_endpoint_op_conflicts_from_markers|nth_local_endpoint_op|local_endpoint_op_count|ParallelAmbiguousEndpointOp|ReentryAmbiguousEndpointOp|PublicEndpointSelector|pub\\(crate\\)[[:space:]]+const[[:space:]]+fn[[:space:]]+nth_local_endpoint_selector|pub\\(crate\\)[[:space:]]+const[[:space:]]+fn[[:space:]]+local_endpoint_selector_count" \
  "endpoint operation validation must stay selector/evidence based" \
  src/global
check_absent_multiline "#\\[derive\\(Clone,[[:space:]]*Copy\\)\\][[:space:]]*pub\\(crate\\)[[:space:]]+struct[[:space:]]+(ProgramSourceData|EffList|RoleLaneScratch|RoleImageBytes|ProgramImageBytes)" \
  "const accumulator Copy derive creates associated-const copy explosion" \
  src/g/source.rs src/global/const_dsl.rs src/global/role_program/image_types.rs src/global/compiled/images/image/blob_storage.rs
check_absent "FrameLabelScratch|frame_key_targets|frame_key_lanes|frame_key_counts|previous\\.to[[:space:]]*==[[:space:]]*atom\\.to[[:space:]]*&&[[:space:]]*previous\\.lane[[:space:]]*==[[:space:]]*atom\\.lane" \
  "frame-label assignment must stay on global FrameLabelKey authority" \
  src/global
check_absent "\\bRouteControllerArm\\b|\\bParallelLaneShape\\b" \
  "route/par forbidden witness name" \
  src/global.rs
check_absent "\\bParallelFragment\\b" \
  "parallel empty-arm forbidden semantic witness name" \
  src/global.rs \
  src/global/program.rs
check_absent "\\bStepNonEmpty\\b" \
  "parallel empty-arm witness forbidden path" \
  src/global/steps.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "projection surface hygiene check passed"
