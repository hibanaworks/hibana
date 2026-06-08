#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if [[ -e "src/sync.rs" ]]; then
  echo "boundary deny pattern detected: runtime sync shim" >&2
  FAILED=1
fi

source ./.github/scripts/lib/hygiene_common.sh

check_absent \
  "mod[[:space:]]+sync;|crate::sync" \
  "runtime fake-sync shim reintroduced" \
  src/lib.rs src/integration.rs src/endpoint.rs src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "poll_fn|async move|stash_pending_branch_preview|take_pending_branch_preview" \
  "localside wrapper-future or endpoint-stashed preview residue reintroduced" \
  src/endpoint.rs \
  src/endpoint/kernel/recv.rs \
  src/endpoint/kernel/decode.rs \
  src/endpoint/kernel/offer.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "production runtime atomics reintroduced" \
  src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "test/runtime atomics reintroduced" \
  src tests

check_absent \
  "validate_sendable_message|assert_sendable|label exceeds universe|0\\.\\.=127|128 labels|LABEL_MAX:[[:space:]]*u8[[:space:]]*=[[:space:]]*127" \
  "obsolete half-range label universe or no-op sendability guard reintroduced" \
  src tests README.md

check_absent \
  "custom demux and channel adapters|decode adapters only|large temporary" \
  "renamed adapter/temporary wording residue" \
  src README.md

check_absent \
  "FrameLabel::new\\([[:digit:]]+\\)" \
  "README must not teach user-chosen numeric frame labels" \
  README.md

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

bash ./.github/scripts/check_surface_test_alias_hygiene.sh

TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS="$(
  (
    rg -n -U "StaticSlot<[^\\n;]*(Endpoint<|RouteBranch<)" tests || true
  ) | rg -v "tests/(integration_surface|public_surface_guards)\\.rs:" || true
)"
if [[ -n "${TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS}" ]]; then
  echo "${TEST_FIXTURE_TYPED_HANDLE_STATIC_SLOTS}" >&2
  echo "boundary deny pattern detected: typed handle static slot" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_CLUSTER_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*SessionCluster<" tests || true
)"
if [[ -n "${TEST_FIXTURE_PURE_CLUSTER_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PURE_CLUSTER_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture pure cluster alias" >&2
  FAILED=1
fi

TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*<.*as[[:space:]]+ProjectRole<.*>::Output;" tests || true
)"
if [[ -n "${TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_PROJECT_ROLE_OUTPUT_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture project-role output alias" >&2
  FAILED=1
fi

TEST_FIXTURE_IMPORT_ALIASES="$(
  rg -n -U "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" tests || true
)"
if [[ -n "${TEST_FIXTURE_IMPORT_ALIASES}" ]]; then
  echo "${TEST_FIXTURE_IMPORT_ALIASES}" >&2
  echo "boundary deny pattern detected: test fixture import alias shim" >&2
  FAILED=1
fi

TEST_FIXTURE_PURE_PROGRAM_ALIASES="$(
  rg -n "^const[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<[^>]+>[[:space:]]*=[[:space:]]*[A-Z][A-Z0-9_]*;" tests || true
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
  rg -n -U '(The exact projected `LocalSteps` type is part of the contract\.|Do not erase `LocalSteps`\.|use hibana::(g::advanced|integration::program)::steps::\{ProjectRole, SendStep, StepCons, StepNil\};|as[[:space:]]+ProjectRole<)' \
    README.md || true
)"
if [[ -n "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" ]]; then
  echo "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" >&2
  echo "boundary deny pattern detected: README old projected-local walkthrough" >&2
  FAILED=1
fi

README_DUAL_PAYLOAD_STORY="$(
  rg -n -U 'owned default path|`WireDecode`|WireEncode` and either `WireDecode`|integration::wire::\{Payload,[[:space:]]*WireDecode,' \
    README.md || true
)"
if [[ -n "${README_DUAL_PAYLOAD_STORY}" ]]; then
  echo "${README_DUAL_PAYLOAD_STORY}" >&2
  echo "boundary deny pattern detected: README dual payload story" >&2
  FAILED=1
fi

CORE_OWNED_MGMT_EPF_DOCS="$(
  rg -n -U '`hibana::integration::mgmt|`hibana::integration::resolver::epf' \
    README.md || true
)"
if [[ -n "${CORE_OWNED_MGMT_EPF_DOCS}" ]]; then
  echo "${CORE_OWNED_MGMT_EPF_DOCS}" >&2
  echo "boundary deny pattern detected: core-owned mgmt/epf doc wording" >&2
  FAILED=1
fi

IN_TREE_CROSS_REPO_HARNESS_DOCS="$(
  rg -n -U '`integration/cross-repo/`|staging location for cross-repo smoke' \
    README.md || true
)"
if [[ -n "${IN_TREE_CROSS_REPO_HARNESS_DOCS}" ]]; then
  echo "${IN_TREE_CROSS_REPO_HARNESS_DOCS}" >&2
  echo "boundary deny pattern detected: in-tree cross-repo harness wording" >&2
  FAILED=1
fi

README_OLD_PROGRAM_ITEM_PATH="$(
  rg -n -U '(App code writes `APP: g::Program<_>`|project\(&PROGRAM\)|(const|static)[[:space:]]+(APP|PROGRAM)[[:space:]]*:[[:space:]]*(g::)?Program<_>)' \
    README.md || true
)"
if [[ -n "${README_OLD_PROGRAM_ITEM_PATH}" ]]; then
  echo "${README_OLD_PROGRAM_ITEM_PATH}" >&2
  echo "boundary deny pattern detected: README/spec old Program item path" >&2
  FAILED=1
fi

EXAMPLE_OWNER_HIDING_ALIASES="$(
  if [[ -e examples ]]; then
    rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(Role<|Msg<|RoleProgram<|Endpoint<|SessionCluster<|g::Program<|StepCons<|SeqSteps<|LoopContinueSteps<|LoopBreakSteps<|LoopDecisionSteps<|<.*as[[:space:]]+ProjectRole<|<.*as[[:space:]]+StepConcat<)" \
      examples || true
  fi
)"
if [[ -n "${EXAMPLE_OWNER_HIDING_ALIASES}" ]]; then
  echo "${EXAMPLE_OWNER_HIDING_ALIASES}" >&2
  echo "boundary deny pattern detected: example owner-hiding type alias" >&2
  FAILED=1
fi

EXAMPLE_IMPORT_ALIASES="$(
  if [[ -e examples ]]; then
    rg -n -U "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" \
      examples || true
  fi
)"
if [[ -n "${EXAMPLE_IMPORT_ALIASES}" ]]; then
  echo "${EXAMPLE_IMPORT_ALIASES}" >&2
  echo "boundary deny pattern detected: example import alias shim" >&2
  FAILED=1
fi

EXAMPLE_UNDERSCORE_CASTS="$(
  if [[ -e examples ]]; then
    rg -n "\\sas _([,;)|]|$)|as \\*const _|as \\*mut _" examples || true
  fi
)"
if [[ -n "${EXAMPLE_UNDERSCORE_CASTS}" ]]; then
  echo "${EXAMPLE_UNDERSCORE_CASTS}" >&2
  echo "boundary deny pattern detected: example underscore cast shim" >&2
  FAILED=1
fi

EXAMPLE_ESCAPE_HATCHES="$(
  if [[ -e examples ]]; then
    rg -n "#\\[doc\\(hidden\\)\\]|#\\[allow\\(dead_code\\)\\]|\\bfallback\\b" examples || true
  fi
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
  echo "boundary deny pattern detected: source-test owner-hiding type alias" >&2
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
  rg -n -U "fn[[:space:]]+requeue<'a>\\(&'a self, rx: &'a mut Self::Rx<'a>\\)[[:space:]]*\\{[[:space:]]*debug_assert!\\(core::ptr::eq\\(rx, rx\\)\\);[[:space:]]*\\}|fn[[:space:]]+metrics\\(&self\\)[[:space:]]*->[[:space:]]*Self::Metrics[[:space:]]*\\{[[:space:]]*Self::Metrics::default\\(\\)[[:space:]]*\\}" \
    src/transport.rs || true
)"
if [[ -n "${TRANSPORT_TRAIT_DEFAULT_NOOPS}" ]]; then
  echo "${TRANSPORT_TRAIT_DEFAULT_NOOPS}" >&2
  echo "boundary deny pattern detected: transport trait fallback default shim" >&2
  FAILED=1
fi
check_absent "fn[[:space:]]+cancel_send<'a>\\(&self, tx: &'a mut Self::Tx<'a>\\)[[:space:]]*\\{[[:space:]]*let _ = tx;[[:space:]]*\\}" \
  "transport send cancellation must be a required transport contract, not a default no-op" \
  src/transport.rs
check_absent "fn[[:space:]]+open<'a>\\(&self, port: PortOpen\\)[[:space:]]*->[[:space:]]*\\(Self::Tx<'a>, Self::Rx<'a>\\)" \
  "Transport::open must bind Tx/Rx handles to the transport borrow" \
  src/transport.rs
if ! grep -Fq "fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);" src/transport.rs; then
  echo "transport surface violation: missing transport-borrow-bound open contract" >&2
  FAILED=1
fi

RESERVED_LABEL_CONTRACT_RESIDUE="$(
  rg -n \
    -g '!tests/docs_surface.rs' \
    -g '!*.stderr' \
    "recv_label_hint|scope_hint|ScopeLabelMeta|scope_label_meta|frame_hint_label|resolved_frame_hint_label|matches_frame_hint_label|record_arm_label|record_dispatch_arm_label|mark_scope_ready_arm_from_label|mark_scope_ready_arm_from_binding_label|scope_label_to_arm|scope_evidence_label_to_arm|binding_scope_evidence_label_to_arm|FrameLabelMask::from_label|contains_label\\(|insert_label\\(|remove_label\\(|singleton_label\\(|binding_label_masks|endpoint_binding_label_masks_bytes|pending_frame_hint_label_masks|pending_frame_hint_labels_for_lane|update_pending_frame_hint_lane_masks|first_recv_dispatch_label_mask|route_scope_first_recv_dispatch_label_mask|reserved control band|0x0300[[:space:]]*\\+[^;]*(LABEL|LOGICAL)|_reserved[[:space:]]*:|LABEL_LOOP_CONTINUE|LABEL_LOOP_BREAK|LABEL_ROUTE_DECISION|LABEL_PROTOCOL_CONTROL|WireControlKind::LABEL|K::LABEL|<[^>]+ as WireControlKind>::LABEL|Message>::LABEL|const[[:space:]]+LABEL[[:space:]]*:[[:space:]]*u8|CapHeader::label|ControlDesc::label|IngressEvidence[[:space:]]*\\{[[:space:]]*label:" \
    README.md src tests || true
)"
if [[ -n "${RESERVED_LABEL_CONTRACT_RESIDUE}" ]]; then
  echo "${RESERVED_LABEL_CONTRACT_RESIDUE}" >&2
  echo "boundary deny pattern detected: reserved control label contract residue" >&2
  FAILED=1
fi

check_absent \
  "TEST_LOOP_CONTINUE_LABEL|TEST_LOOP_BREAK_LABEL|TEST_ROUTE_DECISION_LABEL|const[[:space:]]+[A-Z0-9_]*(LABEL|LOGICAL|FRAME)[A-Z0-9_]*[[:space:]]*:[[:space:]]*u8[[:space:]]*=[[:space:]]*(48|49|57);|Msg<\\{?[[:space:]]*(48|49|57)\\b|LabelMarker<(48|49|57)>|FrameLabel::new\\([^)]*LOGICAL\\)|FrameLabelMask::from_frame_label\\([^)]*LOGICAL\\)" \
  "tests must not preserve retired 48/49/57 control-label fixtures or pass logical labels as frame labels" \
  src tests

check_absent \
  "FrameLabel::new\\([A-Z0-9_]*LABEL\\)|FrameLabelMask::from_frame_label\\([A-Z0-9_]*LABEL\\)" \
  "tests must not pass logical-label fixtures or ambiguous LABEL constants as FrameLabel values" \
  src tests

check_absent \
  "ScopeFrameHint|frame_label[[:space:]]*==[[:space:]]*0|FrameLabel::new\\(<Msg<|outgoing\\.frame_label\\(\\)\\.raw\\(\\)[[:space:]]*==[[:space:]]*<Msg<" \
  "FrameLabel/logical label conflation or zero-sentinel residue" \
  src tests

check_absent \
  "current_step_labels|event_cursor_current_step_labels|refresh_current_step_label\\(|rebuild_current_step_labels\\(" \
  "current-step logical label zero-sentinel residue" \
  src tests

check_absent \
  "first_recv_dispatch_target_for_lane_label|static_poll_route_arm_for_lane_label|find_arm_for_recv_lane_label|\\bfirst_recv_target_for_lane\\(" \
  "FIRST-recv dispatch must name physical frame-label lookup explicitly" \
  src tests

check_absent \
  "RouteDispatchEntry[[:space:]]*\\{[[:space:]]*label|entry\\.label|existing\\.label" \
  "FIRST-recv dispatch entries must store frame_label, not ambiguous label" \
  src/global/typestate/registry.rs src/global/typestate/emit_scope.rs

check_absent \
  "label not found in dispatch table|label remains to probe" \
  "FIRST-recv dispatch comments must say frame label explicitly" \
  src tests

check_absent \
  "label[[:space:]]*->[[:space:]]*continuation|label→continuation|child label evidence|child label\"|\\b_[A-Za-z0-9]*label\\b|\\b_[A-Za-z0-9]*lane\\b" \
  "FIRST-recv dispatch must not hide frame-label/lane evidence behind ambiguous label wording or underscore bindings" \
  src/endpoint/kernel src/global/typestate tests/ui/g-route-unprojectable.rs

check_absent \
  "while[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*<[[:space:]]*lane_limit" \
  "offer hot path must not compare lane sets with all-lane scans" \
  src/endpoint/kernel/core.rs src/endpoint/kernel/offer.rs

check_absent \
  "standard slice traits|EffStruct slices|EffStruct slice via standard slice traits" \
  "EffList docs must not describe the old flat slice surface" \
  src/global/const_dsl.rs

check_absent_multiline \
  "struct[[:space:]]+MsgRuntimeCore[[:space:]]*\\{[^}]*Option<[^>]*FrameLabel" \
  "runtime descriptor FrameLabel must not be optional" \
  src/endpoint/kernel/core.rs

check_absent \
  "\\b(RuntimeSpec|bind_frame_label|with_frame_label|frame_label_patch|patch_frame_label|late_frame_label)\\b|frame_label\\(\\)[[:space:]]*!=[[:space:]]*Some|frame_label\\(\\),[[:space:]]*None" \
  "runtime descriptor late FrameLabel patching residue" \
  src/endpoint tests

check_absent \
  "LabelMismatch[^{]*\\{[^}]*frame_label|LabelMismatch[^{]*\\{[^}]*FrameLabel|expected:[^,\\n]*frame_label|actual:[^,\\n]*frame_label" \
  "RecvError::LabelMismatch must remain logical-label only" \
  src/endpoint tests

check_absent \
  "kernel_recv\\(self,[[:space:]]*logical_label,[[:space:]]*accepts_empty_payload|poll_public_recv\\(logical_label,[[:space:]]*accepts_empty_payload|RecvRuntimeDesc::new\\([^,]+,[^,]+,[^,]+\\)" \
  "deterministic recv must carry control-kind evidence into complete runtime descriptor" \
  src/endpoint src/endpoint/kernel

check_absent \
  "prior_atom\\.label|atom\\.label[[:space:]]*==[[:space:]]*label|label[[:space:]]*=[[:space:]]*current\\.label" \
  "FrameLabel allocation must be edge-unique, not logical-label deduplicated" \
  src/global/typestate/emit_walk.rs

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
  echo "boundary deny pattern detected: resolver replay provider fallback default shim" >&2
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

CONTROL_AUTOMATON_DEFAULT_GRAPH="$(
  rg -n -U "fn[[:space:]]+run<'lease|default this forwards to|Self::run\\(lease,[[:space:]]*seed\\)" \
    src/control/lease/core.rs || true
)"
if [[ -n "${CONTROL_AUTOMATON_DEFAULT_GRAPH}" ]]; then
  echo "${CONTROL_AUTOMATON_DEFAULT_GRAPH}" >&2
  echo "boundary deny pattern detected: control automaton fallback/default graph path" >&2
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
  rg -n -U "pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*const[[:space:]]+AUTO_MINT_EXTERNAL:[[:space:]]+bool[[:space:]]*=[[:space:]]*false[[:space:]]*;|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+caps_mask\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*CapsMask[[:space:]]*\\{[[:space:]]*CapsMask::empty\\(\\)[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+scope_id\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*Option<ScopeId>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+handle_scope\\(" \
    src/control/cap || true
)"
if [[ -n "${RESOURCE_KIND_DEFAULT_HELPERS}" ]]; then
  echo "${RESOURCE_KIND_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: resource kind fallback default shim" >&2
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
  "endpoint control descriptor shorthand alias shim" \
  src/endpoint/kernel/core.rs \
  src/endpoint/flow.rs
check_absent "mem::transmute::<Guard<'_>, Guard<'static>>|mem::transmute::<Guard<'static>, Guard<'rv>>|core::mem::transmute::<_, Port<'cfg, T, crate::control::cap::mint::EpochTbl>>" \
  "rendezvous brand or port transmute shim" \
  src/rendezvous/core.rs
check_absent "transmute::<usize, fn\\(u32\\)>" \
  "observe timestamp-checker transmute shim" \
  src/observe/core.rs
check_absent "LeaseObserve|from_resident_tap|commit_event: Option<TapEvent>|rollback_event: Option<TapEvent>" \
  "unused lease observe/tap authority" \
  src/control/lease/core.rs src/control/lease/bundle.rs
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
  src README.md
check_absent "\\b(test_from_slice|bind_test_storage)\\b" \
  "named cfg-test constructor/helper residue in production source" \
  src
check_absent "SessionKit::new|pub[[:space:]]+fn[[:space:]]+new\\(clock:|init_in_place\\([^)]*clock:" \
  "owned or clock-bearing SessionKit construction must not be reintroduced" \
  src README.md .github/allowlists/integration-public-api.txt
check_absent "\\b(LoopContinueSteps|LoopBreakSteps|LoopDecisionSteps)\\b" \
  "loop control step alias residue in production source" \
  src/global/steps.rs
check_absent "\\bEndpointBinding\\b" \
  "endpoint binding synonym alias residue in production source" \
  src/endpoint.rs src/endpoint/flow.rs src/endpoint/carrier.rs
check_absent "pub[[:space:]]+struct[[:space:]]+StateIndex|pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(new|from_usize|raw|as_usize|is_max)\\(" \
  "typestate StateIndex flat-index helpers must remain crate-private" \
  src/global/typestate/facts.rs
check_absent "\\b(RouteResolutionOutcome|LoopResolutionOutcome)\\b" \
  "resolver result alias residue in production source" \
  src/control/cluster/core.rs
check_absent "TransportAlgorithm,[[:space:]]*TransportError|TransportError,[[:space:]]*TransportEvent" \
  "transport observation detail re-exported from the daily integration transport bucket" \
  src/integration.rs .github/allowlists/integration-public-api.txt
check_absent "\\b(LocalDirection|SendMeta)\\b" \
  "transport send metadata detail re-exported from the daily integration transport bucket" \
  src/integration.rs .github/allowlists/integration-public-api.txt
check_absent "\\bTransportMetricsTapPayload\\b" \
  "transport tap packing payload leaked into public integration surface" \
  src/integration.rs .github/allowlists/integration-public-api.txt
check_absent "\\bTransportAlgorithm\\b" \
  "transport algorithm enum leaked into public integration surface" \
  src/integration.rs .github/allowlists/integration-public-api.txt
BINDING_BLOCK="$(
  awk '
    /^pub mod binding \{/ { inside=1 }
    inside {
      print
      if ($0 ~ /^}/) { exit }
    }
  ' src/integration/buckets.rs
)"
if [[ -n "${BINDING_BLOCK}" ]]; then
  echo "boundary deny pattern detected: integration binding bucket reintroduced instead of transport-owned ingress" >&2
  FAILED=1
fi
check_absent "\\bTransportOpsError\\b|\\bhas_fin\\b|\\bProtocol\\(u64\\)|\\bWriteFailed\\b|\\bOpenFailed\\b" \
  "protocol-specific binding vocabulary leaked into hibana surface" \
  src README.md .github/allowlists/integration-public-api.txt
RESOLVER_BLOCK="$(
  awk '
    /^pub mod resolver \{/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^\/\/\/ Canonical capability-token surface/) { exit }
    }
  ' src/integration/buckets.rs
)"
if [[ -z "${RESOLVER_BLOCK}" ]]; then
  echo "integration resolver block not found" >&2
  FAILED=1
elif printf '%s\n' "${RESOLVER_BLOCK}" | rg -n "pub[[:space:]]+mod[[:space:]]+advanced[[:space:]]*\\{" >/dev/null; then
  echo "boundary deny pattern detected: integration resolver advanced compatibility bucket" >&2
  FAILED=1
else
  for required in \
    "ResolverRef"
  do
    if ! printf '%s\n' "${RESOLVER_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "integration resolver resolver surface missing: ${required}" >&2
      FAILED=1
    fi
  done
  for forbidden in \
    "ResolverContext" \
    "ContextId" \
    "ContextValue" \
    "PolicyAttrs" \
    "PolicySignals," \
    "PolicySlot" \
    "pub mod replay {"
  do
    if printf '%s\n' "${RESOLVER_BLOCK}" | rg -n -F "${forbidden}" >/dev/null; then
      echo "boundary deny pattern detected: integration resolver root replay metadata leak: ${forbidden}" >&2
      FAILED=1
    fi
  done
fi

for forbidden in \
  "PolicyInput" \
  "PolicyAttrs" \
  "PolicySignals," \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "pub mod core {" \
  "pub mod replay {" \
  "advanced::policy"
do
  if rg -n -F "${forbidden}" src/integration/buckets.rs >/dev/null; then
    echo "boundary deny pattern detected: integration resolver replay internals leak: ${forbidden}" >&2
    FAILED=1
  fi
done
check_absent "TransportEventMeta|pub[[:space:]]+(kind|packet_number|payload_len|retransmissions|pn_space|cid_tag):|pub[[:space:]]+(primary|extension):|pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(new_with_metadata|with_pn_space|with_cid_tag|payload_len|retry_count|domain|carrier_tag)\\b" \
  "transport observation detail must stay protocol-neutral and non-extension" \
  src/transport.rs
check_absent "\\bTransportSnapshotParts\\b|from_parts\\(parts:" \
  "transport snapshot option-bag constructor reintroduced" \
  src/transport.rs
check_absent "\\bConfigParts\\b|config\\.into_parts\\(\\)" \
  "runtime config decomposition bag reintroduced" \
  src/runtime/config.rs src/rendezvous/core.rs
check_absent "\\bRegisteredTokenParts\\b|RawRegisteredCapToken::from_parts|take_registered_parts" \
  "registered capability token transfer bag reintroduced" \
  src/control/cap.rs src/endpoint
check_absent "pub[[:space:]]+bytes:[[:space:]]*\\[u8;[[:space:]]*CAP_TOKEN_LEN\\]|fn[[:space:]]+from_parts\\(" \
  "generic capability token wire layout part constructor reintroduced" \
  src/control/cap/mint.rs
check_absent "pub[[:space:]]+fn[[:space:]]+(nonce|tag|control_header|shot|handle_bytes|handle_bytes_ref)\\(&self\\)" \
  "generic capability token low-level accessor leaked as public API" \
  src/control/cap/mint.rs
check_absent "use crate::control::types::\\{RendezvousId, SessionId\\}" \
  "integration root must route identifier signatures through integration::ids" \
  src/integration.rs
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
  "route/par stale witness name" \
  src/global.rs
check_absent "\\bParallelFragment\\b" \
  "parallel empty-arm stale semantic witness name" \
  src/global.rs \
  src/global/program.rs
check_absent "\\bStepNonEmpty\\b" \
  "parallel empty-arm witness shim" \
  src/global/steps.rs
check_absent "(?i)\\b(quic|h3|hq|qpack|alpn)\\b|http/3" \
  "protocol-specific vocabulary in hibana/src" \
  src


bash ./.github/scripts/check_endpoint_surface_owner.sh


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
  paths=(README.md tests src)
  if [[ -e examples ]]; then
    paths+=(examples)
  fi
  rg -n \
    -g '!tests/docs_surface.rs' \
    -g '!tests/ui/const_program_placeholder.rs' \
    -g '!tests/ui/static_program_placeholder.rs' \
    -g '!tests/ui/*.stderr' \
    -g '!src/global/program.rs' \
    "(const|static)[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<_" \
    "${paths[@]}" || true
)"
if [[ -n "${ITEM_LEVEL_PROGRAM_PLACEHOLDER_RESIDUE}" ]]; then
  echo "${ITEM_LEVEL_PROGRAM_PLACEHOLDER_RESIDUE}" >&2
  echo "boundary deny pattern detected: item-level inferred Program placeholder reintroduced" >&2
  FAILED=1
fi

check_absent \
  "(g::advanced::steps|integration::program::steps)" \
  "public step names reintroduced in docs/examples" \
  README.md

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "surface hygiene check passed"
