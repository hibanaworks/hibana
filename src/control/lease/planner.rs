//! LeaseGraph planner and static capacity checks.
//!
//! This module performs a const-time analysis over projected policy markers to
//! determine how many LeaseGraph children are required for delegation and
//! topology automatons. The resulting budget is validated against the capacities
//! advertised by each `LeaseSpec`, triggering a compile-time panic when a
//! program requests more links than the runtime can provision.

use crate::{
    control::cap::mint::ControlOp,
    global::{ControlDesc, const_dsl::PolicyMode},
};

pub(crate) const FACET_CAPS: u8 = 1 << 0;
pub(crate) const FACET_TOPOLOGY: u8 = 1 << 1;
pub(crate) const FACET_DELEGATION: u8 = 1 << 2;

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
            bits: FACET_CAPS | FACET_TOPOLOGY | FACET_DELEGATION,
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
    pub(crate) const fn requires_topology(&self) -> bool {
        (self.bits & FACET_TOPOLOGY) != 0
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
        if self.requires_topology() {
            if wrote {
                f.write_str("|")?;
            }
            f.write_str("topology")?;
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
pub(crate) const fn facets(caps: bool, topology: bool, delegation: bool) -> LeaseFacetNeeds {
    let mut bits = 0;
    if caps {
        bits |= FACET_CAPS;
    }
    if topology {
        bits |= FACET_TOPOLOGY;
    }
    if delegation {
        bits |= FACET_DELEGATION;
    }
    LeaseFacetNeeds::from_bits(bits)
}

#[inline(always)]
pub(crate) const fn facets_caps() -> LeaseFacetNeeds {
    facets(true, false, false)
}

#[inline(always)]
pub(crate) const fn facets_caps_topology() -> LeaseFacetNeeds {
    facets(true, true, false)
}

#[inline(always)]
pub(crate) const fn facets_caps_delegation() -> LeaseFacetNeeds {
    facets(true, false, true)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PolicyRequirements {
    pub(crate) delegation_children: usize,
    pub(crate) topology_children: usize,
    pub(crate) facets: LeaseFacetNeeds,
}

impl PolicyRequirements {
    const fn new() -> Self {
        Self {
            delegation_children: 0,
            topology_children: 0,
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
    pub(crate) topology_children: usize,
    facets: LeaseFacetNeeds,
}

impl LeaseGraphBudget {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            delegation_children: 0,
            topology_children: 0,
            facets: LeaseFacetNeeds::new(),
        }
    }

    #[inline(always)]
    pub(crate) const fn include_atom(
        mut self,
        control_desc: Option<ControlDesc>,
        policy: PolicyMode,
    ) -> Self {
        let req = policy_requirements(control_desc, policy);
        if req.delegation_children > self.delegation_children {
            self.delegation_children = req.delegation_children;
        }
        if req.topology_children > self.topology_children {
            self.topology_children = req.topology_children;
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

        if self.topology_children > 0 {
            if self.topology_children
                > crate::control::automaton::topology::TOPOLOGY_LEASE_MAX_CHILDREN
            {
                panic!("topology lease child capacity exceeded");
            }
            if self.topology_children + 1
                > crate::control::automaton::topology::TOPOLOGY_LEASE_MAX_NODES
            {
                panic!("topology lease node capacity exceeded");
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
    pub(crate) const fn requires_topology(&self) -> bool {
        self.facets.requires_topology()
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn requires_delegation(&self) -> bool {
        self.facets.requires_delegation()
    }
}

#[inline(always)]
pub(crate) const fn policy_requirements(
    control_desc: Option<ControlDesc>,
    policy: PolicyMode,
) -> PolicyRequirements {
    let mut req = match control_desc {
        Some(desc) => PolicyRequirements::with_facets(base_facets_for_control(desc)),
        None => PolicyRequirements::new(),
    };

    let Some(desc) = control_desc else {
        return req;
    };

    // Dynamic policies on topology/reroute control ops require additional resources.
    if policy.is_dynamic() {
        match desc.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck => {
                req.delegation_children = 2;
                req.topology_children = 1;
            }
            ControlOp::CapDelegate => {
                req.delegation_children = 2;
            }
            _ => {}
        }
    }

    req
}

const fn base_facets_for_control(desc: ControlDesc) -> LeaseFacetNeeds {
    match desc.op() {
        ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
            facets_caps_topology()
        }
        ControlOp::CapDelegate => facets_caps_delegation(),
        ControlOp::AbortBegin
        | ControlOp::AbortAck
        | ControlOp::StateSnapshot
        | ControlOp::TxCommit
        | ControlOp::TxAbort
        | ControlOp::StateRestore => facets_caps(),
        ControlOp::Fence
        | ControlOp::RouteDecision
        | ControlOp::LoopContinue
        | ControlOp::LoopBreak => LeaseFacetNeeds::new(),
    }
}
