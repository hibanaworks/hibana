use super::*;
use crate::Endpoint;

type Loan = g::Msg<240, Endpoint<'static, 36>>;
type Install = g::Msg<241, ()>;
type Installed = g::Msg<249, ()>;
type Need = g::Msg<195, ()>;
type Finished = g::Msg<196, ()>;
type Flight = g::Msg<33, u32>;
type Taken = g::Msg<36, ()>;

#[test]
fn a_completed_rolled_prefix_allows_its_parallel_continuation() {
    type Prefix = g::Msg<21, ()>;
    type Ack = g::Msg<22, ()>;
    type Reply = g::Msg<23, u32>;
    let global = g::seq(
        g::seq(g::send::<0, 1, Prefix>(), g::send::<1, 0, Ack>()).roll(),
        g::par(g::send::<0, 1, Flight>(), g::send::<1, 0, Reply>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1668);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            assert!(a.par::<2>().is_err());
            a.send::<Prefix>(&()).await.unwrap();
            b.recv::<Prefix>().await.unwrap();
            assert!(a.par::<2>().is_err());
            b.send::<Ack>(&()).await.unwrap();
            a.recv::<Ack>().await.unwrap();
            let [mut request, mut reply] = a.par().unwrap();
            let [mut receive, mut respond] = b.par().unwrap();
            let (sent, value, received, answered) = tokio::join!(
                request.send::<Flight>(&7),
                reply.recv::<Reply>(),
                receive.recv::<Flight>(),
                respond.send::<Reply>(&8),
            );
            sent.unwrap();
            answered.unwrap();
            assert_eq!(value.unwrap(), 8);
            assert_eq!(received.unwrap(), 7);
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_completed_parallel_roll_stays_in_its_siblings_borrow() {
    type Prefix = g::Msg<21, ()>;
    type Ack = g::Msg<22, ()>;
    type Reply = g::Msg<23, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, Prefix>(), g::send::<1, 0, Ack>()).roll(),
        g::par(g::send::<0, 1, Flight>(), g::send::<1, 0, Reply>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1667);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            a.send::<Prefix>(&()).await.unwrap();
            b.recv::<Prefix>().await.unwrap();
            b.send::<Ack>(&()).await.unwrap();
            a.recv::<Ack>().await.unwrap();
            assert!(a.par::<2>().is_err());
            let [mut prefix, mut request, mut reply] = a.par().unwrap();
            let [mut receive_prefix, mut receive, mut respond] = b.par().unwrap();
            let ((), (), sent, value, received, answered) = tokio::join!(
                async {
                    prefix.send::<Prefix>(&()).await.unwrap();
                    prefix.recv::<Ack>().await.unwrap();
                },
                async {
                    receive_prefix.recv::<Prefix>().await.unwrap();
                    receive_prefix.send::<Ack>(&()).await.unwrap();
                },
                request.send::<Flight>(&7),
                reply.recv::<Reply>(),
                receive.recv::<Flight>(),
                respond.send::<Reply>(&8),
            );
            sent.unwrap();
            answered.unwrap();
            assert_eq!(value.unwrap(), 8);
            assert_eq!(received.unwrap(), 7);
        });
    assert!(network.queue_is_empty());
}

fn installation() -> impl crate::runtime::program::Projectable {
    g::seq(
        g::send::<1, 2, Loan>(),
        g::seq(
            g::send::<36, 1, Install>(),
            g::seq(g::send::<1, 36, Installed>(), g::send::<2, 3, Loan>()),
        ),
    )
}

#[test]
fn returning_a_resource_can_immediately_borrow_the_next_parallel_phase() {
    let global = g::seq(
        installation(),
        g::seq(
            g::send::<3, 1, Loan>(),
            g::seq(
                g::par(
                    g::seq(
                        g::send::<1, 0, Need>(),
                        g::seq(g::send::<0, 1, Finished>(), installation()),
                    ),
                    g::seq(g::send::<3, 2, Flight>(), g::send::<2, 3, Taken>()),
                ),
                g::send::<3, 1, Loan>(),
            ),
        ),
    );
    let input_program = project::<0>(&global);
    let owner_program = project::<1>(&global);
    let installer_program = project::<2>(&global);
    let output_program = project::<3>(&global);
    let resource_program = project::<36>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1669);
    let mut input = rv.enter(sid, &input_program).unwrap();
    let mut owner = rv.enter(sid, &owner_program).unwrap();
    let mut installer = rv.enter(sid, &installer_program).unwrap();
    let mut output = rv.enter(sid, &output_program).unwrap();
    let resource = rv.enter(sid, &resource_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let start = tokio::time::Instant::now();
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        input.recv::<Need>().await.unwrap();
                        tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                        input.send::<Finished>(&()).await.unwrap();
                    },
                    async {
                        owner.send::<Loan>(resource).await.unwrap();
                        owner.recv::<Install>().await.unwrap();
                        owner.send::<Installed>(&()).await.unwrap();
                        let resource = owner.recv::<Loan>().await.unwrap();
                        owner.send::<Need>(&()).await.unwrap();
                        owner.recv::<Finished>().await.unwrap();
                        owner.send::<Loan>(resource).await.unwrap();
                        owner.recv::<Install>().await.unwrap();
                        owner.send::<Installed>(&()).await.unwrap();
                        drop(owner.recv::<Loan>().await.unwrap());
                    },
                    async {
                        let mut resource = installer.recv::<Loan>().await.unwrap();
                        resource.send::<Install>(&()).await.unwrap();
                        resource.recv::<Installed>().await.unwrap();
                        installer.send::<Loan>(resource).await.unwrap();
                        let [mut installation, mut source] = installer.par().unwrap();
                        tokio::join!(
                            async {
                                let mut resource = installation.recv::<Loan>().await.unwrap();
                                resource.send::<Install>(&()).await.unwrap();
                                resource.recv::<Installed>().await.unwrap();
                                installation.send::<Loan>(resource).await.unwrap();
                            },
                            async {
                                assert_eq!(source.recv::<Flight>().await.unwrap(), 7);
                                source.send::<Taken>(&()).await.unwrap();
                            },
                        );
                    },
                    async {
                        let resource = output.recv::<Loan>().await.unwrap();
                        output.send::<Loan>(resource).await.unwrap();
                        let resource = {
                            let [mut installation, mut source] = output.par().unwrap();
                            let (resource, ()) = tokio::join!(
                                async { installation.recv::<Loan>().await.unwrap() },
                                async {
                                    source.send::<Flight>(&7).await.unwrap();
                                    source.recv::<Taken>().await.unwrap();
                                    assert_eq!(tokio::time::Instant::now(), start);
                                },
                            );
                            resource
                        };
                        output.send::<Loan>(resource).await.unwrap();
                    },
                );
            })
            .await
            .expect("the flight must progress while the next installation is pending");
        });
    assert!(network.queue_is_empty());
}
