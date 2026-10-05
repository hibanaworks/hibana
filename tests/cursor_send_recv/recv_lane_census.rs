use super::*;
use core::{future::Future, pin::pin, task::Waker};

#[test]
#[ignore = "release timing probe"]
fn pending_receive_with_sixteen_independent_lanes() {
    let global = g::par(
        g::par(
            g::par(
                g::par(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
                g::par(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
            ),
            g::par(
                g::par(g::send::<0, 1, Msg<5, ()>>(), g::send::<0, 1, Msg<6, ()>>()),
                g::par(g::send::<0, 1, Msg<7, ()>>(), g::send::<0, 1, Msg<8, ()>>()),
            ),
        ),
        g::par(
            g::par(
                g::par(
                    g::send::<0, 1, Msg<9, ()>>(),
                    g::send::<0, 1, Msg<10, ()>>(),
                ),
                g::par(
                    g::send::<0, 1, Msg<11, ()>>(),
                    g::send::<0, 1, Msg<12, ()>>(),
                ),
            ),
            g::par(
                g::par(
                    g::send::<0, 1, Msg<13, ()>>(),
                    g::send::<0, 1, Msg<14, ()>>(),
                ),
                g::par(
                    g::send::<0, 1, Msg<15, ()>>(),
                    g::send::<0, 1, Msg<16, ()>>(),
                ),
            ),
        ),
    );
    let program: RoleProgram<1> = project(&global);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rendezvous = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut worker = rendezvous.enter(SessionId::new(711), &program).unwrap();
    let mut future = pin!(worker.recv::<Msg<16, ()>>());
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100 {
        assert!(future.as_mut().poll(&mut cx).is_pending());
    }
    for sample in 0..3 {
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            assert!(core::hint::black_box(future.as_mut().poll(&mut cx)).is_pending());
        }
        println!(
            "HIBANA_RECV_LANE_MEASUREMENT: sample={sample} polls=10000 elapsed_ns={}",
            started.elapsed().as_nanos()
        );
    }
}

#[test]
fn pending_schema_choice_does_not_consume_another_parallel_lane() {
    let global = g::par(
        g::send::<0, 1, Msg<51, u8>>(),
        g::send::<0, 1, Msg<51, u32>>(),
    );
    let sender_program: RoleProgram<0> = project(&global);
    let receiver_program: RoleProgram<1> = project(&global);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rendezvous = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let session = SessionId::new(712);
    let mut sender = rendezvous.enter(session, &sender_program).unwrap();
    let mut receiver = rendezvous.enter(session, &receiver_program).unwrap();
    futures::executor::block_on(sender.send::<Msg<51, u8>>(&7)).unwrap();
    {
        let mut pending = pin!(receiver.recv::<Msg<51, u32>>());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(pending.as_mut().poll(&mut cx).is_pending());
        futures::executor::block_on(sender.send::<Msg<51, u32>>(&0x1234_5678)).unwrap();
        assert!(matches!(
            pending.as_mut().poll(&mut cx),
            Poll::Ready(Ok(0x1234_5678))
        ));
    }
    assert_eq!(
        futures::executor::block_on(receiver.recv::<Msg<51, u8>>()).unwrap(),
        7
    );
}
