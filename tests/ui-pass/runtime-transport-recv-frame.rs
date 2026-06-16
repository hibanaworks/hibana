use hibana::runtime::{
    transport::{FrameHeader, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [1u8, 2, 3];
    let deterministic = ReceivedFrame::deterministic(Payload::new(&payload));
    let _ = deterministic.payload();

    let header = FrameHeader::from_bytes([0, 0, 0, 1, 2, 3, 4, 5]);
    let framed = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = framed.payload();
    let _ = header.bytes();
}
