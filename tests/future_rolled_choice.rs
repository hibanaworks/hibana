use super::common;
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};
use std::{cell::RefCell, collections::VecDeque};

fn available(input: &RefCell<VecDeque<u32>>) -> Result<DecisionArm, ResolverError> {
    Ok(if input.borrow().is_empty() {
        DecisionArm::Right
    } else {
        DecisionArm::Left
    })
}
fn rejected(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Right)
}

#[test]
fn future_outer_arm_preserves_the_current_nested_local_choice() {
    // This transport is entirely in memory. Do not start an OS I/O driver.
    let task_runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    task_runtime.block_on(async {
        const C: u8 = 0;
        const D: u8 = 1;
        const R: u8 = 2;
        type Work = g::Msg<1, ()>;
        type Skip = g::Msg<2, u32>;
        type Local = g::Msg<3, ()>;
        type Ready = g::Msg<4, u32>;
        type Request = g::Msg<5, ()>;
        type Grant = g::Msg<6, u32>;
        type Test = g::Msg<7, ()>;
        type Pass = g::Msg<8, ()>;
        type Retry = g::Msg<9, ()>;
        let body = g::seq(
            g::send::<R, D, Ready>(),
            g::seq(
                g::send::<D, R, Request>(),
                g::seq(
                    g::send::<R, D, Grant>(),
                    g::seq(
                        g::send::<D, D, Test>(),
                        g::route(g::send::<D, D, Pass>(), g::send::<D, D, Retry>())
                            .resolve::<101>(),
                    ),
                ),
            ),
        );
        let work = g::seq(
            g::send::<C, D, Work>(),
            g::seq(
                g::send::<C, R, Work>(),
                g::par(
                    g::par(g::send::<D, D, Local>(), g::send::<R, R, Local>()),
                    body,
                ),
            ),
        );
        let skip = g::seq(g::send::<C, D, Skip>(), g::send::<C, R, Skip>());
        let global = g::route(work, skip).resolve::<100>().roll();
        let pc = project::<C>(&global);
        let pd = project::<D>(&global);
        let pr = project::<R>(&global);
        for early in [false, true] {
            let input = RefCell::new(VecDeque::from([17]));
            let outcome = ();
            let mut slab = vec![0; 128 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, common::TestTransport::new())
                .unwrap();
            rv.set_resolver(&pc, ResolverRef::<100>::decision_state(&input, available))
                .unwrap();
            rv.set_resolver(&pd, ResolverRef::<101>::decision_state(&outcome, rejected))
                .unwrap();
            let sid = SessionId::new(93);
            let mut c = rv.enter(sid, &pc).unwrap();
            let mut d = rv.enter(sid, &pd).unwrap();
            let mut r = rv.enter(sid, &pr).unwrap();
            c.send::<Work>(&()).await.unwrap();
            c.send::<Work>(&()).await.unwrap();
            d.recv::<Work>().await.unwrap();
            r.recv::<Work>().await.unwrap();
            d.send::<Local>(&()).await.unwrap();
            r.send::<Local>(&()).await.unwrap();
            r.send::<Ready>(&17).await.unwrap();
            assert_eq!(d.recv::<Ready>().await.unwrap(), 17);
            d.send::<Request>(&()).await.unwrap();
            r.recv::<Request>().await.unwrap();
            r.send::<Grant>(&29).await.unwrap();
            assert_eq!(d.recv::<Grant>().await.unwrap(), 29);
            d.send::<Test>(&()).await.unwrap();
            assert_eq!(input.borrow_mut().pop_front(), Some(17));
            if early {
                c.send::<Skip>(&41).await.unwrap();
                c.send::<Skip>(&53).await.unwrap();
            }
            let branch = d.offer().await.unwrap();
            assert_eq!(
                branch.label(),
                9,
                "next visit replaced the current nested choice; early={early}"
            );
            drop(branch);
            d.offer().await.unwrap().recv::<Retry>().await.unwrap();
            if !early {
                c.send::<Skip>(&41).await.unwrap();
                c.send::<Skip>(&53).await.unwrap();
            }
            assert_eq!(d.recv::<Skip>().await.unwrap(), 41);
            assert_eq!(r.recv::<Skip>().await.unwrap(), 53);
        }
    });
}
