use super::*;
use core::{future::Future, task::Poll};

#[test]
fn an_owned_preview_in_the_second_fragment_keeps_its_continuation() {
    type LoanLeft = g::Msg<201, Endpoint<'static, 10>>;
    type LoanRight = g::Msg<203, Endpoint<'static, 10>>;
    type Left = g::Msg<202, u32>;
    type Right = g::Msg<204, u32>;
    type Read = g::Msg<205, u32>;
    type Value = g::Msg<206, u32>;
    type Return = g::Msg<207, Endpoint<'static, 10>>;
    type Choice = g::Route<
        g::Par<g::Send<0, 1, Left>, g::Send<0, 1, LoanLeft>>,
        g::Par<g::Send<0, 1, Right>, g::Send<0, 1, LoanRight>>,
    >;
    type Steps = g::Seq<
        Choice,
        g::Seq<g::Send<10, 3, Read>, g::Seq<g::Send<3, 10, Value>, g::Send<1, 0, Return>>>,
    >;
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    for expected_label in [201u8, 203] {
        let network = TestTransport::new();
        let mut owner_slab = [0; 32 * 1024];
        let mut worker_slab = [0; 32 * 1024];
        let mut service_slab = [0; 32 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut worker_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let worker_rv = worker_storage
            .init()
            .rendezvous(&mut worker_slab, network.clone())
            .unwrap();
        let service_rv = service_storage
            .init()
            .rendezvous(&mut service_slab, network.clone())
            .unwrap();
        let sid = SessionId::new(1701 + u32::from(expected_label));
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let db = owner_rv.enter(sid, &db_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(core::time::Duration::from_secs(10), async {
                    // Arrive through the second fragment before the first is sent.
                    match expected_label {
                        201 => owner.send::<LoanLeft>(db).await.unwrap(),
                        203 => owner.send::<LoanRight>(db).await.unwrap(),
                        _ => unreachable!(),
                    }
                    let label = worker.offer().await.unwrap().label();
                    assert_eq!(label, expected_label);
                    let mut db = {
                        let [mut notice, mut loan] = worker.par().unwrap();
                        let mut db = match label {
                            201 => loan.recv::<LoanLeft>().await.unwrap(),
                            203 => loan.recv::<LoanRight>().await.unwrap(),
                            _ => unreachable!(),
                        };
                        {
                            let reading = db.send::<Read>(&7);
                            tokio::pin!(reading);
                            core::future::poll_fn(|cx| {
                                assert!(reading.as_mut().poll(cx).is_pending());
                                Poll::Ready(())
                            })
                            .await;
                            assert!(network.queue_is_empty());
                            let (sent, received) = match label {
                                201 => tokio::join!(owner.send::<Left>(&41), notice.recv::<Left>()),
                                203 => {
                                    tokio::join!(owner.send::<Right>(&41), notice.recv::<Right>())
                                }
                                _ => unreachable!(),
                            };
                            sent.unwrap();
                            assert_eq!(received.unwrap(), 41);
                            let (sent, received) =
                                tokio::join!(reading.as_mut(), service.recv::<Read>());
                            sent.unwrap();
                            assert_eq!(received.unwrap(), 7);
                        }
                        service.send::<Value>(&8).await.unwrap();
                        db
                    };
                    assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                    worker.send::<Return>(db).await.unwrap();
                    drop(owner.recv::<Return>().await.unwrap());
                })
                .await
                .expect("moving a retained owned preview must preserve its unique continuation");
            });
        assert!(network.queue_is_empty());
    }
}
