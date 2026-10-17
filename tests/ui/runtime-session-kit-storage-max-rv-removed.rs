use core::task::{Context, Poll};

use hibana::runtime::{
    SessionKitStorage,
    transport::{Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
};

struct Carrier(());

impl Transport for Carrier {
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

    fn poll_send<'f>(
        &self,
        _tx: &mut Self::Tx<'_>,
        _outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    {
        Poll::Ready(Ok(()))
    }

    fn cancel_send(&self, _tx: &mut Self::Tx<'_>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'_>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        loop {}
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

type _BadStorage = SessionKitStorage<'static, Carrier, 32>;

fn main() {}
