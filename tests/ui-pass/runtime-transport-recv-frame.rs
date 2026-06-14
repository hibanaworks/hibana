use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, FrameLabel, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [1u8, 2, 3];
    let deterministic = ReceivedFrame::deterministic(Payload::new(&payload));
    let _ = deterministic.payload();

    let header = FrameHeader::new(SessionId::new(1), 2, 3, 4, FrameLabel::new(5));
    let framed = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = framed.payload();
    let _ = header.target_role();
}
