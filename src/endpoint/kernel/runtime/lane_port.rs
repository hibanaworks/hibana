//! Safe wrappers over endpoint lane/port transport access.

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    binding::{BindingSlot, Channel, TransportOpsError},
    control::cap::mint::EpochTable,
    endpoint::SendError,
    rendezvous::port::Port,
    transport::{
        Outgoing, Transport, TransportError,
        wire::{Payload, WireEncode},
    },
};

#[derive(Clone, Copy)]
pub(super) struct ErasedSendPayload<'a> {
    ptr: *const (),
    encode: unsafe fn(*const (), &mut [u8]) -> Result<usize, SendError>,
    _marker: core::marker::PhantomData<&'a ()>,
}

pub(super) struct PendingRecv<'r, T>
where
    T: Transport + 'r,
{
    future: Option<T::Recv<'r>>,
    port_key: Option<*const ()>,
}

impl<'r, T> PendingRecv<'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(super) const fn new() -> Self {
        Self {
            future: None,
            port_key: None,
        }
    }

    #[inline]
    fn clear(&mut self) {
        self.future = None;
        self.port_key = None;
    }

    #[inline]
    pub(super) fn parks_port<E>(&self, port: &Port<'r, T, E>) -> bool
    where
        E: EpochTable + 'r,
    {
        self.future.is_some() && self.port_key == Some(core::ptr::from_ref(port).cast())
    }
}

pub(super) struct PendingSend<'r, T>
where
    T: Transport + 'r,
{
    future: Option<T::Send<'r>>,
}

impl<'r, T> PendingSend<'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(super) const fn new() -> Self {
        Self { future: None }
    }

    #[inline]
    fn clear(&mut self) {
        self.future = None;
    }
}

impl<'a> ErasedSendPayload<'a> {
    #[inline(always)]
    pub(super) fn from_typed<P: WireEncode>(payload: &'a P) -> Self {
        Self {
            ptr: core::ptr::from_ref(payload).cast(),
            encode: encode_send_payload::<P>,
            _marker: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(super) fn encode_into(self, scratch: &mut [u8]) -> Result<usize, SendError> {
        unsafe { (self.encode)(self.ptr, scratch) }
    }
}

#[inline(always)]
unsafe fn encode_send_payload<P: WireEncode>(
    ptr: *const (),
    scratch: &mut [u8],
) -> Result<usize, SendError> {
    let payload = unsafe { &*ptr.cast::<P>() };
    payload.encode_into(scratch).map_err(SendError::Codec)
}

#[inline]
pub(super) fn scratch_mut<'a, 'r, T, E>(port: &'a Port<'r, T, E>) -> &'a mut [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'a,
{
    unsafe { &mut *port.scratch_ptr() }
}

#[inline]
pub(super) fn scratch_ptr<'r, T, E>(port: &Port<'r, T, E>) -> *mut [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    port.scratch_ptr()
}

#[inline]
pub(super) fn frontier_scratch_ptr<'r, T, E>(port: &Port<'r, T, E>) -> *mut [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    port.frontier_scratch_ptr()
}

#[inline]
pub(super) fn staged_payload<'a, 'r, T, E, R>(
    port: &'a Port<'r, T, E>,
    stage: impl FnOnce(&mut [u8]) -> Result<usize, R>,
) -> Result<Payload<'a>, R>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'a,
{
    let scratch = scratch_mut(port);
    let len = stage(scratch)?;
    Ok(Payload::new(&scratch[..len]))
}

#[inline]
pub(super) fn shrink_payload<'a>(payload: Payload<'_>) -> Payload<'a> {
    let bytes = unsafe { &*(payload.as_bytes() as *const [u8]) };
    Payload::new(bytes)
}

#[inline]
pub(super) fn recv_from_binding<'r, B: BindingSlot + 'r>(
    binding_ptr: *mut B,
    channel: Channel,
    scratch_ptr: *mut [u8],
) -> Result<Payload<'r>, TransportOpsError> {
    unsafe { (&mut *binding_ptr).on_recv(channel, &mut *scratch_ptr) }
}

#[inline]
pub(super) fn poll_recv<'r, T, E>(
    pending: &mut PendingRecv<'r, T>,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<Payload<'r>, TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let port_key = core::ptr::from_ref(port).cast();
    if pending.port_key != Some(port_key) {
        pending.clear();
    }
    if pending.future.is_none() {
        let transport = port.transport();
        let rx_ptr = port.rx_ptr();
        pending.future = Some(unsafe { transport.recv(&mut *rx_ptr) });
        pending.port_key = Some(port_key);
    }
    let poll = Pin::new(
        pending
            .future
            .as_mut()
            .expect("pending recv future must exist before polling"),
    )
    .poll(cx)
    .map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn begin_send_outgoing<'f, 'r, T, E>(
    pending: &mut PendingSend<'r, T>,
    port: &Port<'r, T, E>,
    outgoing: Outgoing<'f>,
) where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'f,
{
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    pending.future = Some(unsafe { transport.send(&mut *tx_ptr, outgoing) });
}

#[inline]
pub(super) fn poll_send_outgoing<'r, T>(
    pending: &mut PendingSend<'r, T>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), TransportError>>
where
    T: Transport + 'r,
{
    let poll = Pin::new(
        pending
            .future
            .as_mut()
            .expect("pending send future must exist before polling"),
    )
    .poll(cx)
    .map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn requeue_recv<'r, T, E>(port: &Port<'r, T, E>)
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let transport = port.transport();
    let rx_ptr = port.rx_ptr();
    unsafe {
        transport.requeue(&mut *rx_ptr);
    }
}

#[inline(always)]
pub(super) fn deref_mut_ptr<'a, T>(ptr: *mut T) -> &'a mut T {
    unsafe { &mut *ptr }
}
