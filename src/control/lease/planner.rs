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

/// Maximum number of delegation links tracked in `DelegationChildSet`.
pub(crate) const DELEGATION_CHILD_SET_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PolicyRequirements {
    pub(crate) delegation_children: usize,
    pub(crate) topology_children: usize,
}

impl PolicyRequirements {
    const fn new() -> Self {
        Self {
            delegation_children: 0,
            topology_children: 0,
        }
    }
}

/// Summary of the LeaseGraph capacity required by a projected program.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LeaseGraphBudget {
    pub(crate) delegation_children: usize,
    pub(crate) topology_children: usize,
}

impl LeaseGraphBudget {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            delegation_children: 0,
            topology_children: 0,
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
}

#[inline(always)]
pub(crate) const fn policy_requirements(
    control_desc: Option<ControlDesc>,
    policy: PolicyMode,
) -> PolicyRequirements {
    let mut req = PolicyRequirements::new();

    let Some(desc) = control_desc else {
        return req;
    };

    // Dynamic policies on topology control ops require additional resources.
    if policy.is_dynamic() {
        match desc.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck => {
                req.delegation_children = 2;
                req.topology_children = 1;
            }
            _ => {}
        }
    }

    req
}
