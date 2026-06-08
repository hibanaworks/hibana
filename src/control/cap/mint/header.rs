//! Descriptor-first capability token header codec.

use crate::control::types::{Lane, SessionId};
use crate::global::const_dsl::ControlScopeKind;

use super::{CAP_CONTROL_HEADER_FIXED_LEN, CAP_HEADER_LEN, CapError};
/// Capability shot semantics embedded in the token wire/runtime encoding.
///
/// `CapShot` records the descriptor-level reuse discipline for a concrete token:
/// - `One`: Single-use descriptor discipline.
/// - `Many`: Reusable descriptor semantics.
///
/// Public explicit wire controls always use reusable descriptor semantics.
/// One-shot local control discipline is selected only by Hibana-owned local
/// control kinds. `CapShot` is the runtime encoding of that descriptor decision
/// inside an encoded control token. The token byte value is not itself an
/// affine proof; one-shot enforcement is owned by endpoint-registered token
/// state or by the descriptor terminal contract.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapShot {
    /// Single-use descriptor discipline.
    One = 0,
    /// Reusable descriptor semantics.
    Many = 1,
}

impl CapShot {
    #[inline]
    pub(crate) const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::One),
            1 => Some(Self::Many),
            _ => None,
        }
    }

    #[inline]
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Built-in control-plane operation owned by hibana core.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlOp {
    RouteResolve = 0,
    LoopContinue = 1,
    LoopBreak = 2,
    StateSnapshot = 3,
    StateRestore = 4,
    TopologyBegin = 5,
    TopologyAck = 6,
    TopologyCommit = 7,
    AbortBegin = 9,
    AbortAck = 10,
    Fence = 11,
    TxCommit = 12,
    TxAbort = 13,
}

impl ControlOp {
    #[inline]
    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::RouteResolve),
            1 => Some(Self::LoopContinue),
            2 => Some(Self::LoopBreak),
            3 => Some(Self::StateSnapshot),
            4 => Some(Self::StateRestore),
            5 => Some(Self::TopologyBegin),
            6 => Some(Self::TopologyAck),
            7 => Some(Self::TopologyCommit),
            9 => Some(Self::AbortBegin),
            10 => Some(Self::AbortAck),
            11 => Some(Self::Fence),
            12 => Some(Self::TxCommit),
            13 => Some(Self::TxAbort),
            _ => None,
        }
    }

    #[inline]
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Transport crossing mode for control messages.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlPath {
    Local = 0,
    Wire = 1,
}

impl ControlPath {
    #[inline]
    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Local),
            1 => Some(Self::Wire),
            _ => None,
        }
    }

    #[inline]
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Descriptor-first fixed control header.
///
/// This is a wire codec carrier. Callers must use `encode` / `decode` rather
/// than relying on struct layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CapHeader {
    version: u8,
    sid: SessionId,
    lane: Lane,
    role: u8,
    tag: u8,
    op: ControlOp,
    path: ControlPath,
    shot: CapShot,
    scope_kind: ControlScopeKind,
    flags: u8,
    scope_id: u16,
    epoch: u16,
    handle: [u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN],
}

impl CapHeader {
    const KNOWN_FLAGS_MASK: u8 = 0;

    #[inline]
    pub(crate) const fn new(
        sid: SessionId,
        lane: Lane,
        role: u8,
        tag: u8,
        op: ControlOp,
        path: ControlPath,
        shot: CapShot,
        scope_kind: ControlScopeKind,
        flags: u8,
        scope_id: u16,
        epoch: u16,
        handle: [u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN],
    ) -> Self {
        Self {
            version: 1,
            sid,
            lane,
            role,
            tag,
            op,
            path,
            shot,
            scope_kind,
            flags,
            scope_id,
            epoch,
            handle,
        }
    }

    #[inline]
    pub(crate) fn encode(&self, out: &mut [u8; CAP_HEADER_LEN]) {
        out[0] = self.version;
        out[1..5].copy_from_slice(&self.sid.raw().to_be_bytes());
        out[5] = self.lane.as_wire();
        out[6] = self.role;
        out[7] = self.tag;
        out[8] = self.op.as_u8();
        out[9] = self.path.as_u8();
        out[10] = self.shot.as_u8();
        out[11] = self.scope_kind as u8;
        out[12] = self.flags;
        out[13..15].copy_from_slice(&self.scope_id.to_be_bytes());
        out[15..17].copy_from_slice(&self.epoch.to_be_bytes());
        out[17..].copy_from_slice(&self.handle);
    }

    #[inline]
    pub(crate) fn decode(raw: [u8; CAP_HEADER_LEN]) -> Result<Self, CapError> {
        if raw[0] != 1 {
            return Err(CapError);
        }
        let op = ControlOp::from_u8(raw[8]).ok_or(CapError)?;
        let path = ControlPath::from_u8(raw[9]).ok_or(CapError)?;
        let shot = CapShot::from_u8(raw[10]).ok_or(CapError)?;
        let scope_kind = ControlScopeKind::from_u8(raw[11]).ok_or(CapError)?;
        if raw[12] & !Self::KNOWN_FLAGS_MASK != 0 {
            return Err(CapError);
        }
        let mut handle = [0u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN];
        handle.copy_from_slice(&raw[17..]);
        Ok(Self {
            version: raw[0],
            sid: SessionId::new(u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]])),
            lane: Lane::new(u32::from(raw[5])),
            role: raw[6],
            tag: raw[7],
            op,
            path,
            shot,
            scope_kind,
            flags: raw[12],
            scope_id: u16::from_be_bytes([raw[13], raw[14]]),
            epoch: u16::from_be_bytes([raw[15], raw[16]]),
            handle,
        })
    }

    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(crate) const fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline]
    pub(crate) const fn tag(&self) -> u8 {
        self.tag
    }

    #[inline]
    pub(crate) const fn op(&self) -> ControlOp {
        self.op
    }

    #[inline]
    pub(crate) const fn path(&self) -> ControlPath {
        self.path
    }

    #[inline]
    pub(crate) const fn shot(&self) -> CapShot {
        self.shot
    }

    #[inline]
    pub(crate) const fn scope_kind(&self) -> ControlScopeKind {
        self.scope_kind
    }

    #[inline]
    pub(crate) const fn flags(&self) -> u8 {
        self.flags
    }

    #[inline]
    pub(crate) const fn scope_id(&self) -> u16 {
        self.scope_id
    }

    #[inline]
    pub(crate) const fn epoch(&self) -> u16 {
        self.epoch
    }

    #[inline]
    #[cfg(test)]
    pub(crate) const fn handle(&self) -> &[u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN] {
        &self.handle
    }
}
