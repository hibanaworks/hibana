#![cfg(unix)]

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
    time::{Duration, Instant},
};

use hibana::{
    g::{self, Msg},
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};
use hibana_unix_carrier_proof::UnixDatagramCarrier;

const TEST_TIMEOUT: Duration = Duration::from_secs(3);
const SLAB_BYTES: usize = 64 * 1024;

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        self.0.unpark();
    }
}

struct CountedThreadWake {
    thread: thread::Thread,
    count: AtomicUsize,
}

impl Wake for CountedThreadWake {
    fn wake(self: Arc<Self>) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.thread.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.thread.unpark();
    }
}

fn complete<F: Future>(future: F) -> F::Output {
    let deadline = Instant::now() + TEST_TIMEOUT;
    let waker = Waker::from(std::sync::Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        assert!(
            Instant::now() < deadline,
            "proof carrier operation timed out"
        );
        thread::park_timeout(Duration::from_millis(10));
    }
}

#[test]
fn exact_frames_cross_two_independent_runtimes_in_fifo_order() {
    let program = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<0, 1, Msg<2, u32>>(),
    );
    let origin_program = project::<0, _>(&program);
    let target_program = project::<1, _>(&program);
    let (left, right) = UnixDatagramCarrier::pair(0, 1).expect("Unix socket pair");

    let mut left_slab = vec![0_u8; SLAB_BYTES];
    let mut right_slab = vec![0_u8; SLAB_BYTES];
    let mut left_storage = SessionKitStorage::uninit();
    let mut right_storage = SessionKitStorage::uninit();
    let left_kit = left_storage.init();
    let right_kit = right_storage.init();
    let left_rendezvous = left_kit
        .rendezvous(&mut left_slab, left)
        .expect("left rendezvous");
    let right_rendezvous = right_kit
        .rendezvous(&mut right_slab, right)
        .expect("right rendezvous");
    let session = SessionId::new(9001);
    let mut origin = left_rendezvous
        .enter(session, &origin_program)
        .expect("origin endpoint");
    let mut target = right_rendezvous
        .enter(session, &target_program)
        .expect("target endpoint");

    complete(origin.send::<Msg<1, u32>>(&11)).expect("first send");
    complete(origin.send::<Msg<2, u32>>(&22)).expect("second send");
    assert_eq!(
        complete(target.recv::<Msg<1, u32>>()).expect("first recv"),
        11
    );
    assert_eq!(
        complete(target.recv::<Msg<2, u32>>()).expect("second recv"),
        22
    );
}

#[test]
fn logical_close_wakes_a_remote_receive_after_accepted_frames_drain() {
    let program = g::seq(
        g::send::<0, 1, Msg<3, u32>>(),
        g::send::<0, 1, Msg<5, u32>>(),
    );
    let origin_program = project::<0, _>(&program);
    let target_program = project::<1, _>(&program);
    let (left, right) = UnixDatagramCarrier::pair(0, 1).expect("Unix socket pair");

    let mut left_slab = vec![0_u8; SLAB_BYTES];
    let mut right_slab = vec![0_u8; SLAB_BYTES];
    let mut left_storage = SessionKitStorage::uninit();
    let mut right_storage = SessionKitStorage::uninit();
    let left_kit = left_storage.init();
    let right_kit = right_storage.init();
    let left_rendezvous = left_kit
        .rendezvous(&mut left_slab, left)
        .expect("left rendezvous");
    let right_rendezvous = right_kit
        .rendezvous(&mut right_slab, right)
        .expect("right rendezvous");
    let session = SessionId::new(9002);
    let mut origin = left_rendezvous
        .enter(session, &origin_program)
        .expect("origin endpoint");
    let mut target = right_rendezvous
        .enter(session, &target_program)
        .expect("target endpoint");

    complete(origin.send::<Msg<3, u32>>(&31)).expect("accepted send");
    assert_eq!(
        complete(target.recv::<Msg<3, u32>>()).expect("accepted recv"),
        31
    );

    let mut receive = Box::pin(target.recv::<Msg<5, u32>>());
    let wake = Arc::new(CountedThreadWake {
        thread: thread::current(),
        count: AtomicUsize::new(0),
    });
    let waker = Waker::from(Arc::clone(&wake));
    let mut context = Context::from_waker(&waker);
    assert!(
        receive.as_mut().poll(&mut context).is_pending(),
        "receive must park before its peer closes"
    );
    drop(origin);
    let wake_deadline = Instant::now() + TEST_TIMEOUT;
    while wake.count.load(Ordering::Relaxed) == 0 {
        assert!(
            Instant::now() < wake_deadline,
            "logical close did not wake the registered receiver"
        );
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        complete(receive).is_err(),
        "remote close must terminate the parked receive"
    );
}

#[test]
fn a_fresh_socket_generation_cannot_observe_an_old_session_frame() {
    let program = g::send::<0, 1, Msg<4, u32>>();
    let origin_program = project::<0, _>(&program);
    let target_program = project::<1, _>(&program);
    let session = SessionId::new(9003);

    {
        let (left, right) = UnixDatagramCarrier::pair(0, 1).expect("old socket pair");
        let mut left_slab = vec![0_u8; SLAB_BYTES];
        let mut right_slab = vec![0_u8; SLAB_BYTES];
        let mut left_storage = SessionKitStorage::uninit();
        let mut right_storage = SessionKitStorage::uninit();
        let left_kit = left_storage.init();
        let right_kit = right_storage.init();
        let left_rendezvous = left_kit
            .rendezvous(&mut left_slab, left)
            .expect("old left rendezvous");
        let right_rendezvous = right_kit
            .rendezvous(&mut right_slab, right)
            .expect("old right rendezvous");
        let mut origin = left_rendezvous
            .enter(session, &origin_program)
            .expect("old origin endpoint");
        let target = right_rendezvous
            .enter(session, &target_program)
            .expect("old target endpoint");
        complete(origin.send::<Msg<4, u32>>(&41)).expect("old send");
        drop(target);
    }

    let (left, right) = UnixDatagramCarrier::pair(0, 1).expect("fresh socket pair");
    let mut left_slab = vec![0_u8; SLAB_BYTES];
    let mut right_slab = vec![0_u8; SLAB_BYTES];
    let mut left_storage = SessionKitStorage::uninit();
    let mut right_storage = SessionKitStorage::uninit();
    let left_kit = left_storage.init();
    let right_kit = right_storage.init();
    let left_rendezvous = left_kit
        .rendezvous(&mut left_slab, left)
        .expect("fresh left rendezvous");
    let right_rendezvous = right_kit
        .rendezvous(&mut right_slab, right)
        .expect("fresh right rendezvous");
    let mut origin = left_rendezvous
        .enter(session, &origin_program)
        .expect("fresh origin endpoint");
    let mut target = right_rendezvous
        .enter(session, &target_program)
        .expect("fresh target endpoint");
    complete(origin.send::<Msg<4, u32>>(&42)).expect("fresh send");
    assert_eq!(
        complete(target.recv::<Msg<4, u32>>()).expect("fresh recv"),
        42
    );
}
