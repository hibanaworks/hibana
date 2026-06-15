use hibana::runtime::{
    transport::{FrameHeader, ReceivedFrame},
    wire::Payload,
};

fn main() {
    let payload = [];
    let header = FrameHeader::from_raw(0x0000_0001_0000_0107);
    let frame = ReceivedFrame::framed(header, Payload::new(&payload));
    let _ = frame.evidence();
}
