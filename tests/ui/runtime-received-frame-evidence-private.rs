use hibana::runtime::{
    transport::{FrameHeader, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [];
    let header = FrameHeader::from_bytes([0, 0, 0, 1, 0, 0, 1, 7]);
    let frame = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = frame.evidence();
}
