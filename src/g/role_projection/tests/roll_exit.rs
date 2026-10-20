use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn a_remote_controller_cannot_restart_after_an_unobserved_resource_handoff() {
    type Tick = g::Msg<39, ()>;
    type Ready = g::Msg<38, ()>;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<1, 2, Ready>,
            g::Seq<g::Roll<g::Seq<g::Send<2, 1, Tick>, g::Send<10, 4, Read>>>, g::Send<1, 5, Loan>>,
        >,
    >;
    // Ready proves initial receive causality, but it is outside the roll and
    // cannot authorize every later visit after holder 1 gives up DB.
    let diagnostic = g::ProgramProjection::<Steps, 8>::DIAGNOSTIC.unwrap();
    assert_eq!(
        diagnostic.problem,
        g::ProjectionProblem::ReentryResourceKnowledge
    );
}

#[test]
fn each_visit_gets_the_entry_owners_permission_while_another_roll_stays_independent() {
    type Tick = g::Msg<39, ()>;
    type Permit = g::Msg<38, ()>;
    type Independent = g::Msg<37, u32>;
    type Work =
        g::Roll<g::Seq<g::Send<1, 2, Permit>, g::Seq<g::Send<2, 1, Tick>, g::Send<10, 4, Read>>>>;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<g::Par<Work, g::Roll<g::Send<6, 7, Independent>>>, g::Send<1, 5, Loan>>,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let initial_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let holder_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let controller_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    let recipient_program = crate::runtime::program::project::<5>(&g::Program::<Steps>::new());
    let unrelated_sender_program =
        crate::runtime::program::project::<6>(&g::Program::<Steps>::new());
    let unrelated_receiver_program =
        crate::runtime::program::project::<7>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 80 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9029);
            let mut initial = rv.enter(sid, &initial_program).unwrap();
            let db = rv.enter(sid, &db_program).unwrap();
            let mut holder = rv.enter(sid, &holder_program).unwrap();
            let mut controller = rv.enter(sid, &controller_program).unwrap();
            let mut service = rv.enter(sid, &service_program).unwrap();
            let mut recipient = rv.enter(sid, &recipient_program).unwrap();
            let mut unrelated_sender = rv.enter(sid, &unrelated_sender_program).unwrap();
            let mut unrelated_receiver = rv.enter(sid, &unrelated_receiver_program).unwrap();
            initial.send::<Loan>(db).await.unwrap();
            let mut db = holder.recv::<Loan>().await.unwrap();

            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        for number in [7, 9] {
                            holder.send::<Permit>(&()).await.unwrap();
                            holder.recv::<Tick>().await.unwrap();
                            db.send::<Read>(&number).await.unwrap();
                        }
                        holder.send::<Loan>(db).await.unwrap();
                    },
                    async {
                        for _ in 0..2 {
                            controller.recv::<Permit>().await.unwrap();
                            controller.send::<Tick>(&()).await.unwrap();
                        }
                    },
                    async {
                        assert_eq!(service.recv::<Read>().await.unwrap(), 7);
                        assert_eq!(service.recv::<Read>().await.unwrap(), 9);
                    },
                    async {
                        let transferred = recipient.recv::<Loan>().await.unwrap();
                        // This independent roll remains live after the resource's
                        // work has exited. No common permit or join is inserted.
                        for number in 0..4 {
                            unrelated_sender.send::<Independent>(&number).await.unwrap();
                            assert_eq!(
                                unrelated_receiver.recv::<Independent>().await.unwrap(),
                                number
                            );
                        }
                        drop(transferred);
                    },
                );
            })
            .await
            .expect("permitted reentry and independent work must finish");
            assert!(network.queue_is_empty());
        });
}
