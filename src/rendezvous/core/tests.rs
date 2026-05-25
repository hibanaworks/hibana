use super::*;
use crate::{
    control::cluster::core::{CpCommand, EffectRunner, TopologyOperands},
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

fn cap_token_wire_image(
    nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
    header: [u8; crate::control::cap::mint::CAP_HEADER_LEN],
) -> [u8; crate::control::cap::mint::CAP_TOKEN_LEN] {
    let mut bytes = [0u8; crate::control::cap::mint::CAP_TOKEN_LEN];
    bytes[..crate::control::cap::mint::CAP_NONCE_LEN].copy_from_slice(&nonce);
    bytes[crate::control::cap::mint::CAP_NONCE_LEN
        ..crate::control::cap::mint::CAP_NONCE_LEN + crate::control::cap::mint::CAP_HEADER_LEN]
        .copy_from_slice(&header);
    bytes
}

fn endpoint_cap_token_from_wire(
    nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
    header: [u8; crate::control::cap::mint::CAP_HEADER_LEN],
) -> crate::control::cap::mint::GenericCapToken<crate::control::cap::mint::EndpointResource> {
    crate::control::cap::mint::GenericCapToken::from_bytes(cap_token_wire_image(nonce, header))
}

struct RejectingHandleKind;

impl crate::control::cap::mint::ResourceKind for RejectingHandleKind {
    type Handle = ();

    const TAG: u8 = 0xE1;
    const NAME: &'static str = "RejectingHandle";

    fn encode_handle(_handle: &Self::Handle) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN] {
        [0xA5; crate::control::cap::mint::CAP_HANDLE_LEN]
    }

    fn decode_handle(
        _data: [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, crate::control::cap::mint::CapError> {
        Err(crate::control::cap::mint::CapError::Mismatch)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl crate::control::cap::mint::ClaimableResourceKind for RejectingHandleKind {}

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
    type Metrics = ();

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &'a self,
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

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
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
    type Metrics = ();

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &'a self,
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

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
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

#[path = "tests/claim_caps.rs"]
mod claim_caps;
#[path = "tests/lifecycle_tables.rs"]
mod lifecycle_tables;
#[path = "tests/restore_topology.rs"]
mod restore_topology;
#[path = "tests/topology_effects.rs"]
mod topology_effects;
