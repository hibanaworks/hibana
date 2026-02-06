use core::convert::TryFrom;

use crate::control::cap::MintConfigMarker;
use crate::control::lease::planner::{LeaseFacetNeeds, LeaseGraphBudget, plan_requirements};
use crate::eff::{EffIndex, EffKind};
use crate::global::const_dsl::{EffList, HandlePlan, StaticPlanKind};
use crate::global::role_program::RoleProgram;

use std::fmt::Write as _;
use std::vec::Vec;

/// Per-atom breakdown of facet requirements derived from control plans.
#[derive(Clone, Debug)]
pub struct AtomFacetDetail {
    pub eff_index: EffIndex,
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub is_control: bool,
    pub resource_tag: Option<u8>,
    pub plan: HandlePlan,
    pub needs: LeaseFacetNeeds,
    pub delegation_children: usize,
    pub splice_children: usize,
}

impl AtomFacetDetail {
    /// Returns true when this atom contributes any facet demand.
    #[inline]
    pub fn requires_facets(&self) -> bool {
        !self.needs.is_empty() || self.delegation_children > 0 || self.splice_children > 0
    }
}

/// Aggregated facet report for a role-local projection.
#[derive(Clone, Debug)]
pub struct ProgramFacetReport {
    pub budget: LeaseGraphBudget,
    atoms: Vec<AtomFacetDetail>,
}

impl ProgramFacetReport {
    /// Render a compact summary showing only atoms that request facets.
    pub fn render(&self) -> String {
        self.render_filtered(false)
    }

    /// Render a full summary including atoms with no facet needs.
    pub fn render_with_all_atoms(&self) -> String {
        self.render_filtered(true)
    }

    /// Borrow the collected atom details.
    #[inline]
    pub fn atoms(&self) -> &[AtomFacetDetail] {
        &self.atoms
    }

    fn render_filtered(&self, include_all: bool) -> String {
        let mut output = String::new();
        writeln!(
            output,
            "LeaseGraph budget: facets={} delegation_children={} splice_children={}",
            self.budget.facets(),
            self.budget.delegation_children,
            self.budget.splice_children,
        )
        .unwrap();

        let filtered: Vec<&AtomFacetDetail> = if include_all {
            self.atoms.iter().collect()
        } else {
            self.atoms
                .iter()
                .filter(|atom| atom.requires_facets())
                .collect()
        };

        if filtered.is_empty() {
            writeln!(output, "No atoms require additional facets.").unwrap();
            return output;
        }

        writeln!(
            output,
            " idx  from->to  lbl ctl tag  plan                         needs             deleg splice"
        )
        .unwrap();
        writeln!(
            output,
            "---- -------- ---- --- ---- ---------------------------- ---------------- ----- ------"
        )
        .unwrap();

        for atom in filtered {
            let tag_field = match atom.resource_tag {
                Some(tag) => format!("0x{tag:02X}"),
                None => "-".to_string(),
            };
            let plan_field = match atom.plan {
                HandlePlan::None => "-".to_string(),
                HandlePlan::Static { kind } => match kind {
                    StaticPlanKind::SpliceLocal { dst_lane } => {
                        format!("splice_local(dst={dst_lane})")
                    }
                    StaticPlanKind::RerouteLocal { dst_lane, shard } => {
                        format!("reroute_local(dst={dst_lane},shard={shard})")
                    }
                },
                HandlePlan::Dynamic { policy_id, .. } => format!("dynamic(policy={policy_id})"),
            };
            writeln!(
                output,
                "{:>4} {:>3}->{:<3} {:>3}  {:<3} {:<4} {:<28} {:<16} {:>5} {:>6}",
                atom.eff_index,
                atom.from,
                atom.to,
                atom.label,
                if atom.is_control { "yes" } else { "no" },
                tag_field,
                plan_field,
                atom.needs.to_string(),
                atom.delegation_children,
                atom.splice_children
            )
            .unwrap();
        }

        output
    }
}

/// Produce a facet report directly from an `EffList`.
pub fn list_report(list: &EffList) -> ProgramFacetReport {
    let mut atoms = Vec::new();
    let budget = LeaseGraphBudget::from_eff_list(list);
    let control_plans = list.control_plans();
    let mut plan_idx = 0usize;
    let plan_len = control_plans.len();

    let mut idx = 0usize;
    while idx < list.len() {
        let node = list.node_at(idx);
        if matches!(node.kind, EffKind::Atom) {
            let plan = if plan_idx < plan_len && control_plans[plan_idx].offset == idx {
                let value = control_plans[plan_idx].plan;
                plan_idx += 1;
                value
            } else {
                HandlePlan::None
            };

            let atom = node.atom_data();
            let requirements = plan_requirements(atom.resource, atom.label, plan);

            let eff_index = u16::try_from(idx).expect("EffStruct index exceeds EffIndex range");
            atoms.push(AtomFacetDetail {
                eff_index,
                from: atom.from,
                to: atom.to,
                label: atom.label,
                is_control: atom.is_control,
                resource_tag: atom.resource,
                plan,
                needs: requirements.facets,
                delegation_children: requirements.delegation_children,
                splice_children: requirements.splice_children,
            });
        }
        idx += 1;
    }

    ProgramFacetReport { budget, atoms }
}

/// Produce a facet report for a projected role program.
pub fn program_report<'prog, const ROLE: u8, LocalSteps, Mint>(
    program: &RoleProgram<'prog, ROLE, LocalSteps, Mint>,
) -> ProgramFacetReport
where
    Mint: MintConfigMarker,
{
    list_report(program.eff_list())
}
