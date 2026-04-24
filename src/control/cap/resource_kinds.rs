//! Built-in route/loop control kinds plus their public handle codecs.
//!
//! Built-in route/loop labels live in `runtime::consts`, and sibling protocol
//! control kinds must use the reserved protocol band `106..=127`.
//!
//! This module is intentionally limited to the public built-ins:
//! - `LoopContinueKind`
//! - `LoopBreakKind`
//! - `RouteDecisionKind`
//!
//! Private atomic control codecs live in `control::cap::atomic_codecs`.

use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlOp, ControlPath, ControlResourceKind, ResourceKind,
};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::{
    control::types::{Lane, SessionId},
    observe::ids,
    runtime::consts,
};

/// Route decision handle carrying the selected arm and scope trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RouteArmHandle {
    pub scope: ScopeId,
    pub arm: u8,
}

impl RouteArmHandle {
    pub fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0] = self.arm;
        buf[1..9].copy_from_slice(&self.scope.raw().to_le_bytes());
        buf
    }

    pub fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        let mut scope_bytes = [0u8; 8];
        scope_bytes.copy_from_slice(&data[1..9]);
        Ok(Self {
            scope: ScopeId::from_raw(u64::from_le_bytes(scope_bytes)),
            arm: data[0],
        })
    }
}

/// Loop decision handle carrying session, lane, and scope information.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LoopDecisionHandle {
    pub sid: u32,
    pub lane: u16,
    pub scope: ScopeId,
}

impl LoopDecisionHandle {
    pub fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..4].copy_from_slice(&self.sid.to_le_bytes());
        buf[4..6].copy_from_slice(&self.lane.to_le_bytes());
        buf[6..14].copy_from_slice(&self.scope.raw().to_le_bytes());
        buf
    }

    pub fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let lane = u16::from_le_bytes([data[4], data[5]]);
        let mut scope_bytes = [0u8; 8];
        scope_bytes.copy_from_slice(&data[6..14]);
        Ok(Self {
            sid,
            lane,
            scope: ScopeId::from_raw(u64::from_le_bytes(scope_bytes)),
        })
    }
}

/// Built-in local loop-continue token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopContinueKind;

impl ResourceKind for LoopContinueKind {
    type Handle = LoopDecisionHandle;
    const TAG: u8 = 0x40;
    const NAME: &'static str = "LoopContinue";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        LoopDecisionHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for LoopContinueKind {
    const LABEL: u8 = consts::LABEL_LOOP_CONTINUE;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::LoopContinue;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle {
        LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope,
        }
    }
}

/// Built-in local loop-break token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopBreakKind;

impl ResourceKind for LoopBreakKind {
    type Handle = LoopDecisionHandle;
    const TAG: u8 = 0x41;
    const NAME: &'static str = "LoopBreak";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        LoopDecisionHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for LoopBreakKind {
    const LABEL: u8 = consts::LABEL_LOOP_BREAK;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::LoopBreak;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle {
        LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope,
        }
    }
}

/// Built-in local route decision token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecisionKind;

impl ResourceKind for RouteDecisionKind {
    type Handle = RouteArmHandle;
    const TAG: u8 = 0x4E;
    const NAME: &'static str = "RouteDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        RouteArmHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
    }
}

impl ControlResourceKind for RouteDecisionKind {
    const LABEL: u8 = consts::LABEL_ROUTE_DECISION;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = ids::ROUTE_PICK;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Local;
    const OP: ControlOp = ControlOp::RouteDecision;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
        RouteArmHandle { scope, arm: 0 }
    }
}
