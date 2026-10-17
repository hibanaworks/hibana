//! Readiness suspension must not freeze ownership or source admission facts.
//! Every operation below is a real Endpoint operation on the same actor.
use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};

type Tick = g::Msg<39, ()>;
type Steps = g::Seq<
    g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
    g::Seq<g::Send<11, 1, Loan>, g::Send<0, 2, OtherLoan>>,
>;

#[test]
fn a_pending_body_restart_cannot_publish_after_another_view_hands_its_resource_away() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time().start_paused(true).build().unwrap().block_on(async {
        let network = TestTransport::new();
        let mut slab = [0u8; 64 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let rv = storage.init().rendezvous(&mut slab, network.clone()).unwrap();
        let sid = SessionId::new(9030);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &db_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut ticker = rv.enter(sid, &ticker_program).unwrap();
        let mut reader = rv.enter(sid, &reader_program).unwrap();

        owner.send::<Tick>(&()).await.unwrap();
        db.send::<Read>(&7).await.unwrap();
        ticker.recv::<Tick>().await.unwrap();
        assert_eq!(reader.recv::<Read>().await.unwrap(), 7);

        network.pause_send(0);
        let mut pending = pin!(owner.send::<Tick>(&()));
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        }).await;
        auth.send::<Loan>(db).await.unwrap();
        let transferred = worker.recv::<Loan>().await.unwrap();
        assert!(network.queue_is_empty());
        // SAFETY: auth retains the same actor created with TestTransport.
        let before = unsafe { actor_snapshot(&auth) };
        network.resume_send();
        let outcome = tokio::time::timeout(core::time::Duration::from_secs(10), pending).await;
        // SAFETY: the independently borrowed AUTH view still retains that actor.
        let after = unsafe { actor_snapshot(&auth) };
        assert!(matches!(outcome, Ok(Err(_))),
            "a stale reentry plan sent after DB left: {outcome:?}, before={before:?}, after={after:?}");
        assert!(network.queue_is_empty(), "stale Tick reached the peer queue");
        assert_eq!(before.counter, after.counter);
        assert!(before.steps.iter().map(|step| step.done)
            .eq(after.steps.iter().map(|step| step.done)));
        drop(transferred);
    });
}

#[test]
fn a_pending_send_waits_for_a_fresh_other_view_predecessor_and_then_finishes() {
    type Global = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<g::Send<11, 1, Value>, g::Seq<g::Send<0, 2, Loan>, g::Send<0, 2, OtherLoan>>>,
    >;
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9031);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            network.pause_send(11);
            let mut pending = pin!(auth.send::<Value>(&777));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            owner.send::<Tick>(&()).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            network.resume_send();
            poll_fn(|cx| {
                assert!(
                    pending.as_mut().poll(cx).is_pending(),
                    "fresh DB work must still precede the staged tail"
                );
                Poll::Ready(())
            })
            .await;
            assert!(network.queue_is_empty());
            db.send::<Read>(&9).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 9);
            tokio::time::timeout(core::time::Duration::from_secs(10), pending)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(worker.recv::<Value>().await.unwrap(), 777);
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_pending_owned_send_keeps_its_payload_unavailable_for_resource_dependent_reentry() {
    type Independent = g::Msg<37, u32>;
    type Global = g::Par<Steps, g::Roll<g::Send<0, 8, Independent>>>;
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let independent_program = crate::global::role_program::role_program_from_image::<8>(
        &RoleProjection::<8, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9032);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut independent = rv.enter(sid, &independent_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            network.pause_send(11);
            let mut pending = pin!(auth.send::<Loan>(db));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            // The send future owns DB. Even before transport acceptance, that
            // value cannot perform the next visit's resource action.
            // Independent par work on this same actor must remain enabled.
            for value in [2, 3] {
                owner.send::<Independent>(&value).await.unwrap();
                assert_eq!(independent.recv::<Independent>().await.unwrap(), value);
            }
            let mut restart = pin!(owner.send::<Tick>(&()));
            poll_fn(|cx| {
                assert!(
                    restart.as_mut().poll(cx).is_pending(),
                    "a fresh body started while its resource belonged to an owned send future"
                );
                Poll::Ready(())
            })
            .await;
            assert!(
                network.queue_is_empty(),
                "resource-dependent work escaped before its capability was available"
            );
            network.resume_send();
            tokio::time::timeout(core::time::Duration::from_secs(10), pending)
                .await
                .unwrap()
                .unwrap();
            let transferred = worker.recv::<Loan>().await.unwrap();
            let result = tokio::time::timeout(core::time::Duration::from_secs(10), restart).await;
            assert!(
                matches!(result, Ok(Err(_))),
                "the resource permanently left: {result:?}"
            );
            assert!(network.queue_is_empty());
            drop(transferred);
        });
}
