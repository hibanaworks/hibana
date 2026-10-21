//! Creating the first endpoint of an exclusive resource.
use core::cell::Cell;

/// Permission to create the first endpoint of a delegated role.
///
/// The code that controls access to a physical resource creates one binding.
/// Every transport that can create an endpoint for that resource must return
/// the same binding from [`super::Transport::initial_endpoint_binding`], across
/// all runtimes, role numbers and sessions.
///
/// The binding is consumed once, when the first endpoint is created. Failure
/// before that point leaves it available. Later owners receive the endpoint
/// through the global protocol. Dropping an endpoint or runtime, losing a
/// message, or starting another session does not make the binding available again.
pub struct EndpointBinding {
    issuance: Cell<InitialIssuance>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InitialIssuance {
    Available,
    Reserved,
    Issued,
}

impl EndpointBinding {
    /// Create the binding for an exclusive physical resource.
    ///
    /// # Safety
    /// The caller must control all access to the resource. Every path that can
    /// create its endpoint must use this binding, including other runtimes and
    /// role aliases. The resource must also reject operations that bypass its
    /// endpoint. This binding must outlive all local and remote endpoints it
    /// creates, including endpoints in pending or accepted messages.
    ///
    /// Before replacing the binding, the caller must make all earlier endpoints
    /// and accepted messages unusable at the resource. A timeout or local runtime
    /// drop alone cannot establish this. Rust cannot determine whether two file,
    /// device or remote-service handles refer to the same physical resource.
    pub const unsafe fn new() -> Self {
        Self {
            issuance: Cell::new(InitialIssuance::Available),
        }
    }

    pub(crate) fn reserve(&self) -> Option<InitialEndpointReservation<'_>> {
        if self.issuance.get() != InitialIssuance::Available {
            return None;
        }
        self.issuance.set(InitialIssuance::Reserved);
        Some(InitialEndpointReservation { binding: self })
    }
}

/// Unpublished attachment owns the reservation, including during callbacks.
pub(crate) struct InitialEndpointReservation<'a> {
    binding: &'a EndpointBinding,
}

impl InitialEndpointReservation<'_> {
    pub(crate) fn commit(self) {
        if self.binding.issuance.get() != InitialIssuance::Reserved {
            crate::invariant();
        }
        self.binding.issuance.set(InitialIssuance::Issued);
        // Drop observes Issued and cannot return the authority to Available.
    }
}

impl Drop for InitialEndpointReservation<'_> {
    fn drop(&mut self) {
        if self.binding.issuance.get() == InitialIssuance::Reserved {
            self.binding.issuance.set(InitialIssuance::Available);
        }
    }
}

#[cfg(kani)]
mod kani {
    use super::*;

    #[::kani::proof]
    #[::kani::unwind(7)]
    fn one_physical_binding_can_publish_at_most_one_initial_endpoint() {
        // SAFETY: the harness owns this isolated abstract physical resource.
        let binding = unsafe { EndpointBinding::new() };
        let mut pending = None;
        let mut published = 0u8;
        for _ in 0..6 {
            match ::kani::any::<u8>() % 3 {
                0 => {
                    if pending.is_none() {
                        pending = binding.reserve();
                    } else {
                        assert!(binding.reserve().is_none());
                    }
                }
                1 => {
                    if let Some(reservation) = pending.take() {
                        reservation.commit();
                        published += 1;
                    }
                }
                _ => drop(pending.take()),
            }
            assert!(published <= 1);
            if published == 1 {
                assert!(binding.reserve().is_none());
            }
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    static_assertions::assert_not_impl_any!(EndpointBinding: Clone, Copy, Sync);

    #[test]
    fn failed_unpublished_attachment_returns_the_reservation() {
        // SAFETY: isolated model resource, with one authority and no external I/O.
        let binding = unsafe { EndpointBinding::new() };
        let reservation = binding.reserve().unwrap();
        assert!(binding.reserve().is_none());
        drop(reservation);
        binding.reserve().unwrap().commit();
        assert!(binding.reserve().is_none());
    }

    #[test]
    fn issued_authority_cannot_be_replenished_by_drop() {
        // SAFETY: isolated model resource, with one authority and no external I/O.
        let binding = unsafe { EndpointBinding::new() };
        binding.reserve().unwrap().commit();
        assert!(binding.reserve().is_none());
        assert_eq!(core::mem::size_of::<EndpointBinding>(), 1);
    }

    #[test]
    fn forgetting_an_unpublished_reservation_does_not_issue_twice() {
        // SAFETY: isolated model resource, with one authority and no external I/O.
        let binding = unsafe { EndpointBinding::new() };
        core::mem::forget(binding.reserve().unwrap());
        assert!(binding.reserve().is_none());
    }
}
