use super::common::read;

#[test]
fn lean_program_identity_retains_every_exact_rust_image_field() {
    let authority = read("proofs/lean/Hibana/Authority.lean");

    for identity_field in [
        "roleCount : Nat",
        "atomCount : Nat",
        "routeResolverCount : Nat",
        "routeParticipantCount : Nat",
        "scopeMarkerCount : Nat",
        "blob : List Nat",
    ] {
        assert!(
            authority.contains(identity_field),
            "Lean program identity must retain exact Rust image field: {identity_field}"
        );
    }
}
