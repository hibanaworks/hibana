use super::common::*;

#[test]
fn unsafe_contract_gate_covers_receive_frame_receipt_owner() {
    let gate = read(".github/scripts/check_unsafe_contract_hygiene.sh");
    let receipt = read("src/rendezvous/recv_frame_receipt.rs");
    let port_recv = read("src/rendezvous/port/recv_frame.rs");

    assert!(
        gate.contains("cat src/rendezvous/recv_frame_receipt.rs")
            && gate.contains("if self.outstanding.replace(true)")
            && receipt.contains("if self.outstanding.replace(true)")
            && receipt.contains("if !self.outstanding.get()")
            && port_recv.contains("impl Drop for ReceivedFrameCore"),
        "unsafe contract gate must scan both the receive-frame value owner and its receipt authority owner"
    );
}
