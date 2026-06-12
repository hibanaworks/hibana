use super::common::*;

#[test]
fn transport_contract_is_io_only_and_documented() {
    let transport = transport_source();
    let readme = read("README.md");
    let hygiene = read(".github/scripts/check_surface_hygiene.sh");

    assert!(
        !transport.contains("fn operational_deadline_ticks(&self)")
            && !transport.contains("fn policy_attrs(&self)")
            && !transport.contains("fn drain_events(&self)")
            && !transport.contains("TransportEvent")
            && !transport.contains("apply_pacing_update")
            && !transport.contains("LocalDirection")
            && !transport.contains("pub const fn is_local"),
        "transport must stay protocol-neutral I/O plus rollback/hint hooks"
    );
    for required in [
        "fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);",
        "fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error>;",
        "fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);",
        ") -> Poll<Result<ReceivedFrame<'a>, Self::Error>>;",
        "pub struct ReceivedFrame<'f>",
    ] {
        assert!(
            transport.contains(required),
            "transport surface must keep the minimal payload/observation/rollback contract: {required}"
        );
    }
    assert!(
        !transport.contains("pub struct Incoming<'f>")
            && !transport.contains("fn peek_recv_frame")
            && !transport.contains("recv_frame_hint"),
        "transport receive surface must keep one integrated receive value, not Incoming compatibility or side-channel hooks"
    );
    assert!(
        !transport.contains("fn open<'a>(&self, port: PortOpen)"),
        "Transport::open must bind Tx/Rx handles to the transport borrow, not an unconstrained lifetime"
    );
    assert!(
        !transport.contains("type Metrics") && !transport.contains("fn metrics("),
        "transport surface must not keep a metrics associated type or compatibility hook"
    );
    assert!(
        !hygiene.contains("fn[[:space:]]+apply_pacing_update"),
        "surface hygiene gate must continue rejecting semantic fallback hooks"
    );
    assert!(
        readme.contains("transport sees bytes, frame labels, and readiness")
            && readme.contains("returns `TransportError`")
            && readme.contains("The transport owns:")
            && readme.contains("receive returns a borrowed `ReceivedFrame`")
            && readme
                .contains("The canonical receive-side frame observation is the `IngressEvidence`")
            && readme.contains("`ReceivedFrame` returned by `poll_recv(...)`")
            && readme.contains("Payload and evidence cross the")
            && readme.contains("transport boundary together")
            && readme.contains("there is no separate receive-observation hook")
            && !readme.contains("apply_pacing_update"),
        "README must keep only the canonical transport boundary"
    );
    assert!(
        readme.contains("`cancel_send(...)` for transport cleanup")
            && readme.contains("transport sees bytes, frame labels, and readiness"),
        "README must document transport as frame I/O, rollback, and integrated receive observation only"
    );
}

#[test]
fn endpoint_resident_payload_unsafe_contracts_are_documented() {
    let lane_port = read("src/endpoint/kernel/lane_port.rs");

    for function in ["endpoint_resident_payload"] {
        let marker = format!("unsafe fn {function}");
        let start = lane_port
            .find(&marker)
            .unwrap_or_else(|| panic!("missing unsafe helper: {function}"));
        let prefix_start = if start >= 700 { start - 700 } else { 0 };
        let prefix = &lane_port[prefix_start..start];
        assert!(
            prefix.contains("# Safety"),
            "{function} must carry its unsafe preconditions at the function boundary"
        );
    }
}

#[test]
fn send_payload_borrow_is_owned_by_send_future_not_endpoint_state() {
    let flow = read("src/endpoint/flow.rs");
    let endpoint_boundary = read("src/endpoint/ops.rs")
        + &read("src/endpoint/carrier.rs")
        + &read("src/endpoint/carrier/send.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let public_poll = read("src/endpoint/kernel/public_poll.rs");
    let kernel = read("src/endpoint/kernel/core.rs");

    assert!(
        flow.contains("struct RawSendFuture<'a, 'e, 'r, const ROLE: u8>")
            && flow.contains("payload: kernel::RawSendPayload")
            && flow.contains("raw: RawSendFuture<'a, 'e, 'r, ROLE>")
            && flow.contains("endpoint.poll_send(cx, self.payload.take())")
            && endpoint_boundary.contains("payload: Option<kernel::RawSendPayload>")
            && endpoint_boundary
                .contains("payload: Option<crate::endpoint::kernel::RawSendPayload>")
            && endpoint_boundary.contains("kernel.poll_public_send(cx, payload)")
            && public_poll.contains("payload: Option<lane_port::RawSendPayload>")
            && public_poll.contains("let mut payload = payload;")
            && kernel.contains("payload: &mut Option<lane_port::RawSendPayload>")
            && !runtime_types.contains("payload: Option<lane_port::RawSendPayload>")
            && !public_ops.contains("set_public_send_payload")
            && !endpoint_boundary.contains("set_public_send_payload"),
        "send payload borrows must stay in the send future and cross into the kernel only during poll"
    );
}

#[test]
fn type_level_choreography_stays_segmented_without_new_dsl() {
    let g = read("src/g.rs");
    let global = read("src/global.rs");
    let global_types = read("src/global/types.rs");
    let message = read("src/global/message.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let socket_control = read("tests/cursor_send_recv/socket_control.rs")
        + &read("tests/cursor_send_recv/socket_control_binding.rs");
    let control_lifecycle = read("tests/cursor_send_recv/control_lifecycle.rs");
    let readme = read("README.md");
    let root_allowlist = read(".github/allowlists/g-public-api.txt");
    let production = read_production_rs_tree("src");

    assert!(
        g.contains("pub struct Program<Steps>")
            && g.contains("pub(crate) const ROLE_DOMAIN_SIZE: u8 = 16;")
            && g.contains(
                "pub(crate) const fn role_pair_contract_error<const FROM: u8, const TO: u8>()"
            )
            && g.contains("pub(crate) const fn message_control_contract_error<M>()")
            && g.contains("pub(crate) const fn send_control_contract_error")
            && g.contains("if FROM >= ROLE_DOMAIN_SIZE || TO >= ROLE_DOMAIN_SIZE")
            && g.contains("if let Some(error) = send_control_contract_error::<FROM, TO, M>()")
            && g.contains("let image = const {")
            && g.contains("match ROLE {")
            && g.contains("role_projection_image_for::<15, Steps>()")
            && !g.contains("pub(crate) mod diagnostic")
            && !g.contains("validate_send_control")
            && g.contains("pub use crate::global::Message;")
            && g.contains("pub const fn send<const FROM: u8, const TO: u8, M>()")
            && !g.contains("pub const fn send<const FROM: u8, const TO: u8, M, const LANE")
            && g.contains("pub const fn seq<LeftSteps, RightSteps>(")
            && g.contains("pub const fn route<LeftSteps, RightSteps>(")
            && g.contains("pub const fn par<LeftSteps, RightSteps>(")
            && g.contains("pub struct Msg<const LOGICAL_LABEL: u8, Payload>")
            && g.contains("pub struct ControlMsg<const LOGICAL_LABEL: u8, Kind>")
            && g.contains("pub mod control")
            && g.contains("pub struct StateSnapshot")
            && g.contains("pub struct StateRestore")
            && g.contains("pub struct TxnCommit")
            && g.contains("pub struct TxnAbort")
            && g.contains("pub struct TopologyBegin")
            && g.contains("pub struct TopologyAck")
            && g.contains("pub struct TopologyCommit")
            && !g.contains("pub trait ControlKind")
            && g.contains("pub struct Send<const FROM: u8, const TO: u8, M>")
            && !g.contains("pub struct Send<const FROM: u8, const TO: u8, M, const LANE")
            && g.contains("pub struct Seq<Left, Right>")
            && g.contains("pub struct Route<Left, Right>")
            && g.contains("pub struct Par<Left, Right>")
            && g.contains("pub struct Resolve<Inner, const RESOLVER_ID: u16>")
            && !g.contains("pub(crate) struct Policy<Inner, const POLICY_ID: u16>")
            && !g.contains("pub struct Policy<Inner, const POLICY_ID: u16>")
            && !g.contains("macro_rules!")
            && !g.contains("advanced")
            && !g.contains("loop_"),
        "app-facing choreography DSL must expose only named public witnesses and canonical g combinators"
    );
    assert!(
        global_types.contains("crate::g::ROLE_DOMAIN_SIZE as usize")
            && global.contains("pub(crate) use types::ROLE_DOMAIN_SIZE;")
            && !production.contains("validate_role_index")
            && !production.contains("ROLE_DOMAIN_SIZE: usize = 16"),
        "g must be the single role-domain authority; global internals may consume the size but must not own a second validator or literal domain"
    );
    let project_start = g
        .find("pub(crate) fn project<const ROLE")
        .expect("g project entry must exist");
    let projection_gate = &g[project_start..];
    let gate_validation = projection_gate
        .find("if ROLE >= ROLE_DOMAIN_SIZE")
        .expect("project image gate must validate the role domain");
    let gate_dispatch = projection_gate
        .find("match ROLE {")
        .expect("project image gate must dispatch only validated roles");
    assert!(
        gate_validation < gate_dispatch
            && projection_gate.contains("panic!(\"{}\", ROLE_INDEX_ERROR)")
            && projection_gate.contains("16..=u8::MAX => panic!(\"{}\", ROLE_INDEX_ERROR)")
            && projection_gate.contains("role_projection_image_for::<0, Steps>()")
            && projection_gate.contains("role_projection_image_for::<15, Steps>()")
            && !projection_gate.contains("_ => role_projection_image_for::<0, Steps>()")
            && !projection_gate.contains("role_projection_image_for::<16"),
        "project role validation must stop invalid roles inside g::project before generic RoleProjection can be instantiated"
    );
    assert!(
        !g.contains("pub use crate::global::{par, route, send, seq};"),
        "g combinators must be owned by the app-facing g module, not re-exported from the lower global substrate"
    );
    assert_eq!(
        lines(".github/allowlists/g-public-api.txt"),
        [
            "pub use Program;",
            "pub use Message;",
            "pub use Msg;",
            "pub use ControlMsg;",
            "pub use control;",
            "pub use send, seq, route, par;",
            "pub use Send, Seq, Route, Par;"
        ],
        "semantic surface must guard the app-facing DSL contract instead of pinning internal program-image storage"
    );
    for forbidden in ["advanced", "loop_", "fallback", "legacy", "compat"] {
        assert!(
            !root_allowlist.contains(forbidden) && !readme.contains(&format!("`g::{forbidden}`")),
            "public choreography docs must not grow extra DSL affordances: {forbidden}"
        );
    }
    assert!(
        global.contains("mod message;")
            && global.contains("pub use message::Message;")
            && global.contains("pub(crate) use message::MessageRuntime;")
            && !global.contains(
                "pub(crate) use message::{MessageRuntime, encode_local_control_handle_for};"
            ),
        "message shape and runtime control metadata must live behind a narrow global/message owner"
    );
    let message_start = message
        .find("pub trait Message")
        .expect("Message must exist");
    let message_end = message[message_start..]
        .find("impl<const LOGICAL_LABEL")
        .expect("Message impl must bound public trait body")
        + message_start;
    let message_spec = &message[message_start..message_end];
    assert!(
        message.contains("pub trait Message: seal::Sealed")
            && message.contains("pub(crate) use seal::Sealed as MessageRuntime;")
            && global.contains("pub(crate) struct StaticControlDesc")
            && global.contains("pub(crate) const fn from_runtime_tuple")
            && message.contains("const CONTROL: Option<(u8, u8, u8, u16, u8, u8)>")
            && message.contains(".runtime_tuple()")
            && !message.contains("pub trait Runtime")
            && !message.contains("pub trait Message: seal::Runtime"),
        "public Message must be sealed without exposing a runtime substrate supertrait"
    );
    let public_message_impl = message[message_end..]
        .split("impl<const LOGICAL_LABEL: u8, P> seal::Sealed")
        .next()
        .expect("Message impl segment must be present");
    assert!(
        public_message_impl.contains("Self: seal::Sealed")
            && !public_message_impl.contains("MessageControlSpec"),
        "public Message impl must hide control metadata behind the sealed runtime owner"
    );
    assert!(
        global.contains("pub(crate) trait ControlMsgLowering")
            && global.contains("impl ControlMsgLowering for crate::g::control::LoopContinue")
            && global.contains("impl ControlMsgLowering for crate::g::control::LoopBreak")
            && global.contains("impl ControlMsgLowering for crate::g::control::StateSnapshot")
            && global.contains("impl ControlMsgLowering for crate::g::control::StateRestore")
            && global.contains("impl ControlMsgLowering for crate::g::control::TxnCommit")
            && global.contains("impl ControlMsgLowering for crate::g::control::TxnAbort")
            && !global.contains("impl_local_control_msg_lowering!")
            && !global.contains("impl_wire_control_msg_lowering!")
            && global.contains("impl ControlMsgLowering for crate::g::control::TopologyBegin")
            && global.contains("impl ControlMsgLowering for crate::g::control::TopologyAck")
            && global.contains("impl ControlMsgLowering for crate::g::control::TopologyCommit")
            && g.contains("impl LocalControlKind for StateSnapshot")
            && g.contains("const OP: ControlOp = ControlOp::StateSnapshot")
            && g.contains("impl LocalControlKind for StateRestore")
            && g.contains("const OP: ControlOp = ControlOp::StateRestore")
            && g.contains("impl LocalControlKind for TxnCommit")
            && g.contains("const OP: ControlOp = ControlOp::TxCommit")
            && g.contains("impl LocalControlKind for TxnAbort")
            && g.contains("const OP: ControlOp = ControlOp::TxAbort")
            && !message.contains("impl_control_msg_message!(")
            && !message.contains("impl_control_message!(")
            && message.contains("for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyCommit>")
            && !message.contains("impl<const LOGICAL_LABEL: u8, K> Message for crate::g::ControlMsg<LOGICAL_LABEL, K>")
            && !message.contains("impl<const LOGICAL_LABEL: u8, K> seal::Sealed for crate::g::ControlMsg<LOGICAL_LABEL, K>"),
        "ControlMsg must stay closed to Hibana-owned lowering markers, not arbitrary user kinds"
    );
    assert!(
        g.contains("pub struct StateSnapshot")
            && g.contains("pub struct StateRestore")
            && g.contains("pub struct TxnCommit")
            && g.contains("pub struct TxnAbort")
            && g.contains("ControlScopeKind::State")
            && g.contains("SessionLaneHandle::new")
            && global.contains("impl ControlMsgLowering for crate::g::control::TxnCommit")
            && global.contains("impl ControlMsgLowering for crate::g::control::TxnAbort"),
        "public txn ControlMsg must expose a complete local lifecycle through sealed local lowering, not half-open wire unit markers"
    );
    assert!(
        global.contains("CONTROL_PAYLOAD_LOCAL_UNIT")
            && global.contains("CONTROL_PAYLOAD_WIRE_UNIT")
            && !global.contains("CONTROL_PAYLOAD_WIRE_TOKEN")
            && !message.contains("GenericCapToken")
            && runtime_types.contains("control_payload_kind: u8")
            && send_ops.contains("SendPayloadPlan::MintedWireControl")
            && !send_ops.contains("ExplicitWireControl")
            && send_ops.contains("CONTROL_PAYLOAD_WIRE_UNIT")
            && send_ops.contains("mint_send_wire_control(meta, descriptor)"),
        "wire ControlMsg payload must stay topology-unit only, with no explicit raw token branch"
    );
    assert!(
        socket_control.contains("TcpListener")
            && socket_control.contains("TcpStream")
            && socket_control
                .contains("public_topology_controlmsg_three_phase_roundtrips_over_tcp_loopback")
            && socket_control
                .contains("public_topology_controlmsg_ack_before_begin_fails_closed_over_tcp_loopback")
            && socket_control
                .contains("public_topology_controlmsg_commit_before_ack_fails_closed_over_tcp_loopback")
            && socket_control
                .contains("public_topology_controlmsg_duplicate_begin_fails_closed_over_tcp_loopback")
            && socket_control
                .contains("public_topology_controlmsg_ack_from_source_fails_closed_over_tcp_loopback")
            && socket_control
                .contains("public_topology_controlmsg_peer_drop_removes_role_binding_over_tcp_loopback")
            && socket_control
                .contains("g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>")
            && socket_control
                .contains("g::ControlMsg<TOPOLOGY_ACK_LABEL, g::control::TopologyAck>")
            && socket_control
                .contains("g::ControlMsg<TOPOLOGY_COMMIT_LABEL, g::control::TopologyCommit>")
            && socket_control.contains("ReceivedFrame::framed")
            && socket_control.contains("CONTROL_TOKEN_WIRE_LEN")
            && socket_control.contains("TAP_ENDPOINT_CONTROL")
            && socket_control.contains("TAP_TOPOLOGY_BEGIN")
            && socket_control.contains("TAP_TOPOLOGY_ACK")
            && socket_control.contains("TAP_TOPOLOGY_COMMIT")
            && socket_control.contains("unit public ControlMsg must mint exact wire control token bytes before crossing TCP"),
        "distributed ControlMsg evidence must cross a socket transport, not only a shared-memory test queue"
    );
    assert!(
        control_lifecycle.contains("public_txn_controlmsg_commit_requires_prior_snapshot")
            && control_lifecycle.contains("g::ControlMsg<100, g::control::TxnCommit>")
            && control_lifecycle.contains("g::ControlMsg<101, g::control::StateSnapshot>")
            && control_lifecycle.contains("g::ControlMsg<102, g::control::TxnCommit>")
            && control_lifecycle.contains("g::ControlMsg<104, g::control::TxnAbort>")
            && control_lifecycle.contains("g::ControlMsg<106, g::control::StateRestore>")
            && control_lifecycle.contains("PhaseInvariant")
            && control_lifecycle.contains("TAP_ENDPOINT_CONTROL")
            && control_lifecycle.contains("TAP_STATE_SNAPSHOT_REQ")
            && control_lifecycle.contains("TAP_POLICY_COMMIT")
            && control_lifecycle.contains("TAP_POLICY_TX_ABORT")
            && control_lifecycle.contains("TAP_STATE_RESTORE_REQ")
            && control_lifecycle.contains("transport_queue_is_empty(&transport)"),
        "public txn ControlMsg lifecycle must have runtime evidence for fail-closed missing snapshot and local self-send terminal operations"
    );
    for forbidden in [
        "CONTROL",
        "StaticControlDesc",
        "ENCODE_CONTROL_HANDLE",
        "decode_validated_payload",
        "ControlKind",
        "type Payload: crate::transport::wire::WirePayload",
    ] {
        assert!(
            !message_spec.contains(forbidden),
            "public Message must stay a thin message shape, not expose runtime control substrate: {forbidden}"
        );
    }
}

#[test]
fn ui_diagnostics_stay_on_public_choreography_vocabulary() {
    let ui_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let mut diagnostics = String::new();
    let mut resolver_diagnostics = String::new();
    let mut user_kind_bound_diagnostics = String::new();
    for entry in std::fs::read_dir(&ui_dir)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", ui_dir.display()))
    {
        let path = entry
            .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", ui_dir.display()))
            .path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("stderr") {
            let text = read_plain(&path);
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if file_name.starts_with("g-control-resolver-") {
                resolver_diagnostics.push_str(&text);
            } else if file_name == "g-control-user-kind-message-bound.stderr" {
                user_kind_bound_diagnostics.push_str(&text);
            } else if path.file_name().and_then(|name| name.to_str()).is_some() {
                diagnostics.push_str(&text);
            }
        }
    }

    assert!(
        resolver_diagnostics
            .contains("route resolver site is not supported for this control protocol event"),
        "control resolver misuse diagnostics must expose the public choreography error"
    );
    assert!(
        user_kind_bound_diagnostics.contains("ControlMsg<40, MyControl>: Message"),
        "user-defined control kind diagnostics must expose the public Message bound failure"
    );
    for forbidden in [
        "LocalControlKind",
        "ControlMsgLowering",
        "hibana::global",
        "hibana::control::cap",
    ] {
        assert!(
            !user_kind_bound_diagnostics.contains(forbidden),
            "user-defined control kind diagnostics must not expose internal control substrate: {forbidden}"
        );
    }

    for forbidden in [
        "BuildProgramSource",
        "ResidentProgram",
        "ResidentRole",
        "hibana::g::Choreography",
        "ChoreographyTerm",
        "ProgramTerm",
        "ProgramProjection",
        "RoleProjection",
        "ProgramImage",
        "CompiledRoleImage",
        "ProgramSourceData",
        "ProgramSourceError",
        "ProjectedRole",
        "ProgramRoleImage",
        "crate::global",
        "hibana::global",
        "hibana::global::compiled",
        "PROGRAM_SOURCE",
        "ProjectedProgram",
        "::SOURCE",
        "g::choreography",
        "global::program::source",
        "MessageRuntime",
        "MessageControlSpec",
        "KnownRole",
        "ValidatedProgram",
        "project_typed_program",
        "validate_program_projection",
        "panic_program_source_error",
        "project_role::<",
        "hibana::global::validate",
        "validate_send_contract",
        "validate_message_control_contract",
        "message_control_contract_error",
        "send_control_contract_error",
        "MessageControlContractError",
        "resource_kinds",
        "panic_message_control_contract_error",
        "g::diagnostic",
        "validate_send_control",
        "validate_role_pair",
        "validate_role::<",
        "validate_token_control_payload_contract",
        "validate_control_descriptor_contract",
        "global::types::Message",
        "LabelMarker",
        "LabelTag",
        "RouteArmHead",
        "RouteArmLoopHead",
        "TailLoopControl",
        "FragmentShape",
        "NonEmptyParallelArm",
        "assert_distinct_route_labels",
        "witness_impls",
        "policy control",
        "loop arm order",
        "loop arm pair",
    ] {
        assert!(
            !diagnostics.contains(forbidden),
            "UI diagnostics must use public choreography vocabulary, not internal substrate name: {forbidden}"
        );
    }
    assert!(
        diagnostics.contains("hibana::g::Msg") && diagnostics.contains("no method named `policy`"),
        "UI diagnostics must stay on public g::Msg and avoid internal control-message candidates"
    );
}

#[test]
fn transport_contract_documents_payload_and_staged_frame_observation() {
    let readme = read("README.md");
    let transport = transport_source();
    let transport_tests = read("src/transport/tests.rs");
    let test_transport = read("tests/common/mod.rs");

    for (path, source) in [
        ("README.md", readme.as_str()),
        ("src/transport.rs", transport.as_str()),
    ] {
        assert!(
            source.contains("open(port)") || source.contains("PortOpen"),
            "{path} must document Transport::open as a descriptor-derived port witness"
        );
        assert!(
            !source.contains("pub struct Incoming<'f>"),
            "{path} must not keep the old Incoming receive wrapper"
        );
        assert!(
            source.contains("ReceivedFrame")
                && source.contains("payload")
                && source.contains("IngressEvidence"),
            "{path} must document integrated receive payload/evidence observation"
        );
    }

    assert!(
        !readme.contains("open(local_role")
            && !readme.contains("open(local_role, session_id, lane)"),
        "README must not keep the old raw Transport::open contract"
    );
    assert!(
        readme.contains("`requeue(...)` as the required rollback path")
            && transport.contains("A no-op requeue violates the")
            && transport.contains("endpoint rollback contract"),
        "Transport::requeue must be documented as a required rollback contract, not an optional best-effort hook"
    );
    assert!(
        !readme.contains("peek_recv_frame")
            && !transport.contains("fn peek_recv_frame")
            && !test_transport.contains("peek_recv_frame"),
        "receive metadata must not use a side-channel observation hook"
    );
    assert!(
        !transport_tests.contains("let _ = rx;") && !test_transport.contains("_lane: u8"),
        "test transports must not silently ignore rollback or opened logical lanes"
    );
}

#[test]
fn transport_frame_and_mismatch_evidence_have_single_owners() {
    let recv = read("src/endpoint/kernel/recv.rs");
    let decode_finish = read("src/endpoint/kernel/decode/finish.rs");
    let lane_port = read("src/endpoint/kernel/lane_port.rs");
    let observe = read("src/endpoint/kernel/observe.rs");
    let offer_ingress = read("src/endpoint/kernel/offer/ingress.rs");
    let offer_passive = read("src/endpoint/kernel/offer/passive.rs");
    let offer_materialization = read("src/endpoint/kernel/offer/materialization.rs");
    let offer = read("src/endpoint/kernel/offer.rs");
    let offer_state = read("src/endpoint/kernel/offer/state.rs");
    let public_types = read("src/endpoint/kernel/core/public_types.rs");
    let port = read("src/rendezvous/port.rs");
    let route_hints = read("src/rendezvous/port/route_hints.rs");
    let recv_frame = read("src/rendezvous/port/recv_frame.rs");
    let transport = read("src/transport.rs");
    let buckets = read("src/integration/buckets.rs");
    let ids = read("src/observe/ids.rs");
    let production = read_production_rs_tree("src");

    assert!(
        production.contains("transport_frame_tap_event")
            && production.contains("RawEvent::new(now32, ids::TRANSPORT_FRAME)")
            && buckets.contains("TRANSPORT_FRAME")
            && ids.contains("pub const TRANSPORT_FRAME"),
        "staged transport frames must have canonical TransportFrame evidence for EPF/debug observation"
    );
    assert!(
        recv.contains("poll_accepted_transport_frame(")
            && decode_finish.contains("poll_accepted_transport_frame(")
            && observe.contains("fn poll_accepted_transport_frame(")
            && observe.contains("fn poll_received_transport_frame_for_lane(")
            && lane_port.contains("fn poll_recv_frame_preamble")
            && lane_port.contains("expected_session_raw: u32")
            && lane_port.contains("expected_source_role: u8")
            && lane_port.contains("expected_peer_role: u8")
            && lane_port.contains("expected_label: u8")
            && observe.contains("fn accept_materialized_transport_frame(")
            && offer_ingress.contains("poll_received_transport_frame_for_lane(")
            && offer_passive.contains("poll_received_transport_frame_for_lane(")
            && offer_materialization.contains("accept_materialized_transport_frame(")
            && offer_state.contains("Option<lane_port::PreambleFrame")
            && offer_passive
                .contains("let observed_frame_label = frame.observed_frame_label_raw();")
            && offer_passive.contains("state.stage_transport(frame);")
            && public_types.contains("Transport { frame: lane_port::ReceivedFrame")
            && recv_frame.contains("pub(crate) struct PreambleFrame")
            && recv_frame.contains("FrameObservation")
            && recv_frame.contains("mismatch_preamble(")
            && recv_frame.contains("observed_source_label: ObservedSourceLabel")
            && recv_frame.contains("struct ObservedSourceLabel(u32)")
            && recv_frame.contains("observed: FrameObservation")
            && !recv_frame.contains("observed: Option<FrameObservation>")
            && !recv_frame.contains("label_raw_if_known")
            && !recv_frame.contains("observation_if_known")
            && !recv_frame.contains("from_optional_observation")
            && !recv_frame.contains("const UNKNOWN")
            && recv_frame.contains("pub(crate) const fn observed_frame_label_raw(&self) -> u8")
            && recv_frame.contains("pub(crate) const fn observed_transport_frame(")
            && !recv_frame.contains(") -> Option<FrameObservation>")
            && recv_frame.contains("mismatch_expected(source_role, frame_label)")
            && !recv_frame.contains("if !self.is_known()")
            && recv_frame.contains("expected_session_raw")
            && recv_frame.contains("expected_peer_role")
            && recv_frame.contains("pub(super) fn has_outstanding(&self) -> bool")
            && lane_port.contains("fn poll_recv_payload")
            && lane_port.contains(".evidence()")
            && lane_port.contains(".frame_header()")
            && lane_port.contains("let Some(observed) = observed else")
            && lane_port.contains("emit_transport_frame_observation")
            && transport.contains("Poll<Result<ReceivedFrame<'a>, Self::Error>>")
            && transport.contains("pub struct ReceivedFrame<'f>")
            && transport.contains("pub enum IngressEvidence")
            && !transport.contains("fn peek_recv_frame")
            && !transport.contains("pub struct Incoming<'f>")
            && port.contains("fn has_unresolved_recv_frame(&self) -> bool")
            && route_hints.contains("pub(super) struct RouteHintQueue")
            && offer_state.contains(".map(lane_port::PreambleFrame::observed_frame_label_raw)")
            && !offer_materialization
                .contains(".and_then(lane_port::PreambleFrame::observed_frame_label_raw)")
            && offer_materialization
                .contains("let observed_frame_label = payload.observed_frame_label_raw();")
            && offer_materialization.contains("transport_payload_matches_branch_lane")
            && offer_materialization.contains("MaterializedTransport::DiscardedAndPending")
            && offer.contains("Ok(None) =>")
            && offer.contains("return Poll::Pending;")
            && recv_frame.contains("from_accepted_payload")
            && recv_frame.contains("pub(crate) fn accept_parts(")
            && recv_frame.contains("transport_frame_tap_event")
            && !production.contains("ReceivedFrame::from_port")
            && !production.contains("PreambleFrame::from_port")
            && !recv_frame.contains("pub(crate) fn into_frame")
            && !production.contains("RouteHintContext")
            && !production.contains("peek_current_frame_observation"),
        "direct receive/decode must full-accept observed frames, while offer/passive may only stage PreambleFrame until the selected descriptor promotes it to ReceivedFrame; frame metadata must come from same-Rx staged observation, not route hints"
    );
    assert!(
        recv_frame.contains("#[repr(u8)]")
            && recv_frame.contains("Session = ids::TRANSPORT_MISMATCH_SESSION")
            && recv_frame.contains("Lane = ids::TRANSPORT_MISMATCH_LANE")
            && recv_frame.contains("SourceRole = ids::TRANSPORT_MISMATCH_SOURCE_ROLE")
            && recv_frame.contains("PeerRole = ids::TRANSPORT_MISMATCH_PEER_ROLE")
            && recv_frame.contains("Label = ids::TRANSPORT_MISMATCH_LABEL")
            && recv_frame.contains("pub(crate) const fn tap_reason(self) -> u8")
            && recv_frame.contains("self as u8")
            && recv_frame.contains("pub(crate) fn tap_event(")
            && observe.contains("mismatch.tap_event(")
            && lane_port.contains("mismatch.tap_event("),
        "TransportMismatch reason encoding must be owned by the frame mismatch type"
    );
    for forbidden in [
        "TransportError::Deadline => ids::TRANSPORT_MISMATCH",
        "TransportError::Capacity => ids::TRANSPORT_MISMATCH",
        "TransportError::Offline => ids::TRANSPORT_MISMATCH",
        "TransportError::Failed => ids::TRANSPORT_MISMATCH",
        "RecvError::PhaseInvariant => ids::TRANSPORT_MISMATCH",
        "RecvError::Codec(_) => ids::TRANSPORT_MISMATCH",
    ] {
        assert!(
            !production.contains(forbidden),
            "TransportMismatch must not become a generic bucket for TransportError, PhaseInvariant, or codec/decode failures: {forbidden}"
        );
    }
}

#[test]
fn resolver_reject_error_captures_public_callsite() {
    let reject_line = line!() + 1;
    let error = hibana::integration::resolver::ResolverError::reject();

    assert_eq!(error.operation(), "reject");
    assert!(
        error
            .file()
            .ends_with("tests/semantic_surface/transport_topology.rs")
    );
    assert_eq!(error.line(), reject_line);
}

#[test]
fn topology_validation_has_no_test_only_semantic_owner() {
    let distributed = read("src/control/automaton/distributed.rs");
    let rendezvous_topology = read("src/rendezvous/topology.rs");
    let rendezvous_core = rendezvous_core_source();

    for forbidden in [
        "TopologyCommitAutomaton",
        "pub(crate) fn process_intent",
        "DistributedTopology::process_intent",
        "pub(super) fn topology_commit",
        ".topology.topology_commit(",
    ] {
        assert!(
            !distributed.contains(forbidden)
                && !rendezvous_topology.contains(forbidden)
                && !rendezvous_core.contains(forbidden),
            "topology validation must use production cluster/rendezvous paths, not test-only owner: {forbidden}"
        );
    }

    assert!(
        !repo_file_exists("src/control/automaton/topology.rs"),
        "topology validation must not retain a test-only automaton owner file"
    );

    assert!(
        !rendezvous_core.contains("fn perform_effect("),
        "test-only effect replay must live under src/**/tests/**, not in production rendezvous core modules"
    );
}

#[test]
fn stable_public_api_gate_has_no_nightly_or_rustdoc_json_owner() {
    let script = read(".github/scripts/check_hibana_public_api.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let combined = format!("{script}\n{final_gate}\n{workflow}");

    for required in [
        "export TOOLCHAIN=\"${TOOLCHAIN:-1.95.0}\"",
        "bash ./.github/scripts/run_final_form_gates.sh",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "stable public API check passed",
    ] {
        assert!(
            combined.contains(required),
            "Rust 1.95 public API gate missing required owner: {required}"
        );
    }

    for forbidden in [
        "dtolnay/rust-toolchain@nightly",
        "rustup which cargo --toolchain nightly",
        "rustup which rustc --toolchain nightly",
        "rustup which rustdoc --toolchain nightly",
        "target/doc/hibana.json",
        "HIBANA_RUSTDOC_JSON",
        "-Z unstable-options",
        "--output-format json",
    ] {
        assert!(
            !combined.contains(forbidden),
            "stable public API gate must not depend on nightly rustdoc JSON: {forbidden}"
        );
    }
}
