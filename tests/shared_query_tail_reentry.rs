mod common;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let queries = g::seq(
        g::route(
            g::send::<0, 1, Msg<112, ()>>(),
            g::send::<0, 1, Msg<136, ()>>(),
        ),
        g::route(
            g::send::<1, 0, Msg<113, u32>>(),
            g::send::<1, 0, Msg<114, ()>>(),
        ),
    )
    .roll();
    let releases = g::seq(
        g::route(
            g::send::<0, 1, Msg<117, u32>>(),
            g::send::<0, 1, Msg<137, u32>>(),
        ),
        g::route(
            g::send::<1, 0, Msg<118, u32>>(),
            g::send::<1, 0, Msg<119, u32>>(),
        ),
    )
    .roll();
    project(&g::par(
        queries,
        g::par(
            releases,
            g::seq(
                g::send::<1, 2, Msg<120, ()>>(),
                g::send::<2, 1, Msg<121, ()>>(),
            ),
        ),
    ))
}

#[test]
fn switching_query_source_after_parallel_release_keeps_the_shared_reply_live() {
    let p0 = program::<0>();
    let p1 = program::<1>();
    let mut bytes = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut bytes, common::TestTransport::new())
        .unwrap();
    let mut app = kit.enter(SessionId::new(1), &p0).unwrap();
    let mut owner = kit.enter(SessionId::new(1), &p1).unwrap();
    futures::executor::block_on(async {
        app.send::<Msg<136, ()>>(&()).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<136, ()>>()
            .await
            .unwrap();
        owner.send::<Msg<114, ()>>(&()).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<114, ()>>()
            .await
            .unwrap();
        for _ in 0..2 {
            app.send::<Msg<112, ()>>(&()).await.unwrap();
            owner
                .offer()
                .await
                .unwrap()
                .recv::<Msg<112, ()>>()
                .await
                .unwrap();
            owner.send::<Msg<113, u32>>(&1).await.unwrap();
            assert_eq!(
                app.offer()
                    .await
                    .unwrap()
                    .recv::<Msg<113, u32>>()
                    .await
                    .unwrap(),
                1
            );
        }
        app.send::<Msg<137, u32>>(&1).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<137, u32>>()
            .await
            .unwrap();
        owner.send::<Msg<119, u32>>(&1).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<119, u32>>()
            .await
            .unwrap();
        app.send::<Msg<117, u32>>(&1).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<117, u32>>()
            .await
            .unwrap();
        owner.send::<Msg<118, u32>>(&1).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<118, u32>>()
            .await
            .unwrap();
        app.send::<Msg<136, ()>>(&()).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<136, ()>>()
            .await
            .unwrap();
        owner.send::<Msg<113, u32>>(&9).await.unwrap();
        assert_eq!(
            app.offer()
                .await
                .unwrap()
                .recv::<Msg<113, u32>>()
                .await
                .unwrap(),
            9
        );
    });
}

#[test]
fn every_four_visit_query_and_reply_history_preserves_parallel_release_and_stop() {
    let p0 = program::<0>();
    let p1 = program::<1>();
    let p2 = program::<2>();
    for history in 0..256u32 {
        let mut bytes = [0; 32 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let kit = storage
            .init()
            .rendezvous(&mut bytes, common::TestTransport::new())
            .unwrap();
        let mut app = kit.enter(SessionId::new(1), &p0).unwrap();
        let mut owner = kit.enter(SessionId::new(1), &p1).unwrap();
        let mut control = kit.enter(SessionId::new(1), &p2).unwrap();
        futures::executor::block_on(async {
            for visit in 0..4 {
                if (history >> (visit * 2)) & 1 == 0 {
                    app.send::<Msg<112, ()>>(&()).await.unwrap();
                    let branch = owner.offer().await.unwrap();
                    drop(branch);
                    owner
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<112, ()>>()
                        .await
                        .unwrap();
                } else {
                    app.send::<Msg<136, ()>>(&()).await.unwrap();
                    let branch = owner.offer().await.unwrap();
                    drop(branch);
                    owner
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<136, ()>>()
                        .await
                        .unwrap();
                }
                // An independent rolled lane may progress while the query's
                // reply is pending, including switches in both directions.
                if visit % 2 == 0 {
                    app.send::<Msg<137, u32>>(&history).await.unwrap();
                    assert_eq!(
                        owner
                            .offer()
                            .await
                            .unwrap()
                            .recv::<Msg<137, u32>>()
                            .await
                            .unwrap(),
                        history
                    );
                    owner.send::<Msg<119, u32>>(&history).await.unwrap();
                    assert_eq!(
                        app.offer()
                            .await
                            .unwrap()
                            .recv::<Msg<119, u32>>()
                            .await
                            .unwrap(),
                        history
                    );
                } else {
                    app.send::<Msg<117, u32>>(&history).await.unwrap();
                    assert_eq!(
                        owner
                            .offer()
                            .await
                            .unwrap()
                            .recv::<Msg<117, u32>>()
                            .await
                            .unwrap(),
                        history
                    );
                    owner.send::<Msg<118, u32>>(&history).await.unwrap();
                    assert_eq!(
                        app.offer()
                            .await
                            .unwrap()
                            .recv::<Msg<118, u32>>()
                            .await
                            .unwrap(),
                        history
                    );
                }
                if (history >> (visit * 2 + 1)) & 1 == 0 {
                    owner.send::<Msg<113, u32>>(&history).await.unwrap();
                    assert_eq!(
                        app.offer()
                            .await
                            .unwrap()
                            .recv::<Msg<113, u32>>()
                            .await
                            .unwrap(),
                        history
                    );
                } else {
                    owner.send::<Msg<114, ()>>(&()).await.unwrap();
                    app.offer()
                        .await
                        .unwrap()
                        .recv::<Msg<114, ()>>()
                        .await
                        .unwrap();
                }
            }
            owner.send::<Msg<120, ()>>(&()).await.unwrap();
            control.recv::<Msg<120, ()>>().await.unwrap();
            control.send::<Msg<121, ()>>(&()).await.unwrap();
            owner.recv::<Msg<121, ()>>().await.unwrap();
        });
    }
}

#[test]
fn a_completed_reply_cannot_be_reused_without_receiving_a_fresh_query() {
    let p0 = program::<0>();
    let p1 = program::<1>();
    for source in 0..2 {
        for reply in 0..2 {
            let mut bytes = [0; 32 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let kit = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            let mut app = kit.enter(SessionId::new(1), &p0).unwrap();
            let mut owner = kit.enter(SessionId::new(1), &p1).unwrap();
            futures::executor::block_on(async {
                if source == 0 {
                    app.send::<Msg<112, ()>>(&()).await.unwrap();
                    owner
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<112, ()>>()
                        .await
                        .unwrap();
                } else {
                    app.send::<Msg<136, ()>>(&()).await.unwrap();
                    owner
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<136, ()>>()
                        .await
                        .unwrap();
                }
                owner.send::<Msg<113, u32>>(&1).await.unwrap();
                app.offer()
                    .await
                    .unwrap()
                    .recv::<Msg<113, u32>>()
                    .await
                    .unwrap();
                let error = if reply == 0 {
                    owner.send::<Msg<113, u32>>(&2).await.unwrap_err()
                } else {
                    owner.send::<Msg<114, ()>>(&()).await.unwrap_err()
                };
                assert!(format!("{error:?}").contains("PhaseInvariant"));
                let poisoned = owner.send::<Msg<120, ()>>(&()).await.unwrap_err();
                assert!(format!("{poisoned:?}").contains("SessionFault"));
            });
        }
    }
}
