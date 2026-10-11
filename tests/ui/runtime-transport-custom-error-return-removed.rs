use core::task::{Context, Poll};
use hibana::runtime::transport::{Outgoing, PortOpen, ReceivedFrame, Transport};

struct Carrier;
struct CarrierError;

impl Transport for Carrier {
    type Tx<'a> = () where Self: 'a;
    type Rx<'a> = () where Self: 'a;

    fn open<'a>(&'a self, _port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), CarrierError>>
    where
        'a: 'f,
    {
        Poll::Ready(Err(CarrierError))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, CarrierError>> {
        Poll::Ready(Err(CarrierError))
    }

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), CarrierError> {
        Err(CarrierError)
    }
}

fn main() {}
