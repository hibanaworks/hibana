#[cfg(all(test, feature = "std"))]
mod tests {
    extern crate self as hibana;

    use std::cell::UnsafeCell;

    use crate::test_support::large_choreography::{
        fanout_program, huge_program, linear_program, localside,
    };
    use crate::{
        Endpoint,
        integration::{
            SessionKitStorage,
            ids::{Lane, SessionId},
            runtime::{Config, CounterClock, DefaultLabelUniverse},
            transport::{
                FrameHeader, FrameLabel, Outgoing, ReceivedPayload, Transport, TransportError,
            },
            wire::Payload,
        },
    };

    type LargeChoreographyKit<'a> =
        SessionKitStorage<'a, LargeChoreographyTransport, DefaultLabelUniverse, CounterClock, 2>;

    const LARGE_CHOREOGRAPHY_RING_EVENTS: usize = 128;
    const TARGET_LARGE_CHOREOGRAPHY_SLAB_BYTES: usize = 32_768;
    const HOST_MEASURE_SLAB_BYTES: usize = 262_144;
    const HOST_STACK_BYTES: usize = 32 * 1024;
    const STACK_CANARY_BYTE: u8 = 0xA5;
    const STACK_CANARY_HEADROOM_BYTES: usize = 512;
    const QUEUE_CAPACITY: usize = 16;
    const PAYLOAD_CAPACITY: usize = 96;

    fn retain_large_choreography_fixture_symbols() {
        let _ = huge_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ =
            huge_program::controller_program as fn() -> crate::integration::program::RoleProgram<0>;
        let _ = linear_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = linear_program::controller_program
            as fn() -> crate::integration::program::RoleProgram<0>;
        let _ = fanout_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = fanout_program::controller_program
            as fn() -> crate::integration::program::RoleProgram<0>;
        let _ =
            localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
    }

    #[test]
    fn large_choreography_fixture_symbols_are_reachable() {
        retain_large_choreography_fixture_symbols();
    }

    std::thread_local! {
        static FIXTURE_CLOCK: CounterClock = const { CounterClock::new() };
        static FIXTURE_TAP: UnsafeCell<[crate::observe::core::TapEvent; LARGE_CHOREOGRAPHY_RING_EVENTS]> =
            const { UnsafeCell::new([crate::observe::core::TapEvent::zero(); LARGE_CHOREOGRAPHY_RING_EVENTS]) };
        static FIXTURE_SLAB: UnsafeCell<[u8; HOST_MEASURE_SLAB_BYTES]> =
            const { UnsafeCell::new([0u8; HOST_MEASURE_SLAB_BYTES]) };
        static FIXTURE_TRANSPORT: UnsafeCell<LargeChoreographyTransportState> =
            const { UnsafeCell::new(LargeChoreographyTransportState::new()) };
    }

    #[derive(Clone, Copy, Debug)]
    struct RuntimeShapeMetrics {
        slab_bytes: usize,
        sidecar_scratch_high_water_bytes: usize,
        image_frontier_bytes: usize,
        frontier_workspace_bytes: usize,
        live_endpoint_bytes: usize,
        peak_live_slab_bytes: usize,
        localside_peak_stack_bytes: usize,
        peak_stack_bytes: usize,
    }

    #[derive(Clone, Copy, Debug)]
    struct StackBounds {
        low: usize,
        high: usize,
    }

    #[derive(Clone, Copy)]
    struct FrameOwned {
        meta: u32,
        len: u8,
        payload: [u8; PAYLOAD_CAPACITY],
    }

    impl FrameOwned {
        const fn empty() -> Self {
            Self {
                meta: 0,
                len: 0,
                payload: [0; PAYLOAD_CAPACITY],
            }
        }

        fn fill_from_outgoing(&mut self, source_role: u8, outgoing: Outgoing<'_>) {
            let bytes = outgoing.payload().as_bytes();
            assert!(
                bytes.len() <= PAYLOAD_CAPACITY,
                "large choreography runtime payload exceeds fixed capacity"
            );
            self.meta = ((outgoing.lane() as u32) << 16)
                | ((source_role as u32) << 8)
                | (outgoing.frame_label().raw() as u32);
            self.len = bytes.len() as u8;
            self.payload[..bytes.len()].copy_from_slice(bytes);
        }

        #[inline(always)]
        const fn lane(&self) -> u8 {
            (self.meta >> 16) as u8
        }

        #[inline(always)]
        const fn source_role(&self) -> u8 {
            (self.meta >> 8) as u8
        }

        #[inline(always)]
        const fn frame_label(&self) -> u8 {
            self.meta as u8
        }

        fn as_slice(&self) -> &[u8] {
            &self.payload[..self.len as usize]
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

        fn push_back_outgoing(&mut self, source_role: u8, outgoing: Outgoing<'_>) {
            assert!(
                self.len < QUEUE_CAPACITY,
                "large choreography runtime transport queue capacity exceeded"
            );
            let idx = (self.head + self.len) % QUEUE_CAPACITY;
            self.items[idx].fill_from_outgoing(source_role, outgoing);
            self.len += 1;
        }

        fn push_front_copy(&mut self, item: &FrameOwned) {
            assert!(
                self.len < QUEUE_CAPACITY,
                "large choreography runtime transport queue capacity exceeded"
            );
            self.head = if self.head == 0 {
                QUEUE_CAPACITY - 1
            } else {
                self.head - 1
            };
            self.items[self.head] = *item;
            self.len += 1;
        }

        fn pop_front_into(&mut self, dst: &mut FrameOwned) -> bool {
            if self.len == 0 {
                return false;
            }
            let idx = self.head;
            self.head = (self.head + 1) % QUEUE_CAPACITY;
            self.len -= 1;
            *dst = self.items[idx];
            true
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
    struct LargeChoreographyTransportState {
        roles: [RoleState; 2],
        inflight: [FrameOwned; 2],
        inflight_set: [bool; 2],
    }

    impl LargeChoreographyTransportState {
        const fn new() -> Self {
            Self {
                roles: [RoleState::new(), RoleState::new()],
                inflight: [FrameOwned::empty(), FrameOwned::empty()],
                inflight_set: [false, false],
            }
        }

        fn role_mut(&mut self, role: u8) -> &mut RoleState {
            match role {
                0 | 1 => &mut self.roles[role as usize],
                _ => panic!("large choreography runtime transport role out of range"),
            }
        }

        fn role(&self, role: u8) -> &RoleState {
            match role {
                0 | 1 => &self.roles[role as usize],
                _ => panic!("large choreography runtime transport role out of range"),
            }
        }
    }

    #[derive(Clone, Copy)]
    struct LargeChoreographyTransport;

    struct LargeChoreographyTx;

    struct LargeChoreographyRx {
        role: u8,
        current: bool,
    }

    fn with_transport_state<R>(f: impl FnOnce(&mut LargeChoreographyTransportState) -> R) -> R {
        FIXTURE_TRANSPORT.with(|state| unsafe { f(&mut *state.get()) })
    }

    #[inline(always)]
    const fn large_choreography_source_role(peer: u8) -> u8 {
        match peer {
            0 => 1,
            1 => 0,
            _ => 0,
        }
    }

    #[inline(always)]
    fn large_choreography_header(frame: &FrameOwned, peer_role: u8) -> FrameHeader {
        FrameHeader::new(
            SessionId::new(0x6000),
            Lane::new(frame.lane() as u32),
            frame.source_role(),
            peer_role,
            FrameLabel::new(frame.frame_label()),
        )
    }

    impl Transport for LargeChoreographyTransport {
        type Error = TransportError;
        type Tx<'a>
            = LargeChoreographyTx
        where
            Self: 'a;
        type Rx<'a>
            = LargeChoreographyRx
        where
            Self: 'a;

        fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
            let local_role = port.local_role();
            with_transport_state(|state| {
                let _ = state.role(local_role);
            });
            (
                LargeChoreographyTx,
                LargeChoreographyRx {
                    role: local_role,
                    current: false,
                },
            )
        }

        fn poll_send<'a, 'f>(
            &self,
            _tx: &'a mut Self::Tx<'a>,
            outgoing: Outgoing<'f>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            with_transport_state(|state| {
                state
                    .role_mut(outgoing.peer())
                    .queue
                    .push_back_outgoing(large_choreography_source_role(outgoing.peer()), outgoing);
            });
            core::task::Poll::Ready(Ok(()))
        }

        #[inline(always)]
        fn poll_recv<'a>(
            &'a self,
            rx: &'a mut Self::Rx<'a>,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<ReceivedPayload<'a>, Self::Error>> {
            let frame = with_transport_state(|state| {
                let idx = rx.role as usize;
                if rx.current {
                    state.inflight_set[idx] = false;
                    rx.current = false;
                }
                if !state.inflight_set[idx] {
                    if !state.roles[idx]
                        .queue
                        .pop_front_into(&mut state.inflight[idx])
                    {
                        return None;
                    }
                    state.inflight_set[idx] = true;
                }
                Some(&state.inflight[idx] as *const FrameOwned)
            });
            let Some(frame) = frame else {
                return core::task::Poll::Pending;
            };
            rx.current = true;
            let frame = unsafe { &*frame };
            core::task::Poll::Ready(Ok(ReceivedPayload::frame(
                large_choreography_header(frame, rx.role),
                Payload::new(frame.as_slice()),
            )))
        }

        fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

        fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
            if rx.current {
                with_transport_state(|state| {
                    let idx = rx.role as usize;
                    if state.inflight_set[idx] {
                        state.roles[idx].queue.push_front_copy(&state.inflight[idx]);
                        state.inflight_set[idx] = false;
                    }
                });
                rx.current = false;
            }
            Ok(())
        }
    }

    fn with_large_choreography_fixture<R>(
        f: impl FnOnce(
            &'static CounterClock,
            &'static mut [crate::observe::core::TapEvent; LARGE_CHOREOGRAPHY_RING_EVENTS],
            &'static mut [u8; HOST_MEASURE_SLAB_BYTES],
        ) -> R,
    ) -> R {
        FIXTURE_CLOCK.with(|clock| {
            FIXTURE_TAP.with(|tap| {
                FIXTURE_SLAB.with(|slab| unsafe {
                    let tap = &mut *tap.get();
                    let slab = &mut *slab.get();
                    with_transport_state(|state| *state = LargeChoreographyTransportState::new());
                    tap.fill(crate::observe::core::TapEvent::zero());
                    slab.fill(0);
                    f(
                        &*(clock as *const CounterClock),
                        &mut *(tap as *mut [crate::observe::core::TapEvent;
                            LARGE_CHOREOGRAPHY_RING_EVENTS]),
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
        controller_program: fn() -> crate::integration::program::RoleProgram<0>,
        worker_program: fn() -> crate::integration::program::RoleProgram<1>,
        run: fn(&mut Endpoint<'_, 0>, &mut Endpoint<'_, 1>),
    ) -> RuntimeShapeMetrics {
        let controller_program_image = controller_program();
        let worker_program_image = worker_program();
        let bounds = current_thread_stack_bounds();
        unsafe {
            initialize_stack_canary(bounds);
        }

        assert_eq!(route_scope_count, expected_branch_labels.len());
        assert_eq!(route_scope_count, expected_acks.len());

        let mut runtime_metrics = None::<RuntimeShapeMetrics>;
        with_large_choreography_fixture(|_clock, tap_buf, slab| {
            // The host test fixture itself can consume more stack than the small-target
            // budget. Measure only additional runtime stack below this point.
            let baseline_peak_stack_bytes = measure_peak_stack_bytes(bounds);
            let transport = LargeChoreographyTransport;
            let mut kit_storage = LargeChoreographyKit::uninit();
            let kit = kit_storage.init();
            let rv = kit
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::new()),
                    transport.clone(),
                )
                .expect("register rendezvous");
            let sid = SessionId::new(0x6000);
            let mut controller = rv
                .session(sid)
                .role(&controller_program_image)
                .enter()
                .expect("enter controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_program_image)
                .enter()
                .expect("enter worker");
            let attach_peak_stack_bytes = measure_peak_stack_bytes(bounds)
                .saturating_sub(baseline_peak_stack_bytes)
                .saturating_add(STACK_CANARY_HEADROOM_BYTES);

            unsafe {
                initialize_stack_canary(bounds);
            }
            let localside_baseline_peak_stack_bytes = measure_peak_stack_bytes(bounds);

            run(&mut controller, &mut worker);
            assert!(
                with_transport_state(|state| state.roles.iter().all(|role| role.queue.len == 0)),
                "huge choreography runtime must drain every transport frame"
            );

            let runtime_snapshot = {
                let local_rv = kit
                    .inner
                    .get_local(&rv.rv)
                    .expect("registered rendezvous must stay reachable");
                let sidecar_scratch_high_water_bytes = local_rv.runtime_sidecar_high_water_bytes();
                let image_frontier_bytes = local_rv.runtime_image_frontier_bytes();
                let frontier_workspace_bytes = local_rv.runtime_frontier_workspace_bytes();
                let live_endpoint_bytes = local_rv.live_endpoint_storage_bytes();
                RuntimeShapeMetrics {
                    slab_bytes: TARGET_LARGE_CHOREOGRAPHY_SLAB_BYTES,
                    sidecar_scratch_high_water_bytes,
                    image_frontier_bytes,
                    frontier_workspace_bytes,
                    live_endpoint_bytes,
                    peak_live_slab_bytes: sidecar_scratch_high_water_bytes
                        .saturating_add(live_endpoint_bytes),
                    localside_peak_stack_bytes: 0,
                    peak_stack_bytes: 0,
                }
            };
            let localside_raw_peak_stack_bytes = measure_peak_stack_bytes(bounds);
            let localside_peak_stack_bytes = localside_raw_peak_stack_bytes
                .saturating_sub(localside_baseline_peak_stack_bytes)
                .saturating_add(STACK_CANARY_HEADROOM_BYTES);
            let mut runtime_snapshot = runtime_snapshot;
            runtime_snapshot.localside_peak_stack_bytes = localside_peak_stack_bytes;
            runtime_snapshot.peak_stack_bytes =
                core::cmp::max(attach_peak_stack_bytes, localside_peak_stack_bytes);
            runtime_metrics = Some(runtime_snapshot);
        });

        runtime_metrics.expect("runtime metrics")
    }

    fn assert_large_choreography_runtime_metrics(
        shape: &'static str,
        metrics: RuntimeShapeMetrics,
    ) {
        assert!(
            metrics.peak_stack_bytes <= HOST_STACK_BYTES,
            "{shape} peak stack bytes must fit within the 32 KiB host thread budget: {} > {}",
            metrics.peak_stack_bytes,
            HOST_STACK_BYTES
        );
        assert!(
            metrics.peak_live_slab_bytes <= HOST_MEASURE_SLAB_BYTES,
            "{shape} measured host live slab usage must fit within the host measurement slab"
        );
        println!(
            "large-choreography-runtime shape={shape} slab_bytes={} sidecar_scratch_high_water_bytes={} image_frontier_bytes={} frontier_workspace_bytes={} live_endpoint_bytes={} peak_live_slab_bytes={} localside_peak_stack_bytes={} peak_stack_bytes={}",
            metrics.slab_bytes,
            metrics.sidecar_scratch_high_water_bytes,
            metrics.image_frontier_bytes,
            metrics.frontier_workspace_bytes,
            metrics.live_endpoint_bytes,
            metrics.peak_live_slab_bytes,
            metrics.localside_peak_stack_bytes,
            metrics.peak_stack_bytes,
        );
    }

    #[test]
    #[ignore = "reported by large choreography measurement scripts in release mode"]
    fn large_choreography_runtime_peak_metrics_route_heavy() {
        assert_large_choreography_runtime_metrics(
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
    #[ignore = "reported by large choreography measurement scripts in release mode"]
    fn large_choreography_runtime_peak_metrics_linear_heavy() {
        assert_large_choreography_runtime_metrics(
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
    #[ignore = "reported by large choreography measurement scripts in release mode"]
    fn large_choreography_runtime_peak_metrics_fanout_heavy() {
        assert_large_choreography_runtime_metrics(
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
