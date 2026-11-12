//! A shared public contract must follow every nested resolver choice.
mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;
use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn identical_send_contract_follows_both_nested_right_choices()
-> Result<(), hibana::EndpointError> {
    type Shared = Msg<41, u8>;
    let source = g::route(
        g::seq(
            g::send::<0, 1, Shared>(),
            g::seq(g::send::<0, 2, Shared>(), g::send::<0, 3, Shared>()),
        ),
        g::route(
            g::seq(
                g::send::<0, 2, Shared>(),
                g::seq(g::send::<0, 1, Shared>(), g::send::<0, 3, Shared>()),
            ),
            g::seq(
                g::send::<0, 3, Shared>(),
                g::seq(g::send::<0, 1, Shared>(), g::send::<0, 2, Shared>()),
            ),
        )
        .resolve::<941>(),
    )
    .resolve::<940>();
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    for (outer, inner, order) in [
        (DecisionArm::Left, DecisionArm::Left, [1, 2, 3]),
        (DecisionArm::Right, DecisionArm::Left, [2, 1, 3]),
        (DecisionArm::Right, DecisionArm::Right, [3, 1, 2]),
    ] {
        for parked in [false, true] {
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let runtime = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            runtime
                .set_resolver(
                    &p0,
                    ResolverRef::<940>::decision_state(&outer, |arm| Ok(*arm)),
                )
                .unwrap();
            runtime
                .set_resolver(
                    &p0,
                    ResolverRef::<941>::decision_state(&inner, |arm| Ok(*arm)),
                )
                .unwrap();
            let session = SessionId::new(112);
            let mut sender = runtime.enter(session, &p0).unwrap();
            let mut a = runtime.enter(session, &p1).unwrap();
            let mut b = runtime.enter(session, &p2).unwrap();
            let mut c = runtime.enter(session, &p3).unwrap();
            if parked {
                drop(sender.offer().await?);
            }
            for receiver in order {
                sender.send::<Shared>(&receiver).await?;
                let value = match receiver {
                    1 => a.recv::<Shared>().await?,
                    2 => b.recv::<Shared>().await?,
                    _ => c.recv::<Shared>().await?,
                };
                assert_eq!(value, receiver);
            }
        }
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn shared_send_resolves_ancestors_before_an_inactive_child()
-> Result<(), hibana::EndpointError> {
    use core::cell::Cell;
    use hibana::runtime::resolver::ResolverError;
    type Shared = Msg<41, u8>;
    let source = g::route(
        g::route(
            g::seq(
                g::send::<0, 1, Shared>(),
                g::seq(g::send::<0, 2, Shared>(), g::send::<0, 3, Shared>()),
            ),
            g::seq(
                g::send::<0, 2, Shared>(),
                g::seq(g::send::<0, 1, Shared>(), g::send::<0, 3, Shared>()),
            ),
        )
        .resolve::<941>(),
        g::route(
            g::seq(
                g::send::<0, 3, Shared>(),
                g::seq(g::send::<0, 1, Shared>(), g::send::<0, 2, Shared>()),
            ),
            g::seq(
                g::send::<0, 3, Shared>(),
                g::seq(g::send::<0, 2, Shared>(), g::send::<0, 1, Shared>()),
            ),
        )
        .resolve::<942>(),
    )
    .resolve::<940>();
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    for (outer, inner, order) in [
        (DecisionArm::Left, DecisionArm::Left, [1, 2, 3]),
        (DecisionArm::Left, DecisionArm::Right, [2, 1, 3]),
        (DecisionArm::Right, DecisionArm::Left, [3, 1, 2]),
        (DecisionArm::Right, DecisionArm::Right, [3, 2, 1]),
    ] {
        for parked in [false, true] {
            let outer = (outer, Cell::new(0));
            let left = (
                (outer.0 == DecisionArm::Left).then_some(inner),
                Cell::new(0),
            );
            let right = (
                (outer.0 == DecisionArm::Right).then_some(inner),
                Cell::new(0),
            );
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let runtime = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            runtime
                .set_resolver(
                    &p0,
                    ResolverRef::<940>::decision_state(&outer, |(arm, calls)| {
                        if calls.replace(calls.get() + 1) != 0 {
                            return Err(ResolverError::reject());
                        }
                        Ok(*arm)
                    }),
                )
                .unwrap();
            runtime
                .set_resolver(
                    &p0,
                    ResolverRef::<941>::decision_state(&left, |(arm, calls)| {
                        if calls.replace(calls.get() + 1) != 0 {
                            return Err(ResolverError::reject());
                        }
                        arm.ok_or_else(ResolverError::reject)
                    }),
                )
                .unwrap();
            runtime
                .set_resolver(
                    &p0,
                    ResolverRef::<942>::decision_state(&right, |(arm, calls)| {
                        if calls.replace(calls.get() + 1) != 0 {
                            return Err(ResolverError::reject());
                        }
                        arm.ok_or_else(ResolverError::reject)
                    }),
                )
                .unwrap();
            let session = SessionId::new(113);
            let mut sender = runtime.enter(session, &p0).unwrap();
            let mut a = runtime.enter(session, &p1).unwrap();
            let mut b = runtime.enter(session, &p2).unwrap();
            let mut c = runtime.enter(session, &p3).unwrap();
            if parked {
                drop(sender.offer().await?);
            }
            for receiver in order {
                sender.send::<Shared>(&receiver).await?;
                let value = match receiver {
                    1 => a.recv::<Shared>().await?,
                    2 => b.recv::<Shared>().await?,
                    _ => c.recv::<Shared>().await?,
                };
                assert_eq!(value, receiver);
            }
            assert_eq!(outer.1.get(), 1, "the outer decision is consumed once");
            assert_eq!(left.1.get(), usize::from(left.0.is_some()));
            assert_eq!(right.1.get(), usize::from(right.0.is_some()));
        }
    }
    Ok(())
}
