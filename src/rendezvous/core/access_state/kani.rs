use super::{Cell, EndpointLeaseId, EndpointOperationLease, RendezvousAccessState, ScratchLease};

#[kani::proof]
fn operation_lease_protects_only_its_owner_and_releases_after_use() {
    let initial: RendezvousAccessState = kani::any();
    let owner = Cell::new(initial);
    let foreign = Cell::new(RendezvousAccessState::Available);
    let actor: EndpointLeaseId = kani::any();
    let foreign_actor = EndpointLeaseId(actor.0.wrapping_add(1));
    let Some(lease) = EndpointOperationLease::try_acquire(&owner, actor) else {
        assert!(initial != RendezvousAccessState::Available);
        assert!(owner.get() == initial);
        return;
    };
    assert!(initial == RendezvousAccessState::Available);
    assert!(owner.get() == RendezvousAccessState::EndpointOperation(actor));
    assert!(lease.protects(&owner, actor));
    assert!(!lease.protects(&owner, foreign_actor));
    assert!(!lease.protects(&foreign, actor));
    assert!(EndpointOperationLease::try_acquire(&owner, actor).is_none());
    assert!(foreign.get() == RendezvousAccessState::Available);
    drop(lease);
    assert!(owner.get() == RendezvousAccessState::Available);
    let next_lease = EndpointOperationLease::try_acquire(&owner, actor);
    assert!(next_lease.is_some());
    drop(next_lease);
    assert!(owner.get() == RendezvousAccessState::Available);
}

#[kani::proof]
fn scratch_lease_excludes_reentry_and_restores_its_exact_parent_access() {
    let initial: RendezvousAccessState = kani::any();
    let owner = Cell::new(initial);
    let foreign = Cell::new(RendezvousAccessState::Available);
    let actor: EndpointLeaseId = kani::any();
    let Some(lease) = ScratchLease::try_acquire(&owner) else {
        assert!(initial.begin_scratch().is_none());
        assert!(owner.get() == initial);
        return;
    };
    assert!(initial.begin_scratch().is_some());
    assert!(lease.authorizes(&owner));
    assert!(!lease.authorizes(&foreign));
    assert!(ScratchLease::try_acquire(&owner).is_none());
    assert!(EndpointOperationLease::try_acquire(&owner, actor).is_none());
    assert!(foreign.get() == RendezvousAccessState::Available);
    drop(lease);
    assert!(owner.get() == initial);
    let next_lease = ScratchLease::try_acquire(&owner);
    assert!(next_lease.is_some());
    drop(next_lease);
    assert!(owner.get() == initial);
}
