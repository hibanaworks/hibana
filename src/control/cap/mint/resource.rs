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
