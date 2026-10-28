/// Exclusive access to one logical role's local protocol.
///
/// Successful operations advance the protocol. [`Endpoint::offer`] previews a
/// choice without advancing it, and [`Endpoint::par`] borrows independent parts
/// for concurrent execution.
///
/// An endpoint cannot be cloned. It can be moved as a message payload when the
/// global protocol permits, transferring the same role's continuation. A failed
/// or cancelled send never returns that transferred right to the sender.
///
/// The value borrows its local runtime for `'r` and is neither `Send` nor `Sync`.
/// Communication with another runtime goes through the transport.
pub struct Endpoint<'r, const ROLE: u8> {
    pub(super) owner: super::owner::EndpointOwner,
    pub(super) _borrow: core::marker::PhantomData<&'r mut ()>,
}

/// Preview of a selected route branch returned by [`Endpoint::offer`].
///
/// Inspect [`RouteBranch::label`], then use [`RouteBranch::recv`] or
/// [`RouteBranch::send`] for the previewed operation. Dropping a preview does not
/// advance the protocol. A self-role unit step is consumed with `recv` as a
/// local operation, without transport I/O.
pub struct RouteBranch<'e, 'r, const ROLE: u8> {
    pub(super) target: super::target::OperationTarget<'e, 'r, ROLE, u8>,
}
