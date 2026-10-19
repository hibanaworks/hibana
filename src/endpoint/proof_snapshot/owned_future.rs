//! Passive observations of real owned-future fields. The scoped recorder is
//! test-only and never authorizes a call, owns an Endpoint or changes progress.
use crate::endpoint::{
    EndpointError,
    carrier::{EndpointHandle, KernelEndpointHeader, OperationRegion},
    owner::EndpointOwner,
    target::OperationTarget,
};
use core::{cell::RefCell, ptr::NonNull, task::Poll};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValueIdentity {
    /// Process-local allocation identity, never a wire address or capability.
    pub(crate) actor_address: usize,
    pub(crate) role: u8,
    pub(crate) run: u32,
    pub(crate) nonce: u32,
}

/// # Safety
/// `ptr` must retain its original pinned header allocation. Stored generations
/// identify the value even after acceptance or retirement changes the header.
pub(super) unsafe fn identity(
    ptr: NonNull<KernelEndpointHeader>,
    handle: EndpointHandle,
) -> ValueIdentity {
    // SAFETY: the owner/tag retains this original pinned header allocation.
    let header = unsafe { ptr.as_ref() };
    ValueIdentity {
        actor_address: header.actor().as_ptr().addr(),
        role: header.role(),
        run: handle.generation(),
        nonce: handle.view_generation(),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum OwnedFutureBoundary {
    Seal {
        carrier: ValueIdentity,
        region: OperationRegion,
        payload: ValueIdentity,
        label: u8,
    },
    Poll {
        payload: ValueIdentity,
        result: Poll<Result<(), EndpointError>>,
    },
    Drop {
        payload: ValueIdentity,
    },
}

std::thread_local! {
    static OBSERVATIONS: RefCell<Option<Vec<OwnedFutureBoundary>>> = const { RefCell::new(None) };
}

/// A local diagnostics scope. Unwinding clears it; records never drive the test
/// schedule. No observation state exists in the production library.
pub(crate) struct Capture {
    _local: crate::local::LocalOnly,
}

impl Capture {
    pub(crate) fn start() -> Self {
        OBSERVATIONS.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "nested owned-future observation scope");
            *slot = Some(Vec::new());
        });
        Self {
            _local: crate::local::LocalOnly::new(),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<OwnedFutureBoundary> {
        OBSERVATIONS.with(|slot| slot.borrow().as_ref().expect("live capture").clone())
    }

    pub(crate) fn finish(self) -> Vec<OwnedFutureBoundary> {
        OBSERVATIONS.with(|slot| slot.borrow_mut().take().expect("live capture"))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        OBSERVATIONS.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

fn record(event: impl FnOnce() -> OwnedFutureBoundary) {
    OBSERVATIONS.with(|slot| {
        if let Some(events) = slot.borrow_mut().as_mut() {
            events.push(event());
        }
    });
}

pub(crate) fn record_seal<const ROLE: u8>(
    target: &OperationTarget<'_, '_, ROLE>,
    payload: &EndpointOwner,
    label: u8,
) {
    record(|| {
        let (ptr, handle) = target.parts();
        OwnedFutureBoundary::Seal {
            // SAFETY: these real owned values/borrows retain both headers.
            carrier: unsafe { identity(ptr, handle.endpoint) },
            region: handle.region,
            payload: unsafe { identity(payload.ptr, payload.handle) },
            label,
        }
    });
}

pub(crate) fn record_poll(payload: &EndpointOwner, result: Poll<Result<(), EndpointError>>) {
    record(|| OwnedFutureBoundary::Poll {
        // SAFETY: this is the future's actual retained payload field.
        payload: unsafe { identity(payload.ptr, payload.handle) },
        result,
    });
}

pub(crate) fn record_drop(payload: &EndpointOwner) {
    record(|| OwnedFutureBoundary::Drop {
        // SAFETY: this observes the field before automatic destruction.
        payload: unsafe { identity(payload.ptr, payload.handle) },
    });
}
