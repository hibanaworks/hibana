use super::super::{DecodeRuntimeDesc, RecvRuntimeDesc};
use super::*;
use crate::{
    control::cap::mint::{ControlOp, ControlPath},
    endpoint::flow::send_runtime_desc,
    g,
    global::role_program::LaneSetView,
    global::role_program::{LaneWord, lane_word_index},
    global::{CONTROL_PAYLOAD_LOCAL_UNIT, CONTROL_PAYLOAD_WIRE_UNIT},
    transport::FrameLabel,
    transport::wire::CodecError,
};

fn validate_empty_payload(payload: Payload<'_>) -> Result<(), CodecError> {
    if payload.as_bytes().is_empty() {
        Ok(())
    } else {
        Err(CodecError::Invalid("runtime descriptor test"))
    }
}

fn synthetic_empty_payload<'a>(scratch: &'a mut [u8]) -> Result<Payload<'a>, CodecError> {
    Ok(Payload::new(&scratch[..0]))
}

fn next_preferred_lane_in_lane_set(
    preferred_lane_idx: usize,
    offer_lanes: LaneSetView,
    lane_limit: usize,
    scan_idx: &mut usize,
) -> Option<usize> {
    if *scan_idx == 0 {
        *scan_idx = 1;
        if preferred_lane_idx < lane_limit && offer_lanes.contains(preferred_lane_idx) {
            return Some(preferred_lane_idx);
        }
    }

    let mut start = *scan_idx - 1;
    while let Some(lane_idx) = offer_lanes.next_set_from(start, lane_limit) {
        *scan_idx = lane_idx.checked_add(2).expect("scan index overflow");
        start = lane_idx.checked_add(1).expect("scan index overflow");
        if lane_idx != preferred_lane_idx {
            return Some(lane_idx);
        }
    }

    None
}

#[test]
fn runtime_descriptors_are_constructed_with_frame_label() {
    let recv = RecvRuntimeDesc::new(7, FrameLabel::new(42), true, false);
    assert_eq!(recv.core.logical_label(), 7);
    assert_eq!(recv.frame_label(), FrameLabel::new(42));
    assert!(recv.expects_control());

    let decode = DecodeRuntimeDesc::new(
        8,
        FrameLabel::new(43),
        false,
        validate_empty_payload,
        synthetic_empty_payload,
    );
    assert_eq!(decode.logical_label(), 8);
    assert_eq!(decode.frame_label(), FrameLabel::new(43));

    let send = SendRuntimeDesc::new(
        9,
        FrameLabel::new(44),
        false,
        None,
        0,
        crate::transport::wire::erased_encoder::<()>(),
        None,
    );
    assert_eq!(send.logical_label(), 9);
    assert_eq!(send.frame_label(), FrameLabel::new(44));
    assert_eq!(send.control_payload_kind(), 0);
}

#[test]
fn send_runtime_descriptor_preserves_control_payload_family() {
    let local =
        send_runtime_desc::<g::ControlMsg<10, g::control::LoopContinue>>(FrameLabel::new(10));
    let local_control = local.control().expect("local control descriptor");
    assert!(local.expects_control());
    assert_eq!(local.control_payload_kind(), CONTROL_PAYLOAD_LOCAL_UNIT);
    assert_eq!(local_control.path(), ControlPath::Local);
    assert_eq!(local_control.op(), ControlOp::LoopContinue);
    assert!(local.encode_control_handle().is_some());

    let state =
        send_runtime_desc::<g::ControlMsg<11, g::control::StateSnapshot>>(FrameLabel::new(11));
    let state_control = state.control().expect("state snapshot control descriptor");
    assert!(state.expects_control());
    assert_eq!(state.control_payload_kind(), CONTROL_PAYLOAD_LOCAL_UNIT);
    assert_eq!(state_control.path(), ControlPath::Local);
    assert_eq!(state_control.op(), ControlOp::StateSnapshot);
    assert!(state.encode_control_handle().is_some());

    let tx_commit =
        send_runtime_desc::<g::ControlMsg<12, g::control::TxnCommit>>(FrameLabel::new(12));
    let tx_commit_control = tx_commit.control().expect("txn commit control descriptor");
    assert!(tx_commit.expects_control());
    assert_eq!(tx_commit.control_payload_kind(), CONTROL_PAYLOAD_LOCAL_UNIT);
    assert_eq!(tx_commit_control.path(), ControlPath::Local);
    assert_eq!(tx_commit_control.op(), ControlOp::TxCommit);
    assert!(tx_commit.encode_control_handle().is_some());

    let wire =
        send_runtime_desc::<g::ControlMsg<20, g::control::TopologyBegin>>(FrameLabel::new(20));
    let wire_control = wire.control().expect("wire control descriptor");
    assert!(wire.expects_control());
    assert_eq!(wire.control_payload_kind(), CONTROL_PAYLOAD_WIRE_UNIT);
    assert_eq!(wire_control.path(), ControlPath::Wire);
    assert_eq!(wire_control.op(), ControlOp::TopologyBegin);
}

#[test]
fn preferred_lane_iteration_returns_preferred_then_lower_lanes_then_higher_lanes() {
    let mut words = [0 as LaneWord; 1];
    for lane in [0usize, 5, 7] {
        let (word_idx, bit) = lane_word_index(lane);
        words[word_idx] |= bit;
    }
    let view = LaneSetView::from_parts(words.as_ptr(), words.len());
    let mut scan_idx = 0usize;

    assert_eq!(
        next_preferred_lane_in_lane_set(5, view, 8, &mut scan_idx),
        Some(5)
    );
    assert_eq!(
        next_preferred_lane_in_lane_set(5, view, 8, &mut scan_idx),
        Some(0)
    );
    assert_eq!(
        next_preferred_lane_in_lane_set(5, view, 8, &mut scan_idx),
        Some(7)
    );
    assert_eq!(
        next_preferred_lane_in_lane_set(5, view, 8, &mut scan_idx),
        None
    );
}
