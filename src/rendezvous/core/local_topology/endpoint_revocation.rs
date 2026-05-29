use super::{Clock, LabelUniverse, Lane, Rendezvous, SessionId, Transport};
use crate::rendezvous::core::EndpointLeaseId;

#[must_use = "revoked public endpoint cleanup must be finished"]
pub(crate) struct RevokedPublicEndpoint<'cfg> {
    header: core::ptr::NonNull<()>,
    ops: crate::endpoint::carrier::EndpointOps<'cfg>,
    sid: SessionId,
    finish_entered: bool,
}

impl<'cfg> RevokedPublicEndpoint<'cfg> {
    #[inline]
    pub(crate) fn finish(mut self) {
        self.finish_entered = true;
        unsafe {
            // SAFETY: this cleanup handle is returned only after the matching
            // endpoint carrier callback has validated the resident endpoint
            // header and session. Finishing runs outside ControlCore mutation.
            (self.ops.finish_revoke_for_session)(self.header, self.sid);
        }
    }
}

#[must_use = "prepared public endpoint revocation must be committed"]
pub(crate) struct PreparedEndpointRevocation<'cfg, Phase> {
    inner: Option<EndpointRevocationPlan<'cfg>>,
    _phase: core::marker::PhantomData<Phase>,
}

pub(crate) enum NeedsDescriptorRollback {}
pub(crate) enum ReadyToRelease {}

struct EndpointRevocationPlan<'cfg> {
    header: core::ptr::NonNull<()>,
    ops: crate::endpoint::carrier::EndpointOps<'cfg>,
    sid: SessionId,
    terminal: crate::endpoint::kernel::EndpointRevocationTerminal<'cfg>,
    released_lanes: [Lane; u8::MAX as usize + 1],
    released_len: usize,
    lease_slot: EndpointLeaseId,
    lease_generation: u32,
}

impl<'cfg, Phase> PreparedEndpointRevocation<'cfg, Phase> {
    #[inline]
    fn from_plan(inner: EndpointRevocationPlan<'cfg>) -> Self {
        Self {
            inner: Some(inner),
            _phase: core::marker::PhantomData,
        }
    }

    #[inline]
    fn take_inner(&mut self) -> EndpointRevocationPlan<'cfg> {
        self.inner
            .take()
            .expect("prepared endpoint revocation phase already consumed")
    }
}

impl<'cfg> PreparedEndpointRevocation<'cfg, NeedsDescriptorRollback> {
    #[inline]
    pub(crate) fn into_descriptor_rollback(
        mut self,
    ) -> (
        Option<crate::control::cluster::core::DescriptorTerminal>,
        PreparedEndpointRevocation<'cfg, ReadyToRelease>,
    ) {
        let mut inner = self.take_inner();
        let ticket = inner.terminal.take_descriptor_ticket();
        (
            ticket,
            PreparedEndpointRevocation::<'cfg, ReadyToRelease>::from_plan(inner),
        )
    }
}

impl Drop for RevokedPublicEndpoint<'_> {
    fn drop(&mut self) {
        assert!(
            self.finish_entered,
            "revoked public endpoint cleanup must be entered exactly once"
        );
    }
}

impl<Phase> Drop for PreparedEndpointRevocation<'_, Phase> {
    fn drop(&mut self) {
        assert!(
            self.inner.is_none(),
            "prepared public endpoint revocation must be consumed through its typestate phases"
        );
    }
}

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) fn prepare_one_public_endpoint_revocation(
        &mut self,
        sid: SessionId,
    ) -> Option<PreparedEndpointRevocation<'cfg, NeedsDescriptorRollback>> {
        let mut released_lanes = [Lane::new(0); u8::MAX as usize + 1];
        let lease_capacity = usize::from(self.endpoint_lease_capacity());
        let mut idx = 0usize;
        while idx < lease_capacity {
            let Some((slot, generation)) = self.public_endpoint_lease_by_index(idx) else {
                idx += 1;
                continue;
            };
            let Some((offset, len)) = self.endpoint_lease_storage(slot, generation) else {
                idx += 1;
                continue;
            };
            let (slab_ptr, slab_len) = self.slab_ptr_and_len();
            idx += 1;
            if len == 0 || offset + len > slab_len {
                continue;
            }

            let Some(header) = core::ptr::NonNull::new(
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe {
                    slab_ptr
                        .add(offset)
                        .cast::<crate::endpoint::carrier::KernelEndpointHeader<'cfg>>()
                },
            ) else {
                continue;
            };
            let ops = /* SAFETY: header points into the checked endpoint lease storage and the carrier header owns a valid ops table for this endpoint slot. */ unsafe { header.as_ref().ops() };
            let mut terminal = crate::endpoint::kernel::EndpointRevocationTerminal::none();
            let released = /* SAFETY: topology state owns the pending transition slot and this method holds the source rendezvous owner while preparing endpoint-local revocation obligations. */ unsafe {
                (ops.prepare_revoke_for_session)(
                    header.cast(),
                    sid,
                    released_lanes.as_mut_ptr(),
                    released_lanes.len(),
                    core::ptr::from_mut(&mut terminal).cast(),
                )
            };
            if released == 0 {
                continue;
            }
            return Some(PreparedEndpointRevocation::from_plan(
                EndpointRevocationPlan {
                    header: header.cast(),
                    ops: *ops,
                    sid,
                    terminal,
                    released_lanes,
                    released_len: released,
                    lease_slot: slot,
                    lease_generation: generation,
                },
            ));
        }
        None
    }

    pub(crate) fn commit_prepared_public_endpoint_revocation(
        &mut self,
        mut revocation: PreparedEndpointRevocation<'cfg, ReadyToRelease>,
    ) -> RevokedPublicEndpoint<'cfg> {
        let EndpointRevocationPlan {
            header,
            ops,
            sid,
            terminal,
            released_lanes,
            released_len,
            lease_slot,
            lease_generation,
        } = revocation.take_inner();
        if let Some(lane) = terminal.waiter_lane() {
            self.clear_session_waiter(sid, lane);
        }
        self.release_endpoint_lease(lease_slot, lease_generation);
        let mut released_idx = 0usize;
        while released_idx < released_len {
            let owned_lane = released_lanes[released_idx];
            if let Some(released_sid) = self.release_lane(owned_lane) {
                self.emit_lane_release(released_sid, owned_lane);
            }
            released_idx += 1;
        }
        RevokedPublicEndpoint {
            header,
            ops,
            sid,
            finish_entered: false,
        }
    }
}
