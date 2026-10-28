use super::{OfferLaneScan, incoming_offer_lanes};
use crate::global::role_program::{LANE_SET_VIEW_WORDS, LaneSetView, LaneWord};

#[test]
fn a_waiting_offer_polls_each_direction_once_and_receives_an_independent_choice() {
    use crate::{
        g,
        runtime::{SessionKitStorage, ids::SessionId, program::project},
        test_support::transport::TestTransport,
        transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    };
    use core::{
        future::Future,
        task::{Context, Poll},
    };
    use std::{cell::RefCell, rc::Rc, vec::Vec};

    struct CountedTransport {
        inner: TestTransport,
        receives: Rc<RefCell<Vec<(u8, u8)>>>,
    }
    impl Transport for CountedTransport {
        type Tx<'a> = <TestTransport as Transport>::Tx<'a>;
        type Rx<'a> = (<TestTransport as Transport>::Rx<'a>, u8, u8);

        fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
            let role = port.local_role();
            let lane = port.lane();
            let (tx, rx) = self.inner.open(port);
            (tx, (rx, role, lane))
        }

        fn poll_send<'f>(
            &self,
            tx: &mut Self::Tx<'_>,
            outgoing: Outgoing<'f>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), TransportError>> {
            self.inner.poll_send(tx, outgoing, cx)
        }

        fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
            self.inner.cancel_send(tx);
        }

        fn poll_recv<'a>(
            &'a self,
            rx: &'a mut Self::Rx<'_>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
            self.receives.borrow_mut().push((rx.1, rx.2));
            self.inner.poll_recv(&mut rx.0, cx)
        }

        fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
            self.inner.requeue(&mut rx.0)
        }
    }

    type Left = g::Msg<201, u32>;
    type Right = g::Msg<202, u32>;
    type OtherLeft = g::Msg<203, u32>;
    type OtherRight = g::Msg<204, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
        g::route(g::send::<2, 1, OtherLeft>(), g::send::<2, 1, OtherRight>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let c_program = project::<2>(&global);
    let receives = Rc::new(RefCell::new(Vec::new()));
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let runtime = storage
        .init()
        .rendezvous(
            &mut slab,
            CountedTransport {
                inner: network.clone(),
                receives: Rc::clone(&receives),
            },
        )
        .unwrap();
    let session = SessionId::new(1739);
    let mut a = runtime.enter(session, &a_program).unwrap();
    let mut b = runtime.enter(session, &b_program).unwrap();
    let mut c = runtime.enter(session, &c_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                {
                    let waiting = b.offer();
                    tokio::pin!(waiting);
                    core::future::poll_fn(|cx| {
                        assert!(waiting.as_mut().poll(cx).is_pending());
                        receives.borrow_mut().clear();
                        assert!(waiting.as_mut().poll(cx).is_pending());
                        let reads = receives.borrow();
                        assert_eq!(
                            reads.len(),
                            2,
                            "one pending poll per independent direction: {reads:?}"
                        );
                        assert_ne!(reads[0], reads[1]);
                        Poll::Ready(())
                    })
                    .await;
                    c.send::<OtherRight>(&31).await.unwrap();
                    let branch = waiting.as_mut().await.unwrap();
                    assert_eq!(branch.label(), 204);
                    assert_eq!(branch.recv::<OtherRight>().await.unwrap(), 31);
                }
                a.send::<Right>(&19).await.unwrap();
                assert_eq!(b.offer().await.unwrap().recv::<Right>().await.unwrap(), 19);
            })
            .await
            .expect("an independent choice must wake the waiting offer");
        });
    assert!(network.queue_is_empty());
}

fn collect(words: &[LaneWord; LANE_SET_VIEW_WORDS], preferred: usize) -> [bool; 256] {
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let mut scan = OfferLaneScan::new(preferred, lanes, 256);
    let mut seen = [false; 256];
    while let Some(lane) = scan.next() {
        assert!(!seen[lane], "lane {lane} was yielded twice");
        seen[lane] = true;
    }
    seen
}

#[test]
fn preferred_lane_is_first_and_every_lane_is_yielded_once() {
    let mut words = [0; LANE_SET_VIEW_WORDS];
    for lane in [0usize, 63, 64, 127, 128, 191, 192, 255] {
        let (word, bit) = crate::global::role_program::lane_word_index(lane);
        words[word] |= bit;
    }
    let seen = collect(&words, 128);
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    for (lane, present) in seen.into_iter().enumerate() {
        assert_eq!(present, lanes.contains(lane));
    }

    let mut scan = OfferLaneScan::new(128, lanes, 256);
    assert_eq!(scan.next(), Some(128));
}

#[test]
fn absent_or_out_of_range_preference_does_not_change_membership() {
    let mut words = [0; LANE_SET_VIEW_WORDS];
    let (word, bit) = crate::global::role_program::lane_word_index(255);
    words[word] = bit;
    assert!(collect(&words, 17)[255]);
    assert!(collect(&words, 256)[255]);

    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let mut bounded = OfferLaneScan::new(255, lanes, 255);
    assert_eq!(bounded.next(), None);
}

#[test]
fn incoming_lane_iteration_matches_the_full_scan_at_sparse_word_edges() {
    let mut sparse = [0; LANE_SET_VIEW_WORDS];
    for lane in [0usize, 7, 8, 31, 32, 63, 64, 127, 128, 191, 192, 254, 255] {
        let (word, bit) = crate::global::role_program::lane_word_index(lane);
        sparse[word] |= bit;
    }
    for incoming in [
        [0; LANE_SET_VIEW_WORDS],
        [u32::MAX; LANE_SET_VIEW_WORDS],
        [0x55555555; LANE_SET_VIEW_WORDS],
        sparse,
    ] {
        for deferred in [
            [0; LANE_SET_VIEW_WORDS],
            [u32::MAX; LANE_SET_VIEW_WORDS],
            [0xaaaaaaaa; LANE_SET_VIEW_WORDS],
            sparse,
        ] {
            for limit in [
                0usize, 1, 7, 8, 31, 32, 33, 63, 64, 65, 127, 128, 129, 191, 192, 193, 255, 256,
            ] {
                let expected: std::vec::Vec<_> = (0..limit)
                    .filter(|&lane| {
                        let (word, bit) = crate::global::role_program::lane_word_index(lane);
                        incoming[word] & bit != 0 && deferred[word] & bit == 0
                    })
                    .collect();
                let actual: std::vec::Vec<_> =
                    incoming_offer_lanes(&incoming, &deferred, limit).collect();
                assert_eq!(actual, expected, "limit={limit}");
                assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
            }
        }
    }
}
