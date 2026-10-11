use super::{FrameHeader, FrameLabel};
use crate::session::types::SessionId;

#[kani::proof]
fn frame_header_roundtrip_preserves_every_field() {
    let session_raw: u32 = kani::any();
    let lane: u8 = kani::any();
    let source: u8 = kani::any();
    let target: u8 = kani::any();
    let label_raw: u8 = kani::any();

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
}

#[kani::proof]
fn frame_header_identity_is_exact_and_injective() {
    let left: [u8; 8] = kani::any();
    let right: [u8; 8] = kani::any();

    let left_header = FrameHeader::from_bytes(left);
    let right_header = FrameHeader::from_bytes(right);

    assert!((left_header == right_header) == (left == right));
    assert!((left_header.bytes() == right_header.bytes()) == (left == right));
    kani::cover!(left == right);
    kani::cover!(left != right);
}
