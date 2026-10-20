//! The test injector closes a receive port. It is deliberately not a claim
//! about source-selective EOF on a multiplexed port, or application success.
use super::*;
use crate::session::types::Lane;
use crate::transport::{PortOpen, Transport, TransportError};

#[test]
fn port_close_drains_all_sources_without_closing_another_port() {
    let network = TestTransport::new();
    let sid = SessionId::new(9062);
    let (mut first, _) = network.open(PortOpen::from_descriptor(0, sid, Lane::new(0)));
    let (mut second, _) = network.open(PortOpen::from_descriptor(2, sid, Lane::new(0)));
    let mut rx = network.open_rx(sid, 1, 0);
    network.stage_send(&mut first, 1, 0, 0, &[7]);
    network.stage_send(&mut second, 1, 0, 0, &[8]);
    assert!(matches!(
        network.poll_send_staged(&mut first),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        network.poll_send_staged(&mut second),
        Poll::Ready(Ok(()))
    ));
    network.close_port(sid, 1, 0);
    let mut cx = Context::from_waker(Waker::noop());
    for source in [0, 2] {
        let Poll::Ready(Ok(frame)) = network.poll_recv_current(&mut rx, &mut cx) else {
            panic!("accepted frames from both sources must drain");
        };
        assert_eq!(
            frame.evidence().frame_header().unwrap().source_role(),
            source
        );
    }
    assert!(matches!(
        network.poll_recv_current(&mut rx, &mut cx),
        Poll::Ready(Err(TransportError::Offline))
    ));
    for tx in [&mut first, &mut second] {
        network.stage_send(tx, 1, 0, 0, &[9]);
        assert!(matches!(
            network.poll_send_staged(tx),
            Poll::Ready(Err(TransportError::Offline))
        ));
    }
    let (mut independent, _) = network.open(PortOpen::from_descriptor(0, sid, Lane::new(1)));
    let mut other_rx = network.open_rx(sid, 1, 1);
    network.stage_send(&mut independent, 1, 1, 0, &[10]);
    assert!(matches!(
        network.poll_send_staged(&mut independent),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        network.poll_recv_current(&mut other_rx, &mut cx),
        Poll::Ready(Ok(_))
    ));
    assert!(network.queue_is_empty());
}

#[test]
fn drained_close_and_abort_do_not_encode_application_completion() {
    let closures: [fn(&TestTransport, SessionId, u8, u8); 2] =
        [TestTransport::close_port, TestTransport::abort_port];
    for close in closures {
        let network = TestTransport::new();
        let sid = SessionId::new(9063);
        let mut rx = network.open_rx(sid, 1, 0);
        close(&network, sid, 1, 0);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            network.poll_recv_current(&mut rx, &mut cx),
            Poll::Ready(Err(TransportError::Offline))
        ));
    }
}
