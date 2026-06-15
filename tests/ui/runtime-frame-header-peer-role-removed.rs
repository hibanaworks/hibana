use hibana::runtime::transport::FrameHeader;

fn main() {
    let header = FrameHeader::from_raw(0x0000_0001_0000_0107);
    let _ = header.peer_role();
}
