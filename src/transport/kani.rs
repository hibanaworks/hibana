use super::{FrameHeader, FrameLabel};
use crate::session::types::SessionId;

#[kani::proof]
fn frame_header_roundtrip_preserves_every_field() {
    let session_raw: u32 = kani::any();
    let lane: u8 = kani::any();
    let source: u8 = kani::any();
    let target: u8 = kani::any();
    let label_raw: u16 = kani::any();

    let header = FrameHeader::from_parts(
        SessionId::new(session_raw),
        lane,
        source,
        target,
        FrameLabel::new(label_raw),
    );

    assert!(header.session().raw() == session_raw);
    assert!(header.lane() == lane);
    assert!(header.source_role() == source);
    assert!(header.target_role() == target);
    assert!(header.label().raw() == label_raw);

    let observation = crate::rendezvous::port::FrameObservation::from_header(header);
    assert!(observation.session_raw() == session_raw);
    assert!(observation.lane_wire() == lane);
    assert!(observation.source_role() == source);
    assert!(observation.target_role() == target);
    assert!(observation.label_raw() == label_raw);
    let event = crate::rendezvous::port::transport_frame_tap_event(observation);
    let evidence = event.evidence().input();
    assert!(evidence[0] == session_raw);
    assert!(evidence[1] == ((source as u32) << 24) | ((target as u32) << 16) | label_raw as u32);
    assert!(evidence[3] & 0xff00 == (lane as u32) << 8);
}

#[kani::proof]
fn frame_header_identity_is_exact_and_injective() {
    let left: [u8; 9] = kani::any();
    let right: [u8; 9] = kani::any();

    let left_header = FrameHeader::from_bytes(left);
    let right_header = FrameHeader::from_bytes(right);

    assert!((left_header == right_header) == (left == right));
    assert!((left_header.bytes() == right_header.bytes()) == (left == right));
    kani::cover!(left == right);
    kani::cover!(left != right);
}
