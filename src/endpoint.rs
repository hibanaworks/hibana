//! Localside endpoint facade built on the typestate DSL.
//!
//! Applications interact with `Endpoint` values that are materialised from
//! `RoleProgram` projections.

use crate::{
    binding::BindingHandle,
    control::cap::mint::{EpochTbl, MintConfig, MintConfigMarker},
    transport::{TransportError, wire::CodecError},
};

/// Affine endpoint helpers.
pub(crate) mod affine;
/// Crate-private carrier owners for internal endpoint type packs.
pub(crate) mod carrier;
/// Control-plane helpers for endpoints.
pub(crate) mod control;
/// Flow-based send API.
pub(crate) mod flow;
/// Internal endpoint kernel implementation.
pub(crate) mod kernel;

type EndpointBinding<'r> = BindingHandle<'r>;
type EndpointCfg<'r, K, Mint> = carrier::EndpointCfg<K, Mint, EndpointBinding<'r>>;
type KernelEndpoint<'r, const ROLE: u8, K, Mint> =
    carrier::KernelCursorEndpoint<'r, ROLE, K, EpochTbl, Mint, EndpointBinding<'r>>;

struct EndpointInner<'r, const ROLE: u8, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
    _cfg: core::marker::PhantomData<EndpointCfg<'r, K, Mint>>,
    _local_only: crate::local::LocalOnly,
}

/// Public endpoint facade for app-facing localside interaction.
#[allow(private_bounds)]
pub struct Endpoint<'r, const ROLE: u8, K, Mint = MintConfig>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    inner: EndpointInner<'r, ROLE, K, Mint>,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
#[allow(private_bounds)]
pub struct RouteBranch<'e, 'r, const ROLE: u8, K, Mint = MintConfig>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    label: u8,
    endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
    _borrow: core::marker::PhantomData<&'e mut EndpointCfg<'r, K, Mint>>,
    _local_only: crate::local::LocalOnly,
}

impl<'r, const ROLE: u8, K, Mint> EndpointInner<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    fn from_ptr(endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>) -> Self {
        Self {
            endpoint,
            _cfg: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }
}

#[allow(private_bounds)]
impl<'r, const ROLE: u8, K, Mint> Endpoint<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(crate) fn from_ptr(endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>) -> Self {
        Self {
            inner: EndpointInner::from_ptr(endpoint),
        }
    }
}

#[allow(private_bounds)]
impl<'e, 'r, const ROLE: u8, K, Mint> RouteBranch<'e, 'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(crate) fn from_parts(
        endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
        label: u8,
    ) -> Self {
        Self {
            label,
            endpoint,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }
}

impl<'r, const ROLE: u8, K, Mint> Drop for Endpoint<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    fn drop(&mut self) {
        unsafe {
            core::ptr::drop_in_place(self.inner.endpoint);
        }
    }
}

impl<'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    Endpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub fn flow<'e, M>(
        &'e mut self,
    ) -> SendResult<
        flow::Flow<'e, 'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
    >
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
        'cfg: 'r,
    {
        unsafe {
            Ok(flow::Flow::from_cap_flow(
                (&mut *self.inner.endpoint).flow_for_kit::<M>()?,
            ))
        }
    }

    #[inline]
    pub fn recv<M>(
        &mut self,
    ) -> impl core::future::Future<
        Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'_>>,
    > + '_
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        unsafe { (&mut *self.inner.endpoint).recv_direct::<M>() }
    }

    #[inline]
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = RecvResult<
        RouteBranch<'e, 'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
        >,
    > + 'e {
        async move {
            let branch = unsafe { (&mut *self.inner.endpoint).offer().await? };
            let label = branch.label();
            unsafe {
                (&mut *self.inner.endpoint).stash_pending_branch_preview(branch);
            }
            Ok(RouteBranch::from_parts(
                self.inner.endpoint,
                label,
            ))
        }
    }
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    RouteBranch<'e, 'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub fn decode<M>(
        self,
    ) -> impl core::future::Future<
        Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + 'e
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        async move {
            let endpoint = self.endpoint;
            let mut branch = unsafe { (&mut *endpoint).take_pending_branch_preview() }
                .expect("route branch preview must stay present until consumed");
            let payload = match unsafe {
                (&mut *endpoint)
                    .decode_branch_ptr::<M>(core::ptr::from_mut(&mut branch))
                    .await
            } {
                Ok(payload) => payload,
                Err(err) => {
                    unsafe {
                        (&mut *endpoint).stash_pending_branch_preview(branch);
                    }
                    return Err(err);
                }
            };
            Ok(payload)
        }
    }
}

/// Send error placeholder (will specialise once send/recv API lands).
/// Errors surfaced when sending frames through a cursor endpoint.
#[derive(Debug)]
pub enum SendError {
    /// Payload encoding failed.
    Codec(CodecError),
    /// Transport returned an error while transmitting the frame.
    Transport(TransportError),
    /// Endpoint typestate did not permit a send at this point.
    PhaseInvariant,
    /// Attempted to send a message whose label does not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the send operation.
    PolicyAbort { reason: u16 },
    /// Binding layer hook returned an error.
    Binding,
}

/// Errors surfaced when receiving frames through a cursor endpoint.
#[derive(Debug)]
pub enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Binding layer failed to read from channel.
    Binding(crate::binding::TransportOpsError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate did not permit a receive at this point.
    PhaseInvariant,
    /// Incoming frame label did not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Incoming frame originated from an unexpected peer role.
    PeerMismatch { expected: u8, actual: u8 },
    /// Session or lane did not match the endpoint.
    SessionMismatch {
        expected_sid: u32,
        received_sid: u32,
        expected_lane: u8,
        received_lane: u8,
    },
    /// Policy VM aborted the receive operation.
    PolicyAbort { reason: u16 },
}

/// Send result alias.
pub type SendResult<T> = core::result::Result<T, SendError>;

/// Receive result alias.
pub type RecvResult<T> = core::result::Result<T, RecvError>;

/// Errors surfaced when executing local actions.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalFailureReason {
    code: u16,
}

#[cfg(test)]
impl LocalFailureReason {
    /// Internal invariant violation detected by the runtime.
    pub(crate) const INTERNAL: Self = Self::custom(0xFFFF);

    /// Create a custom failure reason (0x0000-0xFFFE reserved for user space).
    pub(crate) const fn custom(code: u16) -> Self {
        Self { code }
    }

    #[inline]
    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self { code: raw }
    }
}
