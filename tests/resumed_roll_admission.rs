//! Check complete-message prefixes against independent finite source unfoldings.
mod common;
use hibana::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

type Prefix = g::Msg<231, ()>;
type Work = g::Msg<232, ()>;
type Ack = g::Msg<233, ()>;
type Tail = g::Msg<237, ()>;
type Loan<'a> = g::Msg<232, Endpoint<'a, 3>>;
type Returned<'a> = g::Msg<233, Endpoint<'a, 3>>;
type Read = g::Msg<235, u32>;
type Value = g::Msg<236, u32>;

struct Query {
    delegated: bool,
    prefix: &'static [u8],
    probe: u8,
    permitted: bool,
}

fn queries() -> Vec<Query> {
    let input = include_bytes!("data/resumed_roll_admission.bin");
    assert_eq!(&input[..4], b"HNE\x01");
    let count = u32::from_be_bytes(input[4..8].try_into().unwrap());
    assert!(count > 0, "empty resumed-roll source inventory");
    let mut rows = &input[8..];
    let mut found = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..count {
        let [delegated, length, rest @ ..] = rows else {
            panic!("missing resumed-roll query");
        };
        assert!(*delegated <= 1 && *length <= 7);
        let (prefix, rest) = rest.split_at(usize::from(*length));
        let [probe, permitted, tail @ ..] = rest else {
            panic!("truncated resumed-roll query");
        };
        assert!(prefix.iter().all(|letter| *letter < 3));
        assert!(*probe < 3 && *permitted <= 1);
        assert!(keys.insert((*delegated, prefix, *probe)), "duplicate query");
        found.push(Query {
            delegated: *delegated == 1,
            prefix,
            probe: *probe,
            permitted: *permitted == 1,
        });
        rows = tail;
    }
    assert!(rows.is_empty(), "surplus resumed-roll source queries");
    assert!(found.iter().any(|query| query.delegated));
    assert!(found.iter().any(|query| !query.delegated));
    found
}

#[test]
fn ordinary_resumption_matches_every_bounded_complete_exchange_prefix() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = g::seq(
                g::send::<0, 1, Prefix>(),
                g::seq(
                    g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
                    g::send::<0, 1, Tail>(),
                ),
            )
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let mut attempts = 0;
            for query in queries().into_iter().filter(|query| !query.delegated) {
                let network = common::TestTransport::new();
                let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
                let mut storage0 = SessionKitStorage::uninit();
                let mut storage1 = SessionKitStorage::uninit();
                let rv0 = storage0
                    .init()
                    .rendezvous(&mut slab0, network.clone())
                    .unwrap();
                let rv1 = storage1.init().rendezvous(&mut slab1, network).unwrap();
                let sid = SessionId::new(9310);
                let mut a = rv0.enter(sid, &p0).unwrap();
                let mut b = rv1.enter(sid, &p1).unwrap();
                let execution = async {
                    for letter in query.prefix {
                        match letter {
                            0 => {
                                tokio::try_join!(a.send::<Prefix>(&()), b.recv::<Prefix>())?;
                            }
                            1 => {
                                tokio::try_join!(
                                    async {
                                        a.send::<Work>(&()).await?;
                                        a.recv::<Ack>().await
                                    },
                                    async {
                                        b.recv::<Work>().await?;
                                        b.send::<Ack>(&()).await
                                    },
                                )?;
                            }
                            2 => {
                                tokio::try_join!(a.send::<Tail>(&()), b.recv::<Tail>())?;
                            }
                            _ => unreachable!(),
                        }
                    }
                    let result = match query.probe {
                        0 => a.send::<Prefix>(&()).await,
                        1 => a.send::<Work>(&()).await,
                        2 => a.send::<Tail>(&()).await,
                        _ => unreachable!(),
                    };
                    Ok::<_, hibana::EndpointError>(result.is_ok())
                };
                let actual =
                    tokio::time::timeout(core::time::Duration::from_millis(1), execution).await;
                assert!(
                    matches!(actual, Ok(Ok(accepted)) if accepted == query.permitted),
                    "ordinary prefix={:?} probe={} model={} actual={actual:?}",
                    query.prefix,
                    query.probe,
                    query.permitted
                );
                attempts += 1;
            }
            println!("Ordinary resumed-roll admission: attempts={attempts} runtime-only=0 model-only=0 unreachable=0");
        });
}

#[test]
fn delegated_resumption_matches_every_bounded_complete_exchange_prefix() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = g::seq(
                g::send::<0, 1, Prefix>(),
                g::seq(
                    g::seq(
                        g::send::<0, 1, Loan<'_>>(),
                        g::seq(
                            g::send::<3, 2, Read>(),
                            g::seq(
                                g::send::<2, 3, Value>(),
                                g::send::<1, 0, Returned<'_>>(),
                            ),
                        ),
                    )
                    .roll(),
                    g::send::<0, 1, Tail>(),
                ),
            )
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let p3 = project::<3>(&global);
            let mut attempts = 0;
            for query in queries().into_iter().filter(|query| query.delegated) {
                let network = common::TestTransport::new();
                let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
                let rv2 = storage2.init().rendezvous(&mut slab2, network).unwrap();
                let sid = SessionId::new(9311);
                let mut owner = rv0.enter(sid, &p0).unwrap();
                let mut worker = rv1.enter(sid, &p1).unwrap();
                let mut service = rv2.enter(sid, &p2).unwrap();
                let mut db = rv0.enter(sid, &p3).unwrap();
                let execution = async {
                    for letter in query.prefix {
                        match letter {
                            0 => {
                                tokio::try_join!(
                                    owner.send::<Prefix>(&()),
                                    worker.recv::<Prefix>()
                                )?;
                            }
                            1 => {
                                let (returned, (), ()) = tokio::try_join!(
                                    async {
                                        owner.send::<Loan<'_>>(db).await?;
                                        owner.recv::<Returned<'_>>().await
                                    },
                                    async {
                                        let mut db = worker.recv::<Loan<'_>>().await?;
                                        db.send::<Read>(&41).await?;
                                        assert_eq!(db.recv::<Value>().await?, 42);
                                        worker.send::<Returned<'_>>(db).await
                                    },
                                    async {
                                        let number = service.recv::<Read>().await?;
                                        service.send::<Value>(&(number + 1)).await
                                    },
                                )?;
                                db = returned;
                            }
                            2 => {
                                tokio::try_join!(owner.send::<Tail>(&()), worker.recv::<Tail>())?;
                            }
                            _ => unreachable!(),
                        }
                    }
                    let result = match query.probe {
                        0 => owner.send::<Prefix>(&()).await,
                        1 => owner.send::<Loan<'_>>(db).await,
                        2 => owner.send::<Tail>(&()).await,
                        _ => unreachable!(),
                    };
                    Ok::<_, hibana::EndpointError>(result.is_ok())
                };
                let actual =
                    tokio::time::timeout(core::time::Duration::from_millis(1), execution).await;
                assert!(
                    matches!(actual, Ok(Ok(accepted)) if accepted == query.permitted),
                    "delegated prefix={:?} probe={} model={} actual={actual:?}",
                    query.prefix,
                    query.probe,
                    query.permitted
                );
                attempts += 1;
            }
            println!("Delegated resumed-roll admission: attempts={attempts} runtime-only=0 model-only=0 unreachable=0");
        });
}
