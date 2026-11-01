mod common;

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};

#[test]
fn repeated_control_records_identify_the_actor_instead_of_its_lane() {
    type Left = g::Msg<7, ()>;
    type Right = g::Msg<8, ()>;
    let global = g::route(g::send::<7, 7, Left>(), g::send::<7, 7, Right>())
        .resolve::<{ u16::MAX }>()
        .roll();
    let program = project::<7>(&global);
    let compiled_choice = tap::projected(&program)
        .find(|row| row.id() == tap::PROJECTED_CHOICE)
        .unwrap();
    assert_eq!(compiled_choice.arg0(), 7);
    let input = core::cell::Cell::new(DecisionArm::Left);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &program,
        ResolverRef::<{ u16::MAX }>::decision_state(&input, |input| Ok(input.get())),
    )
    .unwrap();
    let mut endpoint = kit.enter(SessionId::new(90), &program).unwrap();
    let mut records = kit.tap();
    let mut cx = Context::from_waker(Waker::noop());
    while records.next().is_some() {}
    for selected in [DecisionArm::Left, DecisionArm::Right, DecisionArm::Left] {
        input.set(selected);
        {
            let preview = pin!(async {
                drop(endpoint.offer().await?);
                Ok::<_, hibana::EndpointError>(())
            });
            assert!(matches!(preview.poll(&mut cx), Poll::Ready(Ok(()))));
        }
        let mut batch: Vec<_> = records.by_ref().collect();
        assert!(!batch.iter().any(|record| record.id() == tap::SCOPE_REENTRY));
        let operation = pin!(async {
            let branch = endpoint.offer().await?;
            match branch.label() {
                7 => {
                    branch.recv::<Left>().await?;
                }
                8 => {
                    branch.recv::<Right>().await?;
                }
                label => panic!("unexpected label {label}"),
            }
            Ok::<_, hibana::EndpointError>(())
        });
        assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
        batch.extend(records.by_ref());
        let audit = batch
            .iter()
            .find(|record| record.id() == tap::RESOLVER_AUDIT)
            .unwrap();
        let selection = batch
            .iter()
            .find(|record| record.id() == tap::ROUTE_ARM_SELECTION)
            .unwrap();
        for record in [audit, selection] {
            assert_eq!(record.causal_key() >> 8, 7);
            assert_eq!(record.arg0(), 90);
            assert_eq!(record.arg1() >> 16, u32::from(compiled_choice.causal_key()));
        }
        assert_eq!(audit.arg1() & 0xffff, u32::from(u16::MAX));
        let arm = u32::from(selected == DecisionArm::Right);
        assert_eq!(u32::from(audit.causal_key() as u8), arm);
        assert_eq!(selection.arg1() & 0xffff, arm);
    }
}

#[test]
fn delegated_control_records_use_the_holder_actor_and_the_static_controller_role() {
    type Loan = g::Msg<1, Endpoint<'static, 9>>;
    type Left = g::Msg<7, ()>;
    type Right = g::Msg<8, ()>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(g::send::<9, 9, Left>(), g::send::<9, 9, Right>())
                .resolve::<17>()
                .roll(),
            g::send::<1, 0, Loan>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<9>(&global);
    let input = core::cell::Cell::new(DecisionArm::Left);
    let mut slab = [0; 16384];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &worker_program,
        ResolverRef::<17>::decision_state(&input, |input| Ok(input.get())),
    )
    .unwrap();
    let session = SessionId::new(91);
    let mut owner = kit.enter(session, &owner_program).unwrap();
    let mut worker = kit.enter(session, &worker_program).unwrap();
    let resource = kit.enter(session, &resource_program).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        let mut records = kit.tap();
        while records.next().is_some() {}
        for (visit, selected) in [DecisionArm::Left, DecisionArm::Right, DecisionArm::Left]
            .into_iter()
            .enumerate()
        {
            input.set(selected);
            let branch = resource.offer().await?;
            match branch.label() {
                7 => {
                    branch.recv::<Left>().await?;
                }
                8 => {
                    branch.recv::<Right>().await?;
                }
                label => panic!("unexpected label {label}"),
            }
            let batch: Vec<_> = records.by_ref().collect();
            for id in [tap::RESOLVER_AUDIT, tap::ROUTE_ARM_SELECTION] {
                let record = batch.iter().find(|record| record.id() == id).unwrap();
                assert_eq!(record.causal_key() >> 8, 1);
                assert_eq!(record.arg0(), session.raw());
                let site = record.arg1() >> 16;
                let choice = tap::projected(&worker_program)
                    .find(|row| {
                        row.id() == tap::PROJECTED_CHOICE && u32::from(row.causal_key()) == site
                    })
                    .unwrap();
                assert_eq!(choice.arg0(), 9);
            }
            let commit = batch
                .iter()
                .find(|record| record.id() == tap::ENDPOINT_RECV)
                .unwrap();
            assert_eq!(commit.causal_key() >> 8, 1);
            assert_eq!(commit.arg1() >> 24, 9);
            let resets: Vec<_> = batch
                .iter()
                .filter(|record| record.id() == tap::SCOPE_REENTRY)
                .collect();
            assert_eq!(resets.is_empty(), visit == 0);
            for reset in resets {
                assert_eq!(reset.causal_key(), (1 << 8) | 9);
                assert_eq!(reset.arg0(), session.raw());
                assert!(tap::projected(&worker_program).any(|row| {
                    row.id() == tap::PROJECTED_SCOPE
                        && u32::from(row.causal_key()) == reset.arg1() >> 16
                }));
            }
        }
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;
        drop(resource);
        Ok::<_, hibana::EndpointError>(())
    });
    assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn committed_reentries_distinguish_inner_rolls_from_their_parent() {
    type Tick = g::Msg<7, ()>;
    type Tail = g::Msg<8, ()>;
    let global = g::seq(g::send::<7, 7, Tick>().roll(), g::send::<7, 7, Tail>()).roll();
    let program = project::<7>(&global);
    let scopes: Vec<_> = tap::projected(&program)
        .filter(|row| row.id() == tap::PROJECTED_SCOPE && row.causal_key() >> 13 == 1)
        .collect();
    let inner = scopes
        .iter()
        .find(|row| row.arg0() == 1 && row.arg1() & 3 == 2)
        .unwrap()
        .causal_key();
    let outer = scopes
        .iter()
        .find(|row| row.arg0() == 2 && row.arg1() & 3 == 2)
        .unwrap()
        .causal_key();
    assert_ne!(inner, outer);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    let mut endpoint = kit.enter(SessionId::new(95), &program).unwrap();
    let mut records = kit.tap();
    let mut captured: Vec<_> = records.by_ref().collect();
    let mut cx = Context::from_waker(Waker::noop());
    {
        let operation = pin!(async {
            endpoint.send::<Tick>(&()).await?;
            captured.extend(records.by_ref());
            endpoint.send::<Tick>(&()).await?;
            captured.extend(records.by_ref());
            endpoint.send::<Tail>(&()).await?;
            captured.extend(records.by_ref());
            endpoint.send::<Tick>(&()).await?;
            captured.extend(records.by_ref());
            endpoint.send::<Tail>(&()).await?;
            captured.extend(records.by_ref());
            Ok::<_, hibana::EndpointError>(())
        });
        assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
    }
    let resets: Vec<_> = captured
        .iter()
        .filter(|record| record.id() == tap::SCOPE_REENTRY)
        .collect();
    assert_eq!(resets.len(), 2);
    for (reset, expected) in resets.into_iter().zip([inner, outer]) {
        assert_eq!(reset.causal_key(), (7 << 8) | 7);
        assert_eq!(reset.arg0(), 95);
        assert_eq!(reset.arg1(), u32::from(expected) << 16);
        let next = captured
            .iter()
            .find(|record| record.ts() > reset.ts() && record.id() == tap::ENDPOINT_SEND)
            .unwrap();
        assert_eq!(next.arg1() & 0xffff, 0);
    }
    let before_rejected = captured.len();
    {
        let rejected = pin!(endpoint.send::<Tail>(&()));
        assert!(matches!(rejected.poll(&mut cx), Poll::Ready(Err(_))));
    }
    captured.extend(records.by_ref());
    assert!(
        !captured[before_rejected..]
            .iter()
            .any(|record| { matches!(record.id(), tap::SCOPE_REENTRY | tap::ENDPOINT_SEND) })
    );
}

#[test]
fn a_rejected_resolver_still_identifies_its_actor_without_an_endpoint_commit() {
    let global = g::route(
        g::send::<7, 7, g::Msg<7, ()>>(),
        g::send::<7, 7, g::Msg<8, ()>>(),
    )
    .resolve::<17>();
    let program = project::<7>(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &program,
        ResolverRef::<17>::decision_state(&(), |_| Err(ResolverError::reject())),
    )
    .unwrap();
    let mut endpoint = kit.enter(SessionId::new(92), &program).unwrap();
    let mut records = kit.tap();
    while records.next().is_some() {}
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(endpoint.offer());
    assert!(matches!(operation.poll(&mut cx), Poll::Ready(Err(_))));
    let batch: Vec<_> = records.collect();
    let audit = batch
        .iter()
        .find(|record| record.id() == tap::RESOLVER_AUDIT)
        .unwrap();
    assert_eq!(audit.causal_key() >> 8, 7);
    assert_eq!(audit.causal_key() as u8, 0xff);
    assert!(!batch.iter().any(|record| matches!(
        record.id(),
        tap::ENDPOINT_SEND | tap::ENDPOINT_RECV | tap::ENDPOINT_SESSION | tap::ROUTE_ARM_SELECTION
    )));
}
