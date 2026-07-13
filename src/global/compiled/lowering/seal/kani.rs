use super::route_role_has_branch_knowledge;

#[kani::proof]
fn route_controller_or_in_band_evidence_is_exact_acceptance_domain() {
    let role = kani::any::<u8>();
    let controller = kani::any::<u8>();
    let observer_paths_mergeable = kani::any::<bool>();

    let accepted = route_role_has_branch_knowledge(role, controller, observer_paths_mergeable);

    assert!(accepted == (role == controller || observer_paths_mergeable));
    if role != controller && !observer_paths_mergeable {
        assert!(!accepted);
    }
}
