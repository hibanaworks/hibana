//! Built-in loop control kinds.
//!
//! Built-in route/loop semantics are identified by descriptor control metadata,
//! never by numeric label reservations.
//!
//! This module is intentionally limited to the public loop built-ins:
//! - `LoopContinueKind`
//! - `LoopBreakKind`
//!
//! Private atomic control codecs live in `control::cap::atomic_codecs`.

use crate::control::cap::mint::{CAP_HANDLE_LEN, CapShot, ControlOp, LocalControlKind};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::{
    control::types::{Lane, SessionId},
    observe::ids,
};

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

    pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..4].copy_from_slice(&self.sid.to_le_bytes());
        buf[4..6].copy_from_slice(&u16::from(self.lane).to_le_bytes());
        buf
    }
}

/// Built-in local loop-continue token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopContinueKind;

const _: LoopContinueKind = LoopContinueKind;

impl LocalControlKind for LoopContinueKind {
    const TAG: u8 = 0x40;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::LoopContinue;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        LoopDecisionHandle::new(sid.raw(), lane.as_wire()).encode()
    }
}

/// Built-in local loop-break token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopBreakKind;

const _: LoopBreakKind = LoopBreakKind;

impl LocalControlKind for LoopBreakKind {
    const TAG: u8 = 0x41;
    const SCOPE: ControlScopeKind = ControlScopeKind::Loop;
    const TAP_ID: u16 = ids::LOOP_DECISION;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::LoopBreak;

    fn encode_local_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> [u8; CAP_HANDLE_LEN] {
        LoopDecisionHandle::new(sid.raw(), lane.as_wire()).encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::CapError;

    fn decode_loop_decision_handle(
        data: [u8; CAP_HANDLE_LEN],
    ) -> Result<LoopDecisionHandle, CapError> {
        if data[6..].iter().any(|byte| *byte != 0) {
            return Err(CapError);
        }
        let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let lane_raw = u16::from_le_bytes([data[4], data[5]]);
        if lane_raw > u8::MAX as u16 {
            return Err(CapError);
        }
        Ok(LoopDecisionHandle::new(sid, lane_raw as u8))
    }

    #[test]
    fn loop_decision_handle_rejects_reserved_tail() {
        let handle = LoopDecisionHandle::new(9, 4);
        let encoded = handle.encode();
        assert_eq!(decode_loop_decision_handle(encoded), Ok(handle));

        let mut trailing = encoded;
        trailing[6] = 0x5A;
        assert_eq!(decode_loop_decision_handle(trailing), Err(CapError));

        let mut out_of_domain_lane = encoded;
        out_of_domain_lane[4..6].copy_from_slice(&256u16.to_le_bytes());
        assert_eq!(
            decode_loop_decision_handle(out_of_domain_lane),
            Err(CapError),
            "loop control handles must fail closed on lanes outside the wire domain"
        );
    }
}
