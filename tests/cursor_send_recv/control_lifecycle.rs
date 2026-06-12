use super::*;

const TAP_ENDPOINT_CONTROL: u16 = 0x0204;
const TAP_STATE_SNAPSHOT_REQ: u16 = 0x0130;
const TAP_STATE_RESTORE_REQ: u16 = 0x0131;
const TAP_POLICY_COMMIT: u16 = 0x0405;
const TAP_POLICY_TX_ABORT: u16 = 0x0411;

fn tap_count(tap: &[hibana::integration::runtime::TapEvent], id: u16) -> usize {
    tap.iter().filter(|event| event.id == id).count()
}

fn tap_slice(
    ptr: *const hibana::integration::runtime::TapEvent,
) -> &'static [hibana::integration::runtime::TapEvent] {
    unsafe { core::slice::from_raw_parts(ptr, runtime_support::RING_EVENTS) }
}

#[test]
fn public_txn_controlmsg_commit_requires_prior_snapshot() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 0, g::ControlMsg<100, g::control::TxnCommit>>();
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(100))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            let err = futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<100, g::control::TxnCommit>>()
                    .expect("txn commit flow")
                    .send(&()),
            )
            .expect_err("txn commit without a state snapshot must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "txn commit without snapshot must fail as a phase invariant, got {err:?}"
            );
            assert_eq!(
                tap_count(
                    unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) },
                    TAP_ENDPOINT_CONTROL,
                ),
                0,
                "failed txn commit must not publish endpoint-control TAP"
            );
            assert_eq!(
                tap_count(
                    unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) },
                    TAP_POLICY_COMMIT,
                ),
                0,
                "failed txn commit must not publish policy commit TAP"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_abort_requires_prior_snapshot() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 0, g::ControlMsg<107, g::control::TxnAbort>>();
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(107))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            let err = futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<107, g::control::TxnAbort>>()
                    .expect("txn abort flow")
                    .send(&()),
            )
            .expect_err("txn abort without a state snapshot must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "txn abort without snapshot must fail as a phase invariant, got {err:?}"
            );
            let tap = tap_slice(tap_ptr);
            assert_eq!(tap_count(tap, TAP_ENDPOINT_CONTROL), 0);
            assert_eq!(tap_count(tap, TAP_POLICY_TX_ABORT), 0);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_restore_requires_prior_snapshot() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 0, g::ControlMsg<108, g::control::StateRestore>>();
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(108))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            let err = futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<108, g::control::StateRestore>>()
                    .expect("state restore flow")
                    .send(&()),
            )
            .expect_err("state restore without a state snapshot must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "state restore without snapshot must fail as a phase invariant, got {err:?}"
            );
            let tap = tap_slice(tap_ptr);
            assert_eq!(tap_count(tap, TAP_ENDPOINT_CONTROL), 0);
            assert_eq!(tap_count(tap, TAP_STATE_RESTORE_REQ), 0);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_snapshot_then_commit_self_send() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 0, g::ControlMsg<101, g::control::StateSnapshot>>(),
                g::send::<0, 0, g::ControlMsg<102, g::control::TxnCommit>>(),
            );
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(101))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<101, g::control::StateSnapshot>>()
                    .expect("state snapshot flow")
                    .send(&()),
            )
            .expect("state snapshot self-send");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<102, g::control::TxnCommit>>()
                    .expect("txn commit flow")
                    .send(&()),
            )
            .expect("txn commit self-send");
            assert!(
                tap_count(
                    unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) },
                    TAP_ENDPOINT_CONTROL,
                ) >= 2,
                "snapshot and commit self-sends must publish endpoint-control TAP"
            );
            let tap = unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_count(tap, TAP_STATE_SNAPSHOT_REQ), 1);
            assert_eq!(tap_count(tap, TAP_POLICY_COMMIT), 1);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_commit_terminal_rejects_later_abort() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 0, g::ControlMsg<109, g::control::StateSnapshot>>(),
                g::seq(
                    g::send::<0, 0, g::ControlMsg<110, g::control::TxnCommit>>(),
                    g::send::<0, 0, g::ControlMsg<111, g::control::TxnAbort>>(),
                ),
            );
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(109))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<109, g::control::StateSnapshot>>()
                    .expect("state snapshot flow")
                    .send(&()),
            )
            .expect("state snapshot self-send");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<110, g::control::TxnCommit>>()
                    .expect("txn commit flow")
                    .send(&()),
            )
            .expect("txn commit self-send");
            let err = futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<111, g::control::TxnAbort>>()
                    .expect("txn abort flow after commit")
                    .send(&()),
            )
            .expect_err("txn abort after commit must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "txn abort after commit must fail as a phase invariant, got {err:?}"
            );
            let tap = tap_slice(tap_ptr);
            assert_eq!(tap_count(tap, TAP_STATE_SNAPSHOT_REQ), 1);
            assert_eq!(tap_count(tap, TAP_POLICY_COMMIT), 1);
            assert_eq!(tap_count(tap, TAP_POLICY_TX_ABORT), 0);
            assert!(tap_count(tap, TAP_ENDPOINT_CONTROL) >= 2);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_commit_terminal_rejects_later_restore() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 0, g::ControlMsg<112, g::control::StateSnapshot>>(),
                g::seq(
                    g::send::<0, 0, g::ControlMsg<113, g::control::TxnCommit>>(),
                    g::send::<0, 0, g::ControlMsg<114, g::control::StateRestore>>(),
                ),
            );
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(112))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<112, g::control::StateSnapshot>>()
                    .expect("state snapshot flow")
                    .send(&()),
            )
            .expect("state snapshot self-send");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<113, g::control::TxnCommit>>()
                    .expect("txn commit flow")
                    .send(&()),
            )
            .expect("txn commit self-send");
            let err = futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<114, g::control::StateRestore>>()
                    .expect("state restore flow after commit")
                    .send(&()),
            )
            .expect_err("state restore after commit must fail closed");
            assert!(
                format!("{err:?}").contains("PhaseInvariant"),
                "state restore after commit must fail as a phase invariant, got {err:?}"
            );
            let tap = tap_slice(tap_ptr);
            assert_eq!(tap_count(tap, TAP_STATE_SNAPSHOT_REQ), 1);
            assert_eq!(tap_count(tap, TAP_POLICY_COMMIT), 1);
            assert_eq!(tap_count(tap, TAP_STATE_RESTORE_REQ), 0);
            assert!(tap_count(tap, TAP_ENDPOINT_CONTROL) >= 2);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_snapshot_then_abort_self_send() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 0, g::ControlMsg<103, g::control::StateSnapshot>>(),
                g::send::<0, 0, g::ControlMsg<104, g::control::TxnAbort>>(),
            );
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(102))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<103, g::control::StateSnapshot>>()
                    .expect("state snapshot flow")
                    .send(&()),
            )
            .expect("state snapshot self-send");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<104, g::control::TxnAbort>>()
                    .expect("txn abort flow")
                    .send(&()),
            )
            .expect("txn abort self-send");
            assert!(
                tap_count(
                    unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) },
                    TAP_ENDPOINT_CONTROL,
                ) >= 2,
                "snapshot and abort self-sends must publish endpoint-control TAP"
            );
            let tap = unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_count(tap, TAP_STATE_SNAPSHOT_REQ), 1);
            assert_eq!(tap_count(tap, TAP_POLICY_TX_ABORT), 1);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn public_txn_controlmsg_snapshot_then_restore_self_send() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf.as_mut_ptr();
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 0, g::ControlMsg<105, g::control::StateSnapshot>>(),
                g::send::<0, 0, g::ControlMsg<106, g::control::StateRestore>>(),
            );
            let role_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut endpoint = rv
                .session(SessionId::new(103))
                .role(&role_program)
                .enter()
                .expect("endpoint");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<105, g::control::StateSnapshot>>()
                    .expect("state snapshot flow")
                    .send(&()),
            )
            .expect("state snapshot self-send");
            futures::executor::block_on(
                endpoint
                    .flow::<g::ControlMsg<106, g::control::StateRestore>>()
                    .expect("state restore flow")
                    .send(&()),
            )
            .expect("state restore self-send");
            assert!(
                tap_count(
                    unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) },
                    TAP_ENDPOINT_CONTROL,
                ) >= 2,
                "snapshot and restore self-sends must publish endpoint-control TAP"
            );
            let tap = unsafe { core::slice::from_raw_parts(tap_ptr, runtime_support::RING_EVENTS) };
            assert_eq!(tap_count(tap, TAP_STATE_SNAPSHOT_REQ), 1);
            assert_eq!(tap_count(tap, TAP_STATE_RESTORE_REQ), 1);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}
