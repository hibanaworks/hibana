//! Safe wrappers over endpoint lane/port transport access.

use core::task::{Context, Poll};

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
pub(crate) struct RawSendPayload {
    ptr: *const (),
    encode: unsafe fn(*const (), &mut [u8]) -> Result<usize, SendError>,
}

pub(crate) struct PendingRecv {
    port_key: Option<*const ()>,
}

impl PendingRecv {
    #[inline]
    pub(super) const fn new() -> Self {
        Self { port_key: None }
    }

    #[inline]
    fn clear(&mut self) {
        self.port_key = None;
    }

    #[inline]
    pub(super) fn parks_port<'r, T, E>(&self, port: &Port<'r, T, E>) -> bool
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        self.port_key == Some(core::ptr::from_ref(port).cast())
    }
}

pub(super) struct PendingSend<'r> {
    outgoing: Option<Outgoing<'r>>,
}

impl<'r> PendingSend<'r> {
    #[inline]
    pub(super) const fn new() -> Self {
        Self { outgoing: None }
    }

    #[inline]
    fn clear(&mut self) {
        self.outgoing = None;
    }
}

impl RawSendPayload {
    #[inline(always)]
    pub(crate) fn from_typed<P: WireEncode>(payload: &P) -> Self {
        Self {
            ptr: core::ptr::from_ref(payload).cast(),
            encode: encode_send_payload::<P>,
        }
    }

    #[inline(always)]
    pub(crate) fn encode_into(self, scratch: &mut [u8]) -> Result<usize, SendError> {
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
pub(crate) fn shrink_payload<'a>(payload: Payload<'_>) -> Payload<'a> {
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
    pending: &mut PendingRecv,
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
    pending.port_key = Some(port_key);
    let transport = port.transport();
    let rx_ptr = port.rx_ptr();
    let poll = unsafe { transport.poll_recv(&mut *rx_ptr, cx) }.map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn begin_send_outgoing<'f, 'r, T, E>(
    pending: &mut PendingSend<'r>,
    port: &Port<'r, T, E>,
    outgoing: Outgoing<'f>,
) where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'f,
{
    pending.outgoing = Some(Outgoing {
        meta: outgoing.meta,
        payload: shrink_payload(outgoing.payload),
    });
    let _ = port;
}

#[inline]
pub(super) fn poll_send_outgoing<'r, T, E>(
    pending: &mut PendingSend<'r>,
    port: &Port<'r, T, E>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), TransportError>>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    let outgoing = pending
        .outgoing
        .expect("pending send must be armed before polling");
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    let poll = unsafe { transport.poll_send(&mut *tx_ptr, outgoing, cx) }.map_err(Into::into);
    if poll.is_ready() {
        pending.clear();
    }
    poll
}

#[inline]
pub(super) fn cancel_send_outgoing<'r, T, E>(pending: &mut PendingSend<'r>, port: &Port<'r, T, E>)
where
    T: Transport + 'r,
    E: EpochTable + 'r,
{
    if pending.outgoing.is_none() {
        return;
    }
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    unsafe {
        transport.cancel_send(&mut *tx_ptr);
    }
    pending.clear();
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
