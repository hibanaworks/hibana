//! Deferred destruction shares the actor's one cursor borrow. Queue links live
//! in its existing operation slots; no borrow can disappear during an I/O call.

use super::{
    OperationKernel, OperationRetirement, ResidentActor, ResidentView, Transport,
    public_types::RetiredOperation,
};
use crate::endpoint::carrier::{OperationRegion, WaiterTransfer};

impl<'r, T: Transport + 'r> ResidentActor<'r, T> {
    pub(crate) fn defer_operation(
        &self,
        view: &ResidentView<'r, T>,
        region: OperationRegion,
        kind: OperationRetirement,
    ) {
        if (region.is_whole() && kind == OperationRetirement::Parallel)
            || view.public_header.actor() != core::ptr::NonNull::from(&self.public_header)
        {
            crate::invariant();
        }
        let operation = crate::invariant_some(view.operation(region.lane(view.primary_lane)));
        let retired = RetiredOperation {
            role: view.public_header.role(),
            region,
            kind,
        };
        if let Some(previous) = operation.retired.get() {
            // Nested borrows can retire the same first-lane slot before the
            // current callback returns. Keep their containing source interval.
            let outer = previous.region.bounds();
            let inner = region.bounds();
            if previous.role != retired.role {
                crate::invariant();
            }
            if previous.kind == OperationRetirement::Parallel
                && kind == OperationRetirement::Release
            {
                if !(outer.start <= inner.start && inner.end <= outer.end) {
                    crate::invariant();
                }
            } else if inner.start <= outer.start && outer.end <= inner.end {
                operation.retired.set(Some(retired));
            } else if !(outer.start <= inner.start && inner.end <= outer.end) {
                crate::invariant();
            }
            return;
        }
        operation.retired.set(Some(retired));
        operation.retired_next.set(
            self.context
                .retired_operations
                .replace(Some(core::ptr::NonNull::from(operation))),
        );
    }

    pub(crate) fn drain_retirements(&self) -> bool {
        loop {
            if let Some(ptr) = self.context.retired_operations.get() {
                // SAFETY: only pinned operation slots in this actor are queued.
                let operation = unsafe { ptr.as_ref() };
                let retired = crate::invariant_some(operation.retired.get());
                let view = crate::invariant_some(self.views.get(retired.role));
                let mut waiters = WaiterTransfer::empty();
                let Some(()) = self.try_with_kernel(view, retired.region, |kernel| {
                    self.context
                        .retired_operations
                        .set(operation.retired_next.take());
                    operation.retired.set(None);
                    kernel.finish_retired_operation(retired.kind, &mut waiters);
                }) else {
                    return false;
                };
                drop(waiters);
            } else if let Some(ptr) = self.context.retired_views.get() {
                // SAFETY: retirement enqueues only pinned records in this actor.
                let view = unsafe { ptr.as_ref() };
                let mut waiters = WaiterTransfer::empty();
                let Some(()) = self.try_with_kernel(view, OperationRegion::WHOLE, |kernel| {
                    self.context.retired_views.set(view.retired_next.take());
                    kernel.clear_endpoint_waiter(&mut waiters);
                    kernel.retire_view();
                }) else {
                    return false;
                };
                drop(waiters);
            } else {
                return true;
            }
        }
    }
}

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    fn finish_retired_operation(
        &mut self,
        kind: OperationRetirement,
        waiters: &mut WaiterTransfer,
    ) {
        match kind {
            OperationRetirement::Release => self.release_public_operation(waiters),
            OperationRetirement::Parallel => self.finish_parallel(waiters),
        }
    }

    /// An external callback can destroy another borrow while this kernel owns
    /// the cursor. Reborrow only the affected I/O; never acquire another cursor
    /// or reconstruct a shared reference over a protected actor Drop.
    pub(crate) fn reconcile_retirements(&mut self) {
        loop {
            if let Some(ptr) = self.context.retired_operations.get() {
                // SAFETY: the queue contains this actor's pinned operation slots.
                let operation = unsafe { ptr.as_ref() };
                let retired = crate::invariant_some(operation.retired.get());
                let view = crate::invariant_some(self.views.get(retired.role));
                if core::ptr::eq(operation.state.get(), self.operation) {
                    crate::invariant();
                }
                self.context
                    .retired_operations
                    .set(operation.retired_next.take());
                operation.retired.set(None);
                let mut waiters = WaiterTransfer::empty();
                {
                    let io = if core::ptr::eq(&view.public_header, self.public_header) {
                        &mut *self.io
                    } else {
                        // SAFETY: this is a different view; the common cursor
                        // borrow excludes any other mutable kernel entry.
                        unsafe { &mut *view.io.get() }
                    };
                    let mut sibling = OperationKernel {
                        public_header: &view.public_header,
                        waiter: &operation.waiter,
                        context: self.context,
                        progress: self.progress,
                        io,
                        // SAFETY: the queued operation is distinct from the
                        // current one, under the common cursor borrow.
                        operation: unsafe { &mut *operation.state.get() },
                        operations: &view.operations,
                        views: self.views,
                        region: retired.region,
                    };
                    sibling.finish_retired_operation(retired.kind, &mut waiters);
                }
                drop(waiters);
            } else if let Some(ptr) = self.context.retired_views.get() {
                // SAFETY: only this actor's pinned view records enter this queue.
                let view = unsafe { ptr.as_ref() };
                if view.public_header.actor() != self.public_header.actor()
                    || core::ptr::eq(&view.public_header, self.public_header)
                {
                    crate::invariant();
                }
                self.context.retired_views.set(view.retired_next.take());
                let mut waiters = WaiterTransfer::empty();
                {
                    let operation = view.primary_operation();
                    let mut sibling = OperationKernel {
                        public_header: &view.public_header,
                        waiter: &operation.waiter,
                        context: self.context,
                        progress: self.progress,
                        // SAFETY: the retired view is disjoint from this view.
                        io: unsafe { &mut *view.io.get() },
                        // SAFETY: this is a distinct view's operation cell;
                        // the common cursor borrow is retained.
                        operation: unsafe { &mut *operation.state.get() },
                        operations: &view.operations,
                        views: self.views,
                        region: OperationRegion::WHOLE,
                    };
                    sibling.clear_endpoint_waiter(&mut waiters);
                    sibling.retire_view();
                }
                drop(waiters);
            } else {
                return;
            }
        }
    }
}
