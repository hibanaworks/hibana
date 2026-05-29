use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

use super::{CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath};
/// Resource taxonomy for capabilities.
///
/// Each `ResourceKind` supplies a handle type that is encoded into the opaque
/// payload section of the capability header. The fixed descriptor prefix stores
/// session, routing, and control metadata; the remaining [`CAP_HANDLE_LEN`]
/// bytes are entirely owned by the resource kind for encoding operands.
pub trait ResourceKind {
    /// Handle associated with this capability.
    type Handle;

    /// Capability tag.
    ///
    /// Control resource kinds must not use `0`. The zero tag is reserved
    /// internally for endpoint capabilities and the non-control `()` sentinel.
    const TAG: u8;

    /// Human-readable name used for observability.
    const NAME: &'static str;

    /// Encode the handle into the resource payload area of the header.
    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN];

    /// Decode the handle from the resource payload area of the header.
    ///
    /// Decoding must be deterministic, side-effect-free, and non-authoritative.
    /// Returning `Ok` only constructs a local handle value; it must not claim,
    /// consume, mutate, or observe rendezvous authority.
    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>;

    /// Structured scope carried inside the resource payload, when a custom
    /// resource deliberately mirrors the descriptor scope.
    ///
    /// Built-in route/loop decision handles use the control header as the
    /// single scope authority and therefore leave this unset.
    fn handle_scope(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }

    /// Zeroize the handle prior to dropping it.
    fn zeroize(handle: &mut Self::Handle);
}

/// Resource kinds that represent control-plane capabilities.
pub trait ControlResourceKind: ResourceKind {
    const SCOPE: ControlScopeKind;
    const PATH: ControlPath;
    const TAP_ID: u16;
    const SHOT: CapShot;
    const OP: ControlOp;
    const AUTO_MINT_WIRE: bool;

    fn mint_handle(session: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle;
}

impl ResourceKind for () {
    type Handle = ();

    const TAG: u8 = 0;
    const NAME: &'static str = "NoControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0u8; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

/// Handle describing an endpoint rendezvous slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointHandle {
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) role: u8,
}

impl EndpointHandle {
    pub(crate) const fn new(sid: SessionId, lane: Lane, role: u8) -> Self {
        Self { sid, lane, role }
    }

    fn zeroed() -> Self {
        Self {
            sid: SessionId::new(0),
            lane: Lane::new(0),
            role: 0,
        }
    }
}

/// Marker for endpoint capabilities (kept internal to hibana).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointResource {}

impl ResourceKind for EndpointResource {
    type Handle = EndpointHandle;
    const TAG: u8 = 0;
    const NAME: &'static str = "EndpointResource";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut data = [0u8; CAP_HANDLE_LEN];
        data[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
        data[4] = handle.lane.as_wire();
        data[5] = handle.role;
        data
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let sid = SessionId::new(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
        let lane = Lane::new(u32::from(data[4]));
        let role = data[5];
        Ok(EndpointHandle::new(sid, lane, role))
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = EndpointHandle::zeroed();
    }
}
