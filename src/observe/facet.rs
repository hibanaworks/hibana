#[cfg(test)]
use crate::control::lease::planner::{LeaseFacetNeeds, LeaseGraphBudget, policy_requirements};
#[cfg(test)]
use crate::global::compiled::{CompiledProgram, MAX_COMPILED_PROGRAM_RESOURCES};

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
    const EMPTY: Self = Self {
        label: 0,
        resource_tag: None,
        needs: LeaseFacetNeeds::new(),
        delegation_children: 0,
        splice_children: 0,
    };

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
    atoms: [AtomFacetDetail; MAX_COMPILED_PROGRAM_RESOURCES],
    atoms_len: usize,
}

#[cfg(test)]
impl ProgramFacetReport {
    /// Borrow the collected atom details.
    #[inline]
    fn atoms(&self) -> &[AtomFacetDetail] {
        &self.atoms[..self.atoms_len]
    }
}

/// Produce a facet report from compiled program facts.
#[cfg(test)]
fn compiled_report(compiled: &CompiledProgram) -> ProgramFacetReport {
    let mut atoms = [AtomFacetDetail::EMPTY; MAX_COMPILED_PROGRAM_RESOURCES];
    let mut atoms_len = 0usize;
    let mut budget = LeaseGraphBudget::new();
    let effect_envelope = compiled.effect_envelope();

    for descriptor in effect_envelope.resources() {
        let policy = effect_envelope.resource_policy(descriptor);
        budget = budget.include_atom(descriptor.label(), Some(descriptor.tag()), policy);
        let requirements = policy_requirements(Some(descriptor.tag()), descriptor.label(), policy);
        assert!(atoms_len < atoms.len(), "facet atom capacity exceeded");
        atoms[atoms_len] = AtomFacetDetail {
            label: descriptor.label(),
            resource_tag: Some(descriptor.tag()),
            needs: requirements.facets,
            delegation_children: requirements.delegation_children,
            splice_children: requirements.splice_children,
        };
        atoms_len += 1;
    }

    ProgramFacetReport {
        budget,
        atoms,
        atoms_len,
    }
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
        crate::runtime::mgmt::with_management_compiled_programs_for_test(
            |controller_program, cluster_program| {
                let controller = compiled_report(controller_program);
                let cluster = compiled_report(cluster_program);

                assert_requires_slots_only(&controller);
                assert_requires_slots_only(&cluster);

                let expected = &[
                    (LABEL_MGMT_LOAD_BEGIN, Some(LoadBeginKind::TAG)),
                    (LABEL_MGMT_LOAD_COMMIT, Some(LoadCommitKind::TAG)),
                ];

                assert_eq!(&collect_atom_keys(&controller), expected);
                assert_eq!(&collect_atom_keys(&cluster), expected);
            },
        );
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

    fn collect_atom_keys(report: &ProgramFacetReport) -> [(u8, Option<u8>); 2] {
        let mut keys = [(0, None); 2];
        let mut len = 0usize;
        for key in report
            .atoms()
            .iter()
            .filter(|atom| atom.requires_facets())
            .map(|atom| (atom.label, atom.resource_tag))
        {
            if !keys[..len].contains(&key) {
                assert!(len < keys.len(), "facet key capacity exceeded");
                keys[len] = key;
                len += 1;
            }
        }
        keys
    }
}
