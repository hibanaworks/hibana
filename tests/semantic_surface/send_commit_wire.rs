use super::common::*;

#[test]
fn inbound_explicit_wire_tokens_share_descriptor_header_authority_before_commit() {
    let recv = read("src/endpoint/kernel/recv.rs");
    let recv_control = read("src/endpoint/kernel/recv_control.rs");
    let decode_finish = read("src/endpoint/kernel/decode/finish.rs");
    let futures = read("src/endpoint/futures.rs");
    let descriptor_controls = read("src/control/cluster/core/descriptor_controls.rs");
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let send_control_ops = read("src/endpoint/kernel/core/send_control_ops.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let topology_from_handle = concat!("topology_", "operands_from_handle");
    let prepare_topology_from_handle = concat!("prepare_", "topology_", "operands_from_handle");
    let validate_topology_from_handle = concat!("validate_", "topology_", "operands_from_handle");

    assert!(
        recv_control.contains("fn validate_inbound_explicit_wire_control(")
            && recv_control.contains("control: Option<ControlDesc>")
            && recv_control.contains("if !matches!(control.path(), ControlPath::Wire)")
            && recv_control.contains("if bytes.len() != CAP_TOKEN_LEN")
            && recv_control.contains(".validate_bound_descriptor_control_frame(")
            && recv_control.contains("self.descriptor_recv_epoch(control, lane)?")
            && recv
                .find("self.validate_inbound_explicit_wire_control(desc, control, payload)")
                .expect("recv must validate inbound explicit wire token")
                < recv
                    .find("let next_index = match self.cursor.try_next_index_past_jumps()")
                    .expect("recv cursor commit must happen after validation"),
        "recv must validate explicit GenericCapToken descriptor/header authority before cursor commit"
    );
    let decode_finish_start = decode_finish
        .find("fn finish_route_branch_decode(")
        .expect("route branch decode finish must exist");
    let decode_finish_body = &decode_finish[decode_finish_start..];
    let decode_finish_body = &decode_finish_body[..decode_finish_body
        .find("\n}\n\nimpl<'r")
        .expect("route branch decode finish body must be bounded")];
    assert!(
        decode_finish_body.contains("control: Option<crate::global::ControlDesc>")
            && decode_finish_body.contains("self.validate_inbound_explicit_wire_control(")
            && decode_finish_body
                .find("self.validate_inbound_explicit_wire_control(recv_desc, control, payload)")
                .expect("decode must validate inbound explicit wire token")
                < decode_finish_body
                    .find("let next_index = self")
                    .expect("decode branch commit must happen after validation"),
        "route-branch decode must share recv explicit-wire descriptor/header validation before branch commit"
    );
    assert!(
        futures.contains("<M as MessageRuntime>::CONTROL.map(ControlDesc::from_static)")
            && descriptor_controls
                .contains("pub(crate) fn validate_bound_descriptor_control_frame")
            && descriptor_controls.contains("pub(crate) struct ValidatedDescriptorControlFrame")
            && descriptor_controls.contains("pub(crate) enum ValidatedDescriptorControlEffect")
            && descriptor_controls.contains("TopologyBegin(TopologyOperands)")
            && descriptor_controls.contains("TopologyAck(TopologyOperands)")
            && descriptor_controls.contains("TopologyCommit(TopologyOperands)")
            && !descriptor_controls.contains("generation: Option<Generation>")
            && !descriptor_controls.contains(topology_from_handle)
            && prepared_send.contains("let frame = self.validate_bound_descriptor_control_frame(")
            && prepared_send.contains("match frame.effect")
            && !prepared_send.contains("prepare_topology_descriptor_terminal")
            && !prepared_send.contains("GenericCapToken::<()>::from_raw_bytes(bytes)")
            && !prepared_send.contains("TopologyDescriptor::decode_for")
            && !prepared_send.contains("self.validate_topology_begin_operands(")
            && !prepared_send.contains("self.validate_topology_ack_operands(")
            && !prepared_send.contains("self.validate_topology_commit_operands(")
            && !send_control_ops.contains(prepare_topology_from_handle)
            && !send_control_ops.contains(validate_topology_from_handle)
            && !send_control_ops.contains("mint_local_topology_begin_control")
            && !send_control_ops.contains("mint_local_topology_ack_control")
            && !send_control_ops.contains("TopologyDescriptor::decode_for")
            && !send_control_ops.contains("TopologyDescriptor,")
            && send_ops.contains(
                "ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit",
            )
            && !send_ops.contains("mint_local_topology_begin_control")
            && !send_ops.contains("mint_local_topology_ack_control")
            && !runtime_types
                .split("pub(crate) struct RecvRuntimeDesc")
                .nth(1)
                .and_then(|tail| tail.split("pub(crate) struct DecodeRuntimeDesc").next())
                .expect("RecvRuntimeDesc body must be readable")
                .contains("ControlDesc")
            && !runtime_types
                .split("pub(crate) struct DecodeRuntimeDesc")
                .nth(1)
                .and_then(|tail| tail.split("pub(crate) struct SendRuntimeDesc").next())
                .expect("DecodeRuntimeDesc body must be readable")
                .contains("ControlDesc"),
        "explicit wire validation must return validated frame facts shared by recv and send without duplicate local token/header/topology decode"
    );
}
