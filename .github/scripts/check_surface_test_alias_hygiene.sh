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

check_absent "^type[[:space:]]+(HandshakeSteps|BodySteps|ExitSteps|ContinueControlStep|BreakControlStep|LoopContSteps|LoopBrkSteps|LoopSeq|ProtocolSteps)[[:space:]]*=" \
  "loop-lane-share step/composition alias shim" \
  tests/loop_lane_share.rs
check_absent "^type[[:space:]]+(LeftControlStep|LeftDataStep|LeftArmSteps|RightControlStep|RightDataStep|RightArmSteps|RouteSteps|TailSteps|ProtocolSteps)[[:space:]]*=" \
  "offer-decode-binding step/composition alias shim" \
  tests/offer_decode_receive_evidence.rs \
  tests/offer_decode_receive_evidence
check_absent "^type[[:space:]]+(TickSteps|AckControlStep|AckDataStep|AckBranch|LossControlStep|LossDataStep|LossBranch|AckLossRoute|BodySteps|ContinueControlStep|ContinueArm|BreakArm|Decision|HandshakeSteps|CombinedSteps)[[:space:]]*=" \
  "nested-loop-route step/composition alias shim" \
  tests/nested_loop_route.rs
check_absent "^type[[:space:]]+(InnerLeftControlStep|InnerLeftDataStep|InnerLeftSteps|InnerRightControlStep|InnerRightDataStep|InnerRightSteps|InnerRouteSteps|OuterLeftControlStep|OuterLeftDataStep|OuterLeftTail|OuterLeftSteps|OuterRightControlStep|OuterRightDataStep|OuterRightSteps|ProtocolSteps)[[:space:]]*=" \
  "nested-route-runtime step/composition alias shim" \
  tests/nested_route_runtime.rs
check_absent "^type[[:space:]]+(LeftSteps|RightSteps|RouteSteps|LoopContSteps|LoopBrkSteps|LoopDecision|NestedLoopContinueSteps|NestedLoopSteps)[[:space:]]*=" \
  "route-dynamic-control step/composition alias shim" \
  tests/route_dynamic_control.rs \
  tests/route_dynamic_control
check_absent "^type[[:space:]]+(ArmAMarkerStep|ArmALoopBodySteps|ArmALoopContControlStep|ArmALoopContArm|ArmALoopBreakArm|ArmALoopDecision|ArmASteps|ArmBMarkerStep|ArmBLoopBodySteps|ArmBLoopContControlStep|ArmBLoopBreakArm|ArmBLoopDecision|ArmBSteps|RouteSteps)[[:space:]]*=" \
  "route-with-internal-loops step/composition alias shim" \
  tests/route_with_internal_loops.rs
check_absent "^type[[:space:]]+(WithPolicyKind|OtherPolicyKind|WithPolicySteps|WithoutPolicySteps|RouteSteps)[[:space:]]*=" \
  "ui route-policy-mismatch alias shim" \
  tests/ui/g-route-policy-mismatch.rs
check_absent "^type[[:space:]]+(Arm0ControlStep|Arm0DataStep|Arm0SameStep|Arm0Tail|Arm0Steps|Arm1ControlStep|Arm1DataStep|Arm1SameStep|Arm1ExtraStep|Arm1InnerTail|Arm1Tail|Arm1Steps|Steps)[[:space:]]*=" \
  "ui route-unprojectable alias shim" \
  tests/ui/g-route-unprojectable.rs
check_absent "struct RouteRightKind;|struct RouteArmKind<const LABEL: u8>;|struct ArmKind<const LABEL: u8>;|impl ResourceKind for RouteRightKind|impl<const LABEL: u8> ResourceKind for RouteArmKind<LABEL>|impl<const LABEL: u8> ResourceKind for ArmKind<LABEL>" \
  "manual route control descriptor boilerplate" \
  tests/route_dynamic_control.rs \
  tests/route_dynamic_control \
  tests/nested_route_runtime.rs \
  tests/offer_decode_receive_evidence.rs \
  tests/offer_decode_receive_evidence \
  tests/ui-pass/g-route-merged.rs \
  tests/ui-pass/g-route-static-control-basic.rs \
  tests/ui-pass/g-route-static-control-prefix-local.rs \
  tests/ui-pass/g-route-static-control-prefix-send.rs \
  tests/ui-pass/dynamic_route_defer_compiles.rs \
  tests/ui/g-route-policy-mismatch.rs \
  tests/ui/g-route-unprojectable.rs

if (( FAILED != 0 )); then
  exit 1
fi

echo "surface test alias hygiene check passed"
