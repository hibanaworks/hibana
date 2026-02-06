#![cfg(feature = "std")]

use hibana::control::cap::ResourceKind;
use hibana::control::cap::resource_kinds::{LoadBeginKind, LoadCommitKind};
use hibana::control::lease::planner::LeaseFacetNeeds;
use hibana::observe::facet;
use hibana::runtime::consts::{LABEL_MGMT_LOAD_BEGIN, LABEL_MGMT_LOAD_COMMIT};
use hibana::runtime::mgmt::session::{CLUSTER_PROGRAM, CONTROLLER_PROGRAM};

#[test]
fn management_roles_report_expected_facet_atoms() {
    let controller = facet::program_report(&CONTROLLER_PROGRAM);
    let cluster = facet::program_report(&CLUSTER_PROGRAM);

    assert_requires_slots_only(&controller);
    assert_requires_slots_only(&cluster);

    let expected = &[
        (LABEL_MGMT_LOAD_BEGIN, Some(LoadBeginKind::TAG)),
        (LABEL_MGMT_LOAD_COMMIT, Some(LoadCommitKind::TAG)),
    ];

    assert_eq!(collect_atom_keys(&controller), expected);
    assert_eq!(collect_atom_keys(&cluster), expected);
}

fn assert_requires_slots_only(report: &facet::ProgramFacetReport) {
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

fn collect_atom_keys(report: &facet::ProgramFacetReport) -> Vec<(u8, Option<u8>)> {
    report
        .atoms()
        .iter()
        .filter(|atom| atom.requires_facets())
        .map(|atom| (atom.label, atom.resource_tag))
        .collect()
}
