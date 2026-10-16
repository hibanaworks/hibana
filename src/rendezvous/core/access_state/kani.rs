use super::{Cell, EndpointOperationLease, RendezvousAccessState};

#[kani::proof]
fn operation_lease_protects_only_its_owner_and_releases_after_use() {
    let initial: RendezvousAccessState = kani::any();
    let owner = Cell::new(initial);
    let foreign = Cell::new(RendezvousAccessState::Available);
    let Some(lease) = EndpointOperationLease::try_acquire(&owner) else {
        assert!(initial != RendezvousAccessState::Available);
        assert!(owner.get() == initial);
        return;
    };
    assert!(initial == RendezvousAccessState::Available);
    assert!(owner.get() == RendezvousAccessState::EndpointOperation);
    assert!(lease.protects(&owner));
    assert!(!lease.protects(&foreign));
    assert!(EndpointOperationLease::try_acquire(&owner).is_none());
    assert!(foreign.get() == RendezvousAccessState::Available);
    drop(lease);
    assert!(owner.get() == RendezvousAccessState::Available);
    let next_lease = EndpointOperationLease::try_acquire(&owner);
    assert!(next_lease.is_some());
    drop(next_lease);
    assert!(owner.get() == RendezvousAccessState::Available);
}
