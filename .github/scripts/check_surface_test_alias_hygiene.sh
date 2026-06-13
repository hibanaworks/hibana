#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

source ./.github/scripts/lib/hygiene_common.sh

check_test_absent_multiline() {
  local pattern="$1"
  local label="$2"
  if rg -n -U "${pattern}" tests; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*[A-Za-z0-9_]+;" \
  "test fixture pure synonym type alias"
check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Role<" \
  "test fixture pure role alias"
check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Msg<" \
  "test fixture pure message alias"
check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Program<" \
  "test fixture pure program type alias"
check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*RoleProgram<" \
  "test fixture pure role-program alias"
check_test_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*Endpoint<" \
  "test fixture pure endpoint alias"

CONTINUE_REMOVED_STEP='Continue''Con''trol''Step'
BREAK_REMOVED_STEP='Break''Con''trol''Step'
LEFT_REMOVED_STEP='Left''Con''trol''Step'
RIGHT_REMOVED_STEP='Right''Con''trol''Step'
ARM0_REMOVED_STEP='Arm0''Con''trol''Step'
ARM1_REMOVED_STEP='Arm1''Con''trol''Step'

check_absent "^type[[:space:]]+(HandshakeSteps|BodySteps|ExitSteps|${CONTINUE_REMOVED_STEP}|${BREAK_REMOVED_STEP}|LoopContSteps|LoopBrkSteps|LoopSeq|ProtocolSteps)[[:space:]]*=" \
  "loop-lane-share step/composition alias bypass path" \
  tests/loop_lane_share.rs
check_absent "^type[[:space:]]+(${LEFT_REMOVED_STEP}|LeftDataStep|LeftArmSteps|${RIGHT_REMOVED_STEP}|RightDataStep|RightArmSteps|RouteSteps|TailSteps|ProtocolSteps)[[:space:]]*=" \
  "offer-decode-binding step/composition alias bypass path" \
  tests/offer_decode_receive_evidence.rs \
  tests/offer_decode_receive_evidence
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Arm|Branch|Route|Decision)[A-Za-z0-9_]*[[:space:]]*=" \
  "nested-loop-route step/composition alias bypass path" \
  tests/nested_loop_route.rs
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Tail|Route)[A-Za-z0-9_]*[[:space:]]*=" \
  "nested-route-runtime step/composition alias bypass path" \
  tests/nested_route_runtime.rs
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Route|Decision)[A-Za-z0-9_]*[[:space:]]*=" \
  "route-dynamic-resolver step/composition alias bypass path" \
  tests/route_dynamic_control.rs \
  tests/route_dynamic_control
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Arm|Route|Decision)[A-Za-z0-9_]*[[:space:]]*=" \
  "route-with-internal-loops step/composition alias bypass path" \
  tests/route_with_internal_loops.rs
check_absent "^type[[:space:]]+(WithResolverKind|OtherResolverKind|WithResolverSteps|WithoutResolverSteps|RouteSteps)[[:space:]]*=" \
  "ui route-resolver-mismatch alias bypass path" \
  tests/ui/g-route-resolver-mismatch.rs
check_absent "^type[[:space:]]+(${ARM0_REMOVED_STEP}|Arm0DataStep|Arm0SameStep|Arm0Tail|Arm0Steps|${ARM1_REMOVED_STEP}|Arm1DataStep|Arm1SameStep|Arm1ExtraStep|Arm1InnerTail|Arm1Tail|Arm1Steps|Steps)[[:space:]]*=" \
  "ui route-unprojectable alias bypass path" \
  tests/ui/g-route-unprojectable.rs
ROUTE_RIGHT_KIND='RouteRight''Kind'
ROUTE_ARM_KIND='RouteArm''Kind'
ARM_KIND='Arm''Kind'
check_absent "struct ${ROUTE_RIGHT_KIND};|struct ${ROUTE_ARM_KIND}<const LABEL: u8>;|struct ${ARM_KIND}<const LABEL: u8>;|impl ResourceKind for ${ROUTE_RIGHT_KIND}|impl<const LABEL: u8> ResourceKind for ${ROUTE_ARM_KIND}<LABEL>|impl<const LABEL: u8> ResourceKind for ${ARM_KIND}<LABEL>" \
  "manual route resource-kind boilerplate" \
  tests/route_dynamic_control.rs \
  tests/route_dynamic_control \
  tests/nested_route_runtime.rs \
  tests/offer_decode_receive_evidence.rs \
  tests/offer_decode_receive_evidence \
  tests/ui-pass/g-route-merged.rs \
  tests/ui-pass/g-route-static-basic.rs \
  tests/ui-pass/g-route-static-prefix-local.rs \
  tests/ui-pass/g-route-static-prefix-send.rs \
  tests/ui-pass/dynamic_route_defer_compiles.rs \
  tests/ui/g-route-resolver-mismatch.rs \
  tests/ui/g-route-unprojectable.rs

if (( FAILED != 0 )); then
  exit 1
fi

echo "surface test alias hygiene check passed"
