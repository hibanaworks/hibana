//! Transport binding layer for hibana choreography.
//!
//! This module provides a protocol-agnostic binding API that connects hibana's
//! flow-centric choreography to underlying transport mechanisms (QUIC streams,
//! Raft RPCs, etc.) without exposing transport details to application code.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Application (uses only hibana flow API)                         │
//! │   endpoint.flow::<M>()?.send(&msg).await                        │
//! └─────────────────────────────────────────────────────────────────┘
//!                              ↓
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ BindingSlot (protocol-specific binder, e.g., QuicBinder)        │
//! │   - Receives SendMetadata from choreography                     │
//! │   - Derives a deterministic action from direction + is_control  │
//! │   - Executes wire operations                                    │
//! └─────────────────────────────────────────────────────────────────┘
//!                              ↓
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Wire (QUIC STREAM/DATAGRAM, Raft RPC, etc.)                     │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Design Philosophy
//!
//! Protocol binders (e.g., `hibana_quic::QuicBinder`) use choreography metadata
//! to **deterministically derive** transport actions without manual configuration:
//!
//! | LocalDirection | is_control | Typical Action |
//! |----------------|------------|----------------|
//! | Send           | false      | Write          |
//! | Send           | true       | WriteFinish    |
//! | Recv           | -          | Read           |
//! | Local          | true       | None (skip)    |
//!
//! This mapping does not consult transport state or heuristics; exceptions can be
//! handled via protocol-specific override APIs.
//!
//! # Key Components
//!
//! - [`SendMetadata`]: Choreography metadata for send operations
//! - [`IncomingClassification`]: Classification for incoming data
//! - [`BindingSlot`]: Trait for protocol-specific binders
//! - [`ChannelStore`]: Label/instance → Channel mappings (no_alloc by default)
//! - [`NoBinding`]: Zero-cost default when binding is not needed

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::RwLock;

use crate::transport::context::TransportContextProvider;

// =============================================================================
// Channel: Opaque handle to a logical channel
// =============================================================================

/// Direction of a logical channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelDirection {
    /// Bidirectional channel (e.g., QUIC bidi stream)
    Bidirectional,
    /// Send-only channel (e.g., QUIC uni stream, outbound)
    SendOnly,
    /// Receive-only channel (e.g., QUIC uni stream, inbound)
    RecvOnly,
}

/// Opaque handle to a logical channel.
///
/// The actual representation is transport-specific:
/// - QUIC: wraps a StreamId (u64)
/// - Raft: might wrap an RPC call ID
/// - Other: custom channel identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Channel(pub u64);

impl Channel {
    /// Create a channel from a raw ID.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw channel ID.
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

/// Key for channel registry: (label, instance).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelKey {
    /// Logical label (protocol-defined)
    pub label: u8,
    /// Instance within label (for multi-channel, e.g., request N)
    pub instance: u16,
}

impl ChannelKey {
    /// Create a new channel key.
    pub const fn new(label: u8, instance: u16) -> Self {
        Self { label, instance }
    }
}

// =============================================================================
// ChannelStore: label/instance → channel mapping
// =============================================================================

/// Error reported by a channel store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelStoreError {
    /// The store has no capacity to register a new entry.
    Full,
}

/// Storage abstraction for logical channels.
pub trait ChannelStore {
    /// Register a channel for a given key.
    fn register(&mut self, key: ChannelKey, channel: Channel) -> Result<(), ChannelStoreError>;

    /// Look up a channel by key.
    fn get(&self, key: ChannelKey) -> Option<Channel>;

    /// Look up a key by channel (reverse lookup for demux).
    fn get_key(&self, channel: Channel) -> Option<ChannelKey>;

    /// Get or allocate the next instance for a label.
    fn next_instance(&mut self, label: u8) -> Result<u16, ChannelStoreError>;

    /// Get the most recently allocated instance for a label, if any.
    fn current_instance(&self, label: u8) -> Option<u16>;

    /// Unregister a channel.
    fn unregister(&mut self, channel: Channel);

    /// Clear all registrations.
    fn clear(&mut self);
}

/// Fixed-capacity, allocator-free channel store.
///
/// This implementation is intended for no_alloc environments. Capacity is
/// fixed at compile time. When full, registration returns `ChannelStoreError::Full`.
#[derive(Clone, Copy, Debug)]
pub struct ArrayChannelStore<const N: usize> {
    entries: [Option<(ChannelKey, Channel)>; N],
    counters: [Option<(u8, u16)>; N],
}

impl<const N: usize> ArrayChannelStore<N> {
    /// Create a new empty store.
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            counters: [None; N],
        }
    }

    fn find_entry_index(&self, key: ChannelKey) -> Option<usize> {
        let mut i = 0;
        while i < N {
            if let Some((k, _)) = self.entries[i] {
                if k == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn find_reverse_index(&self, channel: Channel) -> Option<usize> {
        let mut i = 0;
        while i < N {
            if let Some((_, ch)) = self.entries[i] {
                if ch == channel {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn reserve_slot(&mut self) -> Result<usize, ChannelStoreError> {
        let mut i = 0;
        while i < N {
            if self.entries[i].is_none() {
                return Ok(i);
            }
            i += 1;
        }
        Err(ChannelStoreError::Full)
    }

    fn find_counter_index(&self, label: u8) -> Option<usize> {
        let mut i = 0;
        while i < N {
            if let Some((l, _)) = self.counters[i] {
                if l == label {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn reserve_counter_slot(&mut self) -> Result<usize, ChannelStoreError> {
        let mut i = 0;
        while i < N {
            if self.counters[i].is_none() {
                return Ok(i);
            }
            i += 1;
        }
        Err(ChannelStoreError::Full)
    }
}

impl<const N: usize> Default for ArrayChannelStore<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ChannelStore for ArrayChannelStore<N> {
    fn register(&mut self, key: ChannelKey, channel: Channel) -> Result<(), ChannelStoreError> {
        if let Some(idx) = self.find_entry_index(key) {
            self.entries[idx] = Some((key, channel));
            return Ok(());
        }

        let idx = self.reserve_slot()?;
        self.entries[idx] = Some((key, channel));
        Ok(())
    }

    fn get(&self, key: ChannelKey) -> Option<Channel> {
        self.find_entry_index(key)
            .and_then(|idx| self.entries[idx].map(|(_, ch)| ch))
    }

    fn get_key(&self, channel: Channel) -> Option<ChannelKey> {
        self.find_reverse_index(channel)
            .and_then(|idx| self.entries[idx].map(|(k, _)| k))
    }

    fn next_instance(&mut self, label: u8) -> Result<u16, ChannelStoreError> {
        if let Some(idx) = self.find_counter_index(label) {
            if let Some((_, next)) = &mut self.counters[idx] {
                let current = *next;
                *next = next.wrapping_add(1);
                return Ok(current);
            }
        }

        let idx = self.reserve_counter_slot()?;
        self.counters[idx] = Some((label, 1));
        Ok(0)
    }

    fn current_instance(&self, label: u8) -> Option<u16> {
        self.find_counter_index(label).and_then(|idx| {
            if let Some((_, n)) = self.counters[idx] {
                n.checked_sub(1)
            } else {
                None
            }
        })
    }

    fn unregister(&mut self, channel: Channel) {
        if let Some(idx) = self.find_reverse_index(channel) {
            self.entries[idx] = None;
        }
    }

    fn clear(&mut self) {
        let mut i = 0;
        while i < N {
            self.entries[i] = None;
            self.counters[i] = None;
            i += 1;
        }
    }
}

/// HashMap-backed store for std environments.
#[cfg(feature = "std")]
#[derive(Debug, Default)]
pub struct StdChannelStore {
    forward: RwLock<HashMap<ChannelKey, Channel>>,
    reverse: RwLock<HashMap<Channel, ChannelKey>>,
    next_instance: RwLock<HashMap<u8, u16>>,
}

#[cfg(feature = "std")]
impl StdChannelStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(feature = "std")]
impl ChannelStore for StdChannelStore {
    fn register(&mut self, key: ChannelKey, channel: Channel) -> Result<(), ChannelStoreError> {
        {
            let mut fwd = self.forward.write().unwrap();
            fwd.insert(key, channel);
        }
        {
            let mut rev = self.reverse.write().unwrap();
            rev.insert(channel, key);
        }
        Ok(())
    }

    fn get(&self, key: ChannelKey) -> Option<Channel> {
        let fwd = self.forward.read().unwrap();
        fwd.get(&key).copied()
    }

    fn get_key(&self, channel: Channel) -> Option<ChannelKey> {
        let rev = self.reverse.read().unwrap();
        rev.get(&channel).copied()
    }

    fn next_instance(&mut self, label: u8) -> Result<u16, ChannelStoreError> {
        let mut next = self.next_instance.write().unwrap();
        let instance = *next.get(&label).unwrap_or(&0);
        next.insert(label, instance + 1);
        Ok(instance)
    }

    fn current_instance(&self, label: u8) -> Option<u16> {
        let next = self.next_instance.read().unwrap();
        next.get(&label).copied().and_then(|n| n.checked_sub(1))
    }

    fn unregister(&mut self, channel: Channel) {
        let key = {
            let mut rev = self.reverse.write().unwrap();
            rev.remove(&channel)
        };
        if let Some(key) = key {
            let mut fwd = self.forward.write().unwrap();
            fwd.remove(&key);
        }
    }

    fn clear(&mut self) {
        self.forward.write().unwrap().clear();
        self.reverse.write().unwrap().clear();
        self.next_instance.write().unwrap().clear();
    }
}

// =============================================================================
// TransportOpsError: Common error type for transport operations
// =============================================================================

/// Error from transport operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportOpsError {
    /// Channel not found in registry
    ChannelNotFound,
    /// Failed to open channel
    OpenFailed,
    /// Write failed (partial write)
    WriteFailed { expected: usize, actual: usize },
    /// Channel already finished
    AlreadyFinished,
    /// Invalid operation for current channel state
    InvalidState,
    /// Protocol-specific error code
    Protocol(u64),
    /// Channel store is at capacity
    ChannelStoreFull,
}

impl core::fmt::Display for TransportOpsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ChannelNotFound => write!(f, "channel not found"),
            Self::OpenFailed => write!(f, "failed to open channel"),
            Self::WriteFailed { expected, actual } => {
                write!(
                    f,
                    "write failed: expected {} bytes, wrote {}",
                    expected, actual
                )
            }
            Self::AlreadyFinished => write!(f, "channel already finished"),
            Self::InvalidState => write!(f, "invalid operation for channel state"),
            Self::Protocol(code) => write!(f, "protocol error: {}", code),
            Self::ChannelStoreFull => write!(f, "channel store is full"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TransportOpsError {}

impl From<ChannelStoreError> for TransportOpsError {
    fn from(err: ChannelStoreError) -> Self {
        match err {
            ChannelStoreError::Full => TransportOpsError::ChannelStoreFull,
        }
    }
}

// =============================================================================
// SendMetadata: Choreography metadata for deterministic action mapping
// =============================================================================

/// Direction of a send operation from the local role's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalDirection {
    /// Sending to a peer
    Send,
    /// Receiving from a peer
    Recv,
    /// Local operation (self-send, e.g., CanonicalControl)
    Local,
}

/// Disposition returned by `BindingSlot::on_send_with_meta()`.
///
/// Indicates whether the binder has handled wire transmission or expects
/// core to call `transport.send()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendDisposition {
    /// Core should call `transport.send()` with the payload.
    ///
    /// Use when the binder only performs side effects (state updates, channel
    /// registration) but does not transmit the payload itself.
    BypassTransport,

    /// Core should NOT call `transport.send()`.
    ///
    /// Use when the binder has already transmitted the payload via its own
    /// mechanism (e.g., H3 framing, QPACK encoding).
    Handled,
}

/// Metadata for a send operation, derived from choreography.
///
/// This struct contains all the information needed for a protocol binder
/// to determine the default transport action without requiring
/// manual configuration or transport-state heuristics.
///
/// # Default Action Rules
///
/// Protocol binders use these fields to deterministically map actions:
///
/// | direction | is_control | Typical Action |
/// |-----------|------------|----------------|
/// | Send      | false      | Write          |
/// | Send      | true       | WriteFinish    |
/// | Recv      | -          | Read           |
/// | Local     | true       | None (skip)    |
///
/// # Lane Semantics
///
/// The `lane` field is a **logical lane** (u8) defined by the choreography.
/// Binders translate this to physical lanes via `map_lane()`. This separation
/// enables:
/// - Multiple sessions on the same transport with different lane offsets
/// - H3 control (lane 0/1) vs request/response (lane 2) separation
#[derive(Clone, Copy, Debug)]
pub struct SendMetadata {
    /// Effect index (stable identifier for the choreography step)
    pub eff_index: u16,
    /// Message label
    pub label: u8,
    /// Target peer role
    pub peer: u8,
    /// Logical lane for this message (program-defined, 0-indexed)
    pub lane: u8,
    /// Direction from local perspective
    pub direction: LocalDirection,
    /// Whether this is a control message
    pub is_control: bool,
}

// =============================================================================
// IncomingClassification: Result of classifying incoming data
// =============================================================================

/// Result of classifying an incoming frame.
///
/// This is returned by `BindingSlot::poll_incoming()` and contains
/// all information needed by hibana to select the correct route arm,
/// **without requiring an edge ordinal**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncomingClassification {
    /// Logical label for the incoming data (maps to route arm)
    pub label: u8,
    /// Instance within the label (for multi-channel)
    pub instance: u16,
    /// Whether this frame includes FIN/end-of-stream
    pub has_fin: bool,
    /// Channel handle for subsequent read operations
    pub channel: Channel,
}

// =============================================================================
// BindingSlot: Protocol-agnostic binding trait
// =============================================================================

/// Slot trait for transport binding on CursorEndpoint.
///
/// Protocol implementations (e.g., `hibana_quic::QuicBinder`) implement this
/// trait to connect hibana's flow operations to their wire format.
///
/// When `B = NoBinding` (the default), all methods inline to no-ops at
/// compile time, providing zero runtime overhead.
///
/// # Send Flow
///
/// `on_send_with_meta()` receives choreography metadata and payload, allowing
/// the binder to deterministically select the default transport action. The
/// return value (`SendDisposition`) tells core whether to call `transport.send()`.
///
/// # Receive Flow
///
/// The receive flow uses a lane-aware two-step approach:
///
/// 1. **Classification** (`poll_incoming_for_lane`): Called by `offer()` to
///    determine which route arm to select. Only returns classifications for
///    the specified logical lane.
///
/// 2. **Reading** (`on_recv`): Called after arm selection to read the actual
///    data. The channel comes from the classification.
///
/// Additionally, implementations may provide a `TransportContextProvider` for
/// resolver functions to query protocol-specific state without global registries.
///
/// # Safety
///
/// Implementors **must** guarantee that `on_send_with_meta()` does not block on
/// network I/O. This is a core invariant that enables `g::par` to execute correctly
/// with single-cursor sequential execution. Violating this contract breaks hibana's
/// AMPST progress guarantees.
///
/// Specifically, `on_send_with_meta()` must:
/// - Only perform synchronous buffer enqueue operations
/// - Return `Err` for backpressure (never block/busy-wait)
/// - Complete in bounded time without awaiting external events
pub unsafe trait BindingSlot {
    /// Called when a send operation occurs with choreography metadata.
    ///
    /// # Non-Blocking Send Contract (Core Specification)
    ///
    /// **This method MUST NOT await network I/O.** Implementations may only:
    /// - Perform synchronous buffer enqueue (fixed-size buffer, etc.)
    /// - Return `Err` for backpressure (NOT block)
    ///
    /// This contract enables `g::par` to work correctly with single-cursor
    /// sequential execution. The Transport layer handles actual network I/O
    /// asynchronously. Binders that violate this contract are outside the
    /// hibana model and not supported for interop.
    ///
    /// # Return Value (SendDisposition)
    ///
    /// - `BypassTransport`: Core will call `transport.send()` with the payload.
    /// - `Handled`: Core will NOT call `transport.send()` (binder did wire I/O).
    ///
    /// **Note**: If `meta.direction == Local`, core will skip `transport.send()`
    /// regardless of the disposition (Local messages never go to wire).
    ///
    /// # Action Mapping
    ///
    /// Protocol binders use `meta.direction`, `meta.is_control`, and `meta.lane`
    /// to map to the default transport action:
    ///
    /// | direction | is_control | Typical Action |
    /// |-----------|------------|----------------|
    /// | Send      | false      | Write          |
    /// | Send      | true       | WriteFinish    |
    /// | Recv      | -          | Read (no-op)   |
    /// | Local     | true       | None (skip)    |
    fn on_send_with_meta(
        &mut self,
        meta: SendMetadata,
        payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError>;

    /// Poll for incoming data classification on a specific logical lane.
    ///
    /// Called by `offer()` to determine which route arm to select. Only returns
    /// classifications for data destined to the specified `logical_lane`.
    /// Returns `None` if no data is available for that lane.
    ///
    /// # Lane-Aware Polling
    ///
    /// Different lanes serve different purposes:
    /// - Lane 0: QUIC handshake / transport-control + Client H3 control
    /// - Lane 1: Server H3 control (SETTINGS, GOAWAY)
    /// - Lane 2: Application data (HQ stream, H3 request/response)
    ///
    /// Binders with multiple internal streams/channels must demux here.
    ///
    /// **IMPORTANT**: The label in the returned classification is used to select
    /// the route arm. The `logical_lane` parameter filters which data to consider.
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification>;

    /// Read data from the specified channel into the buffer.
    ///
    /// Called after `poll_incoming_for_lane()` has provided the channel and the
    /// route arm has been determined.
    fn on_recv(&mut self, channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError>;

    /// Returns a transport context provider for resolver state access.
    ///
    /// Protocol binders that wish to expose transport-layer state to resolvers
    /// should implement this method. Resolvers can then query state directly
    /// without relying on global registries.
    ///
    /// Default implementation returns `None`, indicating no context is available.
    #[inline]
    fn transport_context(&self) -> Option<&dyn TransportContextProvider> {
        None
    }

    /// Maps a logical lane (program-defined) to a physical lane (Rendezvous resource).
    ///
    /// This method enables the separation of "virtual" (logical) and "physical" lane
    /// addresses, similar to virtual vs physical memory addressing. The program defines
    /// logical lanes (e.g., "I use my lane 0"), while the binder knows the physical
    /// placement (e.g., "your lane 0 maps to physical lane 104").
    ///
    /// # Lane Separation
    ///
    /// - **Logical lane** (`u8`): Used in choreography/SendMetadata. Values: 0, 1, 2, ...
    /// - **Physical lane** (`Lane`/`LaneId`/`u32`): Used by tap/RouteTable/LoopTable.
    ///
    /// # Default Implementation
    ///
    /// Returns identity mapping: logical lane N → physical lane N.
    ///
    /// # Protocol-Specific Implementations
    ///
    /// - **H3**: `base_stream_id + logical_lane` for multiplexing multiple sessions
    /// - **Splice**: `target_lane + logical_lane` for session relocation
    #[inline]
    fn map_lane(&self, logical_lane: u8) -> crate::rendezvous::Lane {
        crate::rendezvous::Lane::new(logical_lane as u32)
    }
}

// =============================================================================
// NoBinding: Zero-cost default binding
// =============================================================================

/// No-op binding slot for CursorEndpoint.
///
/// This is the default binding type for `CursorEndpoint`. All methods
/// compile to nothing, providing zero runtime overhead when transport
/// binding is not needed.
///
/// `NoBinding` returns `None` from `poll_incoming_for_lane()`, signaling
/// that transport's raw payload should be used directly without buffering.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBinding;

// SAFETY: NoBinding performs no I/O operations. All methods are no-ops that
// complete immediately, trivially satisfying the non-blocking send contract.
unsafe impl BindingSlot for NoBinding {
    #[inline(always)]
    fn on_send_with_meta(
        &mut self,
        _meta: SendMetadata,
        _payload: &[u8],
    ) -> Result<SendDisposition, TransportOpsError> {
        // NoBinding always returns BypassTransport: let core handle transport.send()
        Ok(SendDisposition::BypassTransport)
    }

    #[inline(always)]
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        // NoBinding relies on transport.recv() directly; no internal buffering
        None
    }

    #[inline(always)]
    fn on_recv(&mut self, _channel: Channel, _buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_channel_store_basics() {
        let mut store: ArrayChannelStore<4> = ArrayChannelStore::new();
        let key = ChannelKey::new(1, 0);
        let channel = Channel::new(42);

        store.register(key, channel).unwrap();
        assert_eq!(store.get(key), Some(channel));
        assert_eq!(store.get_key(channel), Some(key));

        store.unregister(channel);
        assert_eq!(store.get(key), None);
        assert_eq!(store.get_key(channel), None);
    }

    #[test]
    fn array_store_next_instance_increments() {
        let mut store: ArrayChannelStore<4> = ArrayChannelStore::new();
        assert_eq!(store.next_instance(1).unwrap(), 0);
        assert_eq!(store.next_instance(1).unwrap(), 1);
        assert_eq!(store.next_instance(1).unwrap(), 2);
        assert_eq!(store.next_instance(2).unwrap(), 0); // Different label
        assert_eq!(store.current_instance(1), Some(2));
    }

    #[cfg(feature = "std")]
    #[test]
    fn std_channel_store_basics() {
        let mut store = StdChannelStore::new();
        let key = ChannelKey::new(1, 0);
        let channel = Channel::new(7);

        store.register(key, channel).unwrap();
        assert_eq!(store.get(key), Some(channel));
        assert_eq!(store.get_key(channel), Some(key));
        assert_eq!(store.next_instance(1).unwrap(), 0);
        assert_eq!(store.current_instance(1), Some(0));
        store.unregister(channel);
        assert_eq!(store.get(key), None);
    }
}
