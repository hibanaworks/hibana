//! Bounded checks of the ownership equation solver. Artifact queries
//! are checked by Rust/Miri and the independent Lean source-path checker.
//! Full projection/implementation progress is outside this harness set.
use super::{EndpointKey, EndpointKind, Owner, Scratch};

const DB: EndpointKey = EndpointKey {
    component: 0,
    role: 10,
};
const AUTH: EndpointKey = EndpointKey {
    component: 0,
    role: 11,
};

#[kani::proof]
#[kani::unwind(8)]
fn independent_equation_roots_keep_arbitrary_actors() {
    let left: u8 = kani::any();
    let right: u8 = kani::any();
    let mut scratch = Scratch::<2, 0>::new();
    assert!(scratch.collect_key(DB, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(AUTH, EndpointKind::Delegated).is_ok());
    scratch.initialize_owners();
    assert!(scratch.unify(Owner::Initial(0), Owner::Actor(left)));
    assert!(scratch.unify(Owner::Initial(1), Owner::Actor(right)));
    assert_eq!(scratch.resolve(Owner::Initial(0)), Some(left));
    assert_eq!(scratch.resolve(Owner::Initial(1)), Some(right));
}

#[kani::proof]
#[kani::unwind(8)]
fn a_conflicting_merge_preserves_both_existing_roots() {
    let left: u8 = kani::any();
    let right: u8 = kani::any();
    let mut scratch = Scratch::<2, 0>::new();
    assert!(scratch.collect_key(DB, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(AUTH, EndpointKind::Delegated).is_ok());
    scratch.initialize_owners();
    assert!(scratch.unify(Owner::Initial(0), Owner::Actor(left)));
    assert!(scratch.unify(Owner::Initial(1), Owner::Actor(right)));
    assert_eq!(
        scratch.unify(Owner::Initial(0), Owner::Initial(1)),
        left == right
    );
    assert_eq!(scratch.resolve(Owner::Initial(0)), Some(left));
    assert_eq!(scratch.resolve(Owner::Initial(1)), Some(right));
}

#[kani::proof]
#[kani::unwind(8)]
fn unified_equations_cannot_be_rebound_to_a_different_actor() {
    let first: u8 = kani::any();
    let later: u8 = kani::any();
    let mut scratch = Scratch::<2, 0>::new();
    assert!(scratch.collect_key(DB, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(AUTH, EndpointKind::Delegated).is_ok());
    scratch.initialize_owners();
    assert!(scratch.unify(Owner::Initial(0), Owner::Initial(1)));
    assert!(scratch.unify(Owner::Initial(1), Owner::Actor(first)));
    assert_eq!(
        scratch.unify(Owner::Initial(0), Owner::Actor(later)),
        first == later
    );
    assert_eq!(scratch.resolve(Owner::Initial(0)), Some(first));
    assert_eq!(scratch.resolve(Owner::Initial(1)), Some(first));
}

#[kani::proof]
#[kani::unwind(8)]
fn root_merge_orientation_preserves_the_forced_owner() {
    let actor: u8 = kani::any();
    let reverse: bool = kani::any();
    let mut scratch = Scratch::<2, 0>::new();
    assert!(scratch.collect_key(DB, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(AUTH, EndpointKind::Delegated).is_ok());
    scratch.initialize_owners();
    let (first, second) = if reverse { (1, 0) } else { (0, 1) };
    assert!(scratch.unify(Owner::Initial(first), Owner::Initial(second)));
    assert!(scratch.unify(Owner::Initial(second), Owner::Actor(actor)));
    assert_eq!(scratch.resolve(Owner::Initial(0)), Some(actor));
    assert_eq!(scratch.resolve(Owner::Initial(1)), Some(actor));
}
