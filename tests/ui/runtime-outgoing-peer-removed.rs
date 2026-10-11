use core::task::{Context, Poll};
use hibana::runtime::transport::{
    Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
};

struct Carrier(());

impl Transport for Carrier {
    type Tx<'a> = () where Self: 'a;
    type Rx<'a> = () where Self: 'a;

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        let _ = outgoing.peer();
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        loop {}
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

fn main() {}
