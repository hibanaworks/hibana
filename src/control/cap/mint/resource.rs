use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

use super::{CAP_HANDLE_LEN, CapShot, ControlOp};

/// Protocol-owned explicit wire control kind.
///
/// This is the single public trait a protocol author implements for explicit
/// wire controls. It carries only the protocol-visible resource tag. Token
/// bytes remain protocol-owned opaque payload; Hibana does not ask external
/// code for endpoint minting, handle decoding, cleanup authority, or control
/// operation selection.
pub(crate) trait WireControlKind {
    /// Capability tag carried in explicit wire tokens.
    ///
    /// Wire control kinds must not use `0`.
    ///
    /// The zero tag is reserved internally for endpoint capabilities and the
    /// non-control `()` sentinel.
    const TAG: u8;
}

/// Crate-owned local controls whose descriptor handle is minted by Hibana.
///
/// Public protocol controls are explicit wire tokens and provide only
/// descriptor metadata. Local endpoint-owned minting is restricted to built-in
/// and internal control effects so external code cannot add hidden runtime
/// authority behind `WireControlKind`.
pub(crate) trait LocalControlKind {
    const TAG: u8;
    const SCOPE: ControlScopeKind;
    const TAP_ID: u16;
    const SHOT: CapShot;
    const OP: ControlOp;

    fn encode_local_handle(session: SessionId, lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN];
}

/// Handle describing an endpoint rendezvous slot.
#[cfg(all(test, hibana_repo_tests))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointHandle {
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) role: u8,
}

#[cfg(all(test, hibana_repo_tests))]
impl EndpointHandle {
    pub(crate) const fn new(sid: SessionId, lane: Lane, role: u8) -> Self {
        Self { sid, lane, role }
    }
}

/// Marker for endpoint capabilities (kept internal to hibana).
#[cfg(all(test, hibana_repo_tests))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointResource {}

#[cfg(all(test, hibana_repo_tests))]
impl EndpointResource {
    pub(crate) const TAG: u8 = 0;

    pub(crate) fn encode_identity(handle: &EndpointHandle) -> [u8; CAP_HANDLE_LEN] {
        let mut data = [0u8; CAP_HANDLE_LEN];
        data[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
        data[4] = handle.lane.as_wire();
        data[5] = handle.role;
        data
    }

    pub(crate) fn decode_identity(
        data: [u8; CAP_HANDLE_LEN],
    ) -> Result<EndpointHandle, super::CapError> {
        let sid = SessionId::new(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
        let lane = Lane::new(u32::from(data[4]));
        let role = data[5];
        Ok(EndpointHandle::new(sid, lane, role))
    }
}
