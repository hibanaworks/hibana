//! LeaseGraph planner and static capacity checks.
//!
//! This module performs a const-time analysis over projected control plans to
//! determine how many LeaseGraph children are required for delegation and
//! splice automatons. The resulting budget is validated against the capacities
//! advertised by each `LeaseSpec`, triggering a compile-time panic when a
//! program requests more links than the runtime can provision.

use crate::control::CpEffect;
use crate::control::cap::ResourceKind;
use crate::control::cap::resource_kinds::{
    CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoadBeginKind, LoadCommitKind,
    PolicyActivateKind, PolicyAnnotateKind, PolicyLoadKind, PolicyRevertKind, RerouteKind,
    RollbackKind, SpliceAckKind, SpliceIntentKind,
};
use crate::{
    eff::EffKind,
    global::const_dsl::{EffList, HandlePlan},
    runtime::consts,
};

pub type LeaseFacetFlags = u8;

pub const FACET_CAPS: LeaseFacetFlags = 1 << 0;
pub const FACET_SLOTS: LeaseFacetFlags = 1 << 1;
pub const FACET_SPLICE: LeaseFacetFlags = 1 << 2;
pub const FACET_DELEGATION: LeaseFacetFlags = 1 << 3;

/// Maximum number of delegation links tracked in [`DelegationChildSet`].
pub const DELEGATION_CHILD_SET_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaseFacetNeeds {
    bits: LeaseFacetFlags,
}

impl LeaseFacetNeeds {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    #[inline(always)]
    pub const fn all() -> Self {
        Self {
            bits: FACET_CAPS | FACET_SLOTS | FACET_SPLICE | FACET_DELEGATION,
        }
    }

    #[inline(always)]
    pub const fn from_bits(bits: LeaseFacetFlags) -> Self {
        Self { bits }
    }

    #[inline(always)]
    pub const fn bits(&self) -> LeaseFacetFlags {
        self.bits
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    pub const fn with_caps(mut self) -> Self {
        self.bits |= FACET_CAPS;
        self
    }

    pub const fn with_slots(mut self) -> Self {
        self.bits |= FACET_SLOTS;
        self
    }

    pub const fn with_splice(mut self) -> Self {
        self.bits |= FACET_SPLICE;
        self
    }

    pub const fn with_delegation(mut self) -> Self {
        self.bits |= FACET_DELEGATION;
        self
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    #[inline(always)]
    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    #[inline(always)]
    pub const fn requires_caps(&self) -> bool {
        (self.bits & FACET_CAPS) != 0
    }

    #[inline(always)]
    pub const fn requires_slots(&self) -> bool {
        (self.bits & FACET_SLOTS) != 0
    }

    #[inline(always)]
    pub const fn requires_splice(&self) -> bool {
        (self.bits & FACET_SPLICE) != 0
    }

    #[inline(always)]
    pub const fn requires_delegation(&self) -> bool {
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
pub const fn facets(caps: bool, slots: bool, splice: bool, delegation: bool) -> LeaseFacetNeeds {
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
pub const fn facets_caps() -> LeaseFacetNeeds {
    FacetCaps::NEEDS
}

#[inline(always)]
pub const fn facets_slots() -> LeaseFacetNeeds {
    FacetSlots::NEEDS
}

#[inline(always)]
pub const fn facets_splice() -> LeaseFacetNeeds {
    FacetSet::<false, false, true, false>::NEEDS
}

#[inline(always)]
pub const fn facets_delegation() -> LeaseFacetNeeds {
    FacetSet::<false, false, false, true>::NEEDS
}

#[inline(always)]
pub const fn facets_caps_slots() -> LeaseFacetNeeds {
    FacetCapsSlots::NEEDS
}

#[inline(always)]
pub const fn facets_caps_splice() -> LeaseFacetNeeds {
    FacetCapsSplice::NEEDS
}

#[inline(always)]
pub const fn facets_caps_delegation() -> LeaseFacetNeeds {
    FacetCapsDelegation::NEEDS
}

/// ZST helper that publishes facet requirements through const generics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FacetSet<const CAPS: bool, const SLOTS: bool, const SPLICE: bool, const DELEGATION: bool>;

impl<const CAPS: bool, const SLOTS: bool, const SPLICE: bool, const DELEGATION: bool>
    FacetSet<CAPS, SLOTS, SPLICE, DELEGATION>
{
    /// Facet requirements encoded by this set.
    pub const NEEDS: LeaseFacetNeeds = facets(CAPS, SLOTS, SPLICE, DELEGATION);

    /// Accessor for the encoded facet requirements.
    #[inline(always)]
    pub const fn needs() -> LeaseFacetNeeds {
        Self::NEEDS
    }
}

/// Facet set requesting only capability tracking.
pub type FacetCaps = FacetSet<true, false, false, false>;
/// Facet set requesting only slot staging.
pub type FacetSlots = FacetSet<false, true, false, false>;
/// Facet set requesting capability tracking plus slot staging.
pub type FacetCapsSlots = FacetSet<true, true, false, false>;
/// Facet set requesting capability tracking plus splice context.
pub type FacetCapsSplice = FacetSet<true, false, true, false>;
/// Facet set requesting capability tracking plus delegation context.
pub type FacetCapsDelegation = FacetSet<true, false, false, true>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlanRequirements {
    pub(crate) delegation_children: usize,
    pub(crate) splice_children: usize,
    pub(crate) facets: LeaseFacetNeeds,
}

impl PlanRequirements {
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
pub struct LeaseGraphBudget {
    pub delegation_children: usize,
    pub splice_children: usize,
    facets: LeaseFacetNeeds,
}

impl LeaseGraphBudget {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            delegation_children: 0,
            splice_children: 0,
            facets: LeaseFacetNeeds::new(),
        }
    }

    /// Analyse the control plans embedded in an effect list.
    pub const fn from_eff_list(list: &EffList) -> Self {
        let mut budget = Self::new();
        let plans = list.control_plans();
        let mut plan_idx = 0;
        let mut idx = 0;
        while idx < list.len() {
            let node = list.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let plan = if plan_idx < plans.len() && plans[plan_idx].offset == idx {
                    let plan_value = plans[plan_idx].plan;
                    plan_idx += 1;
                    plan_value
                } else {
                    HandlePlan::None
                };
                budget = budget.include_atom(atom.label, atom.resource, plan);
            }
            idx += 1;
        }
        budget
    }

    #[inline(always)]
    pub const fn include_atom(mut self, label: u8, tag: Option<u8>, plan: HandlePlan) -> Self {
        let req = plan_requirements(tag, label, plan);
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
    pub const fn validate(&self) {
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
    pub const fn requires_caps(&self) -> bool {
        self.facets.requires_caps()
    }

    #[inline(always)]
    pub const fn requires_slots(&self) -> bool {
        self.facets.requires_slots()
    }

    #[inline(always)]
    pub const fn requires_splice(&self) -> bool {
        self.facets.requires_splice()
    }

    #[inline(always)]
    pub const fn requires_delegation(&self) -> bool {
        self.facets.requires_delegation()
    }

    #[inline(always)]
    pub const fn facets(&self) -> LeaseFacetNeeds {
        self.facets
    }

    #[inline(always)]
    pub const fn covers(&self, needs: LeaseFacetNeeds) -> bool {
        self.facets.contains(needs)
    }
}

#[inline(always)]
#[track_caller]
pub const fn assert_budget_covers(budget: LeaseGraphBudget, needs: LeaseFacetNeeds) {
    assert!(
        budget.covers(needs),
        "lease facet needs exceed role program lease budget"
    );
}

#[inline(always)]
pub const fn facet_needs(tag: u8, plan: HandlePlan) -> LeaseFacetNeeds {
    plan_facets(Some(tag), 0, plan)
}

const fn plan_facets(tag: Option<u8>, label: u8, plan: HandlePlan) -> LeaseFacetNeeds {
    plan_requirements(tag, label, plan).facets
}

#[inline(always)]
pub(crate) const fn plan_requirements(
    tag: Option<u8>,
    label: u8,
    plan: HandlePlan,
) -> PlanRequirements {
    let mut req = match tag {
        Some(tag_value) => PlanRequirements::with_facets(base_facets_for_tag(tag_value)),
        None => PlanRequirements::new(),
    };

    let Some(tag_value) = tag else {
        if label == consts::LABEL_SPLICE_INTENT || label == consts::LABEL_SPLICE_ACK {
            req.facets = req.facets.union(facets_caps_splice());
        } else if label == consts::LABEL_REROUTE {
            req.facets = req.facets.union(facets_caps_delegation());
        }
        return req;
    };

    // Dynamic plans on splice/reroute tags require additional resources
    if (tag_value == SpliceIntentKind::TAG || tag_value == SpliceAckKind::TAG) && plan.is_dynamic()
    {
        req.delegation_children = 2;
        req.splice_children = 1;
    } else if tag_value == RerouteKind::TAG && plan.is_dynamic() {
        req.delegation_children = 2;
    }

    req
}

const fn base_facets_for_tag(tag: u8) -> LeaseFacetNeeds {
    match tag {
        SpliceIntentKind::TAG | SpliceAckKind::TAG => facets_caps_splice(),
        RerouteKind::TAG => facets_caps_delegation(),
        LoadBeginKind::TAG
        | LoadCommitKind::TAG
        | PolicyLoadKind::TAG
        | PolicyActivateKind::TAG
        | PolicyRevertKind::TAG
        | PolicyAnnotateKind::TAG => facets_slots(),
        CancelKind::TAG
        | CancelAckKind::TAG
        | CheckpointKind::TAG
        | CommitKind::TAG
        | RollbackKind::TAG => facets_caps(),
        _ => LeaseFacetNeeds::new(),
    }
}

#[inline(always)]
pub const fn resource_needs(tag: u8) -> LeaseFacetNeeds {
    base_facets_for_tag(tag)
}

#[inline(always)]
pub const fn effect_needs(effect: CpEffect) -> LeaseFacetNeeds {
    match effect {
        CpEffect::SpliceBegin | CpEffect::SpliceAck | CpEffect::SpliceCommit => {
            facets_caps_splice()
        }
        CpEffect::Delegate => facets_caps_delegation(),
        CpEffect::CancelBegin
        | CpEffect::CancelAck
        | CpEffect::Checkpoint
        | CpEffect::Commit
        | CpEffect::Rollback => facets_caps(),
        _ => LeaseFacetNeeds::new(),
    }
}

/// Trait implemented by LeaseSpec types that expose the facet needs they require.
pub trait LeaseSpecFacetNeeds {
    /// Facet requirements advertised by this specification.
    const FACET_NEEDS: LeaseFacetNeeds;

    #[inline(always)]
    fn facet_needs() -> LeaseFacetNeeds {
        Self::FACET_NEEDS
    }
}

impl<const CAPS: bool, const SLOTS: bool, const SPLICE: bool, const DELEGATION: bool>
    LeaseSpecFacetNeeds for FacetSet<CAPS, SLOTS, SPLICE, DELEGATION>
{
    const FACET_NEEDS: LeaseFacetNeeds = FacetSet::<CAPS, SLOTS, SPLICE, DELEGATION>::NEEDS;
}

#[inline(always)]
pub const fn assert_program_covers_facets<const ROLE: u8, LocalSteps, Mint>(
    program: &crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
    needs: LeaseFacetNeeds,
) where
    Mint: crate::control::cap::MintConfigMarker,
{
    assert_budget_covers(program.lease_budget(), needs);
}

#[inline(always)]
pub const fn assert_program_covers_spec<const ROLE: u8, LocalSteps, Mint, Spec>(
    program: &crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
) where
    Mint: crate::control::cap::MintConfigMarker,
    Spec: LeaseSpecFacetNeeds,
{
    assert_program_covers_facets(program, Spec::FACET_NEEDS);
}
