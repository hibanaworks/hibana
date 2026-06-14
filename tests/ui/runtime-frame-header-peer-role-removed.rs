use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, FrameLabel},
};

fn main() {
    let header = FrameHeader::new(SessionId::new(1), 0, 0, 1, FrameLabel::new(7));
    let _ = header.peer_role();
}
