use super::*;
use crate::runtime::resolver::{DecisionArm, ResolverRef};
use core::{
    future::{Future, poll_fn},
    task::Poll,
};

#[test]
fn a_controller_notification_cannot_obstruct_another_borrowers_return() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            type Local = g::Msg<21, ()>;
            type OtherLocal = g::Msg<33, ()>;
            type Again = g::Msg<22, ()>;
            type Done = g::Msg<23, ()>;
            type OtherAgain = g::Msg<24, ()>;
            type OtherDone = g::Msg<25, ()>;
            type Test = g::Msg<26, ()>;
            type Notice = g::Msg<27, ()>;
            type OwnerAgain = g::Msg<28, ()>;
            type OwnerDone = g::Msg<29, ()>;
            type OtherOwnerAgain = g::Msg<30, ()>;
            type OtherOwnerDone = g::Msg<31, ()>;
            type OtherNotice = g::Msg<32, ()>;
            type Exit = g::Msg<34, ()>;
            let visit = g::seq(
                g::send::<0, 1, Loan>(),
                g::seq(
                    g::send::<10, 10, Local>(),
                    g::seq(g::send::<1, 0, Loan>(), g::send::<1, 3, Test>()),
                ),
            );
            let other_visit = g::seq(
                g::send::<0, 2, Loan>(),
                g::seq(
                    g::send::<10, 10, OtherLocal>(),
                    g::seq(g::send::<2, 0, Loan>(), g::send::<2, 5, Test>()),
                ),
            );
            let first_result = g::route(
                g::seq(
                    g::par(
                        g::send::<3, 1, Done>(),
                        g::par(g::send::<3, 0, OwnerDone>(), g::send::<3, 4, Notice>()),
                    ),
                    g::send::<1, 3, Exit>(),
                ),
                g::seq(
                    g::par(
                        g::send::<3, 1, Again>(),
                        g::par(g::send::<3, 0, OwnerAgain>(), g::send::<3, 4, Notice>()),
                    ),
                    visit,
                ),
            )
            .resolve::<771>()
            .roll();
            let second_result = g::route(
                g::seq(
                    g::par(
                        g::send::<5, 2, OtherDone>(),
                        g::par(
                            g::send::<5, 0, OtherOwnerDone>(),
                            g::send::<5, 6, OtherNotice>(),
                        ),
                    ),
                    g::send::<2, 5, Exit>(),
                ),
                g::seq(
                    g::par(
                        g::send::<5, 2, OtherAgain>(),
                        g::par(
                            g::send::<5, 0, OtherOwnerAgain>(),
                            g::send::<5, 6, OtherNotice>(),
                        ),
                    ),
                    other_visit,
                ),
            )
            .resolve::<772>()
            .roll();
            let global = g::seq(
                g::seq(visit, other_visit),
                g::seq(first_result, second_result),
            );
            let owner_program = crate::runtime::program::project::<0>(&global);
            let worker_program = crate::runtime::program::project::<1>(&global);
            let other_program = crate::runtime::program::project::<2>(&global);
            let first_program = crate::runtime::program::project::<3>(&global);
            let second_program = crate::runtime::program::project::<5>(&global);
            let resource_program = crate::runtime::program::project::<10>(&global);
            let mut slab = std::vec![0;128*1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            rv.set_resolver(
                &first_program,
                ResolverRef::<771>::decision_state(&(), |_| Ok(DecisionArm::Right)),
            )
            .unwrap();
            rv.set_resolver(
                &second_program,
                ResolverRef::<772>::decision_state(&(), |_| Ok(DecisionArm::Right)),
            )
            .unwrap();
            let session = SessionId::new(981);
            let mut owner = rv.enter(session, &owner_program).unwrap();
            let mut worker = rv.enter(session, &worker_program).unwrap();
            let mut other = rv.enter(session, &other_program).unwrap();
            let mut first = rv.enter(session, &first_program).unwrap();
            let mut second = rv.enter(session, &second_program).unwrap();
            let resource = rv.enter(session, &resource_program).unwrap();

            owner.send::<Loan>(resource).await.unwrap();
            let mut resource = worker.recv::<Loan>().await.unwrap();
            resource.send::<Local>(&()).await.unwrap();
            worker.send::<Loan>(resource).await.unwrap();
            let resource = owner.recv::<Loan>().await.unwrap();
            worker.send::<Test>(&()).await.unwrap();
            first.recv::<Test>().await.unwrap();
            owner.send::<Loan>(resource).await.unwrap();
            let mut resource = other.recv::<Loan>().await.unwrap();
            resource.send::<OtherLocal>(&()).await.unwrap();
            other.send::<Loan>(resource).await.unwrap();
            let resource = owner.recv::<Loan>().await.unwrap();
            other.send::<Test>(&()).await.unwrap();
            second.recv::<Test>().await.unwrap();

            drop(second.offer().await.unwrap());
            {
                let [mut vm, mut result, mut notice] = second.par().unwrap();
                let (a, b, c) = tokio::join!(
                    vm.send::<OtherAgain>(&()),
                    result.send::<OtherOwnerAgain>(&()),
                    notice.send::<OtherNotice>(&())
                );
                a.unwrap();
                b.unwrap();
                c.unwrap();
            }
            other.recv::<OtherAgain>().await.unwrap();
            drop(first.offer().await.unwrap());
            {
                let [mut vm, mut result, mut notice] = first.par().unwrap();
                let (a, b, c) = tokio::join!(
                    vm.send::<Again>(&()),
                    result.send::<OwnerAgain>(&()),
                    notice.send::<Notice>(&())
                );
                a.unwrap();
                b.unwrap();
                c.unwrap();
            }
            worker.recv::<Again>().await.unwrap();
            owner.recv::<OwnerAgain>().await.unwrap();
            owner.send::<Loan>(resource).await.unwrap();
            let mut resource = worker.recv::<Loan>().await.unwrap();
            resource.send::<Local>(&()).await.unwrap();
            worker.send::<Loan>(resource).await.unwrap();
            let resource = owner.recv::<Loan>().await.unwrap();
            worker.send::<Test>(&()).await.unwrap();
            first.recv::<Test>().await.unwrap();
            owner.recv::<OtherOwnerAgain>().await.unwrap();
            owner.send::<Loan>(resource).await.unwrap();
            let mut resource = other.recv::<Loan>().await.unwrap();
            drop(first.offer().await.unwrap());
            {
                let [mut vm, mut result, mut notice] = first.par().unwrap();
                let (a, b, c) = tokio::join!(
                    vm.send::<Again>(&()),
                    result.send::<OwnerAgain>(&()),
                    notice.send::<Notice>(&())
                );
                a.unwrap();
                b.unwrap();
                c.unwrap();
            }

            let mut returned = core::pin::pin!(owner.recv::<Loan>());
            assert!(
                poll_fn(|cx| Poll::Ready(returned.as_mut().poll(cx)))
                    .await
                    .is_pending(),
                "a fresh controller notification was consumed as a returned Endpoint"
            );
            resource.send::<OtherLocal>(&()).await.unwrap();
            other.send::<Loan>(resource).await.unwrap();
            drop(returned.await.unwrap());
        });
}
