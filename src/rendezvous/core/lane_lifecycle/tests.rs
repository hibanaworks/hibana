use super::*;
use crate::transport::{Outgoing, PortOpen, ReceivedFrame, TransportError};

#[derive(Clone)]
struct LayoutTransport;

impl Transport for LayoutTransport {
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: Outgoing<'_>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        core::task::Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<ReceivedFrame<'a>, TransportError>> {
        core::task::Poll::Ready(Err(TransportError::Failed))
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Err(TransportError::Failed)
    }
}

type TestRv<'a> = Rendezvous<'a, 'a, LayoutTransport>;

#[test]
fn single_slab_carve_layout_accounts_for_alignment_and_tap_ring() {
    let mut storage = [0u8; 8192];
    let slab = &mut storage[1..];
    let layout = TestRv::resident_carve_layout(slab).expect("layout");
    let base = slab.as_ptr() as usize;
    let header_ptr = base + layout.header_start;
    let runtime_ptr = base + layout.runtime_start;
    let tap_ptr = base + layout.tap_start;

    assert_eq!(header_ptr % core::mem::align_of::<TestRv<'_>>(), 0);
    assert_eq!(tap_ptr % core::mem::align_of::<TapRecord>(), 0);
    assert_eq!(runtime_ptr % core::mem::align_of::<TapRecord>(), 0);
    assert!(
        layout.tap_start > 0,
        "misaligned slab must add prefix padding"
    );
    assert!(
        layout.runtime_start
            >= core::mem::size_of::<TestRv<'_>>() + core::mem::size_of::<[TapRecord; TAP_EVENTS]>(),
        "resident prefix must include header and tap ring bytes"
    );
}
