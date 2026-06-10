//! Control lease capacity checks.
//!
//! This module performs a const-time analysis over projected policy markers to
//! determine how many runtime lease children are required for dynamic policy and
//! topology control. The resulting budget is validated against compact runtime
//! capacities, triggering a compile-time panic when a program requests more
//! links than the runtime can provision.

use crate::{
    control::cap::mint::ControlOp,
    global::{ControlDesc, const_dsl::ResolverMode},
};

pub(crate) const DYNAMIC_POLICY_CHILD_SET_CAPACITY: usize = 4;
pub(crate) const DYNAMIC_POLICY_LEASE_MAX_NODES: usize = 8;
pub(crate) const DYNAMIC_POLICY_LEASE_MAX_CHILDREN: usize = 6;
pub(crate) const TOPOLOGY_LEASE_MAX_NODES: usize = 3;
pub(crate) const TOPOLOGY_LEASE_MAX_CHILDREN: usize = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PolicyRequirements {
    pub(crate) dynamic_policy_children: usize,
    pub(crate) topology_children: usize,
}

impl PolicyRequirements {
    const fn new() -> Self {
        Self {
            dynamic_policy_children: 0,
            topology_children: 0,
        }
    }
}

/// Summary of the control lease capacity required by a projected program.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LeaseCapacityBudget {
    pub(crate) dynamic_policy_children: usize,
    pub(crate) topology_children: usize,
}

impl LeaseCapacityBudget {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            dynamic_policy_children: 0,
            topology_children: 0,
        }
    }

    #[inline(always)]
    pub(crate) const fn include_atom(
        mut self,
        control_desc: Option<ControlDesc>,
        policy: ResolverMode,
    ) -> Self {
        let req = policy_requirements(control_desc, policy);
        if req.dynamic_policy_children > self.dynamic_policy_children {
            self.dynamic_policy_children = req.dynamic_policy_children;
        }
        if req.topology_children > self.topology_children {
            self.topology_children = req.topology_children;
        }
        self
    }

    /// Trigger a compile-time panic if the analysed program exceeds the
    /// runtime lease capacity.
    pub(crate) const fn validate(&self) {
        if self.dynamic_policy_children > 0 {
            if self.dynamic_policy_children > DYNAMIC_POLICY_CHILD_SET_CAPACITY {
                panic!("dynamic policy child set capacity exceeded");
            }
            if self.dynamic_policy_children > DYNAMIC_POLICY_LEASE_MAX_CHILDREN {
                panic!("dynamic policy lease child capacity exceeded");
            }
            if self.dynamic_policy_children + 1 > DYNAMIC_POLICY_LEASE_MAX_NODES {
                panic!("dynamic policy lease node capacity exceeded");
            }
        }

        if self.topology_children > 0 {
            if self.topology_children > TOPOLOGY_LEASE_MAX_CHILDREN {
                panic!("topology lease child capacity exceeded");
            }
            if self.topology_children + 1 > TOPOLOGY_LEASE_MAX_NODES {
                panic!("topology lease node capacity exceeded");
            }
        }
    }
}

#[inline(always)]
pub(crate) const fn policy_requirements(
    control_desc: Option<ControlDesc>,
    policy: ResolverMode,
) -> PolicyRequirements {
    let mut req = PolicyRequirements::new();

    let Some(desc) = control_desc else {
        return req;
    };

    // Dynamic policies on topology control ops require additional resources.
    if policy.is_dynamic() {
        match desc.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck => {
                req.dynamic_policy_children = 2;
                req.topology_children = 1;
            }
            _ => {}
        }
    }

    req
}
