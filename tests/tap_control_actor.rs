mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn delegated_resolver_audit_does_not_require_an_inactive_arms_port()
-> Result<(), hibana::EndpointError> {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Left = g::Msg<7, ()>;
    type Right = g::Msg<8, ()>;
    type Other = g::Msg<9, ()>;
    let result = g::route(g::send::<3, 2, Left>(), g::send::<3, 2, Right>()).resolve::<77>();
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(
                g::seq(result, g::send::<3, 0, Other>()),
                g::par(g::send::<3, 0, Other>(), result),
            )
            .resolve::<78>(),
            g::send::<1, 0, Loan>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let peer_program = project::<2>(&global);
    let resource_program = project::<3>(&global);
    for label in [7, 8] {
        let lanes = tap::projected(&worker_program)
            .filter(|row| row.id() == tap::PROJECTED_SEND && row.arg0() as u8 == label)
            .map(|row| (row.arg0() >> 8) as u8)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            lanes.len(),
            2,
            "both source occurrences need distinct ports: {lanes:?}"
        );
    }
    for selected in [DecisionArm::Left, DecisionArm::Right] {
        let mut slab = [0; 32 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let kit = storage
            .init()
            .rendezvous(&mut slab, common::TestTransport::new())
            .unwrap();
        kit.set_resolver(
            &worker_program,
            ResolverRef::<77>::decision_state(&selected, |arm| Ok(*arm)),
        )
        .unwrap();
        kit.set_resolver(
            &worker_program,
            ResolverRef::<78>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
        let session = SessionId::new(181);
        let mut owner = kit.enter(session, &owner_program).unwrap();
        let mut worker = kit.enter(session, &worker_program).unwrap();
        let mut peer = kit.enter(session, &peer_program).unwrap();
        let resource = kit.enter(session, &resource_program).unwrap();
        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        let mut records = kit.tap();
        while records.next().is_some() {}
        resource.send::<Other>(&()).await?;
        owner.recv::<Other>().await?;
        let parent_audits: Vec<_> = records
            .by_ref()
            .filter(|record| record.id() == tap::RESOLVER_AUDIT)
            .collect();
        assert_eq!(parent_audits.len(), 1);
        assert_eq!(parent_audits[0].arg1() & 0xffff, 78);
        let [other, mut result] = resource.par()?;
        match selected {
            DecisionArm::Left => {
                result.send::<Left>(&()).await?;
                peer.recv::<Left>().await?;
            }
            DecisionArm::Right => {
                result.send::<Right>(&()).await?;
                peer.recv::<Right>().await?;
            }
        }
        drop((other, result));
        let audits: Vec<_> = records
            .by_ref()
            .filter(|record| record.id() == tap::RESOLVER_AUDIT)
            .collect();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].arg1() & 0xffff, 77);
        for audit in parent_audits.into_iter().chain(audits) {
            assert_eq!(audit.causal_key() >> 8, 1);
            assert_eq!(audit.arg0(), session.raw());
            assert!(
                tap::projected(&worker_program).any(|row| row.id() == tap::PROJECTED_CHOICE
                    && u32::from(row.causal_key()) == audit.arg1() >> 16
                    && row.arg0() == 3)
            );
        }
        worker.send::<Loan>(resource).await?;
        drop(owner.recv::<Loan>().await?);
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn delegated_nested_resolver_audits_keep_the_actor_during_parallel_sends()
-> Result<(), hibana::EndpointError> {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 4>>;
    type Left = g::Msg<7, ()>;
    type Right = g::Msg<8, ()>;
    type Other = g::Msg<9, ()>;
    let result = g::route(g::send::<4, 1, Left>(), g::send::<4, 1, Right>()).resolve::<77>();
    let global = g::seq(
        g::send::<0, 2, Loan>(),
        g::seq(
            g::route(
                g::seq(result, g::send::<2, 0, Other>()),
                g::par(result, g::send::<2, 0, Other>()),
            )
            .resolve::<78>(),
            g::send::<2, 0, Loan>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let peer_program = project::<1>(&global);
    let worker_program = project::<2>(&global);
    let resource_program = project::<4>(&global);
    for selected in [DecisionArm::Left, DecisionArm::Right] {
        let mut slab = [0; 32 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let kit = storage
            .init()
            .rendezvous(&mut slab, common::TestTransport::new())
            .unwrap();
        kit.set_resolver(
            &worker_program,
            ResolverRef::<77>::decision_state(&selected, |arm| Ok(*arm)),
        )
        .unwrap();
        kit.set_resolver(
            &worker_program,
            ResolverRef::<78>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
        let session = SessionId::new(180);
        let mut owner = kit.enter(session, &owner_program).unwrap();
        let mut peer = kit.enter(session, &peer_program).unwrap();
        let mut worker = kit.enter(session, &worker_program).unwrap();
        let resource = kit.enter(session, &resource_program).unwrap();
        owner.send::<Loan>(resource).await.unwrap();
        let mut resource = worker.recv::<Loan>().await.unwrap();
        let mut records = kit.tap();
        while records.next().is_some() {}
        async {
            async {
                match selected {
                    DecisionArm::Left => {
                        resource.send::<Left>(&()).await?;
                        peer.recv::<Left>().await?;
                    }
                    DecisionArm::Right => {
                        resource.send::<Right>(&()).await?;
                        peer.recv::<Right>().await?;
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            };
            async {
                worker.send::<Other>(&()).await?;
                owner.recv::<Other>().await?;
                Ok::<_, hibana::EndpointError>(())
            };
            Ok::<_, hibana::EndpointError>(())
        }
        .await
        .unwrap();
        let audits: Vec<_> = records
            .by_ref()
            .filter(|record| record.id() == tap::RESOLVER_AUDIT)
            .collect();
        assert_eq!(audits.len(), 2);
        let ids = audits
            .iter()
            .map(|record| record.arg1() & 0xffff)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids, std::collections::BTreeSet::from([77, 78]));
        for audit in audits {
            assert_eq!(audit.causal_key() >> 8, 2);
            assert_eq!(audit.arg0(), session.raw());
            assert!(
                tap::projected(&worker_program).any(|row| row.id() == tap::PROJECTED_CHOICE
                    && u32::from(row.causal_key()) == audit.arg1() >> 16
                    && row.arg0() == 4)
            );
        }
        worker.send::<Loan>(resource).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    }
    Ok(())
}

#[test]
fn branch_observation_uses_the_actor_tap_not_another_logical_views_port() {
    use hibana::runtime::program::Projectable;
    type Intent = g::Msg<70, ()>;
    type Assigned<const LABEL: u8> = g::Msg<LABEL, ()>;
    type Loan<'r> = g::Msg<1, Endpoint<'r, 4>>;
    type Return<'r> = g::Msg<2, Endpoint<'r, 4>>;
    type Request = g::Msg<20, ()>;
    type Reply = g::Msg<21, ()>;
    type Released = g::Msg<80, ()>;
    fn lease<const WORKER: u8, const LABEL: u8>() -> impl Projectable {
        g::seq(
            g::send::<3, 0, Assigned<LABEL>>(),
            g::seq(
                g::seq(
                    g::send::<3, 1, Assigned<LABEL>>(),
                    g::send::<3, 2, Assigned<LABEL>>(),
                ),
                g::seq(
                    g::send::<3, WORKER, Loan>(),
                    g::seq(
                        g::send::<4, 6, Request>(),
                        g::seq(
                            g::send::<6, 4, Reply>(),
                            g::seq(g::send::<WORKER, 3, Return>(), g::send::<3, 2, Released>()),
                        ),
                    ),
                ),
            ),
        )
    }
    let leases = g::par(
        g::par(
            g::send::<0, 3, Intent>().roll(),
            g::send::<2, 3, Intent>().roll(),
        ),
        g::route(
            lease::<0, 10>(),
            g::route(lease::<1, 11>(), lease::<2, 12>()),
        )
        .roll(),
    );
    type OtherLoan<'r> = g::Msg<3, Endpoint<'r, 5>>;
    type OtherReturn<'r> = g::Msg<4, Endpoint<'r, 5>>;
    type OtherRequest = g::Msg<22, ()>;
    let global = g::par(
        leases,
        g::seq(
            g::send::<3, 2, OtherLoan>(),
            g::seq(
                g::send::<5, 6, OtherRequest>(),
                g::send::<2, 3, OtherReturn>(),
            ),
        ),
    );
    let p0 = project::<0>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let p4 = project::<4>(&global);
    let p5 = project::<5>(&global);
    let p6 = project::<6>(&global);
    let captured = core::cell::Cell::new(Vec::new());
    let observer = |event| {
        let mut events = captured.take();
        events.push(event);
        captured.set(events);
    };
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.observe(&observer);
    let sid = SessionId::new(115);
    let mut a = kit.enter(sid, &p0).unwrap();
    let mut c = kit.enter(sid, &p2).unwrap();
    let mut owner = kit.enter(sid, &p3).unwrap();
    let mut tool = kit.enter(sid, &p4).unwrap();
    let other_tool = kit.enter(sid, &p5).unwrap();
    let mut service = kit.enter(sid, &p6).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        a.send::<Intent>(&()).await?;
        owner.recv::<Intent>().await?;
        owner.send::<Assigned<10>>(&()).await?;
        a.recv::<Assigned<10>>().await?;
        owner.send::<Assigned<10>>(&()).await?;
        owner.send::<Assigned<10>>(&()).await?;
        c.recv::<Assigned<10>>().await?;
        owner.send::<Loan>(tool).await?;
        tool = a.recv::<Loan>().await?;
        tool.send::<Request>(&()).await?;
        service.recv::<Request>().await?;
        service.send::<Reply>(&()).await?;
        tool.recv::<Reply>().await?;
        a.send::<Return>(tool).await?;
        tool = owner.recv::<Return>().await?;
        owner.send::<Released>(&()).await?;
        c.recv::<Released>().await?;
        let [mut input, mut work, mut other] = c.par()?;
        input.send::<Intent>(&()).await?;
        owner.recv::<Intent>().await?;
        owner.send::<Assigned<12>>(&()).await?;
        a.recv::<Assigned<12>>().await?;
        owner.send::<Assigned<12>>(&()).await?;
        owner.send::<Assigned<12>>(&()).await?;
        let preview = work.offer().await?;
        assert_eq!(preview.label(), 12);
        preview.recv::<Assigned<12>>().await?;
        // The subsequent ownership transfer must use the same accepted branch.
        owner.send::<Loan>(tool).await?;
        tool = work.recv::<Loan>().await?;
        tool.send::<Request>(&()).await?;
        service.recv::<Request>().await?;
        service.send::<Reply>(&()).await?;
        tool.recv::<Reply>().await?;
        work.send::<Return>(tool).await?;
        drop(owner.recv::<Return>().await?);
        owner.send::<Released>(&()).await?;
        work.recv::<Released>().await?;
        owner.send::<OtherLoan>(other_tool).await?;
        let mut other_tool = other.recv::<OtherLoan>().await?;
        other_tool.send::<OtherRequest>(&()).await?;
        service.recv::<OtherRequest>().await?;
        other.send::<OtherReturn>(other_tool).await?;
        drop(owner.recv::<OtherReturn>().await?);
        Ok::<_, hibana::EndpointError>(())
    });
    assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
    let records = captured.take();
    assert!(
        records
            .iter()
            .any(|record| record.id() == tap::ROUTE_ARM_SELECTION && record.causal_key() >> 8 == 2)
    );
}

#[test]
fn passive_replies_do_not_claim_a_local_resolver_decision() {
    type Left = g::Msg<21, ()>;
    type Right = g::Msg<22, ()>;
    type Reply = g::Msg<23, ()>;
    let global = g::route(
        g::seq(g::send::<0, 1, Left>(), g::send::<1, 0, Reply>()),
        g::seq(g::send::<0, 1, Right>(), g::send::<1, 0, Reply>()),
    )
    .resolve::<77>()
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let input = core::cell::Cell::new(DecisionArm::Left);
    let captured = core::cell::Cell::new(Vec::new());
    let observer = |event| {
        let mut rows = captured.take();
        rows.push(event);
        captured.set(rows);
    };
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    rv.observe(&observer);
    rv.set_resolver(
        &p0,
        ResolverRef::<77>::decision_state(&input, |input| Ok(input.get())),
    )
    .unwrap();
    let mut a = rv.enter(SessionId::new(91), &p0).unwrap();
    let mut b = rv.enter(SessionId::new(91), &p1).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        for arm in [DecisionArm::Left, DecisionArm::Right] {
            input.set(arm);
            let branch = a.offer().await?;
            match arm {
                DecisionArm::Left => {
                    branch.send::<Left>(&()).await?;
                    b.recv::<Left>().await?;
                }
                DecisionArm::Right => {
                    branch.send::<Right>(&()).await?;
                    b.recv::<Right>().await?;
                }
            }
            b.send::<Reply>(&()).await?;
            a.recv::<Reply>().await?;
        }
        Ok::<_, hibana::EndpointError>(())
    });
    assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
    let rows = captured.take();
    let selections: Vec<_> = rows
        .iter()
        .filter(|row| row.id() == tap::ROUTE_ARM_SELECTION && row.causal_key() & 0xff == 2)
        .collect();
    assert_eq!(
        selections.len(),
        2,
        "only the two fresh controller decisions are resolver selections"
    );
    assert!(
        selections.iter().all(|row| row.causal_key() >> 8 == 0),
        "a passive reply cannot claim a resolver decision"
    );
}

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
