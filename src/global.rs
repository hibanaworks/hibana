//! Global session type DSL (iso-recursive).
//!
//! This module exposes the primitives needed to assemble global choreographies
//! as local choreography witnesses and project them to role-local views.

use crate::control::cap::mint::{LocalControlKind, WireControlKind};
use crate::eff::EffIndex;
pub(crate) use types::ROLE_DOMAIN_SIZE;

/// Crate-private lowering owners for unified compilation.
pub(crate) mod compiled;
/// Const-evaluated DSL and effect list plumbing.
pub(crate) mod const_dsl;
/// Descriptor-backed local affine event program rows.
pub(crate) mod event_program;
mod message;
/// Program combinators and route builders.
pub(crate) mod program;
pub use message::Message;
pub(crate) use message::MessageRuntime;
/// Role-local program projection and metadata.
pub(crate) mod role_program;
pub(crate) use role_program::RoleProgramView;
#[cfg(test)]
mod event_program_tests;
/// Type-level step combinators.
pub(crate) mod steps;
/// Role-domain constants consumed by lowering/runtime internals.
mod types;
/// Typestate graph and cursor infrastructure.
pub(crate) mod typestate;

/// Static control-message metadata used across the DSL and runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticControlDesc {
    resource_tag: u8,
    scope_kind: const_dsl::ControlScopeKind,
    path: crate::control::cap::mint::ControlPath,
    tap_id: u16,
    shot: crate::control::cap::mint::CapShot,
    op: crate::control::cap::mint::ControlOp,
}

impl StaticControlDesc {
    pub(crate) const fn of_wire<K>() -> Self
    where
        K: WireControlKind,
    {
        Self {
            resource_tag: K::TAG,
            scope_kind: const_dsl::ControlScopeKind::Policy,
            path: crate::control::cap::mint::ControlPath::Wire,
            tap_id: crate::control::cluster::effects::control_op_tap_event_id(
                crate::control::cap::mint::ControlOp::Fence,
            ),
            shot: crate::control::cap::mint::CapShot::Many,
            op: crate::control::cap::mint::ControlOp::Fence,
        }
    }

    pub(crate) const fn of_local<K>() -> Self
    where
        K: LocalControlKind,
    {
        if K::TAP_ID == 0 {
            panic!("control TAP_ID must be explicit");
        }
        Self {
            resource_tag: K::TAG,
            scope_kind: K::SCOPE,
            path: crate::control::cap::mint::ControlPath::Local,
            tap_id: K::TAP_ID,
            shot: K::SHOT,
            op: K::OP,
        }
    }

    pub(crate) const fn resource_tag(self) -> u8 {
        self.resource_tag
    }

    pub(crate) const fn scope_kind(self) -> const_dsl::ControlScopeKind {
        self.scope_kind
    }

    pub(crate) const fn tap_id(self) -> u16 {
        self.tap_id
    }

    pub(crate) const fn path(self) -> crate::control::cap::mint::ControlPath {
        self.path
    }

    pub(crate) const fn shot(self) -> crate::control::cap::mint::CapShot {
        self.shot
    }

    pub(crate) const fn op(self) -> crate::control::cap::mint::ControlOp {
        self.op
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlDesc {
    eff_index: EffIndex,
    policy_site: u16,
    tap_id: u16,
    resource_tag: u8,
    op: crate::control::cap::mint::ControlOp,
    scope_kind: const_dsl::ControlScopeKind,
    flags: u8,
}

impl ControlDesc {
    pub(crate) const STATIC_POLICY_SITE: u16 = u16::MAX;
    const PATH_MASK: u8 = 0b0000_0001;
    const SHOT_MASK: u8 = 0b0000_0010;

    #[inline(always)]
    pub(crate) const fn from_static(spec: StaticControlDesc) -> Self {
        Self::new(
            EffIndex::MAX,
            Self::STATIC_POLICY_SITE,
            spec.tap_id(),
            spec.resource_tag(),
            spec.op(),
            spec.scope_kind(),
            spec.path(),
            spec.shot(),
        )
    }

    #[inline(always)]
    pub(crate) const fn with_sites(self, eff_index: EffIndex, policy_site: u16) -> Self {
        Self {
            eff_index,
            policy_site,
            tap_id: self.tap_id,
            resource_tag: self.resource_tag,
            op: self.op,
            scope_kind: self.scope_kind,
            flags: self.flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        policy_site: u16,
        tap_id: u16,
        resource_tag: u8,
        op: crate::control::cap::mint::ControlOp,
        scope_kind: const_dsl::ControlScopeKind,
        path: crate::control::cap::mint::ControlPath,
        shot: crate::control::cap::mint::CapShot,
    ) -> Self {
        let mut flags = path.as_u8() & Self::PATH_MASK;
        if matches!(shot, crate::control::cap::mint::CapShot::Many) {
            flags |= Self::SHOT_MASK;
        }
        Self {
            eff_index,
            policy_site,
            tap_id,
            resource_tag,
            op,
            scope_kind,
            flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn policy_site(self) -> u16 {
        self.policy_site
    }

    #[inline(always)]
    pub(crate) const fn tap_id(self) -> u16 {
        self.tap_id
    }

    #[inline(always)]
    pub(crate) const fn resource_tag(self) -> u8 {
        self.resource_tag
    }

    #[inline(always)]
    pub(crate) const fn op(self) -> crate::control::cap::mint::ControlOp {
        self.op
    }

    #[inline(always)]
    pub(crate) const fn scope_kind(self) -> const_dsl::ControlScopeKind {
        self.scope_kind
    }

    #[inline(always)]
    pub(crate) const fn path(self) -> crate::control::cap::mint::ControlPath {
        if (self.flags & Self::PATH_MASK) == 0 {
            crate::control::cap::mint::ControlPath::Local
        } else {
            crate::control::cap::mint::ControlPath::Wire
        }
    }

    #[inline(always)]
    pub(crate) const fn shot(self) -> crate::control::cap::mint::CapShot {
        if (self.flags & Self::SHOT_MASK) == 0 {
            crate::control::cap::mint::CapShot::One
        } else {
            crate::control::cap::mint::CapShot::Many
        }
    }

    #[inline(always)]
    pub(crate) const fn header_flags(self) -> u8 {
        0
    }

    #[inline(always)]
    pub(crate) const fn supports_dynamic_policy(self) -> bool {
        matches!(
            self.op(),
            crate::control::cap::mint::ControlOp::RouteDecision
                | crate::control::cap::mint::ControlOp::LoopContinue
                | crate::control::cap::mint::ControlOp::LoopBreak
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopControlMeaning {
    Continue,
    Break,
}

impl LoopControlMeaning {
    pub(crate) const fn from_control_desc(desc: Option<ControlDesc>) -> Option<Self> {
        match desc {
            Some(desc) => {
                if !matches!(desc.scope_kind(), const_dsl::ControlScopeKind::Loop) {
                    return None;
                }
                match desc.op() {
                    crate::control::cap::mint::ControlOp::LoopContinue => Some(Self::Continue),
                    crate::control::cap::mint::ControlOp::LoopBreak => Some(Self::Break),
                    _ => None,
                }
            }
            None => None,
        }
    }

    pub(crate) const fn from_control_spec(spec: Option<StaticControlDesc>) -> Option<Self> {
        match spec {
            Some(spec) => Self::from_control_desc(Some(ControlDesc::from_static(spec))),
            None => None,
        }
    }

    pub(crate) const fn from_semantic(
        semantic: crate::global::compiled::images::ControlSemanticKind,
    ) -> Option<Self> {
        match semantic {
            crate::global::compiled::images::ControlSemanticKind::LoopContinue => {
                Some(Self::Continue)
            }
            crate::global::compiled::images::ControlSemanticKind::LoopBreak => Some(Self::Break),
            _ => None,
        }
    }
}

/// Per-message control metadata helper trait.
pub(crate) trait MessageControlSpec {
    const CONTROL: Option<StaticControlDesc>;
    const CONTROL_PAYLOAD: bool;
    const CONTROL_PAYLOAD_KIND: u8;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    >;
}

impl<const LOGICAL_LABEL: u8, P> MessageControlSpec for crate::g::Msg<LOGICAL_LABEL, P>
where
    P: crate::transport::wire::WirePayload,
{
    const CONTROL: Option<StaticControlDesc> = None;
    const CONTROL_PAYLOAD: bool = false;
    const CONTROL_PAYLOAD_KIND: u8 = 0;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = None;
}

impl<const LOGICAL_LABEL: u8, K> MessageControlSpec for crate::g::ControlMsg<LOGICAL_LABEL, K>
where
    K: LocalControlKind,
{
    const CONTROL: Option<StaticControlDesc> = Some(StaticControlDesc::of_local::<K>());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 = 1;
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = Some(message::encode_local_control_handle_wire_for::<K>);
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
