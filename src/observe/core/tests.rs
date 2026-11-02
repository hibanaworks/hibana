use super::{TAP_EVENTS, TapRing};
use crate::observe::event::{TapEvent, TapRecord};

#[test]
fn synchronous_observation_retains_the_prefix_when_the_ring_overwrites() {
    let captured = core::cell::Cell::new(std::vec::Vec::new());
    let observer = |event| {
        let mut rows = captured.take();
        rows.push(event);
        captured.set(rows);
    };
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    ring.observe(&observer);
    let written = 4 * TAP_EVENTS;
    for ordinal in 0..written {
        ring.push(TapEvent::new(
            u32::MAX,
            7,
            8,
            ordinal as u32,
            !ordinal as u32,
        ));
    }
    let rows = captured.take();
    assert_eq!(rows.len(), written);
    assert_eq!(ring.port().checkpoint(), Some(written as u32));
    for (ordinal, row) in rows.iter().enumerate() {
        assert_eq!(row.ts(), ordinal as u32);
        assert_eq!(row.arg0(), ordinal as u32);
        assert_eq!(row.arg1(), !ordinal as u32);
    }
    let retained: std::vec::Vec<_> = ring.port().collect();
    assert!(retained == rows[written - TAP_EVENTS..]);
}

struct Immediate;

#[hibana_runtime::main(Immediate)]
#[test]
async fn resolver_observations_match_calls_across_offers_and_rolled_visits()
-> Result<(), core::convert::Infallible> {
    use crate::{
        g,
        runtime::{
            SessionKitStorage,
            ids::SessionId,
            program::project,
            resolver::{DecisionArm, ResolverRef},
            tap,
        },
    };
    type Left = g::Msg<1, ()>;
    type Right = g::Msg<2, ()>;
    type Continue = g::Msg<3, ()>;
    let global = g::route(
        g::seq(g::send::<0, 0, Left>(), g::send::<0, 0, Continue>()),
        g::seq(g::send::<0, 0, Right>(), g::send::<0, 0, Continue>()),
    )
    .resolve::<77>()
    .roll();
    let program = project::<0>(&global);
    for arm in [DecisionArm::Left, DecisionArm::Right] {
        for mode in 0..3 {
            let calls = core::cell::Cell::new(0);
            let input = (&calls, arm);
            let captured = core::cell::Cell::new(std::vec::Vec::new());
            let observer = |event| {
                let mut rows = captured.take();
                rows.push(event);
                captured.set(rows);
            };
            let mut slab = std::vec![0; 128 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(
                    &mut slab,
                    crate::test_support::transport::TestTransport::new(),
                )
                .unwrap();
            rv.observe(&observer);
            rv.set_resolver(
                &program,
                ResolverRef::<77>::decision_state(&input, |input| {
                    input.0.set(input.0.get() + 1);
                    Ok(input.1)
                }),
            )
            .unwrap();
            let mut endpoint = rv.enter(SessionId::new(1), &program).unwrap();
            for visit in 1..=2 {
                if mode == 1 {
                    let branch = endpoint.offer().await.unwrap();
                    assert_eq!(calls.get(), visit);
                    match arm {
                        DecisionArm::Left => branch.send::<Left>(&()).await.unwrap(),
                        DecisionArm::Right => branch.send::<Right>(&()).await.unwrap(),
                    }
                } else {
                    if mode == 2 {
                        drop(endpoint.offer().await.unwrap());
                        assert_eq!(calls.get(), visit);
                    }
                    match arm {
                        DecisionArm::Left => endpoint.send::<Left>(&()).await.unwrap(),
                        DecisionArm::Right => endpoint.send::<Right>(&()).await.unwrap(),
                    }
                }
                endpoint.send::<Continue>(&()).await.unwrap();
                assert_eq!(
                    calls.get(),
                    visit,
                    "continuation must reuse the chosen branch"
                );
            }
            let rows = captured.take();
            let audits: std::vec::Vec<_> = rows
                .iter()
                .filter(|row| row.id() == tap::RESOLVER_AUDIT)
                .collect();
            assert_eq!(audits.len(), calls.get());
            for audit in audits {
                assert_eq!(audit.arg1() & 0xffff, 77);
                assert_eq!(audit.causal_key() & 0xff, u16::from(arm.index()));
            }
        }
    }
    Ok(())
}

impl hibana_runtime::Driver for Immediate {
    fn waker(&self) -> core::task::Waker {
        core::task::Waker::noop().clone()
    }

    fn wait(&mut self) {
        panic!("a local operation needs no external wakeup");
    }
}

#[hibana_runtime::main(Immediate)]
#[test]
async fn an_unyielding_endpoint_burst_retains_the_complete_capture()
-> Result<(), core::convert::Infallible> {
    use crate::{
        g,
        runtime::{SessionKitStorage, ids::SessionId, program::project},
    };
    let captured = core::cell::Cell::new(std::vec::Vec::new());
    let observer = |event| {
        let mut rows = captured.take();
        rows.push(event);
        captured.set(rows);
    };
    let global = g::send::<0, 0, g::Msg<1, ()>>().roll();
    let program = project::<0>(&global);
    let mut slab = std::vec![0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            crate::test_support::transport::TestTransport::new(),
        )
        .unwrap();
    rv.observe(&observer);
    let mut endpoint = rv.enter(SessionId::new(1), &program).unwrap();
    for _ in 0..257 {
        endpoint.send::<g::Msg<1, ()>>(&()).await.unwrap();
    }
    drop(endpoint);
    let checkpoint = rv.tap().checkpoint().unwrap();
    let rows = captured.take();
    assert_eq!(rows.len(), checkpoint as usize);
    assert_eq!(
        rows.iter()
            .filter(|row| row.id() == crate::observe::ids::ENDPOINT_SEND)
            .count(),
        257
    );
    assert!(
        rows.iter()
            .enumerate()
            .all(|(ordinal, row)| row.ts() == ordinal as u32)
    );
    let retained: std::vec::Vec<_> = rv.tap().collect();
    assert!(retained == rows[rows.len() - TAP_EVENTS..]);
    Ok(())
}

#[test]
fn producer_checkpoints_are_independent_of_reader_consumption() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut first = ring.port();
    assert_eq!(first.checkpoint(), Some(0));
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(first.checkpoint(), Some(1));
    assert_eq!(first.checkpoint(), Some(1));
    let mut second = ring.port();
    assert_eq!(first.next().unwrap().ts(), 0);
    assert_eq!(first.checkpoint(), Some(1));
    assert_eq!(second.next().unwrap().ts(), 0);
    assert!(first.next().is_none());
    ring.push(TapEvent::new(0, 7, 8, 11, 12));
    assert_eq!(first.checkpoint(), Some(2));
    assert_eq!(second.checkpoint(), Some(2));
    assert_eq!(first.next().unwrap().ts(), 1);
    assert_eq!(second.next().unwrap().ts(), 1);
}

#[test]
fn checkpoints_reveal_the_produced_prefix_even_after_ring_overwrite() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut reader = ring.port();
    for ordinal in 0..TAP_EVENTS + 1 {
        ring.push(TapEvent::new(0, 7, 8, ordinal as u32, 0));
    }
    assert_eq!(reader.checkpoint(), Some((TAP_EVENTS + 1) as u32));
    assert_eq!(reader.next().unwrap().ts(), 1);
    assert_eq!(reader.count(), TAP_EVENTS - 1);
}

#[test]
fn an_exhausted_checkpoint_never_returns_a_small_ordinal_after_counter_wrap() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let reader = ring.port();
    ring.ring.head.set(u32::MAX as usize - 1);
    assert_eq!(reader.checkpoint(), Some(u32::MAX - 1));
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(reader.checkpoint(), None);
    ring.ring.head.set(usize::MAX);
    assert_eq!(reader.checkpoint(), None);
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(ring.ring.head.get(), 0);
    assert_eq!(reader.checkpoint(), None);
}

#[test]
fn lagging_reader_reconstructs_the_exact_resident_window() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut reader = ring.port();

    let written = 70u32;
    for ordinal in 0..written {
        ring.push(TapEvent::new(u32::MAX, 7, 8, ordinal, !ordinal));
    }

    for ordinal in written - TAP_EVENTS as u32..written {
        let event = reader.next().expect("resident tap record");
        assert_eq!(event.ts(), ordinal);
        assert_eq!(event.id(), 7);
        assert_eq!(event.causal_key(), 8);
        assert_eq!(event.arg0(), ordinal);
        assert_eq!(event.arg1(), !ordinal);
    }
    assert!(reader.next().is_none());
}

#[test]
fn head_wrap_preserves_the_exact_resident_window() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    ring.ring.head.set(usize::MAX - 1);

    for ordinal in 0..TAP_EVENTS as u32 + 2 {
        ring.push(TapEvent::new(0, 7, 8, ordinal, !ordinal));
    }

    let events = ring.port().collect::<std::vec::Vec<_>>();
    assert_eq!(events.len(), TAP_EVENTS);
    for (event, ordinal) in events.iter().zip(2..TAP_EVENTS as u32 + 2) {
        assert_eq!(event.ts(), u32::MAX);
        assert_eq!(event.arg0(), ordinal);
        assert_eq!(event.arg1(), !ordinal);
    }
}
