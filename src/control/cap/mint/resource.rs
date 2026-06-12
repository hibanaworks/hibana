use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

use super::{CAP_HANDLE_LEN, CapShot, ControlOp};

/// Crate-owned local controls whose descriptor handle is minted by Hibana.
///
/// Local endpoint-owned minting is restricted to Hibana-defined control effects
/// so external code cannot add hidden runtime authority behind a public marker.
pub(crate) trait LocalControlKind {
    const TAG: u8;
    const SCOPE: ControlScopeKind;
    const TAP_ID: u16;
    const SHOT: CapShot;
    const OP: ControlOp;

    fn encode_local_handle(session: SessionId, lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN];
}
