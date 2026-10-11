use hibana::runtime::transport::FrameHeader;

fn main() {
    let header = FrameHeader::from_bytes([0, 0, 0, 1, 0, 0, 1, 7]);
    let _ = header.peer_role();
}
