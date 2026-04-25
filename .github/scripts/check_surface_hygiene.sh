#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if [[ -e "src/sync.rs" ]]; then
  echo "boundary deny pattern detected: runtime sync shim" >&2
  FAILED=1
fi

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  for path in "$@"; do
    if [[ -e "${path}" ]]; then
      paths+=("${path}")
    fi
  done
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  if rg -n "${pattern}" "${paths[@]}"; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  for path in "$@"; do
    if [[ -e "${path}" ]]; then
      paths+=("${path}")
    fi
  done
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  if rg -n -U "${pattern}" "${paths[@]}"; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent \
  "mod[[:space:]]+sync;|crate::sync" \
  "runtime fake-sync shim reintroduced" \
  src/lib.rs src/substrate.rs src/endpoint.rs src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "poll_fn|async move|stash_pending_branch_preview|take_pending_branch_preview" \
  "localside wrapper-future or endpoint-stashed preview residue reintroduced" \
  src/endpoint.rs \
  src/endpoint/kernel/recv.rs \
  src/endpoint/kernel/decode.rs \
  src/endpoint/kernel/route_frontier/offer.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "production runtime atomics reintroduced" \
  src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "test/runtime atomics reintroduced" \
  src tests

PURE_SYNONYM_ALIASES="$(
  rg -n "^(pub\\(crate\\)[[:space:]]+)?type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*[A-Za-z0-9_]+;" \
    src || true
)"
if [[ -n "${PURE_SYNONYM_ALIASES}" ]]; then
  echo "${PURE_SYNONYM_ALIASES}" >&2
  echo "boundary deny pattern detected: pure synonym type alias" >&2
  FAILED=1
fi

PURE_PROGRAM_ALIASES="$(
  rg -n "^(pub\\(crate\\)[[:space:]]+)?const[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<[^>]+>[[:space:]]*=[[:space:]]*[A-Z][A-Z0-9_]*;" \
    src || true
)"
if [[ -n "${PURE_PROGRAM_ALIASES}" ]]; then
  echo "${PURE_PROGRAM_ALIASES}" >&2
  echo "boundary deny pattern detected: pure program alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_SYNONYM_ALIASES="$(
  rg -n "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*[A-Za-z0-9_]+;" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_SYNONYM_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_SYNONYM_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure synonym type alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_ROLE_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Role<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_ROLE_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_ROLE_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure role alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_MESSAGE_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Msg<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_MESSAGE_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_MESSAGE_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure message alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_PROGRAM_TYPE_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(g::)?Program<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_PROGRAM_TYPE_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_PROGRAM_TYPE_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure program type alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_ROLE_PROGRAM_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*RoleProgram<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_ROLE_PROGRAM_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_ROLE_PROGRAM_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure role-program alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_ENDPOINT_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*Endpoint<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_ENDPOINT_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_ENDPOINT_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure endpoint alias" >&2
  FAILED=1
fi

TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS="$(
  (
    rg -n -U "StaticSlot<[^\\n;]*(Endpoint<|RouteBranch<)" tests || true
  ) | rg -v "tests/(substrate_surface|public_surface_guards)\\.rs:" || true
)"
if [[ -n "${TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS}" ]]; then
  echo "${TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS}" >&2
  echo "boundary deny pattern detected: typed handle static slot" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_CLUSTER_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*SessionCluster<" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_CLUSTER_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_CLUSTER_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure cluster alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*<.*as[[:space:]]+ProjectRole<.*>::Output;" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture project-role output alias" >&2
  FAILED=1
fi

TEST_FIXTURE_IMPORT_ALIASES="$(
  rg -n -U "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_IMPORT_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_IMPORT_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture import alias shim" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_PROGRAM_ALIASES="$(
  rg -n "^const[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<[^>]+>[[:space:]]*=[[:space:]]*[A-Z][A-Z0-9_]*;" \
    tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_PROGRAM_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_PROGRAM_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure program alias" >&2
  FAILED=1
fi

README_PURE_ROLE_MESSAGE_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(Role<|Msg<)" \
    README.md || true
)"
if [[ -n "${README_PURE_ROLE_MESSAGE_ALIASES}" ]]; then
  echo "${README_PURE_ROLE_MESSAGE_ALIASES}" >&2
  echo "boundary deny pattern detected: README pure role/message alias" >&2
  FAILED=1
fi

README_STEP_PROJECTION_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(StepCons<|SeqSteps<|LoopContinueSteps<|LoopBreakSteps<|LoopDecisionSteps<|<.*as[[:space:]]+ProjectRole<|<.*as[[:space:]]+StepConcat<)" \
    README.md || true
)"
if [[ -n "${README_STEP_PROJECTION_ALIASES}" ]]; then
  echo "${README_STEP_PROJECTION_ALIASES}" >&2
  echo "boundary deny pattern detected: README step/projection alias" >&2
  FAILED=1
fi

README_OLD_PROJECTED_LOCAL_WALKTHROUGH="$(
  rg -n -U '(The exact projected `LocalSteps` type is part of the contract\.|Do not erase `LocalSteps`\.|use hibana::(g::advanced|substrate::program)::steps::\{ProjectRole, SendStep, StepCons, StepNil\};|as[[:space:]]+ProjectRole<)' \
    README.md || true
)"
if [[ -n "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" ]]; then
  echo "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" >&2
  echo "boundary deny pattern detected: README old projected-local walkthrough" >&2
  FAILED=1
fi

README_DUAL_PAYLOAD_STORY="$(
  rg -n -U 'owned default path|`WireDecode`|WireEncode` and either `WireDecode`|substrate::wire::\{Payload,[[:space:]]*WireDecode,' \
    README.md || true
)"
if [[ -n "${README_DUAL_PAYLOAD_STORY}" ]]; then
  echo "${README_DUAL_PAYLOAD_STORY}" >&2
  echo "boundary deny pattern detected: README dual payload story" >&2
  FAILED=1
fi

CORE_OWNED_MGMT_EPF_DOCS="$(
  rg -n -U '`hibana::substrate::mgmt|`hibana::substrate::policy::epf' \
    README.md docs/spec || true
)"
if [[ -n "${CORE_OWNED_MGMT_EPF_DOCS}" ]]; then
  echo "${CORE_OWNED_MGMT_EPF_DOCS}" >&2
  echo "boundary deny pattern detected: core-owned mgmt/epf doc wording" >&2
  FAILED=1
fi

IN_TREE_CROSS_REPO_HARNESS_DOCS="$(
  rg -n -U '`integration/cross-repo/`|staging location for cross-repo smoke' \
    README.md docs/spec || true
)"
if [[ -n "${IN_TREE_CROSS_REPO_HARNESS_DOCS}" ]]; then
  echo "${IN_TREE_CROSS_REPO_HARNESS_DOCS}" >&2
  echo "boundary deny pattern detected: in-tree cross-repo harness wording" >&2
  FAILED=1
fi

README_OLD_PROGRAM_ITEM_PATH="$(
  rg -n -U '(App code writes `APP: g::Program<_>`|project\(&PROGRAM\)|(const|static)[[:space:]]+(APP|PROGRAM)[[:space:]]*:[[:space:]]*(g::)?Program<_>)' \
    README.md docs/spec || true
)"
if [[ -n "${README_OLD_PROGRAM_ITEM_PATH}" ]]; then
  echo "${README_OLD_PROGRAM_ITEM_PATH}" >&2
  echo "boundary deny pattern detected: README/spec old Program item path" >&2
  FAILED=1
fi

EXAMPLE_OWNER_HIDING_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(Role<|Msg<|RoleProgram<|Endpoint<|SessionCluster<|g::Program<|StepCons<|SeqSteps<|LoopContinueSteps<|LoopBreakSteps<|LoopDecisionSteps<|<.*as[[:space:]]+ProjectRole<|<.*as[[:space:]]+StepConcat<)" \
    examples || true
)"
if [[ -n "${EXAMPLE_OWNER_HIDING_ALIASES}" ]]; then
  echo "${EXAMPLE_OWNER_HIDING_ALIASES}" >&2
  echo "boundary deny pattern detected: example owner-hiding type alias" >&2
  FAILED=1
fi

EXAMPLE_IMPORT_ALIASES="$(
  rg -n -U "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" \
    examples || true
)"
if [[ -n "${EXAMPLE_IMPORT_ALIASES}" ]]; then
  echo "${EXAMPLE_IMPORT_ALIASES}" >&2
  echo "boundary deny pattern detected: example import alias shim" >&2
  FAILED=1
fi

EXAMPLE_UNDERSCORE_CASTS="$(
  rg -n "\\sas _([,;)|]|$)|as \\*const _|as \\*mut _" examples || true
)"
if [[ -n "${EXAMPLE_UNDERSCORE_CASTS}" ]]; then
  echo "${EXAMPLE_UNDERSCORE_CASTS}" >&2
  echo "boundary deny pattern detected: example underscore cast shim" >&2
  FAILED=1
fi

EXAMPLE_ESCAPE_HATCHES="$(
  rg -n "#\\[doc\\(hidden\\)\\]|#\\[allow\\(dead_code\\)\\]|\\bfallback\\b" examples || true
)"
if [[ -n "${EXAMPLE_ESCAPE_HATCHES}" ]]; then
  echo "${EXAMPLE_ESCAPE_HATCHES}" >&2
  echo "boundary deny pattern detected: example escape hatch residue" >&2
  FAILED=1
fi

INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES="$(
  rg -n -U "^[[:space:]]*type[[:space:]]+(Sender|Receiver|DataMsg|BodySteps|LoopContinueArm|LoopBreakArm|LoopDecision|SenderLocal|ControllerLoopLocal|TargetLoopLocal|LoopBodySteps|LocalMsg|LocalSteps|LocalProjection|HintController|HintWorker|HintLeftControlMsg|HintRightControlMsg|HintLeftDataMsg|HintRightDataMsg|HintLeftControlStep|HintLeftDataStep|HintLeftArmSteps|HintRightControlStep|HintRightDataStep|HintRightArmSteps|HintRouteSteps|HintControllerLocal|HintWorkerLocal|EntryController|EntryWorker|EntryArm0LocalMsg|EntryArm0SignalMsg|EntryArm0WireMsg|EntryArm1LocalMsg|EntryArm1SignalMsg|EntryArm1WireMsg|EntryArm0LocalStep|EntryArm0SignalStep|EntryArm0WireStep|EntryArm0Tail|EntryArm0Steps|EntryArm1LocalStep|EntryArm1SignalStep|EntryArm1WireStep|EntryArm1Tail|EntryArm1Steps|EntryRouteSteps|EntryControllerLocal|EntryWorkerLocal|Client|Server|RequestStep|ResponseStep|StepsGlobal|ClientLocal|ServerLocal|CancelMsg|CancelSteps|CancelLocal|PrefixSteps|MiddleSteps|AppSteps|MiddleAppSteps|ChainSteps|ChainLocal|PingStep|PongStep|ParallelSteps|ContinueMsg|BreakMsg|TickMsg|RequestMsg|ResponseMsg|Lane0Steps|ContinueControlStep|TickStep|ContinueArm|BreakArm|RouteLaneSteps|ParallelRouteSteps|Actor)[[:space:]]*=" \
    src/global/const_dsl.rs \
    src/global/typestate.rs \
    src/global/role_program.rs \
    src/endpoint/kernel/core.rs \
    src/control/cluster/effects.rs || true
)"
if [[ -n "${INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES}" ]]; then
  echo "${INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES}" >&2
  echo "boundary deny pattern detected: internal source-test owner-hiding type alias" >&2
  FAILED=1
fi

STACK_BACKED_TAP_STORAGE_SHIM="$(
  rg -n "Box::new\\(\\[TapEvent::default\\(\\);[[:space:]]*RING_EVENTS\\]\\)" \
    tests/support/runtime.rs || true
)"
if [[ -n "${STACK_BACKED_TAP_STORAGE_SHIM}" ]]; then
  echo "${STACK_BACKED_TAP_STORAGE_SHIM}" >&2
  echo "boundary deny pattern detected: stack-backed tap storage shim" >&2
  FAILED=1
fi

STACK_TUNING_HELPER_SHIM="$(
  rg -n -g '!tests/huge_choreography_runtime.rs' "stack_size\\(" src tests || true
)"
if [[ -n "${STACK_TUNING_HELPER_SHIM}" ]]; then
  echo "${STACK_TUNING_HELPER_SHIM}" >&2
  echo "boundary deny pattern detected: explicit stack tuning helper" >&2
  FAILED=1
fi

if [[ -e tests/support/large_stack_sync.rs || -e tests/support/large_stack_async.rs ]]; then
  echo "boundary deny pattern detected: deleted large-stack support module restored" >&2
  FAILED=1
fi

PUBLIC_TEST_UTILS_FEATURE="$(
  rg -n "^[[:space:]]*test-utils[[:space:]]*=[[:space:]]*\\[\\]" Cargo.toml || true
)"
if [[ -n "${PUBLIC_TEST_UTILS_FEATURE}" ]]; then
  echo "${PUBLIC_TEST_UTILS_FEATURE}" >&2
  echo "boundary deny pattern detected: public test-utils feature shim" >&2
  FAILED=1
fi

PROJECT_TURBOFISH_RESIDUE="$(
  rg -n -g '!*.stderr' "project::<[[:space:]]*[A-Za-z0-9_]" \
    README.md \
    src \
    tests || true
)"
if [[ -n "${PROJECT_TURBOFISH_RESIDUE}" ]]; then
  echo "${PROJECT_TURBOFISH_RESIDUE}" >&2
  echo "boundary deny pattern detected: project turbofish shim" >&2
  FAILED=1
fi

APP_CONSTRUCTOR_TURBOFISH_RESIDUE="$(
  rg -n -g '!*.stderr' "g::(route|par)::<[[:space:]]*[A-Za-z0-9_:<>, ]+" \
    README.md \
    src \
    tests || true
)"
if [[ -n "${APP_CONSTRUCTOR_TURBOFISH_RESIDUE}" ]]; then
  echo "${APP_CONSTRUCTOR_TURBOFISH_RESIDUE}" >&2
  echo "boundary deny pattern detected: app constructor turbofish shim" >&2
  FAILED=1
fi

CFG_GATED_NOOP_FUNCTIONS="$(
  rg -n -U "#\\[cfg\\(not\\(feature = \\\"[A-Za-z0-9_-]+\\\"\\)\\)\\][[:space:]]*\\n[[:space:]]*(pub\\(crate\\)|pub)?[[:space:]]*fn[[:space:]]+[A-Za-z0-9_]+\\([^;]*\\)[[:space:]]*\\{[[:space:]]*\\}" \
    src || true
)"
if [[ -n "${CFG_GATED_NOOP_FUNCTIONS}" ]]; then
  echo "${CFG_GATED_NOOP_FUNCTIONS}" >&2
  echo "boundary deny pattern detected: cfg-gated no-op seam" >&2
  FAILED=1
fi

TRANSPORT_TRAIT_DEFAULT_NOOPS="$(
  rg -n -U "fn[[:space:]]+requeue<'a>\\(&'a self, rx: &'a mut Self::Rx<'a>\\)[[:space:]]*\\{[[:space:]]*debug_assert!\\(core::ptr::eq\\(rx, rx\\)\\);[[:space:]]*\\}|fn[[:space:]]+drain_events\\(&self, _emit: &mut dyn FnMut\\(TransportEvent\\)\\)[[:space:]]*\\{\\}|fn[[:space:]]+recv_label_hint<'a>\\(&'a self, rx: &'a Self::Rx<'a>\\)[[:space:]]*->[[:space:]]*Option<u8>[[:space:]]*\\{[[:space:]]*debug_assert!\\(core::ptr::eq\\(rx, rx\\)\\);[[:space:]]*None[[:space:]]*\\}|fn[[:space:]]+metrics\\(&self\\)[[:space:]]*->[[:space:]]*Self::Metrics[[:space:]]*\\{[[:space:]]*Self::Metrics::default\\(\\)[[:space:]]*\\}|fn[[:space:]]+apply_pacing_update\\(&self, _interval_us: u32, _burst_bytes: u16\\)[[:space:]]*\\{\\}" \
    src/transport.rs || true
)"
if [[ -n "${TRANSPORT_TRAIT_DEFAULT_NOOPS}" ]]; then
  echo "${TRANSPORT_TRAIT_DEFAULT_NOOPS}" >&2
  echo "boundary deny pattern detected: transport trait fallback default shim" >&2
  FAILED=1
fi

TRANSPORT_METRICS_DEFAULT_NOOPS="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+TransportMetrics:[[:space:]]+Default|fn[[:space:]]+(latency_us|queue_depth|pacing_interval_us|congestion_marks|retransmissions|pto_count|srtt_us|latest_ack_pn|congestion_window|in_flight_bytes|algorithm)\\(&self\\)[[:space:]]*->[[:space:]]*Option<[^>]+>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/transport.rs || true
)"
if [[ -n "${TRANSPORT_METRICS_DEFAULT_NOOPS}" ]]; then
  echo "${TRANSPORT_METRICS_DEFAULT_NOOPS}" >&2
  echo "boundary deny pattern detected: transport metrics trait fallback default shim" >&2
  FAILED=1
fi

POLICY_SIGNALS_PROVIDER_DEFAULT_NOOPS="$(
  rg -n -U "fn[[:space:]]+signals\\(&self,[[:space:]]*[A-Za-z_][A-Za-z0-9_]*:[[:space:]]*Slot\\)[[:space:]]*->[[:space:]]*PolicySignals[[:space:]]*\\{[[:space:]]*PolicySignals::ZERO[[:space:]]*\\}" \
    src/transport/context.rs || true
)"
if [[ -n "${POLICY_SIGNALS_PROVIDER_DEFAULT_NOOPS}" ]]; then
  echo "${POLICY_SIGNALS_PROVIDER_DEFAULT_NOOPS}" >&2
  echo "boundary deny pattern detected: policy signals provider fallback default shim" >&2
  FAILED=1
fi

CORE_TRAIT_DEFAULT_HELPERS="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+ControlHandle[^}]*fn[[:space:]]+visit_delegation_links\\(&self, _f: &mut dyn FnMut\\(RendezvousId\\)\\)[[:space:]]*\\{[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+MintConfigMarker[^}]*fn[[:space:]]+as_config\\(&self\\)[[:space:]]*->[[:space:]]*MintConfig<Self::Spec,[[:space:]]*Self::Policy>[[:space:]]*\\{[[:space:]]*MintConfig::<Self::Spec,[[:space:]]*Self::Policy>::new\\(\\)[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+SessionScopedKind[^}]*fn[[:space:]]+shot\\(\\)[[:space:]]*->[[:space:]]*CapShot[[:space:]]*\\{[[:space:]]*CapShot::One[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+SendableLabel[^}]*fn[[:space:]]+assert_sendable\\(\\)[[:space:]]*\\{[[:space:]]*// Future work: enforce crash/no-send invariants here\\.[[:space:]]*\\}" \
    src/control/cap.rs \
    src/control/cap/mint.rs \
    src/global.rs || true
)"
if [[ -n "${CORE_TRAIT_DEFAULT_HELPERS}" ]]; then
  echo "${CORE_TRAIT_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: core trait fallback default shim" >&2
  FAILED=1
fi

LEASE_FACET_DEFAULT_HELPERS="$(
  rg -n -U "pub\\(crate\\)[[:space:]]+trait[[:space:]]+LeaseFacet[^}]*fn[[:space:]]+on_commit<'ctx>\\(&self,[[:space:]]*_context:[[:space:]]*&mut[[:space:]]*Self::Context<'ctx>\\)[[:space:]]*\\{[[:space:]]*\\}|pub\\(crate\\)[[:space:]]+trait[[:space:]]+LeaseFacet[^}]*fn[[:space:]]+on_rollback<'ctx>\\(&self,[[:space:]]*_context:[[:space:]]*&mut[[:space:]]*Self::Context<'ctx>\\)[[:space:]]*\\{[[:space:]]*\\}" \
    src/control/lease/graph.rs || true
)"
if [[ -n "${LEASE_FACET_DEFAULT_HELPERS}" ]]; then
  echo "${LEASE_FACET_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: lease facet fallback default shim" >&2
  FAILED=1
fi

RESOURCE_KIND_DEFAULT_HELPERS="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*const[[:space:]]+AUTO_MINT_EXTERNAL:[[:space:]]+bool[[:space:]]*=[[:space:]]*false[[:space:]]*;|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+caps_mask\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*CapsMask[[:space:]]*\\{[[:space:]]*CapsMask::empty\\(\\)[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+scope_id\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*Option<ScopeId>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/control/cap/mint.rs || true
)"
if [[ -n "${RESOURCE_KIND_DEFAULT_HELPERS}" ]]; then
  echo "${RESOURCE_KIND_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: resource kind fallback default shim" >&2
  FAILED=1
fi

BINDING_SLOT_DEFAULT_PROVIDER="$(
  rg -n -U "pub[[:space:]]+unsafe[[:space:]]+trait[[:space:]]+BindingSlot[^}]*fn[[:space:]]+policy_signals_provider\\(&self\\)[[:space:]]*->[[:space:]]*Option<&dyn[[:space:]]+PolicySignalsProvider>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/binding.rs || true
)"
if [[ -n "${BINDING_SLOT_DEFAULT_PROVIDER}" ]]; then
  echo "${BINDING_SLOT_DEFAULT_PROVIDER}" >&2
  echo "boundary deny pattern detected: binding slot fallback default shim" >&2
  FAILED=1
fi

WIRE_ENCODE_DEFAULT_HINT="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+WireEncode[^}]*fn[[:space:]]+encoded_len\\(&self\\)[[:space:]]*->[[:space:]]*Option<usize>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/transport/wire.rs || true
)"
if [[ -n "${WIRE_ENCODE_DEFAULT_HINT}" ]]; then
  echo "${WIRE_ENCODE_DEFAULT_HINT}" >&2
  echo "boundary deny pattern detected: wire encode trait fallback default shim" >&2
  FAILED=1
fi

TRANSPORT_TOPOLOGY_FALLBACK_SEAM="$(
  rg -n "quiesce_and_fence|resume_after" src/transport.rs || true
)"
if [[ -n "${TRANSPORT_TOPOLOGY_FALLBACK_SEAM}" ]]; then
  echo "${TRANSPORT_TOPOLOGY_FALLBACK_SEAM}" >&2
  echo "boundary deny pattern detected: transport topology fallback seam" >&2
  FAILED=1
fi

RENDEZVOUS_STACK_TEMP_SHIM="$(
  rg -n -U "let[[:space:]]+rv[[:space:]]*=[[:space:]]*Rendezvous::from_config\\(config, transport\\);[[:space:]]*self\\.add_rendezvous\\(rv\\)" \
    src/control/cluster/core.rs || true
)"
if [[ -n "${RENDEZVOUS_STACK_TEMP_SHIM}" ]]; then
  echo "${RENDEZVOUS_STACK_TEMP_SHIM}" >&2
  echo "boundary deny pattern detected: rendezvous stack temporary shim" >&2
  FAILED=1
fi

check_absent_multiline "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" \
  "source import alias shim" \
  src examples
check_absent "\\sas _([,;)|]|$)" \
  "underscore inferred cast shim" \
  src examples
check_absent "as \\*const _|as \\*mut _" \
  "underscore pointer cast shim" \
  src examples
check_absent "^[[:space:]]*const[[:space:]]+(INDEX|VALUE|LABEL):[[:space:]]+u8[[:space:]]*=[[:space:]]+(IDX|LABEL);" \
  "self-shadowing associated const shim" \
  src tests
check_absent "type[[:space:]]+Controller[[:space:]]*=[[:space:]]*Controller;" \
  "route semantic self-shadowing associated type shim" \
  src/global.rs
check_absent "#\\[allow\\(clippy::empty_loop\\)\\]|#\\[allow\\(unused_variables\\)\\]|#\\[allow\\(clippy::let_unit_value\\)\\]" \
  "stale allow shim" \
  src tests
check_absent "route_inferred|par_inferred" \
  "legacy inferred binary builder vocabulary" \
  src/global.rs \
  src/global/program.rs
check_absent "^type[[:space:]]+(LoopContinueMsg|LoopBreakMsg)[[:space:]]*=" \
  "global const-dsl loop message alias shim" \
  src/global/const_dsl.rs
check_absent "^type[[:space:]]+ControlResource<" \
  "endpoint control-resource shorthand alias shim" \
  src/endpoint/kernel/core.rs \
  src/endpoint/flow.rs
check_absent "mem::transmute::<Guard<'_>, Guard<'static>>|mem::transmute::<Guard<'static>, Guard<'rv>>|core::mem::transmute::<_, Port<'cfg, T, crate::control::cap::mint::EpochTbl>>" \
  "rendezvous brand or port transmute shim" \
  src/rendezvous/core.rs
check_absent "transmute::<usize, fn\\(u32\\)>" \
  "observe timestamp-checker transmute shim" \
  src/observe/core.rs
check_absent "LeaseObserve::new\\(observe.tap\\(\\) as \\*const _\\)|LeaseObserve::new\\(static_ring as \\*const _\\)" \
  "lease observe pointer underscore shim" \
  src/control/lease/bundle.rs
check_absent "#\\[doc\\(hidden\\)\\]" \
  "doc-hidden escape hatch" \
  src examples
check_absent "LocalSteps = steps::StepNil" \
  "RoleProgram StepNil default projection shim" \
  src/global/role_program.rs
check_absent "HIBANA_[A-Z0-9_]+" \
  "env/debug token in hibana core" \
  src
check_absent "(^|[^A-Za-z0-9_])([Qq][Uu][Ii][Cc]|[Hh]3|[Hh][Qq])([^A-Za-z0-9_]|$)" \
  "protocol-specific vocabulary in hibana core" \
  src
check_absent "fallback_|\\bfallback\\b" \
  "fallback residue vocabulary in production source" \
  src
check_absent "\\b(for_test|_for_test)\\b|test-only|Test-only|\\bcompat(ibility)?\\b|\\blegacy\\b|\\brescue\\b|\\bheuristic\\b" \
  "test-only or compatibility residue in production source" \
  src
check_absent "(?i)\\b(compat(ibility|ible)?|legacy|rescue|heuristic|fallback|state machine|infer(red|ence|s|ring)?|absorb mismatch|absorption)\\b" \
  "runtime-intelligence vocabulary residue in public docs or core source" \
  src README.md docs
check_absent "\\b(test_from_slice|bind_test_storage)\\b" \
  "named cfg-test constructor/helper residue in production source" \
  src
check_absent "\\b(LoopContinueSteps|LoopBreakSteps|LoopDecisionSteps)\\b" \
  "loop control step alias residue in production source" \
  src/global/steps.rs
check_absent "\\bEndpointBinding\\b" \
  "endpoint binding synonym alias residue in production source" \
  src/endpoint.rs src/endpoint/flow.rs src/endpoint/carrier.rs
check_absent "\\b(RouteResolutionOutcome|LoopResolutionOutcome)\\b" \
  "resolver result alias residue in production source" \
  src/control/cluster/core.rs
check_absent "TransportAlgorithm,[[:space:]]*TransportError|TransportError,[[:space:]]*TransportEvent" \
  "transport observation detail re-exported from the daily substrate transport bucket" \
  src/substrate.rs .github/allowlists/substrate-public-api.txt
check_absent "\\b(LocalDirection|SendMeta)\\b" \
  "transport send metadata detail re-exported from the daily substrate transport bucket" \
  src/substrate.rs .github/allowlists/substrate-public-api.txt
check_absent "\\bTransportMetricsTapPayload\\b" \
  "transport tap packing payload leaked into public substrate surface" \
  src/substrate.rs .github/allowlists/substrate-public-api.txt
check_absent "\\bTransportAlgorithm\\b" \
  "transport algorithm enum leaked into public substrate surface" \
  src/substrate.rs .github/allowlists/substrate-public-api.txt
check_absent "pub[[:space:]]+use[[:space:]]+crate::binding::\\{[^}]*BindingSlot[^}]*Channel|pub[[:space:]]+use[[:space:]]+crate::binding::\\{[^}]*Channel[^}]*BindingSlot" \
  "binding detail re-exported from the daily substrate binding bucket" \
  src/substrate.rs .github/allowlists/substrate-public-api.txt
POLICY_BLOCK="$(
  awk '
    /^pub mod policy \{/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^\/\/\/ Canonical capability-token surface/) { exit }
    }
  ' src/substrate.rs
)"
if [[ -z "${POLICY_BLOCK}" ]]; then
  echo "substrate policy block not found" >&2
  FAILED=1
elif printf '%s\n' "${POLICY_BLOCK}" | rg -n "pub[[:space:]]+mod[[:space:]]+advanced[[:space:]]*\\{" >/dev/null; then
  echo "boundary deny pattern detected: substrate policy advanced compatibility bucket" >&2
  FAILED=1
else
  POLICY_ROOT_BEFORE_SIGNALS="$(
    printf '%s\n' "${POLICY_BLOCK}" | awk '
      /pub mod signals \{/ { exit }
      { print }
    '
  )"
  POLICY_SIGNALS_BLOCK="$(
    printf '%s\n' "${POLICY_BLOCK}" | awk '
      /pub mod signals \{/ { in_block=1 }
      in_block { print }
    '
  )"
  for required in \
    "PolicySignalsProvider" \
    "pub mod signals {"
  do
    if ! printf '%s\n' "${POLICY_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "substrate policy resolver/provider surface missing: ${required}" >&2
      FAILED=1
    fi
  done
  for forbidden in \
    "ContextId" \
    "ContextValue" \
    "PolicyAttrs" \
    "PolicySignals," \
    "PolicySlot"
  do
    if printf '%s\n' "${POLICY_ROOT_BEFORE_SIGNALS}" | rg -n -F "${forbidden}" >/dev/null; then
      echo "boundary deny pattern detected: substrate policy root signal metadata leak: ${forbidden}" >&2
      FAILED=1
    fi
  done
  for required in \
    "pub use crate::policy_runtime::PolicySlot;" \
    "ContextId, ContextValue, PolicyAttrs, PolicySignals" \
    "pub mod core {"
  do
    if ! printf '%s\n' "${POLICY_SIGNALS_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "substrate policy signals bucket missing: ${required}" >&2
      FAILED=1
    fi
  done
fi
check_absent "pub[[:space:]]+(kind|packet_number|payload_len|retransmissions|pn_space|cid_tag):|pub[[:space:]]+(primary|extension):|pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(new_with_metadata|with_pn_space|with_cid_tag)\\b" \
  "transport observation detail must stay accessor-only and non-literal" \
  src/transport.rs
check_absent "\\bTransportSnapshotParts\\b|from_parts\\(parts:" \
  "transport snapshot option-bag constructor reintroduced" \
  src/transport.rs
check_absent "\\bConfigParts\\b|config\\.into_parts\\(\\)" \
  "runtime config decomposition bag reintroduced" \
  src/runtime/config.rs src/rendezvous/core.rs
check_absent "\\bRegisteredTokenParts\\b|RawRegisteredCapToken::from_parts|take_registered_parts" \
  "registered capability token transfer bag reintroduced" \
  src/control/cap/typed_tokens.rs src/endpoint
check_absent "pub[[:space:]]+bytes:[[:space:]]*\\[u8;[[:space:]]*CAP_TOKEN_LEN\\]|fn[[:space:]]+from_parts\\(" \
  "generic capability token wire layout part constructor reintroduced" \
  src/control/cap/mint.rs
check_absent "pub[[:space:]]+fn[[:space:]]+(nonce|tag|control_header|shot|handle_bytes|handle_bytes_ref)\\(&self\\)" \
  "generic capability token low-level accessor leaked as public API" \
  src/control/cap/mint.rs
check_absent "use crate::control::types::\\{RendezvousId, SessionId\\}" \
  "substrate root must route identifier signatures through substrate::ids" \
  src/substrate.rs
check_absent "\\b(TopologyIntent|TopologyAck)::new\\(" \
  "distributed topology constructor shim reintroduced instead of typed struct authority" \
  src/control/automaton/distributed.rs src/control/automaton/topology.rs src/control/cluster/core.rs src/rendezvous/core.rs
check_absent "\\bTopologyOperands::new\\(" \
  "topology operand constructor shim reintroduced instead of typed struct authority" \
  src/control/cluster/core.rs src/rendezvous/core.rs
check_absent "\\b(StatelessRouteResolverFn|RouteResolverStatePayload[[:space:]]*<[^>]+>[[:space:]]*=|ErasedRouteResolverStatePayload|StatelessLoopResolverFn|LoopResolverStatePayload[[:space:]]*<[^>]+>[[:space:]]*=|ErasedLoopResolverStatePayload)\\b" \
  "resolver storage type alias residue in production source" \
  src/control/cluster/core.rs
check_absent "type[[:space:]]+SessionLaneHandle[[:space:]]*=" \
  "session-lane handle tuple alias residue in production source" \
  src/control/cap/atomic_codecs.rs
check_absent "RawEmittedCapToken::from_bytes" \
  "test-only emitted-token constructor residue in production source" \
  src/endpoint
check_absent "\\b(PayloadValidator|SyntheticPayloadProvider|StageSendPayloadFn|EncodeControlHandleFn|PortStorage|GuardStorage|StoredMint)\\b" \
  "private descriptor/helper alias residue in production source" \
  src/endpoint
check_absent "EndpointArenaLayout::from_footprint\\(" \
  "test-only endpoint layout constructor alias residue in production source" \
  src/endpoint
check_absent "\\b(SlotArena|SlotStorage|SlotBundleHandle|SlotStageRecord|FACET_SLOTS|facets_slots|requires_slots|slot_arena)\\b" \
  "test-only policy slot allocator residue in production source" \
  src/rendezvous.rs src/rendezvous src/control/lease src/control/cluster/core.rs
check_absent "LeaseGraph(::|::<[^>]+>::)new\\(" \
  "test-only lease graph constructor residue in production source" \
  src/control/lease
check_absent "Self::init_empty\\(storage\\.as_mut_ptr\\(\\)\\)" \
  "test-only lease graph storage constructor residue in production source" \
  src/control/lease/graph.rs
check_absent "std::env::var_os\\(" \
  "env lookup in hibana core" \
  src
check_absent "eprintln!\\(" \
  "stderr debug trace in hibana core" \
  src
check_absent "WireFirst" \
  "WireFirst token" \
  src
check_absent "(hint|classification).*(RouteArm|PolicyVerdict::RouteArm)" \
  "hint/classification RouteArm promotion" \
  src/endpoint/kernel/core.rs \
  src/global \
  src/control
check_absent "\\bpoll_arm_from_ready_hint\\b" \
  "hint-derived Poll helper" \
  src/endpoint/kernel/core.rs
check_absent "rebuild_pending_offers|build_frontier_snapshot|select_offer_entry|lag correction|passive takeover" \
  "offer-kernel rescue shim" \
  src/endpoint/kernel/core.rs
check_absent "duplicate route label" \
  "route duplicate-label const panic residue" \
  src/global/program.rs
check_absent "\\b(RouteLabelBits|BitEq|BitsEqual|BitsCons|BitsNil|Bit0|Bit1)\\b" \
  "route label bit-table shim" \
  src/global.rs
check_absent "\\bRouteControllerArm\\b|\\bParallelLaneShape\\b" \
  "route/par stale internal witness name" \
  src/global.rs
check_absent "\\bParallelFragment\\b" \
  "parallel empty-arm stale semantic witness name" \
  src/global.rs \
  src/global/program.rs
check_absent "\\bStepNonEmpty\\b" \
  "parallel empty-arm internal witness shim" \
  src/global/steps.rs
check_absent "^type[[:space:]]+(HandshakeSteps|BodySteps|ExitSteps|ContinueControlStep|BreakControlStep|LoopContSteps|LoopBrkSteps|LoopSeq|ProtocolSteps)[[:space:]]*=" \
  "loop-lane-share step/composition alias shim" \
  tests/loop_lane_share.rs
check_absent "^type[[:space:]]+(LeftControlStep|LeftDataStep|LeftArmSteps|RightControlStep|RightDataStep|RightArmSteps|RouteSteps|TailSteps|ProtocolSteps)[[:space:]]*=" \
  "offer-decode-binding step/composition alias shim" \
  tests/offer_decode_binding_regression.rs
check_absent "^type[[:space:]]+(TickSteps|AckControlStep|AckDataStep|AckBranch|LossControlStep|LossDataStep|LossBranch|AckLossRoute|BodySteps|ContinueControlStep|ContinueArm|BreakArm|Decision|HandshakeSteps|CombinedSteps)[[:space:]]*=" \
  "nested-loop-route step/composition alias shim" \
  tests/nested_loop_route.rs
check_absent "^type[[:space:]]+(InnerLeftControlStep|InnerLeftDataStep|InnerLeftSteps|InnerRightControlStep|InnerRightDataStep|InnerRightSteps|InnerRouteSteps|OuterLeftControlStep|OuterLeftDataStep|OuterLeftTail|OuterLeftSteps|OuterRightControlStep|OuterRightDataStep|OuterRightSteps|ProtocolSteps)[[:space:]]*=" \
  "nested-route-runtime step/composition alias shim" \
  tests/nested_route_runtime.rs
check_absent "^type[[:space:]]+(LeftSteps|RightSteps|RouteSteps|LoopContSteps|LoopBrkSteps|LoopDecision|NestedLoopContinueSteps|NestedLoopSteps)[[:space:]]*=" \
  "route-dynamic-control step/composition alias shim" \
  tests/route_dynamic_control.rs
check_absent "^type[[:space:]]+(ArmAMarkerStep|ArmALoopBodySteps|ArmALoopContControlStep|ArmALoopContArm|ArmALoopBreakArm|ArmALoopDecision|ArmASteps|ArmBMarkerStep|ArmBLoopBodySteps|ArmBLoopContControlStep|ArmBLoopContArm|ArmBLoopBreakArm|ArmBLoopDecision|ArmBSteps|RouteSteps)[[:space:]]*=" \
  "route-with-internal-loops step/composition alias shim" \
  tests/route_with_internal_loops.rs
check_absent "^type[[:space:]]+(CancelSteps|CheckpointStep|RollbackStep|CheckpointSteps|BootstrapSteps)[[:space:]]*=" \
  "cancel-rollback step/composition alias shim" \
  tests/cancel_rollback.rs
check_absent "^type[[:space:]]+(WithPolicyKind|OtherPolicyKind|WithPolicySteps|WithoutPolicySteps|RouteSteps)[[:space:]]*=" \
  "ui route-policy-mismatch alias shim" \
  tests/ui/g-route-policy-mismatch.rs
check_absent "^type[[:space:]]+(Arm0ControlStep|Arm0DataStep|Arm0SameStep|Arm0Tail|Arm0Steps|Arm1ControlStep|Arm1DataStep|Arm1SameStep|Arm1ExtraStep|Arm1InnerTail|Arm1Tail|Arm1Steps|Steps)[[:space:]]*=" \
  "ui route-unprojectable alias shim" \
  tests/ui/g-route-unprojectable.rs
check_absent "struct RouteRightKind;|struct RouteArmKind<const LABEL: u8>;|struct ArmKind<const LABEL: u8>;|impl ResourceKind for RouteRightKind|impl<const LABEL: u8> ResourceKind for RouteArmKind<LABEL>|impl<const LABEL: u8> ResourceKind for ArmKind<LABEL>" \
  "manual route-control resource boilerplate" \
  tests/route_dynamic_control.rs \
  tests/nested_route_runtime.rs \
  tests/offer_decode_binding_regression.rs \
  tests/ui-pass/g-route-merged.rs \
  tests/ui-pass/g-route-static-control-basic.rs \
  tests/ui-pass/g-route-static-control-prefix-local.rs \
  tests/ui-pass/g-route-static-control-prefix-send.rs \
  tests/ui-pass/dynamic_route_defer_compiles.rs \
  tests/ui/g-route-policy-mismatch.rs \
  tests/ui/g-route-unprojectable.rs
check_absent "(?i)\\b(quic|h3|hq|qpack|alpn)\\b|http/3" \
  "protocol-specific vocabulary in hibana/src" \
  src

POLL_READY_BLOCK="$(
  awk '
    /fn poll_arm_from_ready_mask\(/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/route_frontier/scope_evidence_logic.rs
)"
if [[ -z "${POLL_READY_BLOCK}" ]]; then
  echo "poll_arm_from_ready_mask block not found" >&2
  FAILED=1
elif printf '%s\n' "${POLL_READY_BLOCK}" | rg -n "\\bscope_ready_arm_mask\\(" >/dev/null; then
  echo "poll_arm_from_ready_mask must not read demux/materialization-ready mask" >&2
  FAILED=1
fi

ROUTE_SOURCE_BLOCK="$(
  awk '
    /enum RouteDecisionSource/ { in_block=1; next }
    in_block {
      if ($0 ~ /^}/) { exit }
      print
    }
  ' src/endpoint/kernel/route_frontier/authority.rs
)"
if [[ -z "${ROUTE_SOURCE_BLOCK}" ]]; then
  echo "RouteDecisionSource enum block not found" >&2
  FAILED=1
else
  ROUTE_SOURCE_VARIANTS="$(
    {
      printf '%s\n' "${ROUTE_SOURCE_BLOCK}" \
        | rg -n "^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*,?[[:space:]]*$" \
        | awk -F: '
            {
              value=$2
              sub(/^[[:space:]]+/, "", value)
              sub(/[[:space:]]*,?[[:space:]]*$/, "", value)
              print value
            }
          '
    } || true
  )"
  if [[ -z "${ROUTE_SOURCE_VARIANTS}" ]]; then
    echo "RouteDecisionSource variants not found" >&2
    FAILED=1
  else
    BAD_ROUTE_SOURCE_VARIANTS="$(
      printf '%s\n' "${ROUTE_SOURCE_VARIANTS}" | rg -n -v "^(Ack|Resolver|Poll)$" || true
    )"
    if [[ -n "${BAD_ROUTE_SOURCE_VARIANTS}" ]]; then
      echo "${BAD_ROUTE_SOURCE_VARIANTS}" >&2
      echo "RouteDecisionSource domain violation (expected Ack|Resolver|Poll only)" >&2
      FAILED=1
    fi
  fi
fi

POLL_RUN_BLOCK="$(
  awk '
    /fn poll_run\(/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/route_frontier/offer.rs
)"
if [[ -z "${POLL_RUN_BLOCK}" ]]; then
  echo "poll_run block not found" >&2
  FAILED=1
else
  for required in \
    "self.select_scope()" \
    "OfferRunStage::ResolveToken(" \
    "self.resolve_token(" \
    "self.materialize_branch("
  do
    if ! printf '%s\n' "${POLL_RUN_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "offer kernel stage owner missing: ${required}" >&2
      FAILED=1
    fi
  done
fi

SELECT_SCOPE_BLOCK="$(
  awk '
    /fn select_scope\(&mut self\) -> RecvResult<OfferScopeSelection> \{/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/route_frontier/offer.rs
)"
if [[ -z "${SELECT_SCOPE_BLOCK}" ]]; then
  echo "select_scope block not found" >&2
  FAILED=1
elif printf '%s\n' "${SELECT_SCOPE_BLOCK}" | rg -n "poll_binding_for_offer\\(|poll_binding_any_for_offer\\(|take_scope_ack\\(|peek_scope_ack\\(|prepare_route_decision_from_resolver|materialize_branch\\(|\\.await" >/dev/null; then
  echo "select_scope stage consuming/authority regression" >&2
  FAILED=1
fi

RESOLVE_TOKEN_BLOCK="$(
  awk '
    /fn resolve_token\(/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/route_frontier/offer.rs
)"
if [[ -z "${RESOLVE_TOKEN_BLOCK}" ]]; then
  echo "resolve_token block not found" >&2
  FAILED=1
else
  for required in \
    "take_scope_ack(" \
    "peek_scope_ack(" \
    "RouteDecisionToken::from_resolver(" \
    "RouteDecisionToken::from_poll(" \
    "on_frontier_defer("
  do
    if ! printf '%s\n' "${RESOLVE_TOKEN_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "resolve_token stage owner missing: ${required}" >&2
      FAILED=1
    fi
  done
  if printf '%s\n' "${RESOLVE_TOKEN_BLOCK}" | rg -n "materialize_branch\\(|materialize_selected_arm_meta\\(" >/dev/null; then
    echo "resolve_token stage materialization regression" >&2
    FAILED=1
  fi
fi

MATERIALIZE_BRANCH_BLOCK="$(
  awk '
    /fn materialize_branch\(/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/route_frontier/offer.rs
)"
if [[ -z "${MATERIALIZE_BRANCH_BLOCK}" ]]; then
  echo "materialize_branch block not found" >&2
  FAILED=1
elif printf '%s\n' "${MATERIALIZE_BRANCH_BLOCK}" | rg -n "take_scope_ack\\(|peek_scope_ack\\(|prepare_route_decision_from_resolver|on_frontier_defer\\(" >/dev/null; then
  echo "materialize_branch stage authority regression" >&2
  FAILED=1
fi

check_absent "lane_route_arms:|root_frontier_state:|offer_entry_state:|scope_evidence:" \
  "core.rs reabsorbed split endpoint state owners" \
  src/endpoint/kernel/core.rs

check_absent "lane_route_arms\\[[^]]+\\][[:space:]]*=|lane_linger_counts\\[[^]]+\\][[:space:]]*=|lane_offer_state\\[[^]]+\\][[:space:]]*=" \
  "core.rs reintroduced direct route-state table mutation" \
  src/endpoint/kernel/core.rs

check_absent "offer_entry_state\\[[^]]+\\][[:space:]]*=|offer_entry_state\\.get_mut\\(|global_active_entries\\.(insert_entry|remove_entry)" \
  "core.rs reintroduced direct frontier table mutation" \
  src/endpoint/kernel/core.rs

check_absent "root_frontier_state\\[[^]]+\\][[:space:]]*=|global_frontier_observed(_epoch|_key)?[[:space:]]*=|global_offer_lane_mask[[:space:]]*=|global_offer_lane_entry_slot_masks[[:space:]]*=" \
  "core.rs reintroduced direct frontier cache mutation" \
  src/endpoint/kernel/core.rs

for forbidden in \
  "fn record_scope_ack(" \
  "fn ingest_scope_evidence_for_offer(" \
  "fn on_frontier_defer(" \
  "fn align_cursor_to_selected_scope(" \
  "fn frontier_observation_key(" \
  "fn refresh_frontier_observation_cache(" \
  "fn compose_frontier_observed_entries(" \
  "fn offer_refresh_mask(" \
  "fn next_frontier_observation_epoch(" \
  "fn offer_entry_candidate_from_observation(" \
  "fn refresh_offer_entry_state(" \
  "fn sync_lane_offer_state(" \
  "fn refresh_lane_offer_state("
do
  if rg -n -F "${forbidden}" src/endpoint/kernel/core.rs >/dev/null; then
    echo "core.rs reabsorbed split endpoint logic owners: ${forbidden}" >&2
    FAILED=1
  fi
done

if ! rg -n "mod evidence_store;|mod frontier_state;|mod route_state;" src/endpoint/kernel/mod.rs >/dev/null; then
  echo "kernel mod split owner deletion" >&2
  FAILED=1
fi

for required in \
  "src/endpoint/kernel/runtime/evidence_store.rs:pub\\(super\\) struct ScopeEvidenceTable" \
  "src/endpoint/kernel/runtime/frontier_state.rs:pub\\(super\\) struct FrontierState" \
  "src/endpoint/kernel/runtime/route_state.rs:pub\\(super\\) struct RouteState"
do
  path="${required%%:*}"
  pattern="${required#*:}"
  if ! rg -n "${pattern}" "${path}" >/dev/null; then
    echo "split endpoint owner modules missing: ${required}" >&2
    FAILED=1
  fi
done

for required in \
  'src/endpoint/kernel/mod.rs:#[path = "route_frontier/offer.rs"]' \
  'src/endpoint/kernel/route_frontier/offer.rs:fn record_scope_ack(' \
  'src/endpoint/kernel/route_frontier/offer.rs:fn on_frontier_defer(' \
  'src/endpoint/kernel/route_frontier/offer.rs:fn align_cursor_to_selected_scope(' \
  'src/endpoint/kernel/route_frontier/offer.rs:fn frontier_observation_key(' \
  'src/endpoint/kernel/route_frontier/offer.rs:fn refresh_offer_entry_state('
do
  path="${required%%:*}"
  pattern="${required#*:}"
  if ! rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "split endpoint logic owner missing: ${required}" >&2
    FAILED=1
  fi
done

HIDDEN_SRC_FILES="$(
  find src -type f | rg '/_[^/]+$' || true
)"
if [[ -n "${HIDDEN_SRC_FILES}" ]]; then
  echo "${HIDDEN_SRC_FILES}" >&2
  echo "boundary deny pattern detected: underscore source escape hatch" >&2
  FAILED=1
fi

check_absent "MaybeUninit<ErasedPublicEndpointKernel" \
  "inline public endpoint kernel storage" \
  src/control/cluster/core.rs

check_absent \
  "pub[[:space:]]+mod[[:space:]]+steps[[:space:]]*\\{" \
  "public step bucket reintroduced" \
  src/global.rs src/g.rs src

ITEM_LEVEL_PROGRAM_PLACEHOLDER_RESIDUE="$(
  rg -n \
    -g '!tests/docs_surface.rs' \
    -g '!tests/ui/const_program_placeholder.rs' \
    -g '!tests/ui/static_program_placeholder.rs' \
    -g '!tests/ui/*.stderr' \
    -g '!src/global/program.rs' \
    "(const|static)[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<_" \
    README.md docs tests examples src || true
)"
if [[ -n "${ITEM_LEVEL_PROGRAM_PLACEHOLDER_RESIDUE}" ]]; then
  echo "${ITEM_LEVEL_PROGRAM_PLACEHOLDER_RESIDUE}" >&2
  echo "boundary deny pattern detected: item-level inferred Program placeholder reintroduced" >&2
  FAILED=1
fi

check_absent \
  "(g::advanced::steps|substrate::program::steps)" \
  "public step names reintroduced in docs/examples" \
  README.md docs examples

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "surface hygiene check passed"
