//! One initial issuance authority at the physical resource boundary.
use core::cell::Cell;

/// Sole initial Endpoint issuer for one exclusive resource binding.
///
/// The resource integration owns this value, independently of any session kit,
/// runtime, role number, or transport handle. Every transport exposing the same
/// resource returns a reference to this same authority. The runtime reserves it
/// before attachment and consumes it only when the initial view is published.
/// Moving the Endpoint, dropping it, losing delivery, or dropping a runtime
/// never replenishes the authority. Subsequent owners arise only from checked
/// owned receives in the choreography.
///
/// This is integration metadata, not a current-owner table or a second protocol
/// cursor. It records no holder, branch, continuation, or message progress.
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
    /// Establish the unique initial authority for an exclusive physical resource.
    ///
    /// # Safety
    /// The caller must own the resource's exclusive operation boundary. Every
    /// integration path to that resource, including other runtime instances and
    /// role aliases, must use this same binding. No other live binding or
    /// untracked operation right may authorize the same resource. This binding
    /// must outlive every local or remote right it issues. Replacing it requires
    /// fencing all earlier rights and accepted frames at the physical boundary.
    /// Neither a timeout nor destruction of a local runtime establishes a fence.
    ///
    /// The constructor is an integration trust boundary because Rust cannot
    /// infer whether two file, device, or remote-service handles name the same
    /// physical resource. Ordinary Endpoint users do not construct bindings.
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
