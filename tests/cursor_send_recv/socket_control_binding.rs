use super::socket_control::{
    SOCKET_SESSION_SLOT, TAP_TOPOLOGY_BEGIN, TOPOLOGY_BEGIN_LABEL, TcpLoopbackTransport,
    tap_pair_count, with_socket_fixture_pair,
};
use super::*;

use hibana::integration::runtime::DefaultLabelUniverse;

#[test]
fn public_topology_controlmsg_peer_drop_removes_role_binding_over_tcp_loopback() {
    with_socket_fixture_pair(|tap0, slab0, tap1, slab1| {
        let tap0_ptr = tap0.as_mut_ptr();
        let tap1_ptr = tap1.as_mut_ptr();
        let transport = TcpLoopbackTransport::new();
        with_resident_tls_ref(&SOCKET_SESSION_SLOT, |cluster| {
            let program =
                g::send::<0, 1, g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv0 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap0, slab0),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register source rendezvous");
            let rv1 = cluster
                .rendezvous(
                    Config::<DefaultLabelUniverse, _>::from_resources(
                        (tap1, slab1),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register destination rendezvous");

            let sid = SessionId::new(95);
            let mut origin_endpoint = rv0
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("source endpoint");
            let target_endpoint = rv1
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("destination endpoint");
            drop(target_endpoint);

            match origin_endpoint
                .flow::<g::ControlMsg<TOPOLOGY_BEGIN_LABEL, g::control::TopologyBegin>>()
            {
                Ok(flow) => {
                    let err = futures::executor::block_on(flow.send(&()))
                        .expect_err("dropped peer binding must not authorize topology begin");
                    assert!(
                        format!("{err:?}").contains("PhaseInvariant")
                            || format!("{err:?}").contains("EndpointDropped"),
                        "dropped peer must fail closed before topology send, got {err:?}"
                    );
                }
                Err(err) => {
                    assert!(
                        format!("{err:?}").contains("PhaseInvariant")
                            || format!("{err:?}").contains("EndpointDropped"),
                        "dropped peer must fail closed before topology flow, got {err:?}"
                    );
                }
            }
            assert_eq!(transport.sent_frames(), 0);
            assert_eq!(transport.received_frames(), 0);
            let tap0 =
                unsafe { core::slice::from_raw_parts(tap0_ptr, runtime_support::RING_EVENTS) };
            let tap1 =
                unsafe { core::slice::from_raw_parts(tap1_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_pair_count(tap0, tap1, TAP_TOPOLOGY_BEGIN), 0);
        });
    });
}
