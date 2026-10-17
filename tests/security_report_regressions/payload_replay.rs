//! Deliberately violate carrier replay exclusion to show the integrity boundary.
use super::*;

#[test]
fn a_replayed_bool_frame_can_be_valid_bytes_for_the_next_u8() {
    let global = g::seq(
        g::send::<0, 1, Msg<1, bool>>(),
        g::send::<0, 1, Msg<2, u8>>(),
    );
    let captured = Rc::new(RefCell::new(Vec::new()));
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            RecordingCarrier {
                inner: TestTransport::new(),
                frames: captured.clone(),
            },
        )
        .unwrap();
    let mut sender = rv.enter(SessionId::new(1), &project::<0>(&global)).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        sender.send::<Msg<1, bool>>(&true).await.unwrap();
        sender.send::<Msg<2, u8>>(&99).await.unwrap();
    });
    let frames = captured.borrow().clone();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].lane, frames[1].lane);
    assert_eq!(frames[0].label, frames[1].label);
    assert_eq!(frames[0].bytes, [1]);
    assert_eq!(frames[1].bytes, [99]);

    let carrier = TestTransport::new();
    let mut receiver_slab = [0; 8192];
    let mut receiver_storage = SessionKitStorage::<TestTransport>::uninit();
    let receiver_rv = receiver_storage
        .init()
        .rendezvous(&mut receiver_slab, carrier.clone())
        .unwrap();
    let mut receiver = receiver_rv
        .enter(SessionId::new(1), &project::<1>(&global))
        .unwrap();
    inject(&carrier, 0, &frames[0]);
    inject(&carrier, 0, &frames[0]);
    runtime.block_on(async {
        assert!(receiver.recv::<Msg<1, bool>>().await.unwrap());
        // 1 is a canonical u8. Schema checking cannot recover its origin or
        // distinguish this duplicate from the application's intended value 99.
        assert_eq!(receiver.recv::<Msg<2, u8>>().await.unwrap(), 1);
    });
    assert!(carrier.queue_is_empty());
}
