use crate::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

type Early = g::Msg<209, ()>;
type Receipt = g::Msg<189, ()>;
type Stored = g::Msg<190, ()>;
type EarlyStored = g::Msg<210, ()>;
type Skip = g::Msg<211, ()>;
type Skipped = g::Msg<212, ()>;
type Open = g::Msg<168, ()>;
type Ended = g::Msg<171, ()>;
type Done = g::Msg<4, ()>;
type Closed = g::Msg<191, ()>;
type ClosedAck = g::Msg<192, ()>;

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_later_rolled_receipt_can_choose_its_closing_branch() {
    let prefix = g::route(
        g::seq(
            g::send::<0, 1, Early>(),
            g::seq(
                g::send::<1, 2, Receipt>(),
                g::seq(g::send::<2, 1, Stored>(), g::send::<1, 0, EarlyStored>()),
            ),
        ),
        g::seq(g::send::<0, 1, Skip>(), g::send::<1, 2, Skipped>()),
    )
    .roll();
    let body = g::route(
        g::seq(
            g::send::<0, 1, Open>(),
            g::seq(
                g::send::<1, 2, Receipt>(),
                g::seq(g::send::<2, 1, Stored>(), g::send::<1, 0, Ended>()),
            ),
        ),
        g::seq(
            g::send::<0, 1, Done>(),
            g::seq(g::send::<1, 2, Closed>(), g::send::<2, 1, ClosedAck>()),
        ),
    )
    .roll();
    let global = g::seq(prefix, body);
    let source_program = project::<0>(&global);
    let ingress_program = project::<1>(&global);
    let collector_program = project::<2>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, network).unwrap();
    let sid = SessionId::new(9099);
    let mut source = rv.enter(sid, &source_program).unwrap();
    let mut ingress = rv.enter(sid, &ingress_program).unwrap();
    let mut collector = rv.enter(sid, &collector_program).unwrap();
    tokio::time::timeout(core::time::Duration::from_secs(2), async {
        tokio::try_join!(
            async {
                source.send::<Skip>(&()).await?;
                source.send::<Open>(&()).await?;
                source.recv::<Ended>().await?;
                source.send::<Done>(&()).await
            },
            async {
                ingress.recv::<Skip>().await?;
                ingress.send::<Skipped>(&()).await?;
                ingress.recv::<Open>().await?;
                ingress.send::<Receipt>(&()).await?;
                ingress.recv::<Stored>().await?;
                ingress.send::<Ended>(&()).await?;
                ingress.recv::<Done>().await?;
                ingress.send::<Closed>(&()).await?;
                ingress.recv::<ClosedAck>().await
            },
            async {
                collector.recv::<Skipped>().await?;
                collector.offer().await?.recv::<Receipt>().await?;
                collector.send::<Stored>(&()).await?;
                collector.offer().await?.recv::<Closed>().await?;
                collector.send::<ClosedAck>(&()).await
            },
        )
    })
    .await
    .expect("the collector did not observe the closing branch")
    .unwrap();
}
