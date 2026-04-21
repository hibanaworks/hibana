//! Built-in ResourceKind catalogue for route/loop plus private atomic control
//! adapters.
//!
//! Built-in route/loop labels live in `runtime::consts`, and sibling protocol
//! control kinds must use the reserved protocol band `106..=127`.
//!
//! The public built-ins are limited to route/loop. Remaining atomic control
//! operations are identified by private descriptor metadata and raw handle
//! codecs rather than core-owned named kinds. The minted `GenericCapToken<K>` serves
//! simultaneously as:
//! - The payload for protocol messages (e.g., `Msg<LAB, GenericCapToken<LoopContinueKind>>`)
//! - The Eff token carried through projection
//! - The witness the control plane consumes

use crate::control::cap::ControlHandle;
use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, ControlMint, ControlOp, ControlResourceKind, ResourceKind,
};
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    observe::ids,
    runtime::consts,
};
/// Implements a control resource with ResourceKind and ControlResourceKind.
///
/// # Handle Types
/// - `Unit`: `Handle = ()` — stateless marker
/// - `SessionScoped`: `Handle = (u32, u16)` — sid + lane
/// - `LoopDecision`: `Handle = LoopDecisionHandle` — sid + lane + scope
/// - `PolicyHash`: `Handle = (u32, u16)` — low32 + high16 hash
/// - `RouteDecision`: `Handle = RouteArmHandle` — arm + scope
macro_rules! define_control_resource_kind {
    // Unit variant: Handle = ()
    (
        $kind:ident,
        handle: Unit,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        path: $path:ident $(,)?
    ) => {
        define_control_resource_kind!(
            $kind,
            handle: Unit,
            tag: $tag,
            name: $name,
            label: $label,
            scope: None,
            tap_id: 0,
            path: $path,
        );
    };

    // Unit variant with explicit control scope owner
    (
        $kind:ident,
        handle: Unit,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        scope: $scope:ident,
        tap_id: $tap_id:expr,
        path: $path:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = ();
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;

            fn encode_handle(_handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                [0u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN]
            }

            fn decode_handle(
                _data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                Ok(())
            }

            fn zeroize(_handle: &mut Self::Handle) {}
        }

        impl $crate::control::cap::mint::ControlMint for $kind {
            fn mint_handle(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
                _scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                ()
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::$scope;
            const TAP_ID: u16 = $tap_id;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::$path;
        }
    };
    // SessionScoped variant: Handle = (u32, u16)
    (
        $kind:ident,
        handle: SessionScoped,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        scope: $scope:ident,
        tap_id: $tap_id:expr,
        op: $op:expr,
        caps: $caps:expr,
        path: $path:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = (u32, u16);
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                let mut buf = [0u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN];
                buf[0..4].copy_from_slice(&handle.0.to_le_bytes());
                buf[4..6].copy_from_slice(&handle.1.to_le_bytes());
                buf
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let lane = u16::from_le_bytes([data[4], data[5]]);
                Ok((sid, lane))
            }

            fn zeroize(_handle: &mut Self::Handle) {}
        }

        impl $crate::control::cap::mint::ControlMint for $kind {
            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                _scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                (sid.raw(), lane.raw() as u16)
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::$scope;
            const TAP_ID: u16 = $tap_id;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::$path;
            const OP: $crate::substrate::cap::advanced::ControlOp = $op;
            const AUTO_MINT_WIRE: bool = false;

            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                <Self as $crate::control::cap::mint::ControlMint>::mint_handle(sid, lane, scope)
            }
        }
    };

    // LoopDecision variant: Handle = LoopDecisionHandle
    (
        $kind:ident,
        handle: LoopDecision,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        op: $op:expr,
        caps: $caps:expr $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = $crate::control::cap::resource_kinds::LoopDecisionHandle;
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                handle.encode()
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                $crate::control::cap::resource_kinds::LoopDecisionHandle::decode(data)
            }

            fn zeroize(_handle: &mut Self::Handle) {}
        }

        impl $crate::control::cap::mint::ControlMint for $kind {
            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                $crate::control::cap::resource_kinds::LoopDecisionHandle {
                    sid: sid.raw(),
                    lane: lane.raw() as u16,
                    scope,
                }
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::Loop;
            const TAP_ID: u16 = $crate::observe::ids::LOOP_DECISION;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::Local;
            const OP: $crate::substrate::cap::advanced::ControlOp = $op;
            const AUTO_MINT_WIRE: bool = false;

            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                <Self as $crate::control::cap::mint::ControlMint>::mint_handle(sid, lane, scope)
            }
        }
    };

    // PolicyHash variant: Handle = (u32, u16), no SessionScopedKind
    (
        vis: $vis:vis,
        $kind:ident,
        handle: PolicyHash,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        scope: $scope:ident,
        tap_id: $tap_id:expr,
        op: $op:expr,
        caps: $caps:expr,
        path: $path:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = (u32, u16);
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                let mut buf = [0u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN];
                buf[0..4].copy_from_slice(&handle.0.to_le_bytes());
                buf[4..6].copy_from_slice(&handle.1.to_le_bytes());
                buf
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                let low = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let high = u16::from_le_bytes([data[4], data[5]]);
                Ok((low, high))
            }

            fn zeroize(_handle: &mut Self::Handle) {}
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::$scope;
            const TAP_ID: u16 = $tap_id;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::$path;
            const OP: $crate::substrate::cap::advanced::ControlOp = $op;
            const AUTO_MINT_WIRE: bool = false;

            fn mint_handle(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
                _scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                (0, 0)
            }
        }
    };

    // PolicyHash variant: Handle = (u32, u16), no SessionScopedKind
    (
        $kind:ident,
        handle: PolicyHash,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        scope: $scope:ident,
        tap_id: $tap_id:expr,
        op: $op:expr,
        caps: $caps:expr,
        path: $path:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = (u32, u16);
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                let mut buf = [0u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN];
                buf[0..4].copy_from_slice(&handle.0.to_le_bytes());
                buf[4..6].copy_from_slice(&handle.1.to_le_bytes());
                buf
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                let low = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let high = u16::from_le_bytes([data[4], data[5]]);
                Ok((low, high))
            }

            fn zeroize(_handle: &mut Self::Handle) {}
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::$scope;
            const TAP_ID: u16 = $tap_id;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::$path;
            const OP: $crate::substrate::cap::advanced::ControlOp = $op;
            const AUTO_MINT_WIRE: bool = false;

            fn mint_handle(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
                _scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                (0, 0)
            }
        }
    };

    // RouteDecision variant: Handle = RouteArmHandle
    (
        $kind:ident,
        handle: RouteDecision,
        name: $name:expr,
        label: $label:expr $(,)?
    ) => {
        define_control_resource_kind!(
            $kind,
            handle: RouteDecision,
            name: $name,
            label: $label,
            arm: 0,
        );
    };

    (
        $kind:ident,
        handle: RouteDecision,
        name: $name:expr,
        label: $label:expr,
        arm: $arm:expr $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = $crate::control::cap::resource_kinds::RouteArmHandle;
            const TAG: u8 =
                <$crate::substrate::cap::advanced::RouteDecisionKind as $crate::substrate::cap::ResourceKind>::TAG;
            const NAME: &'static str = $name;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                handle.encode()
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                $crate::control::cap::resource_kinds::RouteArmHandle::decode(data)
            }

            fn zeroize(handle: &mut Self::Handle) {
                *handle = $crate::control::cap::resource_kinds::RouteArmHandle::default();
            }
        }

        impl $crate::control::cap::mint::ControlMint for $kind {
            fn mint_handle(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                $crate::control::cap::resource_kinds::RouteArmHandle { scope, arm: $arm }
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::Route;
            const TAP_ID: u16 = <$crate::substrate::cap::advanced::RouteDecisionKind as $crate::substrate::cap::ControlResourceKind>::TAP_ID;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const PATH: $crate::substrate::cap::advanced::ControlPath =
                $crate::substrate::cap::advanced::ControlPath::Local;
            const OP: $crate::substrate::cap::advanced::ControlOp =
                $crate::substrate::cap::advanced::ControlOp::RouteDecision;
            const AUTO_MINT_WIRE: bool = false;

            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                <Self as $crate::control::cap::mint::ControlMint>::mint_handle(sid, lane, scope)
            }
        }
    };
}

/// Flags stored inside [`TopologyHandle::flags`].
pub(crate) mod splice_flags {
    /// Indicates that `seq_tx` / `seq_rx` contain fence counters.
    pub(crate) const FENCES_PRESENT: u16 = 0x0001;
}

/// Handle payload for topology-control operations.
///
/// Encoding layout (big-endian):
/// ```text
/// [ 0..2 )  src_rv
/// [ 2..4 )  dst_rv
/// [ 4..6 )  src_lane
/// [ 6..8 )  dst_lane
/// [ 8..10)  old_gen
/// [10..12)  new_gen
/// [12..16)  seq_tx
/// [16..20)  seq_rx
/// [20..22)  flags (see [`splice_flags`])
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct TopologyHandle {
    pub src_rv: u16,
    pub dst_rv: u16,
    pub src_lane: u16,
    pub dst_lane: u16,
    pub old_gen: u16,
    pub new_gen: u16,
    pub seq_tx: u32,
    pub seq_rx: u32,
    pub flags: u16,
}

impl TopologyHandle {
    /// Encode this handle into the `[u8; CAP_HANDLE_LEN]` payload.
    pub fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..2].copy_from_slice(&self.src_rv.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_rv.to_be_bytes());
        buf[4..6].copy_from_slice(&self.src_lane.to_be_bytes());
        buf[6..8].copy_from_slice(&self.dst_lane.to_be_bytes());
        buf[8..10].copy_from_slice(&self.old_gen.to_be_bytes());
        buf[10..12].copy_from_slice(&self.new_gen.to_be_bytes());
        buf[12..16].copy_from_slice(&self.seq_tx.to_be_bytes());
        buf[16..20].copy_from_slice(&self.seq_rx.to_be_bytes());
        buf[20..22].copy_from_slice(&self.flags.to_be_bytes());
        buf
    }

    /// Decode a payload into a [`TopologyHandle`].
    pub fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        Ok(Self {
            src_rv: u16::from_be_bytes([data[0], data[1]]),
            dst_rv: u16::from_be_bytes([data[2], data[3]]),
            src_lane: u16::from_be_bytes([data[4], data[5]]),
            dst_lane: u16::from_be_bytes([data[6], data[7]]),
            old_gen: u16::from_be_bytes([data[8], data[9]]),
            new_gen: u16::from_be_bytes([data[10], data[11]]),
            seq_tx: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            seq_rx: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            flags: u16::from_be_bytes([data[20], data[21]]),
        })
    }
}

/// Flags stored inside [`DelegationHandle::flags`].
/// Handle payload for delegation operations.
///
/// Encoding layout (big-endian):
/// ```text
/// [ 0..2 )  src_rv
/// [ 2..4 )  dst_rv
/// [ 4..6 )  src_lane
/// [ 6..8 )  dst_lane
/// [ 8..12)  seq_tx
/// [12..16)  seq_rx
/// [16..20)  shard / policy metadata
/// [20..22)  flags
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct DelegationHandle {
    pub src_rv: u16,
    pub dst_rv: u16,
    pub src_lane: u16,
    pub dst_lane: u16,
    pub seq_tx: u32,
    pub seq_rx: u32,
    pub shard: u32,
    pub flags: u16,
}

impl DelegationHandle {
    pub fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..2].copy_from_slice(&self.src_rv.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_rv.to_be_bytes());
        buf[4..6].copy_from_slice(&self.src_lane.to_be_bytes());
        buf[6..8].copy_from_slice(&self.dst_lane.to_be_bytes());
        buf[8..12].copy_from_slice(&self.seq_tx.to_be_bytes());
        buf[12..16].copy_from_slice(&self.seq_rx.to_be_bytes());
        buf[16..20].copy_from_slice(&self.shard.to_be_bytes());
        buf[20..22].copy_from_slice(&self.flags.to_be_bytes());
        buf
    }

    #[cfg(test)]
    pub fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        Ok(Self {
            src_rv: u16::from_be_bytes([data[0], data[1]]),
            dst_rv: u16::from_be_bytes([data[2], data[3]]),
            src_lane: u16::from_be_bytes([data[4], data[5]]),
            dst_lane: u16::from_be_bytes([data[6], data[7]]),
            seq_tx: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            seq_rx: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            shard: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            flags: u16::from_be_bytes([data[20], data[21]]),
        })
    }
}

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

// ControlHandle implementations for custom handle types
impl ControlHandle for LoopDecisionHandle {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for RouteArmHandle {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for TopologyHandle {
    fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId)) {
        f(RendezvousId::new(self.src_rv));
        f(RendezvousId::new(self.dst_rv));
    }
}

impl ControlHandle for DelegationHandle {
    fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId)) {
        f(RendezvousId::new(self.src_rv));
        f(RendezvousId::new(self.dst_rv));
    }
}

pub(crate) type SessionLaneHandle = (u32, u16);

#[cfg(test)]
pub(crate) const TAG_STATE_SNAPSHOT_CONTROL: u8 = 0x42;
#[cfg(test)]
pub(crate) const TAG_ABORT_BEGIN_CONTROL: u8 = 0x45;
#[cfg(test)]
pub(crate) const TAG_CAP_DELEGATE_CONTROL: u8 = 0x49;
#[cfg(test)]
pub(crate) const TAG_TOPOLOGY_BEGIN_CONTROL: u8 = 0x57;

#[cfg(test)]
#[inline]
pub(crate) fn encode_session_lane_handle(handle: SessionLaneHandle) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0..4].copy_from_slice(&handle.0.to_le_bytes());
    buf[4..6].copy_from_slice(&handle.1.to_le_bytes());
    buf
}

#[inline]
pub(crate) fn decode_session_lane_handle(
    data: [u8; CAP_HANDLE_LEN],
) -> Result<SessionLaneHandle, CapError> {
    let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let lane = u16::from_le_bytes([data[4], data[5]]);
    Ok((sid, lane))
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn mint_session_lane_handle(sid: SessionId, lane: Lane) -> SessionLaneHandle {
    (sid.raw(), lane.raw() as u16)
}

define_control_resource_kind!(
    LoopContinueKind,
    handle: LoopDecision,
    tag: 0x40,
    name: "LoopContinue",
    label: consts::LABEL_LOOP_CONTINUE,
    op: ControlOp::LoopContinue,
    caps: OpSet::empty().with(ControlOp::LoopContinue),
);

define_control_resource_kind!(
    LoopBreakKind,
    handle: LoopDecision,
    tag: 0x41,
    name: "LoopBreak",
    label: consts::LABEL_LOOP_BREAK,
    op: ControlOp::LoopBreak,
    caps: OpSet::empty().with(ControlOp::LoopBreak),
);

/// Route decision token (selects a route arm).
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
    const PATH: crate::control::cap::mint::ControlPath =
        crate::control::cap::mint::ControlPath::Local;
    const OP: ControlOp = ControlOp::RouteDecision;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle {
        <Self as ControlMint>::mint_handle(sid, lane, scope)
    }
}

impl ControlMint for RouteDecisionKind {
    fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
        RouteArmHandle { scope, arm: 0 }
    }
}
