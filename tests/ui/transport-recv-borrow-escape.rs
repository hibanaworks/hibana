use core::{cell::Cell, task::{Context, Poll}};
use hibana::runtime::transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError};

struct Handle<'h> {
    stash: Cell<Option<&'h [u8; 1]>>,
    buf: [u8; 1],
}
struct Carrier;
impl Transport for Carrier {
    type Tx<'h> = ();
    type Rx<'h> = Handle<'h>;
    fn open<'h>(&'h self, _: PortOpen) -> (Self::Tx<'h>, Self::Rx<'h>) {
        ((), Handle { stash: Cell::new(None), buf: [0] })
    }
    fn poll_send<'f>(&self, tx: &mut Self::Tx<'_>, frame: Outgoing<'f>, _: &mut Context<'_>)
        -> Poll<Result<(), TransportError>> {
        let _ = (tx, frame);
        Poll::Pending
    }
    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        let _ = tx;
    }
    fn poll_recv<'p>(&'p self, rx: &'p mut Self::Rx<'_>, _: &mut Context<'_>)
        -> Poll<Result<ReceivedFrame<'p>, TransportError>> {
        rx.stash.set(Some(&rx.buf));
        Poll::Pending
    }
    fn requeue(&self, _: &mut Self::Rx<'_>) -> Result<(), TransportError> { Ok(()) }
}
fn main() {}
