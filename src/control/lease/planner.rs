//! LeaseGraph planner and static capacity checks.
//!
//! This module performs a const-time analysis over projected policy markers to
//! determine how many LeaseGraph children are required for delegation and
//! splice automatons. The resulting budget is validated against the capacities
//! advertised by each `LeaseSpec`, triggering a compile-time panic when a
//! program requests more links than the runtime can provision.

use crate::{
    control::cap::mint::ControlOp,
    global::{
        StaticControlDesc,
        const_dsl::{ControlScopeKind, PolicyMode},
    },
};

pub(crate) const FACET_CAPS: u8 = 1 << 0;
pub(crate) const FACET_SLOTS: u8 = 1 << 1;
pub(crate) const FACET_SPLICE: u8 = 1 << 2;
pub(crate) const FACET_DELEGATION: u8 = 1 << 3;

/// Maximum number of delegation links tracked in [`DelegationChildSet`].
pub(crate) const DELEGATION_CHILD_SET_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LeaseFacetNeeds {
    bits: u8,
}

impl LeaseFacetNeeds {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { bits: 0 }
    }

    #[inline(always)]
    pub(crate) const fn all() -> Self {
        Self {
            bits: FACET_CAPS | FACET_SLOTS | FACET_SPLICE | FACET_DELEGATION,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    #[inline(always)]
    pub(crate) const fn requires_caps(&self) -> bool {
        (self.bits & FACET_CAPS) != 0
    }

    #[inline(always)]
    pub(crate) const fn requires_slots(&self) -> bool {
        (self.bits & FACET_SLOTS) != 0
    }

    #[inline(always)]
    pub(crate) const fn requires_splice(&self) -> bool {
        (self.bits & FACET_SPLICE) != 0
    }

    #[inline(always)]
    pub(crate) const fn requires_delegation(&self) -> bool {
        (self.bits & FACET_DELEGATION) != 0
    }
}

impl core::fmt::Display for LeaseFacetNeeds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut wrote = false;
        if self.requires_caps() {
            f.write_str("caps")?;
            wrote = true;
        }
        if self.requires_slots() {
            if wrote {
                f.write_str("|")?;
            }
            f.write_str("slots")?;
            wrote = true;
        }
        if self.requires_splice() {
            if wrote {
                f.write_str("|")?;
            }
            f.write_str("splice")?;
            wrote = true;
        }
        if self.requires_delegation() {
            if wrote {
                f.write_str("|")?;
            }
            f.write_str("delegation")?;
            wrote = true;
        }
        if !wrote {
            f.write_str("-")?;
        }
        Ok(())
    }
}

#[inline(always)]
pub(crate) const fn facets(
    caps: bool,
    slots: bool,
    splice: bool,
    delegation: bool,
) -> LeaseFacetNeeds {
    let mut bits = 0;
    if caps {
        bits |= FACET_CAPS;
    }
    if slots {
        bits |= FACET_SLOTS;
    }
    if splice {
        bits |= FACET_SPLICE;
    }
    if delegation {
        bits |= FACET_DELEGATION;
    }
    LeaseFacetNeeds::from_bits(bits)
}

#[inline(always)]
pub(crate) const fn facets_caps() -> LeaseFacetNeeds {
    facets(true, false, false, false)
}

#[inline(always)]
pub(crate) const fn facets_slots() -> LeaseFacetNeeds {
    facets(false, true, false, false)
}

#[inline(always)]
pub(crate) const fn facets_caps_splice() -> LeaseFacetNeeds {
    facets(true, false, true, false)
}

#[inline(always)]
pub(crate) const fn facets_caps_delegation() -> LeaseFacetNeeds {
    facets(true, false, false, true)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PolicyRequirements {
    pub(crate) delegation_children: usize,
    pub(crate) splice_children: usize,
    pub(crate) facets: LeaseFacetNeeds,
}

impl PolicyRequirements {
    const fn new() -> Self {
        Self {
            delegation_children: 0,
            splice_children: 0,
            facets: LeaseFacetNeeds::new(),
        }
    }

    const fn with_facets(facets: LeaseFacetNeeds) -> Self {
        Self {
            facets,
            ..Self::new()
        }
    }
}

/// Summary of the LeaseGraph capacity required by a projected program.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LeaseGraphBudget {
    pub(crate) delegation_children: usize,
    pub(crate) splice_children: usize,
    facets: LeaseFacetNeeds,
}

impl LeaseGraphBudget {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            delegation_children: 0,
            splice_children: 0,
            facets: LeaseFacetNeeds::new(),
        }
    }

    #[inline(always)]
    pub(crate) const fn include_atom(
        mut self,
        control_spec: Option<StaticControlDesc>,
        policy: PolicyMode,
    ) -> Self {
        let req = policy_requirements(control_spec, policy);
        if req.delegation_children > self.delegation_children {
            self.delegation_children = req.delegation_children;
        }
        if req.splice_children > self.splice_children {
            self.splice_children = req.splice_children;
        }
        self.facets = self.facets.union(req.facets);
        self
    }

    /// Trigger a compile-time panic if the analysed program exceeds the
    /// capacity baked into the LeaseGraph specifications.
    pub(crate) const fn validate(&self) {
        if self.delegation_children > 0 {
            if self.delegation_children > DELEGATION_CHILD_SET_CAPACITY {
                panic!("delegation child set capacity exceeded");
            }
            if self.delegation_children
                > crate::control::automaton::delegation::DELEGATION_LEASE_MAX_CHILDREN
            {
                panic!("delegation lease child capacity exceeded");
            }
            if self.delegation_children + 1
                > crate::control::automaton::delegation::DELEGATION_LEASE_MAX_NODES
            {
                panic!("delegation lease node capacity exceeded");
            }
        }

        if self.splice_children > 0 {
            if self.splice_children > crate::control::automaton::splice::SPLICE_LEASE_MAX_CHILDREN {
                panic!("splice lease child capacity exceeded");
            }
            if self.splice_children + 1 > crate::control::automaton::splice::SPLICE_LEASE_MAX_NODES
            {
                panic!("splice lease node capacity exceeded");
            }
        }
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn requires_caps(&self) -> bool {
        self.facets.requires_caps()
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn requires_slots(&self) -> bool {
        self.facets.requires_slots()
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn requires_splice(&self) -> bool {
        self.facets.requires_splice()
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn requires_delegation(&self) -> bool {
        self.facets.requires_delegation()
    }
}

#[inline(always)]
pub(crate) const fn facet_needs(
    control_spec: Option<StaticControlDesc>,
    policy: PolicyMode,
) -> LeaseFacetNeeds {
    policy_facets(control_spec, policy)
}

const fn policy_facets(
    control_spec: Option<StaticControlDesc>,
    policy: PolicyMode,
) -> LeaseFacetNeeds {
    policy_requirements(control_spec, policy).facets
}

#[inline(always)]
pub(crate) const fn policy_requirements(
    control_spec: Option<StaticControlDesc>,
    policy: PolicyMode,
) -> PolicyRequirements {
    let mut req = match control_spec {
        Some(spec) => PolicyRequirements::with_facets(base_facets_for_control(spec)),
        None => PolicyRequirements::new(),
    };

    let Some(spec) = control_spec else {
        return req;
    };

    // Dynamic policies on splice/reroute control ops require additional resources.
    if policy.is_dynamic() {
        match spec.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck => {
                req.delegation_children = 2;
                req.splice_children = 1;
            }
            ControlOp::CapDelegate => {
                req.delegation_children = 2;
            }
            _ => {}
        }
    }

    req
}

const fn base_facets_for_control(spec: StaticControlDesc) -> LeaseFacetNeeds {
    let mut facets = match spec.op() {
        ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
            facets_caps_splice()
        }
        ControlOp::CapDelegate => facets_caps_delegation(),
        ControlOp::AbortBegin
        | ControlOp::AbortAck
        | ControlOp::StateSnapshot
        | ControlOp::TxCommit
        | ControlOp::StateRestore
        | ControlOp::TxAbort => facets_caps(),
        ControlOp::Fence
        | ControlOp::RouteDecision
        | ControlOp::LoopContinue
        | ControlOp::LoopBreak => LeaseFacetNeeds::new(),
    };

    if matches!(spec.scope_kind(), ControlScopeKind::Policy) {
        facets = facets.union(facets_slots());
    }

    facets
}
