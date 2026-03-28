//! Safe wrappers over endpoint lane/port transport access.

use core::future::Future;

use crate::{
    control::cap::mint::EpochTable,
    rendezvous::port::Port,
    transport::{Outgoing, Transport, TransportError, wire::Payload},
};

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
pub(super) fn scratch<'a, 'r, T, E>(port: &'a Port<'r, T, E>) -> &'a [u8]
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'a,
{
    unsafe { &*port.scratch_ptr() }
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
pub(super) fn copy_payload_into_scratch<'a, 'r, T, E>(
    port: &'a Port<'r, T, E>,
    payload: &Payload<'_>,
) -> Result<usize, ()>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'a,
{
    let bytes = payload.as_bytes();
    let scratch = scratch_mut(port);
    if bytes.len() > scratch.len() {
        return Err(());
    }
    scratch[..bytes.len()].copy_from_slice(bytes);
    Ok(bytes.len())
}

#[inline]
pub(super) fn recv_future<'a, 'r, T, E>(
    port: &'a Port<'r, T, E>,
) -> impl Future<Output = Result<Payload<'a>, TransportError>> + 'a
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'r: 'a,
{
    async move {
        let transport = port.transport();
        let rx_ptr = port.rx_ptr();
        unsafe { transport.recv(&mut *rx_ptr).await.map_err(Into::into) }
    }
}

#[inline]
pub(super) async fn send_outgoing<'a, 'r, 'f, T, E>(
    port: &'a Port<'r, T, E>,
    outgoing: Outgoing<'f>,
) -> Result<(), TransportError>
where
    T: Transport + 'r,
    E: EpochTable + 'r,
    'a: 'f,
{
    let transport = port.transport();
    let tx_ptr = port.tx_ptr();
    unsafe {
        transport
            .send(&mut *tx_ptr, outgoing)
            .await
            .map_err(Into::into)
    }
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
