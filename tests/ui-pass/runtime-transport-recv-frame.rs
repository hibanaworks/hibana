use hibana::runtime::{
    transport::{FrameHeader, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [1u8, 2, 3];
    let deterministic = ReceivedFrame::deterministic(Payload::new(&payload));
    let _ = deterministic.payload();

    let header = FrameHeader::from_raw(0x0000_0001_0203_0405);
    let framed = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = framed.payload();
    let _ = header.raw();
}
