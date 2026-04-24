//! Protocol-neutral substrate surface for protocol implementors.

pub use crate::control::cluster::error::{AttachError, CpError};

pub use crate::control::types::{Lane, RendezvousId, SessionId};
pub use crate::eff::EffIndex;
pub use crate::transport::Transport;

use crate::control;
use crate::control::cluster;

type KernelSessionCluster<'cfg, T, U, C, const MAX_RV: usize> =
    crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>;

/// Protocol-neutral session kit facade for protocol implementors.
///
/// The runtime is intentionally local-only: `SessionKit` is neither `Send` nor
/// `Sync`, and mutation is centralised inside the single-thread substrate
/// owner.
#[repr(transparent)]
pub struct SessionKit<'cfg, T, U, C, const MAX_RV: usize = 4>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    inner: KernelSessionCluster<'cfg, T, U, C, MAX_RV>,
    _cfg: core::marker::PhantomData<crate::endpoint::carrier::SessionCfg<Self>>,
    _local_only: crate::local::LocalOnly,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    pub fn new(clock: &'cfg C) -> Self {
        let mut kit = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_empty(kit.as_mut_ptr(), clock);
            kit.assume_init()
        }
    }

    unsafe fn init_empty(dst: *mut Self, clock: &'cfg C) {
        unsafe {
            crate::control::cluster::core::SessionCluster::init_empty(
                core::ptr::addr_of_mut!((*dst).inner),
                clock,
            );
            core::ptr::addr_of_mut!((*dst)._cfg).write(core::marker::PhantomData);
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    pub fn add_rendezvous_from_config(
        &self,
        config: crate::substrate::runtime::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.inner.add_rendezvous_from_config(config, transport)
    }

    #[inline]
    pub(crate) unsafe fn public_endpoint_kernel_ptr<'r, const ROLE: u8, Mint>(
        &'r self,
        handle: crate::endpoint::carrier::PackedEndpointHandle,
        generation: u32,
    ) -> Option<
        *mut crate::endpoint::carrier::KernelCursorEndpoint<
            'r,
            ROLE,
            Self,
            control::cap::mint::EpochTbl,
            Mint,
            crate::binding::BindingHandle<'r>,
        >,
    >
    where
        Mint: control::cap::mint::MintConfigMarker,
        'cfg: 'r,
    {
        let rv = handle.rendezvous();
        let slot = handle.slot();
        unsafe {
            self.inner
                .public_endpoint_ptr::<ROLE, Mint>(rv, slot, generation)
                .map(|ptr| {
                    ptr.cast::<crate::endpoint::carrier::KernelCursorEndpoint<
                        'r,
                        ROLE,
                        Self,
                        control::cap::mint::EpochTbl,
                        Mint,
                        crate::binding::BindingHandle<'r>,
                    >>()
                })
        }
    }

    #[inline]
    #[allow(private_bounds)]
    pub fn enter<'r, const ROLE: u8, B>(
        &'r self,
        rv: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        binding: B,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        B: crate::binding::BindingArg<'r>,
        'cfg: 'r,
    {
        let binding = binding.into_binding_handle();
        Self::enter_with_binding(self, rv, sid, program, binding)
    }

    #[inline]
    fn enter_with_binding<'r, const ROLE: u8>(
        &'r self,
        rv: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<crate::Endpoint<'r, ROLE>, AttachError>
    where
        'cfg: 'r,
    {
        let (slot, generation) = self.inner.enter::<ROLE>(rv, sid, program, binding)?;
        let handle = crate::endpoint::carrier::PackedEndpointHandle::new(rv, slot);
        Ok(crate::endpoint::Endpoint::from_handle(
            self, handle, generation,
        ))
    }

    #[inline]
    pub fn set_resolver<const POLICY: u16, const ROLE: u8>(
        &self,
        rv: RendezvousId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        resolver: crate::substrate::policy::ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        self.inner
            .set_resolver::<POLICY, ROLE>(rv, program, resolver)
    }
}

/// Everyday runtime setup owners for protocol implementors.
pub mod runtime {
    pub use crate::runtime::config::{Clock, Config, CounterClock};
    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse};
}

/// Canonical tap-event owner retained in core.
pub mod tap {
    pub use crate::observe::core::TapEvent;
}

/// Canonical binding and ingress classification surface.
pub mod binding {
    pub use crate::binding::{
        BindingSlot, Channel, ChannelDirection, ChannelKey, ChannelStore, IncomingClassification,
        NoBinding, TransportOpsError,
    };
}

/// Canonical resolver/policy surface plus advanced metadata buckets.
pub mod policy {
    pub use super::cluster::core::{
        DynamicResolution, ResolverContext, ResolverError, ResolverRef,
    };
    pub use crate::transport::context::{
        ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySignalsProvider,
    };

    /// Advanced fixed metadata keys for resolver-context attributes.
    pub mod core {
        pub use crate::transport::context::core::{
            CONGESTION_MARKS, CONGESTION_WINDOW, IN_FLIGHT_BYTES, LANE, LATENCY_US, LATEST_ACK_PN,
            PACING_INTERVAL_US, PTO_COUNT, QUEUE_DEPTH, RETRANSMISSIONS, RV_ID, SESSION_ID,
            SRTT_US, TAG, TRANSPORT_ALGORITHM,
        };
    }

    pub use crate::policy_runtime::PolicySlot;
}

/// Canonical capability-token surface plus advanced mint/control-kind owners.
pub mod cap {
    /// Deep-dive mint details and the standard control-kind catalogue.
    pub mod advanced {
        pub use super::super::control::cap::mint::{
            CAP_HANDLE_LEN, CapError, CapHeader, ControlOp, ControlPath,
        };
        pub use crate::control::cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RouteDecisionKind,
        };
        pub use crate::global::const_dsl::{ControlScopeKind, ScopeId};
    }

    pub use crate::control::cap::mint::{
        CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
    };
    pub use crate::control::cap::typed_tokens::CapRegisteredToken;
    pub use crate::control::types::{Many, One};
}

/// Canonical wire payload surface.
pub mod wire {
    pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
}

/// Canonical transport I/O surface plus observation/detail owners.
pub mod transport {
    pub use crate::transport::{
        LocalDirection, Outgoing, SendMeta, TransportAlgorithm, TransportError, TransportEvent,
        TransportEventKind, TransportMetrics,
    };
}

#[cfg(all(test, feature = "std"))]
mod tests {
    extern crate self as hibana;

    use std::cell::UnsafeCell;

    use crate::{
        Endpoint,
        substrate::{
            SessionId, SessionKit, Transport,
            binding::NoBinding,
            runtime::{Config, CounterClock, DefaultLabelUniverse},
            transport::{Outgoing, TransportError, TransportEvent},
            wire::Payload,
        },
    };
    mod fanout_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/fanout_program.rs"
        ));
    }
    mod huge_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/huge_program.rs"
        ));
    }
    mod linear_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/linear_program.rs"
        ));
    }
    mod localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/localside.rs"
        ));
    }
    mod route_localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_localside.rs"
        ));
    }
    mod route_control_kinds {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_control_kinds.rs"
        ));
    }

    type PicoKit = SessionKit<'static, PicoTransport, DefaultLabelUniverse, CounterClock, 2>;

    const PICO_RING_EVENTS: usize = 128;
    const TARGET_PICO_SLAB_BYTES: usize = 32_768;
    const HOST_MEASURE_SLAB_BYTES: usize = 131_072;
    const HOST_STACK_BYTES: usize = 32 * 1024;
    const STACK_CANARY_BYTE: u8 = 0xA5;
    const STACK_CANARY_HEADROOM_BYTES: usize = 512;
    const QUEUE_CAPACITY: usize = 16;
    const PAYLOAD_CAPACITY: usize = 96;

    fn retain_pico_smoke_fixture_symbols() {
        let _ = huge_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = huge_program::controller_program as fn() -> crate::g::advanced::RoleProgram<0>;
        let _ = linear_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = linear_program::controller_program as fn() -> crate::g::advanced::RoleProgram<0>;
        let _ = fanout_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = fanout_program::controller_program as fn() -> crate::g::advanced::RoleProgram<0>;
        let _ =
            localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
    }

    #[test]
    fn pico_smoke_fixture_symbols_are_reachable() {
        retain_pico_smoke_fixture_symbols();
    }

    std::thread_local! {
        static FIXTURE_CLOCK: CounterClock = const { CounterClock::new() };
        static FIXTURE_TAP: UnsafeCell<[crate::observe::core::TapEvent; PICO_RING_EVENTS]> =
            const { UnsafeCell::new([crate::observe::core::TapEvent::zero(); PICO_RING_EVENTS]) };
        static FIXTURE_SLAB: UnsafeCell<[u8; HOST_MEASURE_SLAB_BYTES]> =
            const { UnsafeCell::new([0u8; HOST_MEASURE_SLAB_BYTES]) };
        static FIXTURE_TRANSPORT: UnsafeCell<PicoTransportState> =
            const { UnsafeCell::new(PicoTransportState::new()) };
    }

    #[derive(Clone, Copy, Debug)]
    struct RuntimeShapeMetrics {
        slab_bytes: usize,
        sidecar_scratch_high_water_bytes: usize,
        live_endpoint_bytes: usize,
        peak_live_slab_bytes: usize,
        peak_stack_bytes: usize,
    }

    #[derive(Clone, Copy, Debug)]
    struct StackBounds {
        low: usize,
        high: usize,
    }

    #[derive(Clone, Copy)]
    struct FrameOwned {
        len: usize,
        payload: [u8; PAYLOAD_CAPACITY],
    }

    impl FrameOwned {
        const fn empty() -> Self {
            Self {
                len: 0,
                payload: [0; PAYLOAD_CAPACITY],
            }
        }

        fn from_bytes(bytes: &[u8]) -> Self {
            assert!(
                bytes.len() <= PAYLOAD_CAPACITY,
                "pico runtime payload exceeds fixed capacity"
            );
            let mut payload = [0u8; PAYLOAD_CAPACITY];
            payload[..bytes.len()].copy_from_slice(bytes);
            Self {
                len: bytes.len(),
                payload,
            }
        }

        fn as_slice(&self) -> &[u8] {
            &self.payload[..self.len]
        }
    }

    #[derive(Clone, Copy)]
    struct FixedQueue {
        items: [FrameOwned; QUEUE_CAPACITY],
        head: usize,
        len: usize,
    }

    impl FixedQueue {
        const fn new() -> Self {
            Self {
                items: [FrameOwned::empty(); QUEUE_CAPACITY],
                head: 0,
                len: 0,
            }
        }

        fn push_back(&mut self, item: FrameOwned) {
            assert!(
                self.len < QUEUE_CAPACITY,
                "pico runtime transport queue capacity exceeded"
            );
            let idx = (self.head + self.len) % QUEUE_CAPACITY;
            self.items[idx] = item;
            self.len += 1;
        }

        fn push_front(&mut self, item: FrameOwned) {
            assert!(
                self.len < QUEUE_CAPACITY,
                "pico runtime transport queue capacity exceeded"
            );
            self.head = if self.head == 0 {
                QUEUE_CAPACITY - 1
            } else {
                self.head - 1
            };
            self.items[self.head] = item;
            self.len += 1;
        }

        fn pop_front(&mut self) -> Option<FrameOwned> {
            if self.len == 0 {
                return None;
            }
            let idx = self.head;
            self.head = (self.head + 1) % QUEUE_CAPACITY;
            self.len -= 1;
            Some(self.items[idx])
        }
    }

    #[derive(Clone, Copy)]
    struct RoleState {
        queue: FixedQueue,
    }

    impl RoleState {
        const fn new() -> Self {
            Self {
                queue: FixedQueue::new(),
            }
        }
    }

    #[derive(Clone, Copy)]
    struct PicoTransportState {
        roles: [RoleState; 2],
    }

    impl PicoTransportState {
        const fn new() -> Self {
            Self {
                roles: [RoleState::new(), RoleState::new()],
            }
        }

        fn role_mut(&mut self, role: u8) -> &mut RoleState {
            match role {
                0 | 1 => &mut self.roles[role as usize],
                _ => panic!("pico runtime transport role out of range"),
            }
        }

        fn role(&self, role: u8) -> &RoleState {
            match role {
                0 | 1 => &self.roles[role as usize],
                _ => panic!("pico runtime transport role out of range"),
            }
        }
    }

    #[derive(Clone, Copy)]
    struct PicoTransport;

    struct PicoTx;

    struct PicoRx {
        role: u8,
        current: Option<FrameOwned>,
    }

    fn with_transport_state<R>(f: impl FnOnce(&mut PicoTransportState) -> R) -> R {
        FIXTURE_TRANSPORT.with(|state| unsafe { f(&mut *state.get()) })
    }

    impl Transport for PicoTransport {
        type Error = TransportError;
        type Tx<'a>
            = PicoTx
        where
            Self: 'a;
        type Rx<'a>
            = PicoRx
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            with_transport_state(|state| {
                let _ = state.role(local_role);
            });
            (
                PicoTx,
                PicoRx {
                    role: local_role,
                    current: None,
                },
            )
        }

        fn poll_send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            outgoing: Outgoing<'f>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            with_transport_state(|state| {
                state
                    .role_mut(outgoing.meta.peer)
                    .queue
                    .push_back(FrameOwned::from_bytes(outgoing.payload.as_bytes()));
            });
            core::task::Poll::Ready(Ok(()))
        }

        fn poll_recv<'a>(
            &'a self,
            rx: &'a mut Self::Rx<'a>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
            if rx.current.is_some() {
                rx.current = None;
            }
            if rx.current.is_none() {
                let dequeued =
                    with_transport_state(|state| state.role_mut(rx.role).queue.pop_front());
                match dequeued {
                    Some(frame) => rx.current = Some(frame),
                    None => return core::task::Poll::Pending,
                }
            }
            let frame = rx.current.as_ref().expect("queued transport frame");
            let bytes: &'a [u8] = unsafe { &*(frame.as_slice() as *const [u8]) };
            core::task::Poll::Ready(Ok(Payload::new(bytes)))
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

        fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
            if let Some(frame) = rx.current.take() {
                with_transport_state(|state| state.role_mut(rx.role).queue.push_front(frame));
            }
        }

        fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {}

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    fn noop_waker() -> core::task::Waker {
        unsafe fn clone(_: *const ()) -> core::task::RawWaker {
            core::task::RawWaker::new(core::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}

        static VTABLE: core::task::RawWakerVTable =
            core::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop);

        unsafe {
            core::task::Waker::from_raw(core::task::RawWaker::new(core::ptr::null(), &VTABLE))
        }
    }

    fn block_on<F: core::future::Future>(mut future: F) -> F::Output {
        let waker = noop_waker();
        let mut cx = core::task::Context::from_waker(&waker);
        let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };
        loop {
            match future.as_mut().poll(&mut cx) {
                core::task::Poll::Ready(output) => return output,
                core::task::Poll::Pending => core::hint::spin_loop(),
            }
        }
    }

    fn drive<F: core::future::Future>(future: F) -> F::Output {
        block_on(future)
    }

    fn with_pico_fixture<R>(
        f: impl FnOnce(
            &'static CounterClock,
            &'static mut [crate::observe::core::TapEvent; PICO_RING_EVENTS],
            &'static mut [u8; HOST_MEASURE_SLAB_BYTES],
        ) -> R,
    ) -> R {
        FIXTURE_CLOCK.with(|clock| {
            FIXTURE_TAP.with(|tap| {
                FIXTURE_SLAB.with(|slab| unsafe {
                    let tap = &mut *tap.get();
                    let slab = &mut *slab.get();
                    with_transport_state(|state| *state = PicoTransportState::new());
                    tap.fill(crate::observe::core::TapEvent::zero());
                    slab.fill(0);
                    f(
                        &*(clock as *const CounterClock),
                        &mut *(tap as *mut [crate::observe::core::TapEvent; PICO_RING_EVENTS]),
                        &mut *(slab as *mut [u8; HOST_MEASURE_SLAB_BYTES]),
                    )
                })
            })
        })
    }

    #[cfg(target_os = "macos")]
    fn current_thread_stack_bounds() -> StackBounds {
        unsafe {
            let thread = libc::pthread_self();
            let high = libc::pthread_get_stackaddr_np(thread) as usize;
            let size = libc::pthread_get_stacksize_np(thread);
            StackBounds {
                low: high.saturating_sub(size),
                high,
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn current_thread_stack_bounds() -> StackBounds {
        unsafe {
            let thread = libc::pthread_self();
            let mut attr = core::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
            let init = libc::pthread_getattr_np(thread, attr.as_mut_ptr());
            assert_eq!(init, 0, "pthread_getattr_np failed: {init}");
            let mut stack_addr = core::ptr::null_mut();
            let mut stack_size = 0usize;
            let stack =
                libc::pthread_attr_getstack(attr.as_mut_ptr(), &mut stack_addr, &mut stack_size);
            assert_eq!(stack, 0, "pthread_attr_getstack failed: {stack}");
            let mut guard_size = 0usize;
            let guard = libc::pthread_attr_getguardsize(attr.as_mut_ptr(), &mut guard_size);
            assert_eq!(guard, 0, "pthread_attr_getguardsize failed: {guard}");
            let destroy = libc::pthread_attr_destroy(attr.as_mut_ptr());
            assert_eq!(destroy, 0, "pthread_attr_destroy failed: {destroy}");
            let low = stack_addr as usize;
            StackBounds {
                low: low.saturating_add(guard_size),
                high: low.saturating_add(stack_size),
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn current_thread_stack_bounds() -> StackBounds {
        panic!("stack canary runtime metrics are only supported on macOS and Linux hosts")
    }

    #[inline(never)]
    fn current_stack_pointer() -> usize {
        let marker = 0u8;
        core::ptr::from_ref(&marker) as usize
    }

    unsafe fn initialize_stack_canary(bounds: StackBounds) {
        let fill_end = current_stack_pointer()
            .saturating_sub(STACK_CANARY_HEADROOM_BYTES)
            .clamp(bounds.low, bounds.high);
        if fill_end > bounds.low {
            unsafe {
                core::ptr::write_bytes(
                    bounds.low as *mut u8,
                    STACK_CANARY_BYTE,
                    fill_end.saturating_sub(bounds.low),
                );
            }
        }
    }

    fn measure_peak_stack_bytes(bounds: StackBounds) -> usize {
        let mut cursor = bounds.low;
        while cursor < bounds.high {
            let byte = unsafe { *(cursor as *const u8) };
            if byte != STACK_CANARY_BYTE {
                break;
            }
            cursor += 1;
        }
        bounds.high.saturating_sub(cursor)
    }

    #[inline(never)]
    fn run_attached_shape(
        route_scope_count: usize,
        expected_branch_labels: &'static [u8],
        expected_acks: &'static [u8],
        controller_program: fn() -> crate::g::advanced::RoleProgram<0>,
        worker_program: fn() -> crate::g::advanced::RoleProgram<1>,
        run: fn(&mut Endpoint<'_, 0>, &mut Endpoint<'_, 1>),
    ) -> RuntimeShapeMetrics {
        let bounds = current_thread_stack_bounds();
        unsafe {
            initialize_stack_canary(bounds);
        }

        assert_eq!(route_scope_count, expected_branch_labels.len());
        assert_eq!(route_scope_count, expected_acks.len());

        let mut runtime_metrics = None::<RuntimeShapeMetrics>;
        with_pico_fixture(|clock, tap_buf, slab| {
            let transport = PicoTransport;
            let controller_program = controller_program();
            let worker_program = worker_program();
            let kit = PicoKit::new(clock);
            let rv_id = kit
                .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                .expect("register rendezvous");
            let sid = SessionId::new(0x6000);
            let mut controller = kit
                .enter(rv_id, sid, &controller_program, NoBinding)
                .expect("enter controller");
            let mut worker = kit
                .enter(rv_id, sid, &worker_program, NoBinding)
                .expect("enter worker");

            run(&mut controller, &mut worker);
            assert!(
                with_transport_state(|state| state.roles.iter().all(|role| role.queue.len == 0)),
                "huge choreography runtime must drain every transport frame"
            );

            let runtime_snapshot = {
                let rv = kit
                    .inner
                    .get_local(&rv_id)
                    .expect("registered rendezvous must stay reachable");
                let sidecar_scratch_high_water_bytes = rv.runtime_sidecar_high_water_bytes();
                let live_endpoint_bytes = rv.live_endpoint_storage_bytes();
                RuntimeShapeMetrics {
                    slab_bytes: TARGET_PICO_SLAB_BYTES,
                    sidecar_scratch_high_water_bytes,
                    live_endpoint_bytes,
                    peak_live_slab_bytes: sidecar_scratch_high_water_bytes
                        .saturating_add(live_endpoint_bytes),
                    peak_stack_bytes: 0,
                }
            };
            runtime_metrics = Some(runtime_snapshot);
        });

        let mut runtime_metrics = runtime_metrics.expect("runtime metrics");
        runtime_metrics.peak_stack_bytes = measure_peak_stack_bytes(bounds);
        runtime_metrics
    }

    fn assert_pico_runtime_metrics(shape: &'static str, metrics: RuntimeShapeMetrics) {
        assert!(
            metrics.peak_stack_bytes <= HOST_STACK_BYTES,
            "{shape} peak stack bytes must fit within the 32 KiB host thread budget"
        );
        assert!(
            metrics.peak_live_slab_bytes <= HOST_MEASURE_SLAB_BYTES,
            "{shape} measured host live slab usage must fit within the host measurement slab"
        );
        println!(
            "pico-runtime shape={shape} slab_bytes={} sidecar_scratch_high_water_bytes={} live_endpoint_bytes={} peak_live_slab_bytes={} peak_stack_bytes={}",
            metrics.slab_bytes,
            metrics.sidecar_scratch_high_water_bytes,
            metrics.live_endpoint_bytes,
            metrics.peak_live_slab_bytes,
            metrics.peak_stack_bytes,
        );
    }

    #[test]
    #[ignore = "reported by pico smoke scripts in release mode"]
    fn pico_smoke_runtime_peak_metrics_route_heavy() {
        assert_pico_runtime_metrics(
            "route_heavy",
            run_attached_shape(
                huge_program::ROUTE_SCOPE_COUNT,
                &huge_program::EXPECTED_WORKER_BRANCH_LABELS,
                &huge_program::ACK_LABELS,
                huge_program::controller_program,
                huge_program::worker_program,
                huge_program::run,
            ),
        );
    }

    #[test]
    #[ignore = "reported by pico smoke scripts in release mode"]
    fn pico_smoke_runtime_peak_metrics_linear_heavy() {
        assert_pico_runtime_metrics(
            "linear_heavy",
            run_attached_shape(
                linear_program::ROUTE_SCOPE_COUNT,
                &linear_program::EXPECTED_WORKER_BRANCH_LABELS,
                &linear_program::ACK_LABELS,
                linear_program::controller_program,
                linear_program::worker_program,
                linear_program::run,
            ),
        );
    }

    #[test]
    #[ignore = "reported by pico smoke scripts in release mode"]
    fn pico_smoke_runtime_peak_metrics_fanout_heavy() {
        assert_pico_runtime_metrics(
            "fanout_heavy",
            run_attached_shape(
                fanout_program::ROUTE_SCOPE_COUNT,
                &fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
                &fanout_program::ACK_LABELS,
                fanout_program::controller_program,
                fanout_program::worker_program,
                fanout_program::run,
            ),
        );
    }
}
