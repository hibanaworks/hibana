//! Preview a received parallel message beside a repeated nested choice.
#[path = "../examples/support/in_memory.rs"]
mod in_memory;
use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[test]
fn an_independent_receive_can_offer_before_a_repeated_child_choice() {
    use hibana::{
        Endpoint,
        runtime::{
            resolver::{DecisionArm, ResolverRef},
            transport::EndpointBinding,
        },
    };
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            type LoanA<'a> = g::Msg<0, Endpoint<'a, 3>>;
            type ReadA = g::Msg<1, ()>;
            type ValueA = g::Msg<2, ()>;
            type ReturnA<'a> = g::Msg<3, Endpoint<'a, 3>>;
            type LoanB<'a> = g::Msg<4, Endpoint<'a, 3>>;
            type ReadB = g::Msg<5, ()>;
            type ValueB = g::Msg<6, ()>;
            type ReturnB<'a> = g::Msg<7, Endpoint<'a, 3>>;
            type Tick = g::Msg<8, ()>;
            type LoanC<'a> = g::Msg<9, Endpoint<'a, 3>>;
            type ReadC = g::Msg<10, ()>;
            type ValueC = g::Msg<11, ()>;
            type ReturnC<'a> = g::Msg<12, Endpoint<'a, 3>>;
            let global = g::route(
                g::par(
                    g::route(
                        g::seq(
                            g::send::<0, 1, LoanA<'_>>(),
                            g::seq(
                                g::send::<3, 2, ReadA>(),
                                g::seq(g::send::<2, 3, ValueA>(), g::send::<1, 0, ReturnA<'_>>()),
                            ),
                        ),
                        g::seq(
                            g::send::<0, 1, LoanB<'_>>(),
                            g::seq(
                                g::send::<3, 2, ReadB>(),
                                g::seq(g::send::<2, 3, ValueB>(), g::send::<1, 0, ReturnB<'_>>()),
                            ),
                        ),
                    )
                    .resolve::<1>(),
                    g::send::<0, 2, Tick>(),
                ),
                g::seq(
                    g::send::<0, 1, LoanC<'_>>(),
                    g::seq(
                        g::send::<3, 2, ReadC>(),
                        g::seq(g::send::<2, 3, ValueC>(), g::send::<1, 0, ReturnC<'_>>()),
                    ),
                ),
            )
            .resolve::<0>()
            .roll()
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let p3 = project::<3>(&global);
            let sid = SessionId::new(9282);
            let left = DecisionArm::Left;
            // SAFETY: this isolated run has one resource and one initial issuer.
            let network =
                in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
            let [mut slab0, mut slab1, mut slab2] = [
                vec![0; 128 * 1024],
                vec![0; 128 * 1024],
                vec![0; 128 * 1024],
            ];
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
            rv0.set_resolver(&p0, ResolverRef::<0>::decision_state(&left, |arm| Ok(*arm)))
                .unwrap();
            rv0.set_resolver(&p0, ResolverRef::<1>::decision_state(&left, |arm| Ok(*arm)))
                .unwrap();
            let mut owner = rv0.enter(sid, &p0).unwrap();
            let mut worker = rv1.enter(sid, &p1).unwrap();
            let mut service = rv2.enter(sid, &p2).unwrap();
            let mut db = rv0.enter(sid, &p3).unwrap();
            for _ in 0..3 {
                owner.send::<LoanA<'_>>(db).await.unwrap();
                let mut loan = worker.recv::<LoanA<'_>>().await.unwrap();
                loan.send::<ReadA>(&()).await.unwrap();
                service.recv::<ReadA>().await.unwrap();
                service.send::<ValueA>(&()).await.unwrap();
                loan.recv::<ValueA>().await.unwrap();
                worker.send::<ReturnA<'_>>(loan).await.unwrap();
                db = owner.recv::<ReturnA<'_>>().await.unwrap();
                owner.send::<Tick>(&()).await.unwrap();
                service.recv::<Tick>().await.unwrap();
            }
            owner.send::<Tick>(&()).await.unwrap();
            let branch = service.offer().await.unwrap();
            assert_eq!(branch.label(), 8);
            drop(branch);
            let branch = service.offer().await.unwrap();
            assert_eq!(branch.label(), 8);
            branch.recv::<Tick>().await.unwrap();
            owner.send::<LoanA<'_>>(db).await.unwrap();
            let mut loan = worker.recv::<LoanA<'_>>().await.unwrap();
            loan.send::<ReadA>(&()).await.unwrap();
            service.recv::<ReadA>().await.unwrap();
            service.send::<ValueA>(&()).await.unwrap();
            loan.recv::<ValueA>().await.unwrap();
            worker.send::<ReturnA<'_>>(loan).await.unwrap();
            let _db = owner.recv::<ReturnA<'_>>().await.unwrap();
        });
}
