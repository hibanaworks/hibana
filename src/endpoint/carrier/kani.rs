#[kani::proof]
fn view_generation_never_wraps_reuses_or_loses_its_retirement_successor() {
    let current: u32 = kani::any();
    let next = super::next_view_generation(current);
    let valid = current % 2 == 1 && current < u32::MAX - 2;
    assert_eq!(next.is_some(), valid);
    if let Some(next) = next {
        assert!(next > current);
        assert!(next % 2 == 1);
        assert!(next < u32::MAX);
        assert!(next + 1 > next);
        assert!(next + 1 != current);
    }
    kani::cover!(current == u32::MAX - 4 && next.is_some());
    kani::cover!(current == u32::MAX - 2 && next.is_none());
}

#[kani::proof]
fn initial_view_issuance_is_exact_and_retirement_cannot_restore_it() {
    // This total constructor covers every nonzero run, including u32::MAX.
    let run = kani::any::<u32>().max(1);
    let role: u8 = kani::any();
    let candidate: u32 = kani::any();
    // These checks use only the header's numeric identity methods. No actor
    // callback is invoked, so a non-dereferenced actor address is sufficient.
    let actor = core::ptr::NonNull::dangling();
    let header = super::KernelEndpointHeader::new(actor, run, role);
    let accepted = header.issue_initial(candidate);
    let valid = candidate % 2 == 1 && candidate != u32::MAX;
    assert_eq!(accepted, valid);
    assert_eq!(header.actor(), actor);
    assert_eq!(header.role(), role);
    if accepted {
        let handle = super::EndpointHandle::new(run, candidate);
        assert!(handle.matches_header(&header));
        assert!(!header.issue_initial(kani::any()));
        assert_eq!(header.view_generation(), candidate);
        header.retire_generation();
        assert_eq!(header.view_generation(), candidate + 1);
        assert_eq!(header.generation(), 0);
        assert!(!handle.matches_header(&header));
        assert!(!header.issue_initial(kani::any()));
        header.retire_generation();
        assert_eq!(header.view_generation(), candidate + 1);
    } else {
        assert_eq!(header.view_generation(), 0);
        assert_eq!(header.generation(), 0);
    }
    kani::cover!(candidate == u32::MAX - 2 && accepted);
    kani::cover!(candidate == u32::MAX && !accepted);
}

#[kani::proof]
fn received_view_reissue_keeps_the_run_and_rejects_the_old_handle() {
    let run = kani::any::<u32>().max(1);
    let role: u8 = kani::any();
    let counter: u32 = kani::any();
    let previously_issued: bool = kani::any();
    let next = super::next_view_generation(counter);
    assert_eq!(next.is_some(), counter % 2 == 1 && counter < u32::MAX - 2);
    let Some(next) = next else {
        return;
    };
    let actor = core::ptr::NonNull::dangling();
    let header = super::KernelEndpointHeader::new(actor, run, role);
    let old = super::EndpointHandle::new(run, counter);
    if previously_issued {
        assert!(header.issue_initial(counter));
        header.retire_generation();
    }
    assert_eq!(header.generation(), 0);
    header.issue_received(next);
    let current = super::EndpointHandle::new(run, next);
    assert_eq!(header.actor(), actor);
    assert_eq!(header.role(), role);
    assert_eq!(header.generation(), run);
    assert_eq!(header.view_generation(), next);
    assert!(!old.matches_header(&header));
    assert!(current.matches_header(&header));
    // SAFETY: the header remains live at this address. The real raw entry
    // preflight must reject the stale handle before dereferencing its actor.
    let rejected = unsafe {
        crate::runtime::SessionKit::<RejectTransportAccess>::resident_actor_from_header(
            core::ptr::NonNull::from(&header).cast(),
            old,
        )
    };
    assert!(rejected.is_none());
    assert!(current.matches_header(&header));
    assert!(!header.issue_initial(kani::any()));
    header.retire_generation();
    assert_eq!(header.view_generation(), next + 1);
    assert_eq!(header.generation(), 0);
    assert!(!old.matches_header(&header));
    assert!(!current.matches_header(&header));
    kani::cover!(counter == u32::MAX - 4 && previously_issued);
    kani::cover!(counter == 1 && !previously_issued);
}

#[kani::proof]
fn retired_view_preflight_rejects_every_issued_handle() {
    let run = kani::any::<u32>().max(1);
    let role: u8 = kani::any();
    let counter: u32 = kani::any();
    let first_issuance: bool = kani::any();
    let next = super::next_view_generation(counter);
    assert_eq!(next.is_some(), counter % 2 == 1 && counter < u32::MAX - 2);
    let Some(next) = next else {
        return;
    };
    let header = super::KernelEndpointHeader::new(core::ptr::NonNull::dangling(), run, role);
    assert!(header.issue_initial(counter));
    header.retire_generation();
    header.issue_received(next);
    header.retire_generation();
    let nonce = if first_issuance { counter } else { next };
    let stale = super::EndpointHandle::new(run, nonce);
    assert!(!stale.matches_header(&header));
    // SAFETY: the record remains live on the stack, but its actor address is
    // deliberately unusable. Check the same raw entry preflight used by release
    // and public operations, for either issuance and the full valid u32 domain.
    let rejected = unsafe {
        crate::runtime::SessionKit::<RejectTransportAccess>::resident_actor_from_header(
            core::ptr::NonNull::from(&header).cast(),
            stale,
        )
    };
    assert!(rejected.is_none());
    assert_eq!(header.view_generation(), next + 1);
    assert_eq!(header.generation(), 0);
    kani::cover!(first_issuance && counter == u32::MAX - 4);
    kani::cover!(!first_issuance && counter == u32::MAX - 4);
}
/// The real carrier preflight needs a concrete transport type, but must reject
/// stale tags before accessing its actor. No transport is instantiated here.
struct RejectTransportAccess;

impl crate::transport::Transport for RejectTransportAccess {
    type Tx<'a> = ();
    type Rx<'a> = ();

    fn open<'a>(&'a self, _: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        panic!("stale release reached transport open");
    }

    fn poll_send<'f>(
        &self,
        _: &mut Self::Tx<'_>,
        _: crate::transport::Outgoing<'f>,
        _: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), crate::transport::TransportError>> {
        panic!("stale release reached transport send");
    }

    fn cancel_send(&self, _: &mut Self::Tx<'_>) {
        panic!("stale release reached transport cancellation");
    }

    fn poll_recv<'a>(
        &'a self,
        _: &'a mut Self::Rx<'_>,
        _: &mut core::task::Context<'_>,
    ) -> core::task::Poll<
        Result<crate::transport::ReceivedFrame<'a>, crate::transport::TransportError>,
    > {
        panic!("stale release reached transport receive");
    }

    fn requeue<'a>(&self, _: &mut Self::Rx<'a>) -> Result<(), crate::transport::TransportError> {
        panic!("stale release reached transport requeue");
    }
}
