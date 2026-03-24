//! Standard ResourceKind catalogue for control-plane operations.
//!
//! Reserved range: `0x40..=0x5F` for control-plane kinds.
//!
//! Each control operation (loop continue/break, checkpoint, rollback, etc.)
//! is represented as its own ResourceKind. The minted `GenericCapToken<K>`
//! serves simultaneously as:
//! - The payload for protocol messages (e.g., `Msg<LAB, GenericCapToken<LoopContinueKind>>`)
//! - The Eff token carried through projection
//! - The witness the control plane consumes

use crate::control::cap::ControlHandle;
use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CapError, CapShot, CapsMask, ControlMint, ControlResourceKind, ResourceKind,
    SessionScopedKind,
};
use crate::control::cluster::effects::CpEffect;
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    observe::ids,
    runtime::consts,
};
/// Implements a control resource with ResourceKind and ControlResourceKind.
///
/// # Handle Types
/// - `Unit`: `Handle = ()` — stateless marker, no SessionScopedKind
/// - `SessionScoped`: `Handle = (u32, u16)` — sid + lane, implements SessionScopedKind
/// - `LoopDecision`: `Handle = LoopDecisionHandle` — sid + lane + scope, implements SessionScopedKind
/// - `PolicyHash`: `Handle = (u32, u16)` — low32 + high16 hash, no SessionScopedKind
/// - `RouteDecision`: `Handle = RouteDecisionHandle` — arm + scope, implements SessionScopedKind
#[macro_export]
macro_rules! impl_control_resource {
    // Unit variant: Handle = ()
    (
        $kind:ident,
        handle: Unit,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        handling: $handling:ident $(,)?
    ) => {
        $crate::impl_control_resource!(
            $kind,
            handle: Unit,
            tag: $tag,
            name: $name,
            label: $label,
            scope: None,
            tap_id: 0,
            handling: $handling,
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
        handling: $handling:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = ();
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;
            const AUTO_MINT_EXTERNAL: bool = false;

            fn encode_handle(_handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                [0u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN]
            }

            fn decode_handle(
                _data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                Ok(())
            }

            fn zeroize(_handle: &mut Self::Handle) {}

            fn caps_mask(_handle: &Self::Handle) -> $crate::substrate::cap::advanced::CapsMask {
                $crate::substrate::cap::advanced::CapsMask::empty()
            }

            fn scope_id(
                _handle: &Self::Handle,
            ) -> Option<$crate::substrate::cap::advanced::ScopeId> {
                None
            }
        }

        impl $crate::substrate::cap::advanced::ControlMint for $kind {
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
            const HANDLING: $crate::substrate::cap::advanced::ControlHandling =
                $crate::substrate::cap::advanced::ControlHandling::$handling;
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
        caps: $caps:expr,
        handling: $handling:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = (u32, u16);
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;
            const AUTO_MINT_EXTERNAL: bool = false;

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

            fn caps_mask(_handle: &Self::Handle) -> $crate::substrate::cap::advanced::CapsMask {
                $caps
            }

            fn scope_id(
                _handle: &Self::Handle,
            ) -> Option<$crate::substrate::cap::advanced::ScopeId> {
                None
            }
        }

        impl $crate::substrate::cap::advanced::SessionScopedKind for $kind {
            fn handle_for_session(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
            ) -> Self::Handle {
                (sid.raw(), lane.raw() as u16)
            }

            fn shot() -> $crate::substrate::cap::CapShot {
                $crate::substrate::cap::CapShot::One
            }
        }

        impl $crate::substrate::cap::advanced::ControlMint for $kind {
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
            const HANDLING: $crate::substrate::cap::advanced::ControlHandling =
                $crate::substrate::cap::advanced::ControlHandling::$handling;
        }
    };

    // LoopDecision variant: Handle = LoopDecisionHandle
    (
        $kind:ident,
        handle: LoopDecision,
        tag: $tag:expr,
        name: $name:expr,
        label: $label:expr,
        caps: $caps:expr $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = $crate::substrate::cap::advanced::LoopDecisionHandle;
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;
            const AUTO_MINT_EXTERNAL: bool = false;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                handle.encode()
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                $crate::substrate::cap::advanced::LoopDecisionHandle::decode(data)
            }

            fn zeroize(_handle: &mut Self::Handle) {}

            fn caps_mask(_handle: &Self::Handle) -> $crate::substrate::cap::advanced::CapsMask {
                $caps
            }

            fn scope_id(
                handle: &Self::Handle,
            ) -> Option<$crate::substrate::cap::advanced::ScopeId> {
                Some(handle.scope)
            }
        }

        impl $crate::substrate::cap::advanced::SessionScopedKind for $kind {
            fn handle_for_session(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
            ) -> Self::Handle {
                $crate::substrate::cap::advanced::LoopDecisionHandle {
                    sid: sid.raw(),
                    lane: lane.raw() as u16,
                    scope: $crate::substrate::cap::advanced::ScopeId::generic(0),
                }
            }

            fn shot() -> $crate::substrate::cap::CapShot {
                $crate::substrate::cap::CapShot::One
            }
        }

        impl $crate::substrate::cap::advanced::ControlMint for $kind {
            fn mint_handle(
                sid: $crate::substrate::SessionId,
                lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                $crate::substrate::cap::advanced::LoopDecisionHandle {
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
            const HANDLING: $crate::substrate::cap::advanced::ControlHandling =
                $crate::substrate::cap::advanced::ControlHandling::Canonical;
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
        caps: $caps:expr,
        handling: $handling:ident $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl $crate::substrate::cap::ResourceKind for $kind {
            type Handle = (u32, u16);
            const TAG: u8 = $tag;
            const NAME: &'static str = $name;
            const AUTO_MINT_EXTERNAL: bool = false;

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

            fn caps_mask(_handle: &Self::Handle) -> $crate::substrate::cap::advanced::CapsMask {
                $caps
            }

            fn scope_id(
                _handle: &Self::Handle,
            ) -> Option<$crate::substrate::cap::advanced::ScopeId> {
                None
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::$scope;
            const TAP_ID: u16 = $tap_id;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const HANDLING: $crate::substrate::cap::advanced::ControlHandling =
                $crate::substrate::cap::advanced::ControlHandling::$handling;
        }
    };

    // RouteDecision variant: Handle = RouteDecisionHandle
    (
        $kind:ident,
        handle: RouteDecision,
        name: $name:expr,
        label: $label:expr $(,)?
    ) => {
        $crate::impl_control_resource!(
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
            type Handle = $crate::substrate::cap::advanced::RouteDecisionHandle;
            const TAG: u8 =
                <$crate::substrate::cap::advanced::RouteDecisionKind as $crate::substrate::cap::ResourceKind>::TAG;
            const NAME: &'static str = $name;
            const AUTO_MINT_EXTERNAL: bool = false;

            fn encode_handle(handle: &Self::Handle) -> [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN] {
                handle.encode()
            }

            fn decode_handle(
                data: [u8; $crate::substrate::cap::advanced::CAP_HANDLE_LEN],
            ) -> Result<Self::Handle, $crate::substrate::cap::advanced::CapError> {
                $crate::substrate::cap::advanced::RouteDecisionHandle::decode(data)
            }

            fn zeroize(handle: &mut Self::Handle) {
                *handle = $crate::substrate::cap::advanced::RouteDecisionHandle::default();
            }

            fn caps_mask(_handle: &Self::Handle) -> $crate::substrate::cap::advanced::CapsMask {
                $crate::substrate::cap::advanced::CapsMask::empty()
            }

            fn scope_id(
                handle: &Self::Handle,
            ) -> Option<$crate::substrate::cap::advanced::ScopeId> {
                Some(handle.scope)
            }
        }

        impl $crate::substrate::cap::advanced::SessionScopedKind for $kind {
            fn handle_for_session(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
            ) -> Self::Handle {
                $crate::substrate::cap::advanced::RouteDecisionHandle::default()
            }

            fn shot() -> $crate::substrate::cap::CapShot {
                $crate::substrate::cap::CapShot::One
            }
        }

        impl $crate::substrate::cap::advanced::ControlMint for $kind {
            fn mint_handle(
                _sid: $crate::substrate::SessionId,
                _lane: $crate::substrate::Lane,
                scope: $crate::substrate::cap::advanced::ScopeId,
            ) -> Self::Handle {
                $crate::substrate::cap::advanced::RouteDecisionHandle { scope, arm: $arm }
            }
        }

        impl $crate::substrate::cap::ControlResourceKind for $kind {
            const LABEL: u8 = $label;
            const SCOPE: $crate::substrate::cap::advanced::ControlScopeKind =
                $crate::substrate::cap::advanced::ControlScopeKind::Route;
            const TAP_ID: u16 = <$crate::substrate::cap::advanced::RouteDecisionKind as $crate::substrate::cap::ControlResourceKind>::TAP_ID;
            const SHOT: $crate::substrate::cap::CapShot = $crate::substrate::cap::CapShot::One;
            const HANDLING: $crate::substrate::cap::advanced::ControlHandling =
                $crate::substrate::cap::advanced::ControlHandling::Canonical;
        }
    };
}

#[inline]
pub fn pad_handle(bytes: [u8; 6]) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0..6].copy_from_slice(&bytes);
    buf
}

#[inline]
pub fn trim_handle(data: [u8; CAP_HANDLE_LEN]) -> [u8; 6] {
    [data[0], data[1], data[2], data[3], data[4], data[5]]
}

/// Flags stored inside [`SpliceHandle::flags`].
pub(crate) mod splice_flags {
    /// Indicates that `seq_tx` / `seq_rx` contain fence counters.
    pub(crate) const FENCES_PRESENT: u16 = 0x0001;
}

/// Handle payload for distributed splice operations (intent + ack).
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
pub struct SpliceHandle {
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

impl SpliceHandle {
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

    /// Decode a payload into a [`SpliceHandle`].
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

    /// Returns true when the handle encodes fence counters.
    pub const fn has_fences(&self) -> bool {
        (self.flags & splice_flags::FENCES_PRESENT) != 0
    }

    /// Returns fence counters if encoded.
    pub const fn fences(&self) -> Option<(u32, u32)> {
        if self.has_fences() {
            Some((self.seq_tx, self.seq_rx))
        } else {
            None
        }
    }
}

/// Flags stored inside [`RerouteHandle::flags`].
pub(crate) mod reroute_flags {
    /// Indicates that sequence fences are populated.
    pub(crate) const FENCES_PRESENT: u16 = 0x0001;
}

/// Handle payload for reroute operations.
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
/// [20..22)  flags (see [`reroute_flags`])
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RerouteHandle {
    pub src_rv: u16,
    pub dst_rv: u16,
    pub src_lane: u16,
    pub dst_lane: u16,
    pub seq_tx: u32,
    pub seq_rx: u32,
    pub shard: u32,
    pub flags: u16,
}

impl RerouteHandle {
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

    pub const fn has_fences(&self) -> bool {
        (self.flags & reroute_flags::FENCES_PRESENT) != 0
    }

    pub const fn fences(&self) -> Option<(u32, u32)> {
        if self.has_fences() {
            Some((self.seq_tx, self.seq_rx))
        } else {
            None
        }
    }
}

/// Route decision handle carrying the selected arm and scope trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RouteDecisionHandle {
    pub scope: ScopeId,
    pub arm: u8,
}

impl RouteDecisionHandle {
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

impl ControlHandle for RouteDecisionHandle {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {}
}

impl ControlHandle for SpliceHandle {
    fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId)) {
        f(RendezvousId::new(self.src_rv));
        f(RendezvousId::new(self.dst_rv));
    }
}

impl ControlHandle for RerouteHandle {
    fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId)) {
        f(RendezvousId::new(self.src_rv));
        f(RendezvousId::new(self.dst_rv));
    }
}

impl_control_resource!(
    LoopContinueKind,
    handle: LoopDecision,
    tag: 0x40,
    name: "LoopContinue",
    label: consts::LABEL_LOOP_CONTINUE,
    caps: CapsMask::empty().with(CpEffect::Delegate),
);

impl_control_resource!(
    LoopBreakKind,
    handle: LoopDecision,
    tag: 0x41,
    name: "LoopBreak",
    label: consts::LABEL_LOOP_BREAK,
    caps: CapsMask::empty(),
);

impl_control_resource!(
    CheckpointKind,
    handle: SessionScoped,
    tag: 0x42,
    name: "Checkpoint",
    label: consts::LABEL_CHECKPOINT,
    scope: Checkpoint,
    tap_id: ids::CHECKPOINT_REQ,
    caps: CapsMask::empty().with(CpEffect::Checkpoint),
    handling: Canonical,
);

impl_control_resource!(
    CommitKind,
    handle: SessionScoped,
    tag: 0x43,
    name: "Commit",
    label: consts::LABEL_COMMIT,
    scope: Checkpoint,
    tap_id: ids::POLICY_RA_OK,
    caps: CapsMask::empty().with(CpEffect::SpliceCommit),
    handling: Canonical,
);

impl_control_resource!(
    RollbackKind,
    handle: SessionScoped,
    tag: 0x44,
    name: "Rollback",
    label: consts::LABEL_ROLLBACK,
    scope: Checkpoint,
    tap_id: ids::ROLLBACK_REQ,
    caps: CapsMask::empty().with(CpEffect::Rollback),
    handling: Canonical,
);

impl_control_resource!(
    CancelKind,
    handle: SessionScoped,
    tag: 0x45,
    name: "Cancel",
    label: consts::LABEL_CANCEL,
    scope: Cancel,
    tap_id: ids::CANCEL_BEGIN,
    caps: CapsMask::empty(),
    handling: Canonical,
);

impl_control_resource!(
    CancelAckKind,
    handle: SessionScoped,
    tag: 0x46,
    name: "CancelAck",
    label: consts::LABEL_CANCEL,
    scope: Cancel,
    tap_id: ids::CANCEL_ACK,
    caps: CapsMask::empty(),
    handling: Canonical,
);

/// Splice intent (cross-role, wire transmission).
///
/// Used for distributed splice where intent is communicated across roles.
/// Uses ExternalControl with AUTO_MINT_EXTERNAL for automatic token minting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpliceIntentKind;

impl ResourceKind for SpliceIntentKind {
    type Handle = SpliceHandle;
    const TAG: u8 = 0x57;
    const NAME: &'static str = "SpliceIntent";

    /// Splice intent requires auto-minting to populate the proper
    /// handle with splice operands from the resolver.
    const AUTO_MINT_EXTERNAL: bool = true;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        SpliceHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
            .with(CpEffect::SpliceBegin)
            .with(CpEffect::SpliceCommit)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl SessionScopedKind for SpliceIntentKind {
    fn handle_for_session(_sid: SessionId, lane: Lane) -> Self::Handle {
        SpliceHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: 0,
            old_gen: 0,
            new_gen: 0,
            seq_tx: 0,
            seq_rx: 0,
            flags: 0,
        }
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl ControlResourceKind for SpliceIntentKind {
    const LABEL: u8 = consts::LABEL_SPLICE_INTENT;
    const SCOPE: ControlScopeKind = ControlScopeKind::Splice;
    const TAP_ID: u16 = ids::SPLICE_BEGIN;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::External;
}

impl ControlMint for SpliceIntentKind {
    fn mint_handle(_sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        SpliceHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: 0,
            old_gen: 0,
            new_gen: 0,
            seq_tx: 0,
            seq_rx: 0,
            flags: 0,
        }
    }
}

/// Splice acknowledgement (cross-role, wire transmission).
///
/// Used for distributed splice where ack is communicated across roles.
/// Uses ExternalControl with AUTO_MINT_EXTERNAL for automatic token minting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpliceAckKind;

impl ResourceKind for SpliceAckKind {
    type Handle = SpliceHandle;
    const TAG: u8 = 0x58;
    const NAME: &'static str = "SpliceAck";

    /// Splice ack requires auto-minting to populate the proper
    /// handle with splice operands from cached context.
    const AUTO_MINT_EXTERNAL: bool = true;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        SpliceHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty().with(CpEffect::SpliceAck)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl SessionScopedKind for SpliceAckKind {
    fn handle_for_session(_sid: SessionId, lane: Lane) -> Self::Handle {
        SpliceHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: lane.raw() as u16,
            old_gen: 0,
            new_gen: 0,
            seq_tx: 0,
            seq_rx: 0,
            flags: 0,
        }
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl ControlResourceKind for SpliceAckKind {
    const LABEL: u8 = consts::LABEL_SPLICE_ACK;
    const SCOPE: ControlScopeKind = ControlScopeKind::Splice;
    const TAP_ID: u16 = ids::SPLICE_COMMIT;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::External;
}

impl ControlMint for SpliceAckKind {
    fn mint_handle(_sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        SpliceHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: lane.raw() as u16,
            old_gen: 0,
            new_gen: 0,
            seq_tx: 0,
            seq_rx: 0,
            flags: 0,
        }
    }
}

/// Reroute request.
///
/// Payload: `(sid:u32, lane:u16)`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RerouteKind;

impl ResourceKind for RerouteKind {
    type Handle = RerouteHandle;
    const TAG: u8 = 0x49;
    const NAME: &'static str = "Reroute";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        RerouteHandle::decode(data)
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty().with(CpEffect::Delegate)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl SessionScopedKind for RerouteKind {
    fn handle_for_session(_sid: SessionId, lane: Lane) -> Self::Handle {
        RerouteHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: lane.raw() as u16,
            seq_tx: 0,
            seq_rx: 0,
            shard: 0,
            flags: 0,
        }
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl ControlResourceKind for RerouteKind {
    const LABEL: u8 = consts::LABEL_REROUTE;
    const SCOPE: ControlScopeKind = ControlScopeKind::Reroute;
    const TAP_ID: u16 = ids::ROUTE_PICK;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::Canonical;
}

impl ControlMint for RerouteKind {
    fn mint_handle(_sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        RerouteHandle {
            src_rv: 0,
            dst_rv: 0,
            src_lane: lane.raw() as u16,
            dst_lane: lane.raw() as u16,
            seq_tx: 0,
            seq_rx: 0,
            shard: 0,
            flags: 0,
        }
    }
}

/// Route decision token (selects a route arm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecisionKind;

impl ResourceKind for RouteDecisionKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = 0x4E;
    const NAME: &'static str = "RouteDecision";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    fn scope_id(handle: &Self::Handle) -> Option<ScopeId> {
        Some(handle.scope)
    }
}

impl SessionScopedKind for RouteDecisionKind {
    fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::default()
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl ControlResourceKind for RouteDecisionKind {
    const LABEL: u8 = consts::LABEL_ROUTE_DECISION;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = ids::ROUTE_PICK;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::Canonical;
}

impl ControlMint for RouteDecisionKind {
    fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
        RouteDecisionHandle { scope, arm: 0 }
    }
}

impl_control_resource!(
    MgmtRouteLoadKind,
    handle: RouteDecision,
    name: "MgmtRouteLoad",
    label: consts::LABEL_MGMT_ROUTE_LOAD,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteActivateKind,
    handle: RouteDecision,
    name: "MgmtRouteActivate",
    label: consts::LABEL_MGMT_ROUTE_ACTIVATE,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteRevertKind,
    handle: RouteDecision,
    name: "MgmtRouteRevert",
    label: consts::LABEL_MGMT_ROUTE_REVERT,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteStatsKind,
    handle: RouteDecision,
    name: "MgmtRouteStats",
    label: consts::LABEL_MGMT_ROUTE_STATS,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteLoadFamilyKind,
    handle: RouteDecision,
    name: "MgmtRouteLoadFamily",
    label: consts::LABEL_MGMT_ROUTE_LOAD_FAMILY,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteLoadAndActivateKind,
    handle: RouteDecision,
    name: "MgmtRouteLoadAndActivate",
    label: consts::LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteReplyErrorKind,
    handle: RouteDecision,
    name: "MgmtRouteReplyError",
    label: consts::LABEL_MGMT_ROUTE_REPLY_ERROR,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteReplyLoadedKind,
    handle: RouteDecision,
    name: "MgmtRouteReplyLoaded",
    label: consts::LABEL_MGMT_ROUTE_REPLY_LOADED,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteReplyActivatedKind,
    handle: RouteDecision,
    name: "MgmtRouteReplyActivated",
    label: consts::LABEL_MGMT_ROUTE_REPLY_ACTIVATED,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteReplyRevertedKind,
    handle: RouteDecision,
    name: "MgmtRouteReplyReverted",
    label: consts::LABEL_MGMT_ROUTE_REPLY_REVERTED,
    arm: 0,
);

impl_control_resource!(
    MgmtRouteReplyStatsKind,
    handle: RouteDecision,
    name: "MgmtRouteReplyStats",
    label: consts::LABEL_MGMT_ROUTE_REPLY_STATS,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteCommandFamilyKind,
    handle: RouteDecision,
    name: "MgmtRouteCommandFamily",
    label: consts::LABEL_MGMT_ROUTE_COMMAND_FAMILY,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteCommandTailKind,
    handle: RouteDecision,
    name: "MgmtRouteCommandTail",
    label: consts::LABEL_MGMT_ROUTE_COMMAND_TAIL,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteReplySuccessFamilyKind,
    handle: RouteDecision,
    name: "MgmtRouteReplySuccessFamily",
    label: consts::LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteReplySuccessTailKind,
    handle: RouteDecision,
    name: "MgmtRouteReplySuccessTail",
    label: consts::LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL,
    arm: 1,
);

impl_control_resource!(
    MgmtRouteReplySuccessFinalKind,
    handle: RouteDecision,
    name: "MgmtRouteReplySuccessFinal",
    label: consts::LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL,
    arm: 1,
);

impl_control_resource!(
    PolicyLoadKind,
    handle: PolicyHash,
    tag: 0x4A,
    name: "PolicyLoad",
    label: consts::LABEL_POLICY_LOAD,
    scope: Policy,
    tap_id: ids::POLICY_EFFECT,
    caps: CapsMask::empty().with(CpEffect::Fence),
    handling: Canonical,
);

impl_control_resource!(
    PolicyActivateKind,
    handle: PolicyHash,
    tag: 0x4B,
    name: "PolicyActivate",
    label: consts::LABEL_POLICY_ACTIVATE,
    scope: Policy,
    tap_id: ids::POLICY_COMMIT,
    caps: CapsMask::empty().with(CpEffect::Fence),
    handling: Canonical,
);

impl_control_resource!(
    PolicyRevertKind,
    handle: PolicyHash,
    tag: 0x4C,
    name: "PolicyRevert",
    label: consts::LABEL_POLICY_REVERT,
    scope: Policy,
    tap_id: ids::POLICY_ROLLBACK,
    caps: CapsMask::empty().with(CpEffect::Fence),
    handling: Canonical,
);

/// Policy annotate decision.
///
/// Payload: `(key:u24, value:u24)` stored as `(u32, u16)` where high 8 bits are zero
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyAnnotateKind;

impl ResourceKind for PolicyAnnotateKind {
    type Handle = (u32, u32); // (key, value) both u24 but stored as u32
    const TAG: u8 = 0x4D;
    const NAME: &'static str = "PolicyAnnotate";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; 6];
        buf[0..3].copy_from_slice(&handle.0.to_le_bytes()[0..3]);
        buf[3..6].copy_from_slice(&handle.1.to_le_bytes()[0..3]);
        pad_handle(buf)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let data = trim_handle(data);
        let key = u32::from_le_bytes([data[0], data[1], data[2], 0]);
        let value = u32::from_le_bytes([data[3], data[4], data[5], 0]);
        Ok((key, value))
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty().with(CpEffect::Fence)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl ControlResourceKind for PolicyAnnotateKind {
    const LABEL: u8 = consts::LABEL_POLICY_ANNOTATE;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = ids::POLICY_ANNOT;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::Canonical;
}

// Extended range: 0x50-0x5F for management-specific control operations

/// Policy load begin (management session).
///
/// Payload: `(slot:u8, hash_low:u32, hash_high:u8)` - slot + 40-bit hash
/// Additional metadata (code_len, fuel_max, mem_len) travels as separate wire message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadBeginKind;

impl ResourceKind for LoadBeginKind {
    type Handle = (u8, u64); // (slot, hash40)
    const TAG: u8 = 0x50;
    const NAME: &'static str = "LoadBegin";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; 6];
        buf[0] = handle.0; // slot
        // Pack lower 40 bits of hash into remaining 5 bytes
        let hash_bytes = handle.1.to_le_bytes();
        buf[1..6].copy_from_slice(&hash_bytes[0..5]);
        pad_handle(buf)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let data = trim_handle(data);
        let slot = data[0];
        let mut hash_bytes = [0u8; 8];
        hash_bytes[0..5].copy_from_slice(&data[1..6]);
        let hash = u64::from_le_bytes(hash_bytes);
        Ok((slot, hash))
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty().with(CpEffect::Fence)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl ControlResourceKind for LoadBeginKind {
    const LABEL: u8 = consts::LABEL_MGMT_LOAD_BEGIN;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = ids::POLICY_EFFECT;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::External;
}

impl ControlMint for LoadBeginKind {
    fn mint_handle(_sid: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {
        (0, 0) // AUTO_MINT_EXTERNAL = false
    }
}

/// Policy load commit (management session).
///
/// Payload: `(slot:u8, _reserved:u40)`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadCommitKind;

/// Derive the [`CapsMask`] associated with a resource tag and raw handle bytes.
///
/// Used by the rendezvous control plane to validate capability tokens during
/// mint/claim operations without reconstructing ResourceKind types at runtime.
pub(crate) fn caps_mask_from_tag(tag: u8, raw: [u8; CAP_HANDLE_LEN]) -> Result<CapsMask, CapError> {
    macro_rules! decode_mask {
        ($kind:ty) => {{
            let mut handle = <$kind as ResourceKind>::decode_handle(raw)?;
            let mask = <$kind as ResourceKind>::caps_mask(&handle);
            <$kind as ResourceKind>::zeroize(&mut handle);
            Ok(mask)
        }};
    }

    match tag {
        LoopContinueKind::TAG => decode_mask!(LoopContinueKind),
        LoopBreakKind::TAG => decode_mask!(LoopBreakKind),
        CheckpointKind::TAG => decode_mask!(CheckpointKind),
        CommitKind::TAG => decode_mask!(CommitKind),
        RollbackKind::TAG => decode_mask!(RollbackKind),
        CancelKind::TAG => decode_mask!(CancelKind),
        CancelAckKind::TAG => decode_mask!(CancelAckKind),
        SpliceIntentKind::TAG => decode_mask!(SpliceIntentKind),
        SpliceAckKind::TAG => decode_mask!(SpliceAckKind),
        RerouteKind::TAG => decode_mask!(RerouteKind),
        RouteDecisionKind::TAG => decode_mask!(RouteDecisionKind),
        PolicyLoadKind::TAG => decode_mask!(PolicyLoadKind),
        PolicyActivateKind::TAG => decode_mask!(PolicyActivateKind),
        PolicyRevertKind::TAG => decode_mask!(PolicyRevertKind),
        PolicyAnnotateKind::TAG => decode_mask!(PolicyAnnotateKind),
        LoadBeginKind::TAG => decode_mask!(LoadBeginKind),
        LoadCommitKind::TAG => decode_mask!(LoadCommitKind),
        _ => Err(CapError::Mismatch),
    }
}

impl ResourceKind for LoadCommitKind {
    type Handle = u8; // slot
    const TAG: u8 = 0x51;
    const NAME: &'static str = "LoadCommit";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; 6];
        buf[0] = *handle;
        pad_handle(buf)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let data = trim_handle(data);
        Ok(data[0])
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty().with(CpEffect::Fence)
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl ControlResourceKind for LoadCommitKind {
    const LABEL: u8 = consts::LABEL_MGMT_LOAD_COMMIT;
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = ids::POLICY_COMMIT;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: crate::global::ControlHandling = crate::global::ControlHandling::External;
}

impl ControlMint for LoadCommitKind {
    fn mint_handle(_sid: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {
        0 // AUTO_MINT_EXTERNAL = false
    }
}
