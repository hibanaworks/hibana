use super::{EndpointHandle, NonNull};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn drop_public_endpoint_raw(ptr: NonNull<()>, handle: EndpointHandle) {
        let Some(actor) = (/* SAFETY: validate the immutable header and live generation before accessing the resident allocation. */unsafe { Self::resident_actor_from_header::<'cfg>(ptr, handle) })
        else {
            return;
        };
        // Keep the original allocation provenance for final destruction.
        // A pointer reconstructed from `&ResidentActor` cannot authorize Drop's
        // unique borrow over the immutable prefix of that allocation.
        let actor_ptr = unsafe {
            ptr.cast::<crate::endpoint::carrier::KernelEndpointHeader>()
                .as_ref()
                .actor()
                .cast::<crate::endpoint::kernel::ResidentActor<'cfg, T>>()
        };
        let operation_lease = actor.context.try_operation_lease();
        // SAFETY: immutable header validation above established this role.
        let role = unsafe {
            ptr.cast::<crate::endpoint::carrier::KernelEndpointHeader>()
                .as_ref()
        }
        .role();
        let view = crate::invariant_some(actor.views.get(role));
        actor.retire_view(view);
        // Reentry from a sibling view's poll only enqueues destruction. The
        // active entry drains it after releasing the common progress borrow.
        if !actor.drain_retired_views() || actor.context.live_views.get() != 0 {
            return;
        }
        actor.finish_all_view_io();
        let context = &actor.context;
        let generation = context.public_generation.get();
        let release = if context.public_slot_ownership.get()
            == crate::endpoint::kernel::PublicSlotOwnership::Owned
        {
            context
                .public_slot_ownership
                .set(crate::endpoint::kernel::PublicSlotOwnership::Borrowed);
            Some((
                context.session.cluster(),
                context.public_rv,
                context.public_slot,
                generation,
            ))
        } else {
            None
        };
        context.public_generation.set(0);
        // SAFETY: no issued view or active progress borrow remains, and all
        // external I/O destructors and waiters were retired above.
        unsafe {
            core::ptr::drop_in_place(actor_ptr.as_ptr());
        }
        drop(operation_lease);
        if let Some((cluster, rv_id, slot, generation)) = release {
            cluster.release_public_endpoint_slot_owned(rv_id, slot, generation);
        }
    }
}
