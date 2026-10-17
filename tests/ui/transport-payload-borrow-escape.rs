use core::{cell::Cell, task::{Context, Poll}};
use hibana::runtime::transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError};

struct Handle<'h> {
    stash: Cell<Option<&'h [u8]>>,
    buf: [u8; 1],
}
struct Carrier;
impl Transport for Carrier {
    type Tx<'h> = Handle<'h>;
    type Rx<'h> = ();
    fn open<'h>(&'h self, _: PortOpen) -> (Self::Tx<'h>, Self::Rx<'h>) {
        (Handle { stash: Cell::new(None), buf: [0] }, ())
    }
    fn poll_send<'f>(&self, tx: &mut Self::Tx<'_>, frame: Outgoing<'f>, _: &mut Context<'_>)
        -> Poll<Result<(), TransportError>> {
        tx.stash.set(Some(frame.payload().as_bytes()));
        Poll::Pending
    }
    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        let _ = tx;
    }
    fn poll_recv<'p>(&'p self, rx: &'p mut Self::Rx<'_>, _: &mut Context<'_>)
        -> Poll<Result<ReceivedFrame<'p>, TransportError>> {
        let _ = rx;
        Poll::Pending
    }
    fn requeue(&self, _: &mut Self::Rx<'_>) -> Result<(), TransportError> { Ok(()) }
}
fn main() {}
