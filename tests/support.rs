use hibana::observe::TapEvent;
use hibana::runtime::config::CounterClock;
use hibana::runtime::consts::RING_EVENTS;
use std::boxed::Box;

pub fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}

#[allow(dead_code)]
pub fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

#[allow(dead_code)]
pub fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

// ---------------------------------------------------------------------------
// Test harness builder
// ---------------------------------------------------------------------------

use hibana::{
    control::types::RendezvousId,
    rendezvous::Rendezvous,
    runtime::{SessionCluster, config::Config, consts::DefaultLabelUniverse},
    transport::Transport,
};

/// Default slab size for test harnesses.
pub const DEFAULT_SLAB_SIZE: usize = 2048;

/// Builder for creating test harnesses with reduced boilerplate.
///
/// # Example
///
/// ```ignore
/// let harness = TestHarnessBuilder::new()
///     .with_rendezvous(transport.clone())
///     .build();
/// let (cluster, rv_id) = harness.into_parts();
/// ```
#[allow(dead_code)]
pub struct TestHarnessBuilder<T: Transport + Clone + Default + 'static> {
    slab_size: usize,
    _transport: core::marker::PhantomData<T>,
}

#[allow(dead_code)]
impl<T: Transport + Clone + Default + 'static> TestHarnessBuilder<T> {
    /// Create a new test harness builder with default settings.
    pub fn new() -> Self {
        Self {
            slab_size: DEFAULT_SLAB_SIZE,
            _transport: core::marker::PhantomData,
        }
    }

    /// Set the slab size for the rendezvous configuration.
    pub fn with_slab_size(mut self, size: usize) -> Self {
        self.slab_size = size;
        self
    }

    /// Build a cluster with a single rendezvous using the given transport.
    pub fn build_with_transport(self, transport: T) -> TestHarness<T> {
        let clock = leak_clock();
        let cluster: &'static SessionCluster<'static, T, DefaultLabelUniverse, CounterClock, 4> =
            Box::leak(Box::new(SessionCluster::new(clock)));

        let config = Config::new(leak_tap_storage(), leak_slab(self.slab_size));
        let rendezvous: Rendezvous<'_, '_, T, DefaultLabelUniverse, CounterClock> =
            Rendezvous::from_config(config, transport.clone());

        let rv_id = cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");

        TestHarness {
            cluster,
            rv_id,
            transport,
        }
    }

    /// Build a cluster with a single rendezvous using the default transport.
    pub fn build(self) -> TestHarness<T> {
        self.build_with_transport(T::default())
    }
}

impl<T: Transport + Clone + Default + 'static> Default for TestHarnessBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A test harness containing a cluster and its associated rendezvous.
#[allow(dead_code)]
pub struct TestHarness<T: Transport + 'static> {
    pub cluster: &'static SessionCluster<'static, T, DefaultLabelUniverse, CounterClock, 4>,
    pub rv_id: RendezvousId,
    pub transport: T,
}

#[allow(dead_code)]
impl<T: Transport + 'static> TestHarness<T> {
    /// Get a reference to the cluster.
    pub fn cluster(
        &self,
    ) -> &'static SessionCluster<'static, T, DefaultLabelUniverse, CounterClock, 4> {
        self.cluster
    }

    /// Get the rendezvous ID.
    pub fn rv_id(&self) -> RendezvousId {
        self.rv_id
    }

    /// Get a reference to the transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Decompose the harness into its parts.
    pub fn into_parts(
        self,
    ) -> (
        &'static SessionCluster<'static, T, DefaultLabelUniverse, CounterClock, 4>,
        RendezvousId,
        T,
    ) {
        (self.cluster, self.rv_id, self.transport)
    }
}

/// Convenience function to run a test with a large stack.
///
/// Many hibana tests require substantial stack space due to deep recursion
/// in the typestate machinery. This helper spawns a thread with a large stack
/// (defaults to 32MB, overridable via HIBANA_TEST_STACK/RUST_MIN_STACK).
#[allow(dead_code)]
pub fn run_with_large_stack<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    fn env_stack_size() -> usize {
        const DEFAULT: usize = 32 * 1024 * 1024;
        fn parse_var(key: &str) -> Option<usize> {
            std::env::var(key).ok()?.parse().ok()
        }
        parse_var("HIBANA_TEST_STACK")
            .or_else(|| parse_var("RUST_MIN_STACK"))
            .map(|size| size.max(DEFAULT))
            .unwrap_or(DEFAULT)
    }

    std::thread::Builder::new()
        .stack_size(env_stack_size())
        .spawn(f)
        .expect("spawn large-stack thread")
        .join()
        .expect("join large-stack thread")
}

/// Async version of run_with_large_stack.
///
/// Spawns a thread with a large stack and runs a tokio runtime inside it.
/// NOTE: This is NOT truly async - it blocks while the spawned thread runs.
/// Use this only when you need large stack space for hibana's typestate machinery.
#[allow(dead_code)]
pub fn run_with_large_stack_async<F, Fut, R>(f: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    use std::sync::mpsc;

    fn env_stack_size() -> usize {
        const DEFAULT: usize = 32 * 1024 * 1024;
        fn parse_var(key: &str) -> Option<usize> {
            std::env::var(key).ok()?.parse().ok()
        }
        parse_var("HIBANA_TEST_STACK")
            .or_else(|| parse_var("RUST_MIN_STACK"))
            .map(|size| size.max(DEFAULT))
            .unwrap_or(DEFAULT)
    }

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(env_stack_size())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            let result = rt.block_on(f());
            let _ = tx.send(result);
        })
        .expect("spawn large-stack thread")
        .join()
        .expect("join large-stack thread");

    rx.recv().expect("receive result from large-stack thread")
}
