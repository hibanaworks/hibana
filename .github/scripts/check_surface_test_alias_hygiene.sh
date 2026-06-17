#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

source ./.github/scripts/lib/hygiene_common.sh

check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*[A-Za-z0-9_]+;" \
  "test support pure synonym type alias" \
  tests
check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Role<" \
  "test support pure role alias" \
  tests
check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Msg<" \
  "test support pure message alias" \
  tests
check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Program<" \
  "test support pure program type alias" \
  tests
check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*RoleProgram<" \
  "test support pure role-program alias" \
  tests
check_absent_multiline "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*Endpoint<" \
  "test support pure endpoint alias" \
  tests

LEFT_REMOVED_STEP='LeftControlStep'
RIGHT_REMOVED_STEP='RightControlStep'
ARM0_REMOVED_STEP='Arm0ControlStep'
ARM1_REMOVED_STEP='Arm1ControlStep'

check_absent "^type[[:space:]]+(${LEFT_REMOVED_STEP}|LeftDataStep|LeftArmSteps|${RIGHT_REMOVED_STEP}|RightDataStep|RightArmSteps|RouteSteps|TailSteps|ProtocolSteps)[[:space:]]*=" \
  "offer-decode-binding step/composition alias forbidden path" \
  tests/offer_branch_recv_evidence.rs
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Tail|Route)[A-Za-z0-9_]*[[:space:]]*=" \
  "nested-route-runtime step/composition alias forbidden path" \
  tests/nested_route_runtime.rs
check_absent "^type[[:space:]]+(${ARM0_REMOVED_STEP}|Arm0DataStep|Arm0SameStep|Arm0Tail|Arm0Steps|${ARM1_REMOVED_STEP}|Arm1DataStep|Arm1SameStep|Arm1ExtraStep|Arm1InnerTail|Arm1Tail|Arm1Steps|Steps)[[:space:]]*=" \
  "ui route-unprojectable alias forbidden path" \
  tests/ui/g-route-unprojectable.rs
ROUTE_RIGHT_KIND='RouteRightKind'
ROUTE_ARM_KIND='RouteArmKind'
ARM_KIND='ArmKind'
check_absent "struct ${ROUTE_RIGHT_KIND};|struct ${ROUTE_ARM_KIND}<const LABEL: u8>;|struct ${ARM_KIND}<const LABEL: u8>;|impl ResourceKind for ${ROUTE_RIGHT_KIND}|impl<const LABEL: u8> ResourceKind for ${ROUTE_ARM_KIND}<LABEL>|impl<const LABEL: u8> ResourceKind for ${ARM_KIND}<LABEL>" \
  "manual route resource-kind boilerplate" \
  tests/nested_route_runtime.rs \
  tests/offer_branch_recv_evidence.rs \
  tests/ui-pass/g-route-merged.rs \
  tests/ui-pass/g-route-static-basic.rs \
  tests/ui-pass/g-route-static-prefix-local.rs \
  tests/ui-pass/g-route-static-prefix-send.rs \
  tests/ui/g-route-unprojectable.rs

if (( FAILED != 0 )); then
  exit 1
fi

echo "surface test alias hygiene check passed"
