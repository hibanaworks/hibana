use super::*;
use crate::{
    control::cluster::core::TopologyOperands,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
    runtime::{config::Config, consts::RING_EVENTS},
    transport::{Transport, TransportError, wire::Payload},
};
use core::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    ptr,
};
use std::thread_local;

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
    ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Offline))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn recv_frame_hint<'a>(&self, _rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        None
    }
}

struct DropTransport;

impl Drop for DropTransport {
    fn drop(&mut self) {
        DROP_TRANSPORT_COUNT.with(|count| count.set(count.get().saturating_add(1)));
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
    ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Offline))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn recv_frame_hint<'a>(&self, _rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        None
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
        DROP_CLOCK_COUNT.with(|count| count.set(count.get().saturating_add(1)));
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
    static POLICY_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static IMAGE_TEST_SLAB: UnsafeCell<[u8; 32768]> =
        const { UnsafeCell::new([0u8; 32768]) };
    static IMAGE_TEST_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
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
        POLICY_TEST_SLAB.with(|slab| {
            POLICY_TEST_RENDEZVOUS.with(|rendezvous| unsafe {
                let tap = &mut *tap.get();
                tap.fill(TapEvent::zero());
                let slab = &mut *slab.get();
                slab.fill(0);
                let config = Config::from_resources((tap, slab), CounterClock::new());
                let ptr = (*rendezvous.get()).as_mut_ptr();
                let rv_id = RendezvousId::new(1);
                TestRendezvous::init_from_config(ptr, rv_id, config, DummyTransport);
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
    })
}

fn with_image_test_rendezvous_slots<R>(f: impl FnOnce(&mut TestRendezvous) -> R) -> R {
    POLICY_TEST_TAP.with(|tap| {
        IMAGE_TEST_SLAB.with(|slab| {
            IMAGE_TEST_RENDEZVOUS.with(|rendezvous| unsafe {
                let tap = &mut *tap.get();
                tap.fill(TapEvent::zero());
                let slab = &mut *slab.get();
                slab.fill(0);
                let config = Config::from_resources((tap, slab), CounterClock::new());
                let ptr = (*rendezvous.get()).as_mut_ptr();
                let rv_id = RendezvousId::new(2);
                TestRendezvous::init_from_config(ptr, rv_id, config, DummyTransport);
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
