use hibana::integration::{
    ids::{Lane, SessionId},
    transport::PortOpen,
};

fn main() {
    let _ = PortOpen::from_descriptor(0, SessionId::new(7), Lane::new(0));
}
