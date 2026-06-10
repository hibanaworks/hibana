use super::*;
use crate::{
    control::cluster::core::TopologyOperands,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
    runtime::{config::Config, consts::RING_EVENTS},
    transport::{ReceivedFrame, Transport, TransportError},
};
use core::{
    cell::{Cell, UnsafeCell},
    ptr,
};
use std::thread_local;

fn bind_topology_test_scope<'rv, 'cfg, T, U, C, E>(
    rendezvous: &mut Rendezvous<'rv, 'cfg, T, U, C, E>,
    lane: Lane,
) -> Option<()>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    rendezvous.ensure_topology_control_storage_for_lane_slots(lane.raw() as usize + 1)?;
    rendezvous.initialise_control_scope(lane, crate::global::const_dsl::ControlScopeKind::Topology);
    Some(())
}

struct DummyTransport;

impl Transport for DummyTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        core::task::Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Offline))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        unreachable!("this fixture never exercises endpoint rollback")
    }
}

struct DropTransport;

impl Drop for DropTransport {
    fn drop(&mut self) {
        DROP_TRANSPORT_COUNT.with(|count| {
            count.set(count.get().checked_add(1).expect("drop count overflow"));
        });
    }
}

impl Transport for DropTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        core::task::Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Offline))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        unreachable!("this fixture never exercises endpoint rollback")
    }
}

struct DropClock;

impl crate::runtime::config::Clock for DropClock {
    fn now32(&self) -> u32 {
        0
    }
}

impl Drop for DropClock {
    fn drop(&mut self) {
        DROP_CLOCK_COUNT.with(|count| {
            count.set(count.get().checked_add(1).expect("drop count overflow"));
        });
    }
}

type TestRendezvous = Rendezvous<
    'static,
    'static,
    DummyTransport,
    crate::runtime::consts::DefaultLabelUniverse,
    crate::runtime::config::CounterClock,
    crate::control::cap::mint::EpochTbl,
>;
type DropTestRendezvous = Rendezvous<
    'static,
    'static,
    DropTransport,
    crate::runtime::consts::DefaultLabelUniverse,
    DropClock,
    crate::control::cap::mint::EpochTbl,
>;
thread_local! {
    static POLICY_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static POLICY_TEST_SLAB: UnsafeCell<[u8; 32768]> =
        const { UnsafeCell::new([0u8; 32768]) };
    static IMAGE_TEST_SLAB: UnsafeCell<[u8; 32768]> =
        const { UnsafeCell::new([0u8; 32768]) };
    static DROP_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static DROP_TEST_TINY_SLAB: UnsafeCell<[u8; 1]> =
        const { UnsafeCell::new([0u8; 1]) };
    static DROP_TRANSPORT_COUNT: Cell<u32> = const { Cell::new(0) };
    static DROP_CLOCK_COUNT: Cell<u32> = const { Cell::new(0) };
}

fn reset_drop_counts() {
    DROP_TRANSPORT_COUNT.with(|count| count.set(0));
    DROP_CLOCK_COUNT.with(|count| count.set(0));
}

fn drop_counts() -> (u32, u32) {
    let transport = DROP_TRANSPORT_COUNT.with(Cell::get);
    let clock = DROP_CLOCK_COUNT.with(Cell::get);
    (transport, clock)
}

fn with_epf_test_rendezvous<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
    POLICY_TEST_TAP.with(|tap| {
        POLICY_TEST_SLAB.with(|slab| unsafe {
            let tap = &mut *tap.get();
            tap.fill(TapEvent::zero());
            let slab = &mut *slab.get();
            slab.fill(0);
            let config = Config::from_resources((tap, slab), CounterClock::new());
            let rv_id = RendezvousId::new(1);
            let ptr = TestRendezvous::init_in_slab_auto(rv_id, config, DummyTransport)
                .expect("test rendezvous must fit in its slab");
            (*ptr)
                .ensure_core_lane_storage_for_lane_slots(usize::from(
                    crate::runtime::consts::LANE_DOMAIN_SIZE,
                ))
                .expect("direct rendezvous tests declare their lane span explicitly");
            let result = f(&mut *ptr);
            ptr::drop_in_place(ptr);
            result
        })
    })
}

fn publish_state_snapshot(rendezvous: &TestRendezvous, sid: SessionId, lane: Lane) -> Generation {
    let generation = rendezvous.lane_generation(lane);
    let proof = rendezvous
        .prepare_state_snapshot_effect(sid, lane, generation)
        .expect("snapshot proof must be prepared from current lane generation");
    rendezvous.publish_prepared_state_snapshot_effect(proof);
    generation
}

fn publish_state_restore(
    rendezvous: &TestRendezvous,
    sid: SessionId,
    lane: Lane,
    generation: Generation,
) -> Result<(), StateRestoreError> {
    let proof = rendezvous.prepare_state_restore_effect(sid, lane, generation)?;
    rendezvous.publish_prepared_state_restore_effect(proof);
    Ok(())
}

fn publish_tx_commit(
    rendezvous: &TestRendezvous,
    sid: SessionId,
    lane: Lane,
    generation: Generation,
) -> Result<(), TxCommitError> {
    let proof = rendezvous.prepare_tx_commit_effect(sid, lane, generation)?;
    rendezvous.publish_prepared_tx_commit_effect(proof);
    Ok(())
}

fn publish_tx_abort(
    rendezvous: &TestRendezvous,
    sid: SessionId,
    lane: Lane,
    generation: Generation,
) -> Result<(), TxAbortError> {
    let proof = rendezvous.prepare_tx_abort_effect(sid, lane, generation)?;
    rendezvous.publish_prepared_tx_abort_effect(proof);
    Ok(())
}

fn test_session_registered(rendezvous: &TestRendezvous, sid: SessionId) -> bool {
    rendezvous.assoc.find_lane(sid).is_some()
}

fn advance_test_lane_generation_to(rendezvous: &TestRendezvous, lane: Lane, target: Generation) {
    if rendezvous.r#gen.last(lane).is_none() {
        rendezvous.r#gen.publish_prepared(lane, Generation::ZERO);
    }
    if target != Generation::ZERO {
        rendezvous.r#gen.publish_prepared(lane, target);
    }
}

fn stage_test_topology_begin(
    rendezvous: &TestRendezvous,
    sid: SessionId,
    lane: Lane,
    fences: Option<(u32, u32)>,
    generation: Generation,
    expected_ack: Option<TopologyAck>,
) -> Result<(), TopologyError> {
    let expected_ack = expected_ack.ok_or(TopologyError::NoPending { lane })?;
    let (seq_tx, seq_rx) = fences.unwrap_or((0, 0));
    let intent = TopologyIntent {
        src_rv: expected_ack.src_rv,
        dst_rv: expected_ack.dst_rv,
        sid: sid.raw(),
        old_gen: rendezvous.lane_generation(lane),
        new_gen: generation,
        seq_tx,
        seq_rx,
        src_lane: lane,
        dst_lane: expected_ack.new_lane,
    };
    rendezvous.prepare_topology_begin_from_intent(intent)?;
    rendezvous.publish_prepared_topology_begin(sid, lane, generation);
    Ok(())
}

fn with_image_test_rendezvous_slots<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
    POLICY_TEST_TAP.with(|tap| {
        IMAGE_TEST_SLAB.with(|slab| unsafe {
            let tap = &mut *tap.get();
            tap.fill(TapEvent::zero());
            let slab = &mut *slab.get();
            slab.fill(0);
            let config = Config::from_resources((tap, slab), CounterClock::new());
            let rv_id = RendezvousId::new(2);
            let ptr = TestRendezvous::init_in_slab_auto(rv_id, config, DummyTransport)
                .expect("test rendezvous must fit in its slab");
            (*ptr)
                .ensure_core_lane_storage_for_lane_slots(usize::from(
                    crate::runtime::consts::LANE_DOMAIN_SIZE,
                ))
                .expect("direct rendezvous tests declare their lane span explicitly");
            let result = f(&mut *ptr);
            ptr::drop_in_place(ptr);
            result
        })
    })
}

fn with_image_test_rendezvous<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
    with_image_test_rendezvous_slots(f)
}

#[path = "tests/lifecycle_tables.rs"]
mod lifecycle_tables;
#[path = "tests/restore_topology.rs"]
mod restore_topology;
#[path = "tests/topology_effects.rs"]
mod topology_effects;
