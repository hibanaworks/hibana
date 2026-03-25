//! Transport binding layer for hibana choreography.
//!
//! This module provides a protocol-agnostic binding API that connects hibana's
//! flow-centric choreography to underlying transport mechanisms (stream/datagram
//! transports, RPCs, etc.) without exposing transport details to application code.
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
//! │ BindingSlot (protocol-specific binder)                           │
//! │   - Demuxes incoming carrier data per logical lane              │
//! │   - Exposes channel reads after route materialization           │
//! │   - Supplies slot-scoped policy signals                         │
//! └─────────────────────────────────────────────────────────────────┘
//!                              ↓
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Wire (stream/datagram frames, RPC payloads, etc.)               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Design Philosophy
//!
//! The transport seam owns wire send authority. Bindings are limited to ingress
//! demux, channel reads, and policy-signal observation.
//!
//! # Key Components
//!
//! - [`IncomingClassification`]: Classification for incoming data
//! - [`BindingSlot`]: Trait for protocol-specific binders
//! - [`ChannelStore`]: Label/instance → Channel mappings (no_alloc by default)
//! - [`NoBinding`]: Zero-cost default when binding is not needed

use crate::transport::context::PolicySignalsProvider;

// =============================================================================
// Channel: Opaque handle to a logical channel
// =============================================================================

/// Direction of a logical channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelDirection {
    /// Bidirectional channel (e.g., stream transport bidi channel)
    Bidirectional,
    /// Send-only channel (e.g., outbound unidirectional channel)
    SendOnly,
    /// Receive-only channel (e.g., inbound unidirectional channel)
    RecvOnly,
}

/// Opaque handle to a logical channel.
///
/// The actual representation is transport-specific:
/// - Stream transport: wraps a stream identifier (u64)
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

/// Storage abstraction for logical channels.
pub trait ChannelStore {
    /// Register a channel for a given key.
    fn register(&mut self, key: ChannelKey, channel: Channel) -> Result<(), TransportOpsError>;

    /// Look up a channel by key.
    fn get(&self, key: ChannelKey) -> Option<Channel>;

    /// Look up a key by channel (reverse lookup for demux).
    fn get_key(&self, channel: Channel) -> Option<ChannelKey>;

    /// Get or allocate the next instance for a label.
    fn next_instance(&mut self, label: u8) -> Result<u16, TransportOpsError>;

    /// Get the most recently allocated instance for a label, if any.
    fn current_instance(&self, label: u8) -> Option<u16>;

    /// Unregister a channel.
    fn unregister(&mut self, channel: Channel);

    /// Clear all registrations.
    fn clear(&mut self);
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

/// Slot trait for transport binding on an attached endpoint.
///
/// Transport/runtime adapters implement this trait to connect hibana's flow
/// operations to their ingress/egress substrate.
///
/// When `B = NoBinding` (the default), all methods inline to no-ops at
/// compile time, providing zero runtime overhead.
///
/// # Receive Flow
///
/// The receive flow uses a lane-aware two-step approach:
///
/// 1. **Classification** (`poll_incoming_for_lane`): Called by `offer()` to
///    gather demux evidence for the selected logical lane. Only returns
///    classifications for that lane.
///
/// 2. **Reading** (`on_recv`): Called after arm selection to read the actual
///    data. The channel comes from the classification.
///
/// Additionally, implementations may provide a slot-scoped
/// `PolicySignalsProvider` for policy evaluation and resolver context.
///
pub trait BindingSlot {
    /// Poll for incoming data classification on a specific logical lane.
    ///
    /// Called by `offer()` to gather demux evidence for the selected scope/lane.
    /// Only returns classifications for data destined to the specified `logical_lane`.
    /// Returns `None` if no data is available for that lane.
    ///
    /// # Lane-Aware Polling
    ///
    /// Different lanes serve different purposes:
    /// - Lane 0: transport-level control traffic
    /// - Lane 1: transport early-data traffic
    /// - Lane 2+: appkit / application-owned traffic
    ///
    /// Binders with multiple internal streams/channels must demux here.
    ///
    /// **IMPORTANT**: The label in the returned classification is for demux and
    /// decode channel selection only. Route arm authority remains
    /// `RouteDecisionToken(Ack|Resolver|Poll)`.
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification>;

    /// Read data from the specified channel into the buffer.
    ///
    /// Called after `poll_incoming_for_lane()` has provided the channel and the
    /// route arm has been determined.
    fn on_recv(&mut self, channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError>;

    /// Returns a policy signals provider for slot-scoped policy input.
    ///
    /// Returning `None` indicates all-zero input and empty attributes.
    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider>;
}

// =============================================================================
// NoBinding: Zero-cost default binding
// =============================================================================

/// No-op binding slot for attached endpoints.
///
/// This is the default binding type for `Endpoint`. All methods
/// compile to nothing, providing zero runtime overhead when transport
/// binding is not needed.
///
/// `NoBinding` returns `None` from `poll_incoming_for_lane()`, signaling
/// that transport's raw payload should be used directly without buffering.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBinding;

impl BindingSlot for NoBinding {
    #[inline(always)]
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        // NoBinding relies on transport.recv() directly; no internal buffering
        None
    }

    #[inline(always)]
    fn on_recv(&mut self, _channel: Channel, _buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        Ok(0)
    }

    #[inline(always)]
    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epf::vm::Slot;
    use crate::transport::context::PolicySignals;

    #[test]
    fn no_binding_policy_signals_are_zero_for_all_slots() {
        let binding = NoBinding;
        for slot in [
            Slot::Forward,
            Slot::EndpointRx,
            Slot::EndpointTx,
            Slot::Rendezvous,
            Slot::Route,
        ] {
            let signals = binding
                .policy_signals_provider()
                .map(|provider| provider.signals(slot))
                .unwrap_or(PolicySignals::ZERO);
            assert_eq!(signals, PolicySignals::ZERO);
        }
    }
}
