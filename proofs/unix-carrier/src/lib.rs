#![cfg(unix)]
#![deny(unsafe_code)]

//! Proof carrier backed by a connected Unix datagram socket pair.
//!
//! This crate deliberately lives outside the Hibana package and public API. It
//! is a concrete carrier witness for the host-side proof gate, not a preferred
//! application transport or a resident Pico dependency.

use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    io,
    os::unix::net::UnixDatagram,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};

use hibana::runtime::{
    transport::{FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};

const DATA: u8 = 0;
const CLOSE: u8 = 1;
const HEADER_LEN: usize = 8;
const LENGTH_LEN: usize = 4;
const DATA_PREFIX_LEN: usize = 1 + HEADER_LEN + LENGTH_LEN;
const CLOSE_LEN: usize = 1 + HEADER_LEN;
const MAX_PAYLOAD_LEN: usize = 60 * 1024;
const MAX_DATAGRAM_LEN: usize = DATA_PREFIX_LEN + MAX_PAYLOAD_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ChannelKey {
    session: u32,
    lane: u8,
    source: u8,
    target: u8,
}

impl ChannelKey {
    fn outbound(port: PortOpen, peer_role: u8) -> Self {
        Self {
            session: port.session_id().raw(),
            lane: port.lane(),
            source: port.local_role(),
            target: peer_role,
        }
    }

    fn header(self, label: u8) -> [u8; HEADER_LEN] {
        let session = self.session.to_be_bytes();
        [
            session[0],
            session[1],
            session[2],
            session[3],
            self.lane,
            self.source,
            self.target,
            label,
        ]
    }

    fn from_header(header: [u8; HEADER_LEN]) -> Self {
        Self {
            session: u32::from_be_bytes([header[0], header[1], header[2], header[3]]),
            lane: header[4],
            source: header[5],
            target: header[6],
        }
    }
}

struct OwnedFrame {
    key: ChannelKey,
    header: [u8; HEADER_LEN],
    payload: Vec<u8>,
}

enum InboundPacket {
    Data(OwnedFrame),
    Close(ChannelKey),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InboxHealth {
    #[default]
    Live,
    Failed,
}

#[derive(Default)]
struct Inbox {
    frames: VecDeque<OwnedFrame>,
    closed: HashSet<ChannelKey>,
    waiters: HashMap<ChannelKey, Waker>,
    health: InboxHealth,
}

struct Inner {
    socket: UnixDatagram,
    inbox: Arc<Mutex<Inbox>>,
    open_ports: Mutex<HashMap<ChannelKey, usize>>,
    stopping: Arc<AtomicBool>,
    receiver: Mutex<Option<JoinHandle<()>>>,
    local_role: u8,
    peer_role: u8,
}

impl Inner {
    fn new(socket: UnixDatagram, local_role: u8, peer_role: u8) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        let receiver_socket = socket.try_clone()?;
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let receiver = spawn_receiver(
            receiver_socket,
            Arc::clone(&inbox),
            Arc::clone(&stopping),
            local_role,
            peer_role,
        );
        Ok(Self {
            socket,
            inbox,
            open_ports: Mutex::new(HashMap::new()),
            stopping,
            receiver: Mutex::new(Some(receiver)),
            local_role,
            peer_role,
        })
    }

    fn retain_port(&self, key: ChannelKey) {
        let mut open_ports = lock(&self.open_ports);
        match open_ports.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(2);
            }
            Entry::Occupied(_) => panic!("carrier channel opened more than once"),
        }
    }

    fn release_port(&self, key: ChannelKey) {
        let close = {
            let mut open_ports = lock(&self.open_ports);
            let count = open_ports
                .get_mut(&key)
                .expect("transport handle released without an open port");
            *count = count
                .checked_sub(1)
                .expect("transport handle count underflow");
            if *count == 0 {
                open_ports.remove(&key);
                true
            } else {
                false
            }
        };
        if close {
            self.send_close(key);
        }
    }

    fn send_close(&self, key: ChannelKey) {
        let mut packet = [0_u8; CLOSE_LEN];
        packet[0] = CLOSE;
        packet[1..].copy_from_slice(&key.header(0));
        loop {
            match self.socket.send(&packet) {
                Ok(CLOSE_LEN) => return,
                Ok(_) => panic!("Unix datagram close was not atomic"),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) if is_terminal_peer_error(&error) => return,
                Err(error) => panic!("Unix datagram close failed: {error}"),
            }
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        assert!(
            lock(&self.open_ports).is_empty(),
            "carrier dropped before all logical port handles"
        );
        self.stopping.store(true, Ordering::Release);
        let receiver = lock(&self.receiver)
            .take()
            .expect("receiver thread joined twice");
        receiver.join().expect("receiver thread panicked");
    }
}

/// One side of a fresh, peer-bound Unix datagram carrier generation.
pub struct UnixDatagramCarrier {
    inner: Arc<Inner>,
}

impl UnixDatagramCarrier {
    /// Create two role-bound carrier generations connected only to each other.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when both endpoints name the same
    /// role, or propagates socket creation and configuration failures.
    pub fn pair(left_role: u8, right_role: u8) -> io::Result<(Self, Self)> {
        if left_role == right_role {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "carrier peers must be distinct roles",
            ));
        }
        let (left, right) = UnixDatagram::pair()?;
        Ok((
            Self {
                inner: Arc::new(Inner::new(left, left_role, right_role)?),
            },
            Self {
                inner: Arc::new(Inner::new(right, right_role, left_role)?),
            },
        ))
    }
}

struct HandleLease<'a> {
    inner: &'a Inner,
    key: ChannelKey,
}

impl Drop for HandleLease<'_> {
    fn drop(&mut self) {
        self.inner.release_port(self.key);
    }
}

/// Send handle whose successful poll publishes one whole datagram or nothing.
pub struct UnixTx<'a> {
    lease: HandleLease<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameVisibility {
    Delivered,
    Requeued,
}

struct CurrentFrame {
    frame: OwnedFrame,
    visibility: FrameVisibility,
}

/// Receive handle owning the payload view returned through `poll_recv`.
pub struct UnixRx<'a> {
    lease: HandleLease<'a>,
    current: Option<CurrentFrame>,
}

impl Transport for UnixDatagramCarrier {
    type Tx<'a> = UnixTx<'a>;
    type Rx<'a> = UnixRx<'a>;

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        assert_eq!(
            port.local_role(),
            self.inner.local_role,
            "port role does not match the peer-bound carrier"
        );
        let outbound = ChannelKey::outbound(port, self.inner.peer_role);
        self.inner.retain_port(outbound);
        (
            UnixTx {
                lease: HandleLease {
                    inner: &self.inner,
                    key: outbound,
                },
            },
            UnixRx {
                lease: HandleLease {
                    inner: &self.inner,
                    key: outbound,
                },
                current: None,
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        if lock(&self.inner.inbox).health == InboxHealth::Failed {
            return Poll::Ready(Err(TransportError::Failed));
        }
        if outgoing.target_role() != tx.lease.key.target || outgoing.lane() != tx.lease.key.lane {
            return Poll::Ready(Err(TransportError::Failed));
        }
        let payload = outgoing.payload().as_bytes();
        if payload.len() > MAX_PAYLOAD_LEN {
            return Poll::Ready(Err(TransportError::Capacity));
        }
        let payload_len = u32::try_from(payload.len()).expect("bounded payload length");
        let mut packet = Vec::with_capacity(DATA_PREFIX_LEN + payload.len());
        packet.push(DATA);
        packet.extend_from_slice(&tx.lease.key.header(outgoing.frame_label().raw()));
        packet.extend_from_slice(&payload_len.to_be_bytes());
        packet.extend_from_slice(payload);
        match self.inner.socket.send(&packet) {
            Ok(sent) if sent == packet.len() => Poll::Ready(Ok(())),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Poll::Ready(Err(TransportError::Capacity))
            }
            Err(error) if is_terminal_peer_error(&error) => {
                fail_inbox(&self.inner.inbox);
                Poll::Ready(Err(TransportError::Offline))
            }
            Ok(_) | Err(_) => {
                fail_inbox(&self.inner.inbox);
                Poll::Ready(Err(TransportError::Failed))
            }
        }
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {
        // `poll_send` never retains payload state and never returns `Pending`.
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        let visibility = rx.current.as_ref().map(|current| current.visibility);
        if visibility == Some(FrameVisibility::Requeued) {
            let current = rx
                .current
                .as_mut()
                .expect("requeue visibility without a current frame");
            current.visibility = FrameVisibility::Delivered;
            return Poll::Ready(Ok(borrowed_frame(&current.frame)));
        }
        rx.current = None;
        let inbound = ChannelKey {
            source: rx.lease.key.target,
            target: rx.lease.key.source,
            ..rx.lease.key
        };
        let next = {
            let mut inbox = lock(&self.inner.inbox);
            if inbox.health == InboxHealth::Failed {
                Some(Err(TransportError::Failed))
            } else if let Some(position) =
                inbox.frames.iter().position(|frame| frame.key == inbound)
            {
                Some(Ok(inbox
                    .frames
                    .remove(position)
                    .expect("located frame disappeared")))
            } else if inbox.closed.contains(&inbound) {
                Some(Err(TransportError::Offline))
            } else {
                inbox.waiters.insert(inbound, cx.waker().clone());
                None
            }
        };
        match next {
            Some(Ok(frame)) => {
                rx.current = Some(CurrentFrame {
                    frame,
                    visibility: FrameVisibility::Delivered,
                });
                Poll::Ready(Ok(borrowed_frame(
                    &rx.current.as_ref().expect("stored frame disappeared").frame,
                )))
            }
            Some(Err(error)) => Poll::Ready(Err(error)),
            None => Poll::Pending,
        }
    }

    fn requeue(&self, rx: &mut Self::Rx<'_>) -> Result<(), TransportError> {
        match rx.current.as_mut() {
            Some(current) if current.visibility == FrameVisibility::Delivered => {
                current.visibility = FrameVisibility::Requeued;
                Ok(())
            }
            Some(_) | None => Err(TransportError::Failed),
        }
    }
}

fn borrowed_frame(frame: &OwnedFrame) -> ReceivedFrame<'_> {
    ReceivedFrame::framed(
        FrameHeader::from_bytes(frame.header),
        Payload::new(&frame.payload),
    )
}

fn spawn_receiver(
    socket: UnixDatagram,
    inbox: Arc<Mutex<Inbox>>,
    stopping: Arc<AtomicBool>,
    local_role: u8,
    peer_role: u8,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut datagram = vec![0_u8; MAX_DATAGRAM_LEN];
        while !stopping.load(Ordering::Acquire) {
            match socket.recv(&mut datagram) {
                Ok(length) => {
                    let Ok(packet) = parse_packet(&datagram[..length], local_role, peer_role)
                    else {
                        fail_inbox(&inbox);
                        return;
                    };
                    accept_packet(&inbox, packet);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) if is_terminal_peer_error(&error) => {
                    fail_inbox(&inbox);
                    return;
                }
                Err(_) => {
                    fail_inbox(&inbox);
                    return;
                }
            }
        }
    })
}

fn parse_packet(packet: &[u8], local_role: u8, peer_role: u8) -> Result<InboundPacket, ()> {
    if packet.len() < CLOSE_LEN {
        return Err(());
    }
    let header: [u8; HEADER_LEN] = packet[1..CLOSE_LEN].try_into().map_err(|_| ())?;
    let key = ChannelKey::from_header(header);
    if key.source != peer_role || key.target != local_role {
        return Err(());
    }
    match packet[0] {
        DATA if packet.len() >= DATA_PREFIX_LEN => {
            let payload_len = u32::from_be_bytes(
                packet[CLOSE_LEN..DATA_PREFIX_LEN]
                    .try_into()
                    .map_err(|_| ())?,
            ) as usize;
            if payload_len > MAX_PAYLOAD_LEN || packet.len() != DATA_PREFIX_LEN + payload_len {
                return Err(());
            }
            Ok(InboundPacket::Data(OwnedFrame {
                key,
                header,
                payload: packet[DATA_PREFIX_LEN..].to_vec(),
            }))
        }
        CLOSE if packet.len() == CLOSE_LEN => Ok(InboundPacket::Close(key)),
        _ => Err(()),
    }
}

fn accept_packet(inbox: &Mutex<Inbox>, packet: InboundPacket) {
    let waiters = {
        let mut inbox = lock(inbox);
        if inbox.health == InboxHealth::Failed {
            Vec::new()
        } else {
            match packet {
                InboundPacket::Data(frame) => {
                    if inbox.closed.contains(&frame.key) {
                        fail_locked(&mut inbox)
                    } else {
                        let key = frame.key;
                        inbox.frames.push_back(frame);
                        inbox.waiters.remove(&key).into_iter().collect()
                    }
                }
                InboundPacket::Close(key) => {
                    if inbox.closed.insert(key) {
                        inbox.waiters.remove(&key).into_iter().collect()
                    } else {
                        fail_locked(&mut inbox)
                    }
                }
            }
        }
    };
    for waker in waiters {
        waker.wake();
    }
}

fn fail_inbox(inbox: &Mutex<Inbox>) {
    let waiters = {
        let mut inbox = lock(inbox);
        fail_locked(&mut inbox)
    };
    for waker in waiters {
        waker.wake();
    }
}

fn fail_locked(inbox: &mut Inbox) -> Vec<Waker> {
    inbox.health = InboxHealth::Failed;
    inbox.frames.clear();
    inbox.closed.clear();
    inbox.waiters.drain().map(|(_, waker)| waker).collect()
}

fn is_terminal_peer_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("proof carrier mutex poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_or_wrong_peer_packets_fail_closed() {
        assert!(parse_packet(&[], 1, 0).is_err());

        let wrong_peer = ChannelKey {
            session: 7,
            lane: 0,
            source: 2,
            target: 1,
        };
        let mut packet = [0_u8; CLOSE_LEN];
        packet[0] = CLOSE;
        packet[1..].copy_from_slice(&wrong_peer.header(0));
        assert!(parse_packet(&packet, 1, 0).is_err());
    }

    #[test]
    fn close_is_not_replayable() {
        let inbox = Mutex::new(Inbox::default());
        let key = ChannelKey {
            session: 8,
            lane: 1,
            source: 0,
            target: 1,
        };
        accept_packet(&inbox, InboundPacket::Close(key));
        accept_packet(&inbox, InboundPacket::Close(key));
        assert_eq!(lock(&inbox).health, InboxHealth::Failed);
    }

    #[test]
    fn failure_is_absorbing_and_quarantines_accepted_frames() {
        let inbox = Mutex::new(Inbox::default());
        let key = ChannelKey {
            session: 9,
            lane: 1,
            source: 0,
            target: 1,
        };
        let frame = || {
            InboundPacket::Data(OwnedFrame {
                key,
                header: key.header(3),
                payload: vec![1],
            })
        };

        accept_packet(&inbox, frame());
        fail_inbox(&inbox);
        accept_packet(&inbox, frame());

        let inbox = lock(&inbox);
        assert_eq!(inbox.health, InboxHealth::Failed);
        assert!(inbox.frames.is_empty());
        assert!(inbox.closed.is_empty());
    }
}
