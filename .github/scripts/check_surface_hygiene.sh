#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if [[ -e "src/sync.rs" ]]; then
  echo "boundary deny pattern detected: runtime sync forbidden path" >&2
  FAILED=1
fi

source ./.github/scripts/lib/hygiene_common.sh

OLD_WORD='leg''acy'
MODE_WORD='comp''at'
ALT_WORD='fall''back'
RECOVERY_WORD='res''cue'
GUESS_WORD='heur''istic'
UNIVERSE_WORD='uni''verse'
RECONFIG_TOKEN='fe''nce'

FORBIDDEN_ROLE_TOKEN_PATTERN='g::''Role<'
FORBIDDEN_ROLE_TOKEN_MATCHES="$(
  rg -n "${FORBIDDEN_ROLE_TOKEN_PATTERN}" src tests README.md .github/scripts || true
)"
if [[ -n "${FORBIDDEN_ROLE_TOKEN_MATCHES}" ]]; then
  echo "${FORBIDDEN_ROLE_TOKEN_MATCHES}" >&2
  echo "boundary deny pattern detected: forbidden g role token API residue" >&2
  FAILED=1
fi

check_absent \
  "mod[[:space:]]+sync;|crate::sync" \
  "runtime sync substitute forbidden path detected" \
  src/lib.rs src/runtime.rs src/endpoint.rs src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "poll_fn|async move|stash_pending_branch_preview|take_pending_branch_preview" \
  "localside wrapper-future or endpoint-stashed preview residue detected" \
  src/endpoint.rs \
  src/endpoint/kernel/recv.rs \
  src/endpoint/kernel/decode.rs \
  src/endpoint/kernel/offer.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "production runtime atomics detected" \
  src/rendezvous/core.rs src/observe/core.rs

check_absent \
  "Atomic(Bool|U8|U16|U32|U64|Usize|Ptr)" \
  "test/runtime atomics detected" \
  src tests

check_absent \
  "validate_sendable_message|assert_sendable|label exceeds ${UNIVERSE_WORD}|0\\.\\.=127|128 labels|LABEL_MAX:[[:space:]]*u8[[:space:]]*=[[:space:]]*127" \
  "forbidden half-range label domain or empty sendability guard detected" \
  src tests README.md

check_absent \
  "custom demux and channel adap""ters|decode adap""ters only|large[[:space:]]+tem""porar(y)" \
  "renamed transport wording residue" \
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

TEST_SUPPORT_TYPED_HANDLE_STATIC_SLOTS="$(
  (
    rg -n -U "StaticSlot<[^\\n;]*(Endpoint<|RouteBranch<)" tests || true
  ) | rg -v "tests/(runtime_surface|public_surface_guards)\\.rs:" || true
)"
if [[ -n "${TEST_SUPPORT_TYPED_HANDLE_STATIC_SLOTS}" ]]; then
  echo "${TEST_SUPPORT_TYPED_HANDLE_STATIC_SLOTS}" >&2
  echo "boundary deny pattern detected: typed handle static slot" >&2
  FAILED=1
fi

TEST_SUPPORT_PURE_CLUSTER_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*SessionCluster<" tests || true
)"
if [[ -n "${TEST_SUPPORT_PURE_CLUSTER_ALIASES}" ]]; then
  echo "${TEST_SUPPORT_PURE_CLUSTER_ALIASES}" >&2
  echo "boundary deny pattern detected: test support pure cluster alias" >&2
  FAILED=1
fi

TEST_SUPPORT_PROJECT_ROLE_OUTPUT_ALIASES="$(
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*<.*as[[:space:]]+ProjectRole<.*>::Output;" tests || true
)"
if [[ -n "${TEST_SUPPORT_PROJECT_ROLE_OUTPUT_ALIASES}" ]]; then
  echo "${TEST_SUPPORT_PROJECT_ROLE_OUTPUT_ALIASES}" >&2
  echo "boundary deny pattern detected: test support project-role output alias" >&2
  FAILED=1
fi

TEST_SUPPORT_IMPORT_ALIASES="$(
  rg -n -U "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" tests || true
)"
if [[ -n "${TEST_SUPPORT_IMPORT_ALIASES}" ]]; then
  echo "${TEST_SUPPORT_IMPORT_ALIASES}" >&2
  echo "boundary deny pattern detected: test support import alias forbidden path" >&2
  FAILED=1
fi

TEST_SUPPORT_PURE_PROGRAM_ALIASES="$(
  rg -n "^const[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<[^>]+>[[:space:]]*=[[:space:]]*[A-Z][A-Z0-9_]*;" tests || true
)"
if [[ -n "${TEST_SUPPORT_PURE_PROGRAM_ALIASES}" ]]; then
  echo "${TEST_SUPPORT_PURE_PROGRAM_ALIASES}" >&2
  echo "boundary deny pattern detected: test support pure program alias" >&2
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
  rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(StepCons<|SeqSteps<|<.*as[[:space:]]+ProjectRole<|<.*as[[:space:]]+StepConcat<)" \
    README.md || true
)"
if [[ -n "${README_STEP_PROJECTION_ALIASES}" ]]; then
  echo "${README_STEP_PROJECTION_ALIASES}" >&2
  echo "boundary deny pattern detected: README step/projection alias" >&2
  FAILED=1
fi

README_OLD_PROJECTED_LOCAL_WALKTHROUGH="$(
  rg -n -U '(The exact projected `LocalSteps` type is part of the contract\.|Do not erase `LocalSteps`\.|use hibana::(g::advanced|runtime::program)::steps::\{ProjectRole, SendStep, StepCons, StepNil\};|as[[:space:]]+ProjectRole<)' \
    README.md || true
)"
if [[ -n "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" ]]; then
  echo "${README_OLD_PROJECTED_LOCAL_WALKTHROUGH}" >&2
  echo "boundary deny pattern detected: README forbidden projected-local walkthrough" >&2
  FAILED=1
fi

README_DUAL_PAYLOAD_STORY="$(
  rg -n -U 'owned default path|`WireDecode`|WireEncode` and either `WireDecode`|runtime::wire::\{Payload,[[:space:]]*WireDecode,' \
    README.md || true
)"
if [[ -n "${README_DUAL_PAYLOAD_STORY}" ]]; then
  echo "${README_DUAL_PAYLOAD_STORY}" >&2
  echo "boundary deny pattern detected: README dual payload story" >&2
  FAILED=1
fi

CORE_OWNED_MGMT_EPF_DOCS="$(
  rg -n -U '`hibana::runtime::mgmt|`hibana::runtime::resolver::epf' \
    README.md || true
)"
if [[ -n "${CORE_OWNED_MGMT_EPF_DOCS}" ]]; then
  echo "${CORE_OWNED_MGMT_EPF_DOCS}" >&2
  echo "boundary deny pattern detected: core-owned mgmt/epf doc wording" >&2
  FAILED=1
fi

IN_TREE_CROSS_REPO_HARNESS_DOCS="$(
  rg -n -U '`runtime/cross-repo/`|staging location for cross-repo smoke' \
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
  echo "boundary deny pattern detected: README/spec forbidden Program item path" >&2
  FAILED=1
fi

EXAMPLE_OWNER_HIDING_ALIASES="$(
  if [[ -e examples ]]; then
    rg -n -U "^type[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*(Role<|Msg<|RoleProgram<|Endpoint<|SessionCluster<|g::Program<|StepCons<|SeqSteps<|<.*as[[:space:]]+ProjectRole<|<.*as[[:space:]]+StepConcat<)" \
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
  echo "boundary deny pattern detected: example import alias forbidden path" >&2
  FAILED=1
fi

EXAMPLE_UNDERSCORE_CASTS="$(
  if [[ -e examples ]]; then
    rg -n "\\sas _([,;)|]|$)|as \\*const _|as \\*mut _" examples || true
  fi
)"
if [[ -n "${EXAMPLE_UNDERSCORE_CASTS}" ]]; then
  echo "${EXAMPLE_UNDERSCORE_CASTS}" >&2
  echo "boundary deny pattern detected: example underscore cast forbidden path" >&2
  FAILED=1
fi

EXAMPLE_FORBIDDEN_SURFACE="$(
  if [[ -e examples ]]; then
    rg -n "#\\[doc\\(hidden\\)\\]|#\\[allow\\(dead_code\\)\\]|\\b${ALT_WORD}\\b" examples || true
  fi
)"
if [[ -n "${EXAMPLE_FORBIDDEN_SURFACE}" ]]; then
  echo "${EXAMPLE_FORBIDDEN_SURFACE}" >&2
  echo "boundary deny pattern detected: example forbidden residue" >&2
  FAILED=1
fi

INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES="$(
  rg -n -U "^[[:space:]]*type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Arm|Msg|Local|Projection|Route|Endpoint|Actor)[A-Za-z0-9_]*[[:space:]]*=" \
    src/global/const_dsl.rs \
    src/global/typestate.rs \
    src/global/role_program.rs \
    src/endpoint/kernel/core.rs \
    src/session/cluster/effects.rs || true
)"
if [[ -n "${INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES}" ]]; then
  echo "${INTERNAL_SOURCE_TEST_OWNER_HIDING_ALIASES}" >&2
  echo "boundary deny pattern detected: source-test owner-hiding type alias" >&2
  FAILED=1
fi

STACK_BACKED_TAP_STORAGE_RESIDUE="$(
  rg -n "Box::new\\(\\[TapEvent::default\\(\\);[[:space:]]*RING_EVENTS\\]\\)" \
    tests/support/runtime.rs || true
)"
if [[ -n "${STACK_BACKED_TAP_STORAGE_RESIDUE}" ]]; then
  echo "${STACK_BACKED_TAP_STORAGE_RESIDUE}" >&2
  echo "boundary deny pattern detected: stack-backed tap storage forbidden path" >&2
  FAILED=1
fi

STACK_TUNING_HELPER_RESIDUE="$(
  rg -n -g '!tests/huge_choreography_runtime.rs' "stack_size\\(" src tests || true
)"
if [[ -n "${STACK_TUNING_HELPER_RESIDUE}" ]]; then
  echo "${STACK_TUNING_HELPER_RESIDUE}" >&2
  echo "boundary deny pattern detected: explicit stack tuning helper" >&2
  FAILED=1
fi

if [[ -e tests/support/large_stack_sync.rs || -e tests/support/large_stack_async.rs ]]; then
  echo "boundary deny pattern detected: forbidden large-stack support module restored" >&2
  FAILED=1
fi

PUBLIC_TEST_UTILS_FEATURE="$(
  rg -n "^[[:space:]]*test-utils[[:space:]]*=[[:space:]]*\\[\\]" Cargo.toml || true
)"
if [[ -n "${PUBLIC_TEST_UTILS_FEATURE}" ]]; then
  echo "${PUBLIC_TEST_UTILS_FEATURE}" >&2
  echo "boundary deny pattern detected: public test-utils feature forbidden path" >&2
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
  echo "boundary deny pattern detected: project turbofish forbidden path" >&2
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
  echo "boundary deny pattern detected: app constructor turbofish forbidden path" >&2
  FAILED=1
fi

CFG_GATED_EMPTY_FUNCTIONS="$(
  rg -n -U "#\\[cfg\\(not\\(feature = \\\"[A-Za-z0-9_-]+\\\"\\)\\)\\][[:space:]]*\\n[[:space:]]*(pub\\(crate\\)|pub)?[[:space:]]*fn[[:space:]]+[A-Za-z0-9_]+\\([^;]*\\)[[:space:]]*\\{[[:space:]]*\\}" \
    src || true
)"
if [[ -n "${CFG_GATED_EMPTY_FUNCTIONS}" ]]; then
  echo "${CFG_GATED_EMPTY_FUNCTIONS}" >&2
  echo "boundary deny pattern detected: cfg-gated empty function body" >&2
  FAILED=1
fi

TRANSPORT_TRAIT_EMPTY_DEFAULTS="$(
  rg -n -U "fn[[:space:]]+requeue<'a>\\(&'a self, rx: &'a mut Self::Rx<'a>\\)[[:space:]]*\\{[[:space:]]*debug_assert!\\(core::ptr::eq\\(rx, rx\\)\\);[[:space:]]*\\}|fn[[:space:]]+metrics\\(&self\\)[[:space:]]*->[[:space:]]*Self::Metrics[[:space:]]*\\{[[:space:]]*Self::Metrics::default\\(\\)[[:space:]]*\\}" \
    src/transport.rs || true
)"
if [[ -n "${TRANSPORT_TRAIT_EMPTY_DEFAULTS}" ]]; then
  echo "${TRANSPORT_TRAIT_EMPTY_DEFAULTS}" >&2
  echo "boundary deny pattern detected: transport trait default forbidden path" >&2
  FAILED=1
fi
check_absent "fn[[:space:]]+cancel_send<'a>\\(&self, tx: &'a mut Self::Tx<'a>\\)[[:space:]]*\\{[[:space:]]*let _ = tx;[[:space:]]*\\}" \
  "transport send cancellation must be a required transport contract, not an empty trait body" \
  src/transport.rs
check_absent "fn[[:space:]]+open<'a>\\(&self, port: PortOpen\\)[[:space:]]*->[[:space:]]*\\(Self::Tx<'a>, Self::Rx<'a>\\)" \
  "Transport::open must bind Tx/Rx handles to the transport borrow" \
  src/transport.rs
if ! grep -Fq "fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);" src/transport.rs; then
  echo "transport surface violation: missing transport-borrow-bound open contract" >&2
  FAILED=1
fi

LOGICAL_FRAME_LABEL_RESIDUE="$(
  rg -n \
    -g '!tests/docs_surface.rs' \
    -g '!*.stderr' \
    "recv_label_hint|scope_hint|ScopeLabelMeta|scope_label_meta|frame_hint_label|resolved_frame_hint_label|matches_frame_hint_label|record_arm_label|record_dispatch_arm_label|mark_scope_ready_arm_from_label|mark_scope_ready_arm_from_binding_label|scope_label_to_arm|scope_evidence_label_to_arm|binding_scope_evidence_label_to_arm|FrameLabelMask::from_label|contains_label\\(|insert_label\\(|remove_label\\(|singleton_label\\(|binding_label_masks|endpoint_binding_label_masks_bytes|pending_frame_hint_label_masks|pending_frame_hint_labels_for_lane|update_pending_frame_hint_lane_masks|first_recv_dispatch_label_mask|route_scope_first_recv_dispatch_label_mask|reserved protocol[[:space:]]+band|0x0300[[:space:]]*\\+[^;]*(LABEL|LOGICAL)|_reserved[[:space:]]*:|LABEL_LOOP_CONTINUE|LABEL_LOOP_BREAK|LABEL_ROUTE_ARM_SELECTION|LABEL_PROTOCOL_CONTROL|Message>::LABEL|const[[:space:]]+LABEL[[:space:]]*:[[:space:]]*u8|IngressEvidence[[:space:]]*\\{[[:space:]]*label:" \
    README.md src tests || true
)"
if [[ -n "${LOGICAL_FRAME_LABEL_RESIDUE}" ]]; then
  echo "${LOGICAL_FRAME_LABEL_RESIDUE}" >&2
  echo "boundary deny pattern detected: logical/frame label contract residue" >&2
  FAILED=1
fi

check_absent \
  "TEST_LOOP_CONTINUE_LABEL|TEST_LOOP_BREAK_LABEL|TEST_ROUTE_ARM_SELECTION_LABEL|const[[:space:]]+[A-Z0-9_]*(LABEL|LOGICAL|FRAME)[A-Z0-9_]*[[:space:]]*:[[:space:]]*u8[[:space:]]*=[[:space:]]*(48|49|57);|Msg<\\{?[[:space:]]*(48|49|57)\\b|LabelMarker<(48|49|57)>|FrameLabel::new\\([^)]*LOGICAL\\)|FrameLabelMask::from_frame_label\\([^)]*LOGICAL\\)" \
  "tests must not preserve retired 48/49/57 protocol-label test support or pass logical labels as frame labels" \
  src tests

check_absent \
  "FrameLabel::new\\([A-Z0-9_]*LABEL\\)|FrameLabelMask::from_frame_label\\([A-Z0-9_]*LABEL\\)" \
  "tests must not pass logical-label test support or ambiguous LABEL constants as FrameLabel values" \
  src tests

check_absent \
  "ScopeFrameHint|frame_label[[:space:]]*==[[:space:]]*0|FrameLabel::new\\(<Msg<|outgoing\\.frame_label\\(\\)\\.raw\\(\\)[[:space:]]*==[[:space:]]*<Msg<" \
  "FrameLabel/logical label conflation or zero-valued label residue" \
  src tests

check_absent \
  "current_step_labels|event_cursor_current_step_labels|refresh_current_step_label\\(|rebuild_current_step_labels\\(" \
  "current-step logical label cache residue" \
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
  "EffList docs must not describe the forbidden flat slice surface" \
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
  "kernel_recv\\(self,[[:space:]]*logical_label,[[:space:]]*accepts_empty_""payload|poll_public_recv\\(logical_label,[[:space:]]*accepts_empty_""payload|RecvRuntimeDesc::new\\([^,]+,[^,]+,[^,]+\\)" \
  "deterministic recv must carry descriptor evidence into complete runtime descriptor" \
  src/endpoint src/endpoint/kernel

check_absent \
  "pri""or_atom\\.label|atom\\.label[[:space:]]*==[[:space:]]*label|label[[:space:]]*=[[:space:]]*current\\.label" \
  "FrameLabel allocation must be edge-unique, not logical-label deduplicated" \
  src/global/typestate/emit_walk.rs

DEFAULT_WORD='Def''ault'
TRANSPORT_METRICS_EMPTY_DEFAULTS="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+TransportMetrics:[[:space:]]+${DEFAULT_WORD}|fn[[:space:]]+(latency_us|queue_depth|pacing_interval_us|congestion_marks|retransmissions|pto_count|srtt_us|latest_ack_pn|congestion_window|in_flight_bytes|algorithm)\\(&self\\)[[:space:]]*->[[:space:]]*Option<[^>]+>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/transport.rs || true
)"
if [[ -n "${TRANSPORT_METRICS_EMPTY_DEFAULTS}" ]]; then
  echo "${TRANSPORT_METRICS_EMPTY_DEFAULTS}" >&2
  echo "boundary deny pattern detected: transport metrics trait default forbidden path" >&2
  FAILED=1
fi

if [[ -e src/transport/context.rs ]]; then
  echo "src/transport/context.rs" >&2
  echo "boundary deny pattern detected: forbidden transport context owner detected" >&2
  FAILED=1
fi
DELETED_SESSION_CAP_DIR="src/session/""cap"
DELETED_SESSION_CAP_RS="src/session/""cap.rs"
if [[ -e "${DELETED_SESSION_CAP_DIR}" || -e "${DELETED_SESSION_CAP_RS}" ]]; then
  echo "${DELETED_SESSION_CAP_DIR}" >&2
  echo "${DELETED_SESSION_CAP_RS}" >&2
  echo "boundary deny pattern detected: forbidden session token owner detected" >&2
  FAILED=1
fi

CORE_TRAIT_DEFAULT_HELPERS="$(
  rg -n -U "pub[[:space:]]+trait[^{]+\\{[^}]*fn[[:space:]]+[A-Za-z0-9_]+\\([^;{}]*\\)[^{;]*\\{" \
    src/session/brand.rs \
    src/global.rs || true
)"
if [[ -n "${CORE_TRAIT_DEFAULT_HELPERS}" ]]; then
  echo "${CORE_TRAIT_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: core trait default forbidden path" >&2
  FAILED=1
fi

CONTROL_AUTOMATON_DEFAULT_GRAPH="$(
  rg -n -U "fn[[:space:]]+run<'lease|default this forwards to|Self::run\\(lease,[[:space:]]*seed\\)" \
    src/session/lease/core.rs || true
)"
if [[ -n "${CONTROL_AUTOMATON_DEFAULT_GRAPH}" ]]; then
  echo "${CONTROL_AUTOMATON_DEFAULT_GRAPH}" >&2
  echo "boundary deny pattern detected: session automaton default graph path" >&2
  FAILED=1
fi

RESOURCE_KIND_DEFAULT_HELPERS=""
if [[ -e "${DELETED_SESSION_CAP_DIR}" ]]; then
  RESOURCE_KIND_DEFAULT_HELPERS="$(
    rg -n -U "pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*const[[:space:]]+AUTO_MINT_EXTERNAL:[[:space:]]+bool[[:space:]]*=[[:space:]]*false[[:space:]]*;|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+caps_mask\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*CapsMask[[:space:]]*\\{[[:space:]]*CapsMask::empty\\(\\)[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+scope_id\\(_handle:[[:space:]]*&Self::Handle\\)[[:space:]]*->[[:space:]]*Option<ScopeId>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}|pub[[:space:]]+trait[[:space:]]+ResourceKind[^}]*fn[[:space:]]+handle_scope\\(" \
      "${DELETED_SESSION_CAP_DIR}" || true
  )"
fi
if [[ -n "${RESOURCE_KIND_DEFAULT_HELPERS}" ]]; then
  echo "${RESOURCE_KIND_DEFAULT_HELPERS}" >&2
  echo "boundary deny pattern detected: resource kind default forbidden path" >&2
  FAILED=1
fi

WIRE_ENCODE_DEFAULT_HINT="$(
  rg -n -U "pub[[:space:]]+trait[[:space:]]+WireEncode[^}]*fn[[:space:]]+encoded_len\\(&self\\)[[:space:]]*->[[:space:]]*Option<usize>[[:space:]]*\\{[[:space:]]*None[[:space:]]*\\}" \
    src/transport/wire.rs || true
)"
if [[ -n "${WIRE_ENCODE_DEFAULT_HINT}" ]]; then
  echo "${WIRE_ENCODE_DEFAULT_HINT}" >&2
  echo "boundary deny pattern detected: wire encode trait default forbidden path" >&2
  FAILED=1
fi

TRANSPORT_RECONFIG_ALTERNATE="$(
  rg -n "quiesce_and_${RECONFIG_TOKEN}|resume_after" src/transport.rs || true
)"
if [[ -n "${TRANSPORT_RECONFIG_ALTERNATE}" ]]; then
  echo "${TRANSPORT_RECONFIG_ALTERNATE}" >&2
  echo "boundary deny pattern detected: transport reconfiguration alternate path" >&2
  FAILED=1
fi

RENDEZVOUS_STACK_RESIDUE="$(
  rg -n -U "let[[:space:]]+rv[[:space:]]*=[[:space:]]*Rendezvous::from_config\\(config, transport\\);[[:space:]]*self\\.add_rendezvous\\(rv\\)" \
    src/session/cluster/core.rs || true
)"
if [[ -n "${RENDEZVOUS_STACK_RESIDUE}" ]]; then
  echo "${RENDEZVOUS_STACK_RESIDUE}" >&2
  echo "boundary deny pattern detected: rendezvous stack forbidden path" >&2
  FAILED=1
fi

check_absent_multiline "^[[:space:]]*use[[:space:]][^;]*\\bas[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^;]*;" \
  "source import alias forbidden path" \
  src examples
check_absent "\\sas _([,;)|]|$)" \
  "underscore inferred cast forbidden path" \
  src examples
check_absent "as \\*const _|as \\*mut _" \
  "underscore pointer cast forbidden path" \
  src examples
check_absent "^[[:space:]]*const[[:space:]]+(INDEX|VALUE|LABEL):[[:space:]]+u8[[:space:]]*=[[:space:]]+(IDX|LABEL);" \
  "self-shadowing associated const forbidden path" \
  src tests
check_absent "type[[:space:]]+Controller[[:space:]]*=[[:space:]]*Controller;" \
  "route semantic self-shadowing associated type forbidden path" \
  src/global.rs
STALE_VAR_ALLOW='un''used_variables'
check_absent "#\\[allow\\(clippy::empty_loop\\)\\]|#\\[allow\\(${STALE_VAR_ALLOW}\\)\\]|#\\[allow\\(clippy::let_unit_value\\)\\]" \
  "forbidden allow attribute" \
  src tests
check_absent "route_inferred|par_inferred" \
  "forbidden inferred binary builder vocabulary" \
  src/global.rs \
  src/global/program.rs
check_absent "^type[[:space:]]+[A-Za-z0-9_]*Msg[A-Za-z0-9_]*[[:space:]]*=" \
  "global const-dsl message alias forbidden path" \
  src/global/const_dsl.rs
REMOVED_RESOURCE='Con''trol''Resource'
check_absent "^type[[:space:]]+${REMOVED_RESOURCE}<" \
  "endpoint forbidden descriptor shorthand alias forbidden path" \
  src/endpoint/kernel/core.rs \
  src/endpoint/flow.rs
check_absent "mem::transmute::<Guard<'_>, Guard<'static>>|mem::transmute::<Guard<'static>, Guard<'rv>>|core::mem::transmute::<_, Port<[^>]+>>" \
  "rendezvous brand or port transmute forbidden path" \
  src/rendezvous/core.rs
check_absent "transmute::<usize, fn\\(u32\\)>" \
  "observe timestamp-checker transmute forbidden path" \
  src/observe/core.rs
check_absent "LeaseObserve|from_resident_tap|commit_event: Option<TapEvent>|requeue_event: Option<TapEvent>" \
  "unconsumed lease observe/tap authority" \
  src/session/lease/core.rs
check_absent "#\\[doc\\(hidden\\)\\]" \
  "doc-hidden forbidden path" \
  src examples
check_absent "LocalSteps = steps::StepNil" \
  "RoleProgram StepNil default projection forbidden path" \
  src/global/role_program.rs
check_absent "HIBANA_[A-Z0-9_]+" \
  "env/debug token in hibana core" \
  src
check_absent "(^|[^A-Za-z0-9_])([Qq][Uu][Ii][Cc]|[Hh]3|[Hh][Qq])([^A-Za-z0-9_]|$)" \
  "protocol-specific vocabulary in hibana core" \
  src
check_absent "${ALT_WORD}_|\\b${ALT_WORD}\\b" \
  "alternate-path residue vocabulary in production source" \
  src
FOR_TEST_WORD='for_''test'
FOR_TEST_SUFFIX='_for_''test'
check_absent "\\b(${FOR_TEST_WORD}|${FOR_TEST_SUFFIX})\\b|test-only|Test-only|\\b${MODE_WORD}(ibility)?\\b|\\b${OLD_WORD}\\b|\\b${RECOVERY_WORD}\\b|\\b${GUESS_WORD}\\b" \
  "test-only or forbidden-surface residue in production source" \
  src
check_absent "(?i)\\b(${MODE_WORD}(ibility|ible)?|${OLD_WORD}|${RECOVERY_WORD}|${GUESS_WORD}|${ALT_WORD}|state machine|infer(red|ence|s|ring)?|absorb mismatch|absorption)\\b" \
  "runtime-intelligence vocabulary residue in public docs or core source" \
  src README.md
check_absent "#\\[allow\\(dead_code\\)\\]|DiscardedAndPending|keeps_waiting|absorbed" \
  "static hygiene residue in production source" \
  src
check_absent_multiline "wake_by_ref\\(\\);[[:space:]]*\\n[[:space:]]*return Poll::Pending" \
  "transport mismatch wake-and-pending recovery path" \
  src/endpoint/kernel/lane_port.rs src/endpoint/kernel/observe.rs src/endpoint/kernel/offer.rs src/endpoint/kernel/offer/materialization.rs
check_absent_multiline "wake_by_ref\\(\\);[[:space:]]*\\n[[:space:]]*Poll::Pending" \
  "transport mismatch wake-and-pending recovery expression" \
  src/endpoint/kernel/lane_port.rs src/endpoint/kernel/observe.rs src/endpoint/kernel/offer.rs src/endpoint/kernel/offer/materialization.rs
check_absent "\\b(test_from_slice|bind_test_storage)\\b" \
  "named cfg-test constructor/helper residue in production source" \
  src
check_absent "SessionKit::new|pub[[:space:]]+fn[[:space:]]+new\\(clock:|init_in_place\\([^)]*clock:" \
  "owned or clock-bearing SessionKit construction must not be detected" \
  src README.md .github/allowlists/runtime-public-api.txt
check_absent "^type[[:space:]]+[A-Za-z0-9_]*(Step|Steps|Arm|Decision)[A-Za-z0-9_]*[[:space:]]*=" \
  "step/composition alias residue in production source" \
  src/global/steps.rs
check_absent "\\bEndpointBinding\\b" \
  "endpoint binding synonym alias residue in production source" \
  src/endpoint.rs src/endpoint/flow.rs src/endpoint/carrier.rs
check_absent "pub[[:space:]]+struct[[:space:]]+StateIndex|pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(new|from_usize|raw|as_usize|is_max)\\(" \
  "typestate StateIndex flat-index helpers must remain crate-private" \
  src/global/typestate/facts.rs
check_absent "\\b(RouteResolutionOutcome|LoopResolutionOutcome|RollResolutionOutcome)\\b" \
  "resolver result alias residue in production source" \
  src/session/cluster/core.rs
check_absent "TransportAlgorithm,[[:space:]]*TransportError|TransportError,[[:space:]]*TransportEvent" \
  "transport observation detail re-exported from the daily runtime transport bucket" \
  src/runtime.rs .github/allowlists/runtime-public-api.txt
check_absent "\\b(LocalDirection|SendMeta)\\b" \
  "transport send metadata detail re-exported from the daily runtime transport bucket" \
  src/runtime.rs .github/allowlists/runtime-public-api.txt
check_absent "\\bTransportMetricsTapPayload\\b" \
  "transport tap packing payload leaked into public runtime surface" \
  src/runtime.rs .github/allowlists/runtime-public-api.txt
check_absent "\\bTransportAlgorithm\\b" \
  "transport algorithm enum leaked into public runtime surface" \
  src/runtime.rs .github/allowlists/runtime-public-api.txt
BINDING_BLOCK="$(
  awk '
    /^pub mod binding \{/ { inside=1 }
    inside {
      print
      if ($0 ~ /^}/) { exit }
    }
  ' src/runtime/buckets.rs
)"
if [[ -n "${BINDING_BLOCK}" ]]; then
  echo "boundary deny pattern detected: runtime binding bucket detected instead of transport-owned ingress" >&2
  FAILED=1
fi
check_absent "\\bTransportOpsError\\b|\\bhas_fin\\b|\\bProtocol\\(u64\\)|\\bWriteFailed\\b|\\bOpenFailed\\b" \
  "protocol-specific binding vocabulary leaked into hibana surface" \
  src README.md .github/allowlists/runtime-public-api.txt
RESOLVER_BLOCK="$(
  awk '
    /^pub mod resolver \{/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^\/\/\/ Wire payload codec surface\./) { exit }
    }
  ' src/runtime/buckets.rs
)"
if [[ -z "${RESOLVER_BLOCK}" ]]; then
  echo "runtime resolver block not found" >&2
  FAILED=1
elif printf '%s\n' "${RESOLVER_BLOCK}" | rg -n "pub[[:space:]]+mod[[:space:]]+advanced[[:space:]]*\\{" >/dev/null; then
  echo "boundary deny pattern detected: runtime resolver advanced extra bucket" >&2
  FAILED=1
else
  for required in \
    "ResolverRef"
  do
    if ! printf '%s\n' "${RESOLVER_BLOCK}" | rg -n -F "${required}" >/dev/null; then
      echo "runtime resolver resolver surface missing: ${required}" >&2
      FAILED=1
    fi
  done
  for forbidden in \
    "ResolverContext" \
    "ContextId" \
    "ContextValue" \
    "ResolverAttrs" \
    "ResolverSignals," \
    "ResolverSlot" \
    "pub mod replay {"
  do
    if printf '%s\n' "${RESOLVER_BLOCK}" | rg -n -F "${forbidden}" >/dev/null; then
      echo "boundary deny pattern detected: runtime resolver root replay metadata leak: ${forbidden}" >&2
      FAILED=1
    fi
  done
fi

for forbidden in \
  "ResolverInput" \
  "ResolverAttrs" \
  "ResolverSignals," \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "pub mod core {" \
  "pub mod replay {" \
  "advanced::resolver"
do
  if rg -n -F "${forbidden}" src/runtime/buckets.rs >/dev/null; then
    echo "boundary deny pattern detected: runtime resolver replay internals leak: ${forbidden}" >&2
    FAILED=1
  fi
done
check_absent "TransportEventMeta|pub[[:space:]]+(kind|packet_number|payload_len|retransmissions|pn_space|cid_tag):|pub[[:space:]]+(primary|extension):|pub[[:space:]]+const[[:space:]]+fn[[:space:]]+(new_with_metadata|with_pn_space|with_cid_tag|payload_len|retry_count|domain|carrier_tag)\\b" \
  "transport observation detail must stay protocol-neutral and non-extension" \
  src/transport.rs
check_absent "\\bTransportSnapshotParts\\b|from_parts\\(parts:" \
  "transport snapshot option-bag constructor detected" \
  src/transport.rs
check_absent "\\bConfigParts\\b|config\\.into_parts\\(\\)" \
  "runtime config decomposition bag detected" \
  src/runtime/config.rs src/rendezvous/core.rs
check_absent "\\bRegisteredTokenParts\\b|RawRegisteredCapToken::from_parts|take_registered_parts" \
  "registered token transfer bag detected" \
  src/session/brand.rs src/endpoint
check_absent "pub[[:space:]]+bytes:[[:space:]]*\\[u8;[[:space:]]*CAP_TOKEN_LEN\\]|fn[[:space:]]+from_parts\\(" \
  "generic token wire layout part constructor detected" \
  src/session/brand.rs
check_absent "pub[[:space:]]+fn[[:space:]]+(nonce|tag|control_header|shot|handle_bytes|handle_bytes_ref)\\(&self\\)" \
  "generic token low-level accessor leaked as public API" \
  src/session/brand.rs
check_absent "use crate::session::types::\\{RendezvousId, SessionId\\}" \
  "runtime root must route identifier signatures through runtime::ids" \
  src/runtime.rs
if [[ -e src/session/automaton/distributed.rs ]]; then
  echo "src/session/automaton/distributed.rs" >&2
  echo "boundary deny pattern detected: forbidden distributed session owner detected" >&2
  FAILED=1
fi
check_absent "\\b(StatelessRouteResolverFn|RouteResolverStatePayload[[:space:]]*<[^>]+>[[:space:]]*=|ErasedRouteResolverStatePayload|StatelessLoopResolverFn|LoopResolverStatePayload[[:space:]]*<[^>]+>[[:space:]]*=|ErasedLoopResolverStatePayload|StatelessRollResolverFn|RollResolverStatePayload[[:space:]]*<[^>]+>[[:space:]]*=|ErasedRollResolverStatePayload)\\b" \
  "resolver storage type alias residue in production source" \
  src/session/cluster/core.rs
check_absent "type[[:space:]]+SessionLaneHandle[[:space:]]*=" \
  "session-lane handle tuple alias residue in production source" \
  "${DELETED_SESSION_CAP_DIR}/atomic_codecs.rs"
check_absent "RawEmittedCapToken::from_bytes" \
  "test-only emitted-token constructor residue in production source" \
  src/endpoint
payload_validator="Payload""Validator"
zero_payload_provider_alias="Syn""thetic""Payload""Provider"
stage_send_payload_fn="Stage""Send""Payload""Fn"
encode_handle_fn="Encode""Con""trol""Handle""Fn"
port_storage="Port""Storage"
guard_storage="Guard""Storage"
stored_forbidden_alias="Stored""Mi""nt"
check_absent "\\b(${payload_validator}|${zero_payload_provider_alias}|${stage_send_payload_fn}|${encode_handle_fn}|${port_storage}|${guard_storage}|${stored_forbidden_alias})\\b" \
  "private descriptor/helper alias residue in production source" \
  src/endpoint
check_absent "\\b(SlotArena|SlotStorage|SlotBundleHandle|SlotStageRecord|FACET_SLOTS|facets_slots|requires_slots|slot_arena)\\b" \
  "test-only resolver slot allocator residue in production source" \
  src/rendezvous.rs src/rendezvous src/session/lease src/session/cluster/core.rs
check_absent "std::env::var_os\\(" \
  "env lookup in hibana core" \
  src
check_absent "eprintln!\\(" \
  "stderr debug trace in hibana core" \
  src
check_absent "WireFirst" \
  "WireFirst token" \
  src
check_absent "(hint|classification).*(RouteArm|ResolverVerdict::RouteArm)" \
  "hint/classification RouteArm promotion" \
  src/endpoint/kernel/core.rs \
  src/global \
  src/session
check_absent "\\bpoll_arm_from_ready_hint\\b" \
  "hint-derived Poll helper" \
  src/endpoint/kernel/core.rs
check_absent "rebuild_pending_offers|build_frontier_snapshot|select_offer_entry|lag correction|passive takeover" \
  "offer-kernel cleanup forbidden path" \
  src/endpoint/kernel/core.rs
check_absent "duplicate route label" \
  "route duplicate-label const panic residue" \
  src/global/program.rs
check_absent "\\b(RouteLabelBits|BitEq|BitsEqual|BitsCons|BitsNil|Bit0|Bit1)\\b" \
  "route label bit-table forbidden path" \
  src/global.rs
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
check_absent "(?i)\\b(quic|h3|hq|qpack|alpn)\\b|http/3" \
  "protocol-specific vocabulary in hibana/src" \
  src
check_absent "FrameHeader\\(u64\\)|from_raw\\(raw:[[:space:]]*u64\\)|raw\\(self\\)[[:space:]]*->[[:space:]]*u64|pack_frame_header|raw_header|carrier-owned \`u64\`" \
  "u64 FrameHeader public/raw header surface detected" \
  src/transport.rs README.md .github/allowlists/runtime-public-api.txt
check_absent "\\bu64\\b|1u64|\\[u64;|word[0-9]|>>[[:space:]]*6|<<[[:space:]]*6|\\*[[:space:]]*64|/[[:space:]]*64" \
  "wide integer FrameLabelMask helper detected" \
  src/transport/labels.rs
check_absent "ScopeFrameLabelMasks::EMPTY|frame_label_masks|frame_label_meta:[[:space:]]*&[[:space:]]*ScopeFrameLabelMeta|\\)[[:space:]]*->[[:space:]]*ScopeFrameLabelMeta|\\.frame_hint_mask\\(&|fn[[:space:]]+selection_frame_label_meta\\(|fn[[:space:]]+offer_scope_frame_label_meta\\(|fn[[:space:]]+scope_frame_label_meta(_at)?\\(" \
  "scope frame-label hot path by-value mask plumbing detected" \
  src/endpoint/kernel/core/frontier_helpers.rs \
  src/endpoint/kernel/core/scope_evidence_logic.rs \
  src/endpoint/kernel/offer.rs \
  src/endpoint/kernel/offer/facts.rs \
  src/endpoint/kernel/offer/passive.rs \
  src/endpoint/kernel/offer/select.rs


bash ./.github/scripts/check_endpoint_surface_owner.sh


HIDDEN_SRC_FILES="$(
  find src -type f | rg '/_[^/]+$' || true
)"
if [[ -n "${HIDDEN_SRC_FILES}" ]]; then
  echo "${HIDDEN_SRC_FILES}" >&2
  echo "boundary deny pattern detected: underscore source forbidden path" >&2
  FAILED=1
fi

check_absent "MaybeUninit<ErasedPublicEndpointKernel" \
  "inline public endpoint kernel storage" \
  src/session/cluster/core.rs

check_absent \
  "pub[[:space:]]+mod[[:space:]]+steps[[:space:]]*\\{" \
  "public step bucket detected" \
  src/global.rs src/g.rs src

ITEM_LEVEL_PROGRAM_INFERENCE_RESIDUE="$(
  paths=(README.md tests src)
  if [[ -e examples ]]; then
    paths+=(examples)
  fi
  rg -n \
    -g '!tests/docs_surface.rs' \
    -g '!tests/ui/*.stderr' \
    -g '!src/global/program.rs' \
    "(const|static)[[:space:]]+[A-Z0-9_]+[[:space:]]*:[[:space:]]+(g::)?Program<_" \
    "${paths[@]}" || true
)"
if [[ -n "${ITEM_LEVEL_PROGRAM_INFERENCE_RESIDUE}" ]]; then
  echo "${ITEM_LEVEL_PROGRAM_INFERENCE_RESIDUE}" >&2
  echo "boundary deny pattern detected: item-level inferred Program type detected" >&2
  FAILED=1
fi

check_absent \
  "(g::advanced::steps|runtime::program::steps)" \
  "public step names detected in docs/examples" \
  README.md

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "surface hygiene check passed"
