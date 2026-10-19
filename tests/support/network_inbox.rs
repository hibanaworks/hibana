//! Test-only stable logical inboxes accessed through independent TCP connections.
//! Actors share neither socket descriptors nor runtime memory. The separate
//! server stores bytes, readiness and a test journal; it knows no choreography,
//! current holder, Endpoint binding or continuation. This loopback fixture is
//! not an authenticated production transport or a brokerless migration test.
#![deny(unsafe_code)]

use hibana::runtime::{
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    io::{self, BufRead, Write},
    net::SocketAddr,
    pin::Pin,
    process::{Child, Command, Stdio},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
    vec,
    vec::Vec,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify},
};

const SEND: u8 = 1;
const RECV: u8 = 2;
const JOURNAL: u8 = 3;
const MAX_PACKET: usize = 8 + 60 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);

type Key = (u32, u8, u8);
type IoFuture<T> = Pin<std::boxed::Box<dyn Future<Output = Result<T, TransportError>>>>;

#[derive(Default)]
struct Inbox {
    frames: Mutex<VecDeque<Vec<u8>>>,
    ready: Notify,
}

struct Server {
    inboxes: Mutex<BTreeMap<Key, Arc<Inbox>>>,
    journal: Mutex<Vec<(u8, Vec<u8>)>>,
    acknowledgement_delay: fn(FrameHeader) -> Duration,
}

impl Server {
    async fn inbox(&self, key: Key) -> Arc<Inbox> {
        Arc::clone(self.inboxes.lock().await.entry(key).or_default())
    }

    async fn connection(&self, mut stream: TcpStream) -> io::Result<()> {
        match stream.read_u8().await? {
            SEND => {
                let packet = read_packet(&mut stream).await?;
                let header: [u8; 8] = packet[..8].try_into().unwrap();
                let key = (
                    u32::from_be_bytes(header[..4].try_into().unwrap()),
                    header[6],
                    header[4],
                );
                let inbox = self.inbox(key).await;
                let mut frames = inbox.frames.lock().await;
                self.journal.lock().await.push((SEND, packet.clone()));
                frames.push_back(packet);
                drop(frames);
                inbox.ready.notify_waiters();
                let delay = (self.acknowledgement_delay)(FrameHeader::from_bytes(header));
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                // Acceptance precedes this acknowledgement. Losing its TCP
                // connection must not retract or duplicate the queued frame.
                stream.write_u8(0).await?;
            }
            RECV => {
                let session = stream.read_u32().await?;
                let role = stream.read_u8().await?;
                let lane = stream.read_u8().await?;
                let inbox = self.inbox((session, role, lane)).await;
                loop {
                    let ready = inbox.ready.notified();
                    tokio::pin!(ready);
                    ready.as_mut().enable();
                    let mut frames = inbox.frames.lock().await;
                    if let Some(packet) = frames.pop_front() {
                        self.journal.lock().await.push((RECV, packet.clone()));
                        drop(frames);
                        write_packet(&mut stream, &packet).await?;
                        break;
                    }
                    drop(frames);
                    ready.await;
                }
            }
            JOURNAL => {
                let journal = self.journal.lock().await.clone();
                stream
                    .write_u32(u32::try_from(journal.len()).unwrap())
                    .await?;
                for (operation, packet) in journal.iter() {
                    stream.write_u8(*operation).await?;
                    write_packet(&mut stream, packet).await?;
                }
            }
            _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
        }
        stream.shutdown().await
    }
}

async fn read_packet(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let length = usize::try_from(stream.read_u32().await?).unwrap();
    if !(8..=MAX_PACKET).contains(&length) {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut packet = vec![0; length];
    stream.read_exact(&mut packet).await?;
    Ok(packet)
}

async fn write_packet(stream: &mut TcpStream, packet: &[u8]) -> io::Result<()> {
    stream
        .write_u32(u32::try_from(packet.len()).unwrap())
        .await?;
    stream.write_all(packet).await
}

pub async fn serve(acknowledgement_delay: fn(FrameHeader) -> Duration) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::println!("inbox={}", listener.local_addr().unwrap());
    io::stdout().flush().unwrap();
    let server = Arc::new(Server {
        inboxes: Mutex::new(BTreeMap::new()),
        journal: Mutex::new(Vec::new()),
        acknowledgement_delay,
    });
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.connection(stream).await });
    }
}

pub struct Process(pub Child);

impl Process {
    pub async fn wait_success(&mut self) {
        tokio::time::timeout(DEADLINE, async {
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    assert!(status.success(), "network peer failed: {status}");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("network peer exits within the test deadline");
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().expect("reap test network process");
    }
}

pub struct Broker {
    pub address: SocketAddr,
    _process: Process,
}

impl Broker {
    pub async fn start(entrypoint: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", entrypoint, "--nocapture"])
            .env("HIBANA_NETWORK_BROKER", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let process = Process(child);
        let address = tokio::time::timeout(
            DEADLINE,
            tokio::task::spawn_blocking(move || {
                for line in io::BufReader::new(stdout).lines() {
                    let line = line.unwrap();
                    if let Some((_, address)) = line.split_once("inbox=") {
                        return address.parse::<SocketAddr>().unwrap();
                    }
                }
                panic!("broker exited without publishing its TCP address");
            }),
        )
        .await
        .unwrap()
        .unwrap();
        Self {
            address,
            _process: process,
        }
    }

    pub async fn journal(&self) -> Vec<(u8, Vec<u8>)> {
        tokio::time::timeout(DEADLINE, async {
            let mut stream = TcpStream::connect(self.address).await.unwrap();
            stream.write_u8(JOURNAL).await.unwrap();
            let length = stream.read_u32().await.unwrap();
            let mut journal = Vec::new();
            for _ in 0..length {
                let operation = stream.read_u8().await.unwrap();
                assert!(matches!(operation, SEND | RECV));
                journal.push((operation, read_packet(&mut stream).await.unwrap()));
            }
            journal
        })
        .await
        .unwrap()
    }
}

pub struct NetworkTransport<'a> {
    address: SocketAddr,
    initial: Option<(u8, &'a EndpointBinding)>,
}

impl<'a> NetworkTransport<'a> {
    pub fn new(address: SocketAddr, initial: Option<(u8, &'a EndpointBinding)>) -> Self {
        Self { address, initial }
    }
}

enum SendIo {
    Idle,
    Pending(IoFuture<()>),
    Retired,
}

pub struct Tx {
    port: PortOpen,
    state: SendIo,
}

enum ReceiveIo {
    Idle,
    Pending(IoFuture<Vec<u8>>),
    Delivered(Vec<u8>),
    Requeued(Vec<u8>),
    Failed(TransportError),
}

pub struct Rx {
    port: PortOpen,
    state: ReceiveIo,
}

async fn deadline<T>(work: impl Future<Output = io::Result<T>>) -> Result<T, TransportError> {
    tokio::time::timeout(DEADLINE, work)
        .await
        .map_err(|_| TransportError::Deadline)?
        .map_err(|_| TransportError::Offline)
}

impl Transport for NetworkTransport<'_> {
    type Tx<'a>
        = Tx
    where
        Self: 'a;
    type Rx<'a>
        = Rx
    where
        Self: 'a;

    fn initial_endpoint_binding(&self, role: u8) -> Option<&EndpointBinding> {
        self.initial
            .filter(|&(expected, _)| expected == role)
            .map(|(_, binding)| binding)
    }

    fn open(&self, port: PortOpen) -> (Tx, Rx) {
        (
            Tx {
                port,
                state: SendIo::Idle,
            },
            Rx {
                port,
                state: ReceiveIo::Idle,
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Tx,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        if matches!(tx.state, SendIo::Idle) {
            if outgoing.lane() != tx.port.lane()
                || outgoing.payload().as_bytes().len() > MAX_PACKET - 8
            {
                return Poll::Ready(Err(TransportError::Capacity));
            }
            let mut packet = tx.port.session_id().raw().to_be_bytes().to_vec();
            packet.extend_from_slice(&[
                tx.port.lane(),
                tx.port.local_role(),
                outgoing.target_role(),
                outgoing.frame_label().raw(),
            ]);
            packet.extend_from_slice(outgoing.payload().as_bytes());
            let address = self.address;
            tx.state = SendIo::Pending(std::boxed::Box::pin(deadline(async move {
                let mut stream = TcpStream::connect(address).await?;
                stream.write_u8(SEND).await?;
                write_packet(&mut stream, &packet).await?;
                if stream.read_u8().await? != 0 {
                    return Err(io::Error::from(io::ErrorKind::InvalidData));
                }
                Ok(())
            })));
        }
        let SendIo::Pending(pending) = &mut tx.state else {
            return Poll::Ready(Err(TransportError::Offline));
        };
        let result = pending.as_mut().poll(cx);
        if let Poll::Ready(result) = &result {
            tx.state = if result.is_ok() {
                SendIo::Idle
            } else {
                SendIo::Retired
            };
        }
        result
    }

    fn cancel_send(&self, tx: &mut Tx) {
        // Publication may already have reached the separate server. Never
        // reconnect this logical Tx after cancelling an uncertain send.
        tx.state = SendIo::Retired;
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Rx,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if let ReceiveIo::Failed(error) = rx.state {
            return Poll::Ready(Err(error));
        }
        if matches!(rx.state, ReceiveIo::Delivered(_)) {
            rx.state = ReceiveIo::Idle;
        }
        if matches!(rx.state, ReceiveIo::Requeued(_)) {
            let ReceiveIo::Requeued(packet) = std::mem::replace(&mut rx.state, ReceiveIo::Idle)
            else {
                unreachable!();
            };
            rx.state = ReceiveIo::Delivered(packet);
        }
        if matches!(rx.state, ReceiveIo::Idle) {
            let address = self.address;
            let port = rx.port;
            rx.state = ReceiveIo::Pending(std::boxed::Box::pin(deadline(async move {
                let mut stream = TcpStream::connect(address).await?;
                stream.write_u8(RECV).await?;
                stream.write_u32(port.session_id().raw()).await?;
                stream.write_u8(port.local_role()).await?;
                stream.write_u8(port.lane()).await?;
                read_packet(&mut stream).await
            })));
        }
        if let ReceiveIo::Pending(pending) = &mut rx.state {
            match pending.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    rx.state = ReceiveIo::Failed(error);
                    return Poll::Ready(Err(error));
                }
                Poll::Ready(Ok(packet)) => rx.state = ReceiveIo::Delivered(packet),
            }
        }
        let ReceiveIo::Delivered(packet) = &rx.state else {
            unreachable!()
        };
        Poll::Ready(Ok(ReceivedFrame::framed(
            FrameHeader::from_bytes(packet[..8].try_into().unwrap()),
            Payload::new(&packet[8..]),
        )))
    }

    fn requeue(&self, rx: &mut Rx) -> Result<(), TransportError> {
        let previous = std::mem::replace(&mut rx.state, ReceiveIo::Idle);
        let ReceiveIo::Delivered(packet) = previous else {
            rx.state = previous;
            return Err(TransportError::Failed);
        };
        rx.state = ReceiveIo::Requeued(packet);
        Ok(())
    }
}
