#[cfg(test)]
use crate::control::lease::planner::{LeaseFacetNeeds, LeaseGraphBudget, policy_requirements};
#[cfg(test)]
use crate::global::compiled::CompiledProgram;
#[cfg(test)]
use std::vec::Vec;

/// Per-atom breakdown of facet requirements derived from policy markers.
#[cfg(test)]
#[derive(Clone, Debug)]
struct AtomFacetDetail {
    label: u8,
    resource_tag: Option<u8>,
    needs: LeaseFacetNeeds,
    delegation_children: usize,
    splice_children: usize,
}

#[cfg(test)]
impl AtomFacetDetail {
    /// Returns true when this atom contributes any facet demand.
    #[inline]
    fn requires_facets(&self) -> bool {
        !self.needs.is_empty() || self.delegation_children > 0 || self.splice_children > 0
    }
}

/// Aggregated facet report for a role-local projection.
#[cfg(test)]
#[derive(Clone, Debug)]
struct ProgramFacetReport {
    budget: LeaseGraphBudget,
    atoms: Vec<AtomFacetDetail>,
}

#[cfg(test)]
impl ProgramFacetReport {
    /// Borrow the collected atom details.
    #[inline]
    fn atoms(&self) -> &[AtomFacetDetail] {
        &self.atoms
    }
}

/// Produce a facet report from compiled program facts.
#[cfg(test)]
fn compiled_report(compiled: &CompiledProgram) -> ProgramFacetReport {
    let mut atoms = Vec::new();
    let mut budget = LeaseGraphBudget::new();

    for descriptor in compiled.effect_envelope().resources() {
        budget = budget.include_atom(
            descriptor.label(),
            Some(descriptor.tag()),
            descriptor.policy(),
        );
        let requirements = policy_requirements(
            Some(descriptor.tag()),
            descriptor.label(),
            descriptor.policy(),
        );
        atoms.push(AtomFacetDetail {
            label: descriptor.label(),
            resource_tag: Some(descriptor.tag()),
            needs: requirements.facets,
            delegation_children: requirements.delegation_children,
            splice_children: requirements.splice_children,
        });
    }

    ProgramFacetReport { budget, atoms }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::ResourceKind;
    use crate::control::cap::resource_kinds::{LoadBeginKind, LoadCommitKind};
    use crate::control::lease::planner::LeaseFacetNeeds;
    use crate::runtime::consts::{LABEL_MGMT_LOAD_BEGIN, LABEL_MGMT_LOAD_COMMIT};

    #[test]
    fn management_roles_report_expected_facet_atoms() {
        let (controller_program, cluster_program) =
            crate::runtime::mgmt::management_compiled_programs();
        let controller = compiled_report(&controller_program);
        let cluster = compiled_report(&cluster_program);

        assert_requires_slots_only(&controller);
        assert_requires_slots_only(&cluster);

        let expected = &[
            (LABEL_MGMT_LOAD_BEGIN, Some(LoadBeginKind::TAG)),
            (LABEL_MGMT_LOAD_COMMIT, Some(LoadCommitKind::TAG)),
        ];

        assert_eq!(collect_atom_keys(&controller), expected);
        assert_eq!(collect_atom_keys(&cluster), expected);
    }

    fn assert_requires_slots_only(report: &ProgramFacetReport) {
        assert!(report.budget.requires_slots(), "slots facet required");
        assert!(
            !report.budget.requires_caps()
                && !report.budget.requires_splice()
                && !report.budget.requires_delegation(),
            "only slots facet should be requested"
        );
        for atom in report.atoms().iter().filter(|a| a.requires_facets()) {
            assert!(
                atom.needs.requires_slots(),
                "atom {} must require slots",
                atom.label
            );
            assert_eq!(
                atom.needs,
                LeaseFacetNeeds::new().with_slots(),
                "atom {} should not request other facets",
                atom.label
            );
            assert_eq!(
                (atom.delegation_children, atom.splice_children),
                (0, 0),
                "atom {} must not request additional LeaseGraph capacity",
                atom.label
            );
        }
    }

    fn collect_atom_keys(report: &ProgramFacetReport) -> Vec<(u8, Option<u8>)> {
        let mut keys = Vec::new();
        for key in report
            .atoms()
            .iter()
            .filter(|atom| atom.requires_facets())
            .map(|atom| (atom.label, atom.resource_tag))
        {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }
}
