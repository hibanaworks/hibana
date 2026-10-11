/// App-facing affine executor for a projected role.
///
/// The endpoint is intentionally local-only and moves forward one descriptor
/// step at a time. Successful sends, receives, and route branch first-step
/// operations consume progress. Dropped unpolled sends do not publish runtime
/// progress; dropped in-flight sends and route previews restore the endpoint to
/// its detached step. Once a committed fault is observed, the same session
/// generation cannot produce a new continuation.
pub struct Endpoint<'r, const ROLE: u8> {
    pub(super) ptr: core::ptr::NonNull<super::carrier::KernelEndpointHeader<'r>>,
    pub(super) handle: super::carrier::PackedEndpointHandle,
    pub(super) _borrow: core::marker::PhantomData<&'r mut ()>,
    pub(super) _local_only: crate::local::LocalOnly,
}

/// Preview of a selected route branch returned by [`Endpoint::offer`].
///
/// `RouteBranch` exposes the selected logical label. If the selected arm begins
/// with a receive, call [`RouteBranch::recv`]. If it begins with a send, call
/// [`RouteBranch::send`]. The label is descriptor/resolver evidence, not the
/// result of parsing payload bytes.
pub struct RouteBranch<'e, 'r, const ROLE: u8> {
    pub(super) endpoint: *mut Endpoint<'r, ROLE>,
    pub(super) label: u8,
    pub(super) _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
    pub(super) _local_only: crate::local::LocalOnly,
}
