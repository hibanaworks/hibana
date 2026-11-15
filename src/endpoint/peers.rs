//! Actual values excluded by an operation's exclusive Rust borrow.
use super::{Endpoint, carrier::KernelActorHeader};
use core::ptr::NonNull;

/// Actual Endpoint storage borrowed alongside one operation.
///
/// Implementations are sealed: an Endpoint, mutable references, nested pairs,
/// fixed arrays, optional owned values and the empty tuple. Role numbers cannot
/// fabricate a peer.
/// These values grant no operation rights and carry no protocol progress.
pub trait EndpointPeers: sealed::Peers {}
impl<T: sealed::Peers> EndpointPeers for T {}

pub(super) mod sealed {
    pub trait Peers {
        fn contains(&self, actor: usize, role: u8) -> bool;
    }
}
impl sealed::Peers for () {
    fn contains(&self, _: usize, _: u8) -> bool {
        false
    }
}
impl<T: EndpointPeers + ?Sized> sealed::Peers for &mut T {
    fn contains(&self, actor: usize, role: u8) -> bool {
        sealed::Peers::contains(&**self, actor, role)
    }
}
impl<A: EndpointPeers, B: EndpointPeers> sealed::Peers for (A, B) {
    fn contains(&self, actor: usize, role: u8) -> bool {
        sealed::Peers::contains(&self.0, actor, role)
            || sealed::Peers::contains(&self.1, actor, role)
    }
}
impl<T: EndpointPeers, const N: usize> sealed::Peers for [T; N] {
    fn contains(&self, actor: usize, role: u8) -> bool {
        self.iter()
            .any(|peer| sealed::Peers::contains(peer, actor, role))
    }
}
impl<T: EndpointPeers> sealed::Peers for Option<T> {
    fn contains(&self, actor: usize, role: u8) -> bool {
        self.as_ref()
            .is_some_and(|peer| sealed::Peers::contains(peer, actor, role))
    }
}
impl<const ROLE: u8> sealed::Peers for Endpoint<'_, ROLE> {
    fn contains(&self, actor: usize, role: u8) -> bool {
        // SAFETY: the actual Endpoint owns this live, runtime-bound view.
        let header = unsafe { self.owner.ptr.as_ref() };
        role == ROLE
            && header.actor().as_ptr().addr() == actor
            && header.generation() == self.owner.handle.generation()
            && header.view_generation() == self.owner.handle.view_generation()
    }
}

/// Non-owning metadata, valid only under EndpointBorrow's exclusive lifetime.
/// It is passed into a synchronous kernel call and never stored in the actor.
#[derive(Clone, Copy)]
pub(crate) struct HeldViews {
    data: *const (),
    contains: unsafe fn(*const (), usize, u8) -> bool,
}
impl HeldViews {
    pub(super) fn new<P: EndpointPeers>(peers: &mut P) -> Self {
        unsafe fn contains<P: EndpointPeers>(data: *const (), actor: usize, role: u8) -> bool {
            // SAFETY: EndpointBorrow retains the original mutable storage borrow;
            // callbacks read only Endpoint ownership metadata for this one poll.
            let peers = unsafe { &*data.cast::<P>() };
            sealed::Peers::contains(peers, actor, role)
        }
        Self {
            data: core::ptr::from_ref(peers).cast(),
            contains: contains::<P>,
        }
    }
    pub(crate) fn contains(self, actor: NonNull<KernelActorHeader>, role: u8) -> bool {
        // SAFETY: only an operation borrowing the corresponding EndpointBorrow
        // can pass this metadata to the kernel, within that borrow's lifetime.
        unsafe { (self.contains)(self.data, actor.as_ptr().addr(), role) }
    }
}
