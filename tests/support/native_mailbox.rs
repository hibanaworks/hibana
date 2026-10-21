//! Native proof I/O. Each logical (role, lane) has one OS datagram queue.
//! Inherited descriptors name that same queue in every process. Only an
//! Endpoint operation reads it; no background reader can steal a future
//! holder's data. This module knows no holder, branch or protocol cursor.
#![deny(unsafe_code)]

use hibana::runtime::{
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};
use std::{
    borrow::ToOwned,
    collections::BTreeMap,
    fs,
    future::Future,
    io,
    os::unix::{fs::DirBuilderExt, net::UnixDatagram},
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
    vec::Vec,
};
use tokio::{io::ReadBuf, net::UnixDatagram as AsyncDatagram, time::Sleep};

const HEADER: usize = 8;
const MAX_PACKET: usize = HEADER + 60 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(3);

pub struct Mailboxes {
    directory: PathBuf,
    sockets: BTreeMap<(u8, u8), UnixDatagram>,
}

impl Mailboxes {
    pub fn new(roles: &[u8], lanes: usize) -> io::Result<Self> {
        assert!((1..=256).contains(&lanes));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // AF_UNIX pathname limits are shorter than some macOS TMPDIR paths.
        let directory =
            Path::new("/tmp").join(std::format!("hb-owned-{}-{nonce:x}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let mut mailboxes = Self {
            directory,
            sockets: BTreeMap::new(),
        };
        for &role in roles {
            for lane in 0..lanes {
                let lane = u8::try_from(lane).unwrap();
                let socket = UnixDatagram::bind(address(&mailboxes.directory, role, lane))?;
                socket.set_nonblocking(true)?;
                assert!(mailboxes.sockets.insert((role, lane), socket).is_none());
            }
        }
        Ok(mailboxes)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn descriptors(&self, roles: &[u8]) -> io::Result<Vec<((u8, u8), UnixDatagram)>> {
        self.sockets
            .iter()
            .filter(|((role, _), _)| roles.contains(role))
            .map(|(&key, socket)| Ok((key, socket.try_clone()?)))
            .collect()
    }
}

impl Drop for Mailboxes {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).expect("remove this run's private socket namespace");
    }
}

fn address(directory: &Path, role: u8, lane: u8) -> PathBuf {
    directory.join(std::format!("{role:x}-{lane:x}"))
}

pub struct NativeTransport<'binding> {
    directory: PathBuf,
    sockets: BTreeMap<(u8, u8), AsyncDatagram>,
    initial: Option<(u8, &'binding EndpointBinding)>,
    accepted: Option<&'binding dyn Fn(FrameHeader) -> Result<(), TransportError>>,
}

impl<'binding> NativeTransport<'binding> {
    pub fn new(
        directory: &Path,
        descriptors: Vec<((u8, u8), UnixDatagram)>,
        initial: Option<(u8, &'binding EndpointBinding)>,
    ) -> io::Result<Self> {
        let mut sockets = BTreeMap::new();
        for ((role, lane), socket) in descriptors {
            assert_eq!(
                socket.local_addr()?.as_pathname(),
                Some(address(directory, role, lane).as_path())
            );
            assert!(
                sockets
                    .insert((role, lane), AsyncDatagram::from_std(socket)?)
                    .is_none()
            );
        }
        Ok(Self {
            directory: directory.to_owned(),
            sockets,
            initial,
            accepted: None,
        })
    }

    /// Test hook after the OS accepted a datagram, before poll_send returns.
    /// It can observe preemption or inject an uncertain-delivery error. It
    /// never selects a branch, changes bytes, or duplicates a frame.
    pub fn after_send_acceptance(
        &mut self,
        observer: &'binding dyn Fn(FrameHeader) -> Result<(), TransportError>,
    ) {
        self.accepted = Some(observer);
    }
}

#[derive(Default)]
struct Wait(Option<Pin<std::boxed::Box<Sleep>>>);

impl Wait {
    fn pending<T>(&mut self, cx: &mut Context<'_>) -> Poll<Result<T, TransportError>> {
        let sleep = self
            .0
            .get_or_insert_with(|| std::boxed::Box::pin(tokio::time::sleep(IO_DEADLINE)));
        match sleep.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Err(TransportError::Deadline)),
        }
    }
}

pub struct Tx<'a> {
    socket: &'a AsyncDatagram,
    role: u8,
    lane: u8,
    session: u32,
    wait: Wait,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Receipt {
    Waiting,
    Delivered,
    Requeued,
}

pub struct Rx<'a> {
    socket: &'a AsyncDatagram,
    role: u8,
    lane: u8,
    session: u32,
    packet: Vec<u8>,
    receipt: Receipt,
    wait: Wait,
}

impl Transport for NativeTransport<'_> {
    type Tx<'a>
        = Tx<'a>
    where
        Self: 'a;
    type Rx<'a>
        = Rx<'a>
    where
        Self: 'a;

    fn initial_endpoint_binding(
        &self,
        _session: hibana::runtime::ids::SessionId,
        role: u8,
    ) -> Option<&EndpointBinding> {
        self.initial
            .filter(|&(bound, _)| bound == role)
            .map(|(_, binding)| binding)
    }

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        let role = port.local_role();
        let lane = port.lane();
        let session = port.session_id().raw();
        let socket = self
            .sockets
            .get(&(role, lane))
            .expect("projected logical mailbox supplied by deployment");
        (
            Tx {
                socket,
                role,
                lane,
                session,
                wait: Wait::default(),
            },
            Rx {
                socket,
                role,
                lane,
                session,
                packet: Vec::with_capacity(MAX_PACKET + 1),
                receipt: Receipt::Waiting,
                wait: Wait::default(),
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Tx<'_>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        if outgoing.lane() != tx.lane {
            return Poll::Ready(Err(TransportError::Failed));
        }
        let payload = outgoing.payload().as_bytes();
        if payload.len() > MAX_PACKET - HEADER {
            return Poll::Ready(Err(TransportError::Capacity));
        }
        let mut packet = Vec::with_capacity(HEADER + payload.len());
        packet.extend_from_slice(&tx.session.to_be_bytes());
        packet.extend_from_slice(&[
            tx.lane,
            tx.role,
            outgoing.target_role(),
            outgoing.frame_label().raw(),
        ]);
        packet.extend_from_slice(payload);
        let target = address(&self.directory, outgoing.target_role(), tx.lane);
        match tx.socket.poll_send_to(cx, &packet, target) {
            Poll::Pending => tx.wait.pending(cx),
            Poll::Ready(Ok(sent)) if sent == packet.len() => {
                tx.wait.0 = None;
                if let Some(observer) = self.accepted {
                    return Poll::Ready(observer(FrameHeader::from_bytes(
                        packet[..HEADER].try_into().unwrap(),
                    )));
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err(TransportError::Failed)),
            Poll::Ready(Err(error)) if error.raw_os_error() == Some(libc::ENOBUFS) => {
                Poll::Ready(Err(TransportError::Capacity))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(TransportError::Offline)),
        }
    }

    fn cancel_send(&self, tx: &mut Tx<'_>) {
        // Atomic datagram publication: Pending retained no bytes or pointer.
        tx.wait.0 = None;
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.receipt != Receipt::Requeued {
            rx.receipt = Receipt::Waiting;
            rx.packet.resize(MAX_PACKET + 1, 0);
            let mut buffer = ReadBuf::new(&mut rx.packet);
            let source = match rx.socket.poll_recv_from(cx, &mut buffer) {
                Poll::Pending => return rx.wait.pending(cx),
                Poll::Ready(Err(_)) => return Poll::Ready(Err(TransportError::Offline)),
                Poll::Ready(Ok(source)) => source,
            };
            let length = buffer.filled().len();
            rx.packet.truncate(length);
            if !(HEADER..=MAX_PACKET).contains(&length)
                || rx.packet[..4] != rx.session.to_be_bytes()
                || rx.packet[4] != rx.lane
                || rx.packet[6] != rx.role
                || source.as_pathname()
                    != Some(address(&self.directory, rx.packet[5], rx.lane).as_path())
            {
                return Poll::Ready(Err(TransportError::Failed));
            }
            rx.wait.0 = None;
        }
        rx.receipt = Receipt::Delivered;
        let header = FrameHeader::from_bytes(rx.packet[..HEADER].try_into().unwrap());
        Poll::Ready(Ok(ReceivedFrame::framed(
            header,
            Payload::new(&rx.packet[HEADER..]),
        )))
    }

    fn requeue(&self, rx: &mut Rx<'_>) -> Result<(), TransportError> {
        if rx.receipt != Receipt::Delivered {
            return Err(TransportError::Failed);
        }
        rx.receipt = Receipt::Requeued;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hibana::{
        g,
        runtime::{SessionKitStorage, ids::SessionId, program::project},
    };
    use std::{pin::pin, task::Waker, time::Instant, vec};

    #[tokio::test(flavor = "current_thread")]
    async fn requeued_preview_is_consumed_once_before_the_next_datagram() {
        type First = g::Msg<1, u32>;
        type Other = g::Msg<2, u32>;
        let global = g::seq(
            g::route(g::send::<0, 1, First>(), g::send::<0, 1, Other>()),
            g::send::<0, 1, First>(),
        );
        let source_program = project::<0>(&global);
        let target_program = project::<1>(&global);
        let mailboxes = Mailboxes::new(&[0, 1], 1).unwrap();
        let source_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[0]).unwrap(),
            None,
        )
        .unwrap();
        let target_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[1]).unwrap(),
            None,
        )
        .unwrap();
        let mut source_slab = vec![0; 64 * 1024];
        let mut target_slab = vec![0; 64 * 1024];
        let mut source_storage = SessionKitStorage::uninit();
        let mut target_storage = SessionKitStorage::uninit();
        let source_rv = source_storage
            .init()
            .rendezvous(&mut source_slab, source_io)
            .unwrap();
        let target_rv = target_storage
            .init()
            .rendezvous(&mut target_slab, target_io)
            .unwrap();
        let sid = SessionId::new(9011);
        let mut source = source_rv.enter(sid, &source_program).unwrap();
        let mut target = target_rv.enter(sid, &target_program).unwrap();
        source.send::<First>(&7).await.unwrap();
        source.send::<First>(&8).await.unwrap();
        let preview = target.offer().await.unwrap();
        assert_eq!(preview.label(), 1);
        drop(preview);
        let preview = target.offer().await.unwrap();
        assert_eq!(preview.label(), 1);
        assert_eq!(preview.recv::<First>().await.unwrap(), 7);
        assert_eq!(target.recv::<First>().await.unwrap(), 8);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_valid_frame_from_another_os_peer_is_rejected() {
        type Message = g::Msg<1, u32>;
        let global = g::send::<0, 1, Message>();
        let source_program = project::<0>(&global);
        let target_program = project::<1>(&global);
        let mailboxes = Mailboxes::new(&[0, 1, 2], 1).unwrap();
        let source_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[0]).unwrap(),
            None,
        )
        .unwrap();
        let target_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[1]).unwrap(),
            None,
        )
        .unwrap();
        let mut source_slab = vec![0; 64 * 1024];
        let mut target_slab = vec![0; 64 * 1024];
        let mut source_storage = SessionKitStorage::uninit();
        let mut target_storage = SessionKitStorage::uninit();
        let source_rv = source_storage
            .init()
            .rendezvous(&mut source_slab, source_io)
            .unwrap();
        let target_rv = target_storage
            .init()
            .rendezvous(&mut target_slab, target_io)
            .unwrap();
        let sid = SessionId::new(9012);
        let mut source = source_rv.enter(sid, &source_program).unwrap();
        let mut target = target_rv.enter(sid, &target_program).unwrap();
        source.send::<Message>(&42).await.unwrap();
        // Capture actual valid output, then forge its claimed sender from a
        // different bound OS socket. The header and payload are unchanged.
        let mut packet = [0; 64];
        let length = mailboxes.sockets[&(1, 0)].recv(&mut packet).unwrap();
        mailboxes.sockets[&(2, 0)]
            .send_to(&packet[..length], address(mailboxes.directory(), 1, 0))
            .unwrap();
        assert!(target.recv::<Message>().await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_a_parked_send_cannot_publish_its_old_payload() {
        type Message = g::Msg<1, u32>;
        let global = g::send::<0, 1, Message>();
        let source_program = project::<0>(&global);
        let target_program = project::<1>(&global);
        let mailboxes = Mailboxes::new(&[0, 1], 1).unwrap();
        let source_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[0]).unwrap(),
            None,
        )
        .unwrap();
        let target_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[1]).unwrap(),
            None,
        )
        .unwrap();
        let mut source_slab = vec![0; 64 * 1024];
        let mut target_slab = vec![0; 64 * 1024];
        let mut source_storage = SessionKitStorage::uninit();
        let mut target_storage = SessionKitStorage::uninit();
        let source_rv = source_storage
            .init()
            .rendezvous(&mut source_slab, source_io)
            .unwrap();
        let target_rv = target_storage
            .init()
            .rendezvous(&mut target_slab, target_io)
            .unwrap();
        let sid = SessionId::new(9013);
        let mut source = source_rv.enter(sid, &source_program).unwrap();
        let mut target = target_rv.enter(sid, &target_program).unwrap();
        let receiver = &mailboxes.sockets[&(1, 0)];
        {
            // A fresh registration in this current-thread runtime waits for
            // its first readiness dispatch. Observe actual Pending without
            // advancing the driver, then drop the operation before dispatch.
            let mut send = pin!(source.send::<Message>(&7));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(send.as_mut().poll(&mut cx).is_pending());
        }
        let mut packet = [0; 64];
        assert_eq!(
            receiver.recv(&mut packet).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        source.send::<Message>(&8).await.unwrap();
        assert_eq!(target.recv::<Message>().await.unwrap(), 8);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_stalled_receive_reports_the_transport_deadline() {
        type Message = g::Msg<1, u32>;
        let global = g::send::<0, 1, Message>();
        let target_program = project::<1>(&global);
        let mailboxes = Mailboxes::new(&[0, 1], 1).unwrap();
        let io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[1]).unwrap(),
            None,
        )
        .unwrap();
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage.init().rendezvous(&mut slab, io).unwrap();
        let mut target = rv.enter(SessionId::new(9014), &target_program).unwrap();
        let start = Instant::now();
        assert!(
            tokio::time::timeout(
                IO_DEADLINE + Duration::from_secs(1),
                target.recv::<Message>()
            )
            .await
            .unwrap()
            .is_err()
        );
        assert!(start.elapsed() >= IO_DEADLINE);
    }
}
