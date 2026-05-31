//! Transport binding layer for hibana choreography.
//!
//! This module provides a protocol-agnostic binding API that connects hibana's
//! flow-centric choreography to protocol-owned ingress buffers without exposing
//! transport details to application code.
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
//! │ EndpointSlot (protocol-specific binder)                           │
//! │   - Demuxes incoming carrier data per logical lane              │
//! │   - Exposes channel reads after route materialization           │
//! └─────────────────────────────────────────────────────────────────┘
//!                              ↓
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Wire payload view                                               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Design Philosophy
//!
//! The transport seam owns wire send authority. Bindings are limited to ingress
//! demux and channel reads.
//!
//! # Key Components
//!
//! - `IngressEvidence`: Lane-local ingress evidence
//! - `EndpointSlot`: Trait for protocol-specific binders used by
//!   `enter_with_binding(...)`
//! - `enter()`: zero-cost direct transport attach when binding is not needed

/// Opaque handle to a binding-owned ingress payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Channel(u64);

impl Channel {
    /// Create a channel from a binding-owned stable word.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the binding-owned stable word.
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

// =============================================================================
// BindingError: binding-owned receive failures
// =============================================================================

/// Error returned by a binding when it cannot produce a payload view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// The ingress handle is no longer available.
    ChannelUnavailable,
    /// The binding could not return the payload bytes for the selected handle.
    ReadFailed,
}

impl core::fmt::Display for BindingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ChannelUnavailable => write!(f, "binding channel unavailable"),
            Self::ReadFailed => write!(f, "binding read failed"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BindingError {}

// =============================================================================
// IngressEvidence: demux evidence for incoming data
// =============================================================================

/// Lane-local demux evidence for an incoming frame.
///
/// This is returned by `EndpointSlot::poll_incoming_for_lane()` and contains
/// transport-observable facts for demux and decode channel selection. It is not
/// route authority; route decisions remain descriptor-checked by the localside
/// kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IngressEvidence {
    /// Transport/binding discriminator observed on ingress.
    pub frame_label: crate::transport::FrameLabel,
    /// Binding-local discriminator within the frame label.
    pub instance: u16,
    /// Channel handle for subsequent read operations
    pub channel: Channel,
}

// =============================================================================
// EndpointSlot: Protocol-agnostic binding trait
// =============================================================================

/// Slot trait for transport binding on an attached endpoint.
///
/// Transport/runtime integrations implement this trait to connect hibana's
/// localside send/recv operations to their ingress/egress integration.
///
/// The canonical attach path uses `enter()` and reads directly from transport.
/// Integrations that own ingress demux state attach this slot explicitly with
/// `enter_with_binding(...)`.
///
/// # Receive Path
///
/// The receive path uses a lane-aware two-step approach:
///
/// 1. **Evidence** (`poll_incoming_for_lane`): Called by `offer()` to gather
///    demux evidence for the selected logical lane. Only returns evidence for
///    that lane.
///
/// 2. **Reading** (`on_recv`): Called after arm selection to read the actual
///    data. The channel comes from ingress evidence.
///
pub trait EndpointSlot {
    /// Poll for incoming demux evidence on a specific logical lane.
    ///
    /// Called by `offer()` to gather demux evidence for the selected scope/lane.
    /// Only returns evidence for data destined to the specified `logical_lane`.
    /// Returns `None` if no data is available for that lane.
    ///
    /// Binders with multiple internal ingress handles must demux here. Lane
    /// meaning is supplied by the projected descriptor and the integration that
    /// owns the transport; the binding only reports lane-local ingress evidence.
    ///
    /// **IMPORTANT**: The frame label in the returned evidence is for demux and
    /// decode channel selection only. It never selects a route arm by itself.
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence>;

    /// Read data from the specified channel and return a borrowed payload view.
    ///
    /// Called after `poll_incoming_for_lane()` has provided the channel and the
    /// route arm has been determined. Implementations may either fill `scratch`
    /// and return a view into it, or return a view borrowed from binding-owned
    /// storage.
    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, BindingError>;
}

pub(crate) enum BindingHandle<'a> {
    None(NoBinding),
    Borrowed(&'a mut dyn EndpointSlot),
}

impl BindingHandle<'_> {
    #[inline(always)]
    pub(crate) const fn uses_binding_storage(&self) -> bool {
        matches!(self, Self::Borrowed(_))
    }
}

impl EndpointSlot for BindingHandle<'_> {
    #[inline(always)]
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence> {
        match self {
            Self::None(binding) => binding.poll_incoming_for_lane(logical_lane),
            Self::Borrowed(binding) => binding.poll_incoming_for_lane(logical_lane),
        }
    }

    #[inline(always)]
    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, BindingError> {
        match self {
            Self::None(binding) => binding.on_recv(channel, scratch),
            Self::Borrowed(binding) => binding.on_recv(channel, scratch),
        }
    }
}

// =============================================================================
// NoBinding: Zero-cost default binding
// =============================================================================

/// No-op binding slot for attached endpoints.
///
/// This is the default binding type for `Endpoint`. It does not allocate
/// binding storage or expose demux channels; receives use transport directly.
///
/// `NoBinding` returns `None` from `poll_incoming_for_lane()`, signaling
/// that transport's raw payload should be used directly without buffering.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBinding;

impl EndpointSlot for NoBinding {
    #[inline(always)]
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        // NoBinding relies on transport.recv() directly; no internal buffering
        None
    }

    #[inline(always)]
    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, BindingError> {
        Err(BindingError::ChannelUnavailable)
    }
}
