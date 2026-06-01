use crate::control::cap::mint::{CAP_HANDLE_LEN, ControlOp, LocalControlKind};
use crate::control::cap::resource_kinds::RouteDecisionKind;
use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};

type RouteWireHandle = (u8, u64);

fn encode_route_handle(handle: RouteWireHandle) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0] = handle.0;
    buf[1..9].copy_from_slice(&handle.1.to_le_bytes());
    buf
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteControl<const ARM: u8>;

impl<const ARM: u8> LocalControlKind for RouteControl<ARM> {
    const TAG: u8 = <RouteDecisionKind as LocalControlKind>::TAG;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as LocalControlKind>::TAP_ID;
    const SHOT: crate::control::cap::mint::CapShot = crate::control::cap::mint::CapShot::One;
    const OP: ControlOp = ControlOp::RouteDecision;

    fn encode_local_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        encode_route_handle((ARM, scope.raw()))
    }
}
