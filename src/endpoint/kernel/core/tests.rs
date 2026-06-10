use super::super::{DecodeRuntimeDesc, RecvRuntimeDesc};
use super::*;
use crate::{
    global::role_program::LaneSetView,
    global::role_program::{LaneWord, lane_word_index},
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
        crate::transport::wire::erased_encoder::<()>(),
        None,
    );
    assert_eq!(send.logical_label(), 9);
    assert_eq!(send.frame_label(), FrameLabel::new(44));
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
