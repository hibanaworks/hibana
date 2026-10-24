use crate::test_support::transport::TestTransport;
use crate::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[test]
fn incoming_choice_can_arrive_at_either_of_the_actors_views() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for right in [false, true] {
                for db_first in [false, true] {
                    type Left = g::Msg<0, ()>;
                    type Right = g::Msg<1, ()>;
                    type Loan<'r> = g::Msg<2, Endpoint<'r, 3>>;
                    type Tail = g::Msg<3, ()>;
                    let global = g::seq(
                        g::route(g::send::<0, 1, Left>(), g::send::<0, 3, Right>()),
                        g::seq(g::send::<1, 2, Loan<'_>>(), g::send::<0, 3, Tail>()),
                    );
                    let p0 = project::<0>(&global);
                    let p1 = project::<1>(&global);
                    let p2 = project::<2>(&global);
                    let p3 = project::<3>(&global);
                    let id = SessionId::new(281);
                    let network = TestTransport::new();
                    let mut slab0 = [0; 65536];
                    let mut slab1 = [0; 65536];
                    let mut slab2 = [0; 65536];
                    let mut storage0 = SessionKitStorage::uninit();
                    let mut storage1 = SessionKitStorage::uninit();
                    let mut storage2 = SessionKitStorage::uninit();
                    let rv0 = storage0
                        .init()
                        .rendezvous(&mut slab0, network.clone())
                        .unwrap();
                    let rv1 = storage1
                        .init()
                        .rendezvous(&mut slab1, network.clone())
                        .unwrap();
                    let rv2 = storage2
                        .init()
                        .rendezvous(&mut slab2, network.clone())
                        .unwrap();
                    let mut sender = rv0.enter(id, &p0).unwrap();
                    let mut worker = rv1.enter(id, &p1).unwrap();
                    let mut next = rv2.enter(id, &p2).unwrap();
                    let mut db = rv1.enter(id, &p3).unwrap();
                    tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::try_join!(
                    biased;
                    async {
                        if right {
                            sender.send::<Right>(&()).await?;
                        } else {
                            sender.send::<Left>(&()).await?;
                        }
                        sender.send::<Tail>(&()).await
                    },
                    async {
                        let label = if db_first {
                            tokio::select! {
                                biased;
                                branch = db.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                                branch = worker.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                            }
                        } else {
                            tokio::select! {
                                biased;
                                branch = worker.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                                branch = db.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                            }
                        };
                        assert_eq!(label, u8::from(right), "right={right} db_first={db_first}");
                        match label {
                            0 => {
                                let branch = worker.offer().await?;
                                branch.recv::<Left>().await?;
                            }
                            1 => {
                                let branch = db.offer().await?;
                                branch.recv::<Right>().await?;
                            }
                            _ => panic!("unexpected choice"),
                        }
                        worker.send::<Loan<'_>>(db).await
                    },
                    async {
                        let mut db = next.recv::<Loan<'_>>().await?;
                        db.recv::<Tail>().await
                    }
                )
            })
            .await
            .expect("choice did not progress")
            .unwrap_or_else(|error| panic!("right={right}, db_first={db_first}: {error:?}"));
                    assert!(network.queue_is_empty());
                }
            }
        });
}
