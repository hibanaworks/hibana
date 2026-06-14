use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, FrameLabel, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [];
    let header = FrameHeader::new(SessionId::new(1), 0, 0, 1, FrameLabel::new(7));
    let frame = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = frame.evidence();
}
