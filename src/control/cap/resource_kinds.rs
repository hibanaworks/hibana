//! Built-in route/loop control kinds plus their public handle codecs.
//!
//! Built-in route/loop semantics are identified by descriptor control metadata,
//! never by numeric label reservations.
//!
//! This module is intentionally limited to the public built-ins:
//! - `LoopContinueKind`
//! - `LoopBreakKind`
//! - `RouteDecisionKind`
//!
//! Private atomic control codecs live in `control::cap::atomic_codecs`.

use crate::control::cap::mint::{CAP_HANDLE_LEN, CapError, CapShot, ControlOp, LocalControlKind};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::{
    control::types::{Lane, SessionId},
    observe::ids,
};

#[inline]
#[cfg(test)]
fn bytes_are_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// Route decision handle carrying the selected binary arm.
///
/// The decision scope is the control-header scope; it is not duplicated in the
/// built-in handle payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct RouteArmHandle {
    arm: u8,
}

impl RouteArmHandle {
    #[inline]
    pub(crate) fn new(arm: u8) -> Result<Self, CapError> {
        if arm > 1 {
            return Err(CapError);
        }
        Ok(Self { arm })
    }

    #[inline]
    pub(crate) const fn new_unchecked(arm: u8) -> Self {
        Self { arm }
    }

    pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0] = self.arm;
        buf
    }

    #[cfg(test)]
    pub(crate) fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        if data[0] > 1 || !bytes_are_zero(&data[1..]) {
            return Err(CapError);
        }
        Self::new(data[0])
    }
}

/// Loop decision handle carrying session and lane information.
///
/// The loop scope is the control-header scope; it is not duplicated in the
/// built-in handle payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct LoopDecisionHandle {
    sid: u32,
    lane: u8,
}

impl LoopDecisionHandle {
    #[inline]
    pub(crate) const fn new(sid: u32, lane: u8) -> Self {
        Self { sid, lane }
    }

    #[inline]
    pub(crate) const fn new_unchecked(sid: u32, lane: u8) -> Self {
        Self { sid, lane }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn sid(self) -> u32 {
        self.sid
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..4].copy_from_slice(&self.sid.to_le_bytes());
        buf[4..6].copy_from_slice(&u16::from(self.lane).to_le_bytes());
        buf
    }

    #[cfg(test)]
    pub(crate) fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        if !bytes_are_zero(&data[6..]) {
            return Err(CapError);
        }
        let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let lane_raw = u16::from_le_bytes([data[4], data[5]]);
        if lane_raw > u8::MAX as u16 {
            return Err(CapError);
        }
        Ok(Self::new(sid, lane_raw as u8))
    }
}

/// Built-in local loop-continue token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopContinueKind;

impl LocalControlKind for LoopContinueKind {
    const TAG: u8 = 0x40;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::LoopContinue;

    fn encode_local_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        let _ = scope;
        LoopDecisionHandle::new_unchecked(sid.raw(), lane.as_wire()).encode()
    }
}

/// Built-in local loop-break token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopBreakKind;

impl LocalControlKind for LoopBreakKind {
    const TAG: u8 = 0x41;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::LoopBreak;

    fn encode_local_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        let _ = scope;
        LoopDecisionHandle::new_unchecked(sid.raw(), lane.as_wire()).encode()
    }
}

/// Built-in local route decision token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecisionKind;

impl LocalControlKind for RouteDecisionKind {
    const TAG: u8 = 0x4E;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = ids::ROUTE_PICK;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::RouteDecision;

    fn encode_local_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        let _ = scope;
        RouteArmHandle::new_unchecked(0).encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_arm_handle_rejects_non_binary_arms_and_reserved_tail() {
        let handle = RouteArmHandle::new(1).expect("binary route arm");
        let encoded = handle.encode();
        assert_eq!(RouteArmHandle::decode(encoded), Ok(handle));

        let mut non_binary = encoded;
        non_binary[0] = 2;
        assert_eq!(RouteArmHandle::decode(non_binary), Err(CapError));

        let mut trailing = encoded;
        trailing[9] = 0xA5;
        assert_eq!(RouteArmHandle::decode(trailing), Err(CapError));
    }

    #[test]
    fn loop_decision_handle_rejects_reserved_tail() {
        let handle = LoopDecisionHandle::new(9, 4);
        let encoded = handle.encode();
        assert_eq!(LoopDecisionHandle::decode(encoded), Ok(handle));

        let mut trailing = encoded;
        trailing[6] = 0x5A;
        assert_eq!(LoopDecisionHandle::decode(trailing), Err(CapError));

        let mut out_of_domain_lane = encoded;
        out_of_domain_lane[4..6].copy_from_slice(&256u16.to_le_bytes());
        assert_eq!(
            LoopDecisionHandle::decode(out_of_domain_lane),
            Err(CapError),
            "loop control handles must fail closed on lanes outside the wire domain"
        );
    }
}
