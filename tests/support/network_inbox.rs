//! Test-only stable logical TCP inboxes, owned by their I/O participant.
//! The peer stores bytes and a journal; it has no choreography or holder table.
#![deny(unsafe_code)]

use hibana::runtime::{
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};
pub use hibana_runtime::host::Process;
use hibana_runtime::host::{Io, deadline as timeout, sleep};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    pin::Pin,
    process::{Command, Stdio},
    task::{Context, Poll},
    time::{Duration, Instant},
    vec,
    vec::Vec,
};

const SEND: u8 = 1;
const RECV: u8 = 2;
const JOURNAL: u8 = 3;
const HEADER: usize = 9;
const MAX_PACKET: usize = HEADER + 60 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);

type Key = (u32, u8, u8);
type IoFuture<T> = Pin<std::boxed::Box<dyn Future<Output = Result<T, TransportError>>>>;

// These fields are partial socket input/output, not application progress.
struct Connection {
    stream: TcpStream,
    input: Vec<u8>,
    output: Vec<u8>,
    written: usize,
    acknowledgement_at: Instant,
}

pub struct Peer {
    pub address: SocketAddr,
    listener: TcpListener,
    connections: Vec<Connection>,
    inboxes: BTreeMap<Key, VecDeque<Vec<u8>>>,
    journal: Vec<(u8, Vec<u8>)>,
    acknowledgement_delay: fn(FrameHeader) -> Duration,
}

impl Peer {
    pub fn bind(acknowledgement_delay: fn(FrameHeader) -> Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            address: listener.local_addr().unwrap(),
            listener,
            connections: Vec::new(),
            inboxes: BTreeMap::new(),
            journal: Vec::new(),
            acknowledgement_delay,
        }
    }

    /// Drive this peer beside the Endpoint participants in an explicit scope.
    /// The fixture polls actual sockets; it makes no transport throughput claim.
    pub async fn serve(&mut self) -> io::Result<()> {
        loop {
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(true)?;
                        self.connections.push(Connection {
                            stream,
                            input: Vec::new(),
                            output: Vec::new(),
                            written: 0,
                            acknowledgement_at: Instant::now(),
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            // A delayed ACK or empty receive never stops another connection.
            let inboxes = &mut self.inboxes;
            let journal = &mut self.journal;
            let acknowledgement_delay = self.acknowledgement_delay;
            self.connections.retain_mut(|connection| {
                let result = (|| -> io::Result<bool> {
                    if connection.output.is_empty() {
                        let mut bytes = [0; 4096];
                        loop {
                            match connection.stream.read(&mut bytes) {
                                Ok(0) => return Ok(false),
                                Ok(count) => {
                                    connection.input.extend_from_slice(&bytes[..count]);
                                    if connection.input.len() > MAX_PACKET + 5 {
                                        return Err(io::ErrorKind::InvalidData.into());
                                    }
                                }
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                                    continue;
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        let input = &connection.input;
                        match input.first().copied() {
                            Some(SEND) if input.len() >= 5 => {
                                let length =
                                    u32::from_be_bytes(input[1..5].try_into().unwrap()) as usize;
                                if !(HEADER..=MAX_PACKET).contains(&length) {
                                    return Err(io::ErrorKind::InvalidData.into());
                                }
                                if input.len() == length + 5 {
                                    let packet = input[5..].to_vec();
                                    let header: [u8; HEADER] = packet[..HEADER].try_into().unwrap();
                                    let key = (
                                        u32::from_be_bytes(header[..4].try_into().unwrap()),
                                        header[6],
                                        header[4],
                                    );
                                    journal.push((SEND, packet.clone()));
                                    inboxes.entry(key).or_default().push_back(packet);
                                    // Publication precedes the ACK, including its
                                    // configured delay and a subsequent disconnect.
                                    connection.output.push(0);
                                    connection.acknowledgement_at = Instant::now()
                                        + acknowledgement_delay(FrameHeader::from_bytes(header));
                                } else if input.len() > length + 5 {
                                    return Err(io::ErrorKind::InvalidData.into());
                                }
                            }
                            Some(RECV) if input.len() == 7 => {
                                let key = (
                                    u32::from_be_bytes(input[1..5].try_into().unwrap()),
                                    input[5],
                                    input[6],
                                );
                                if let Some(packet) =
                                    inboxes.get_mut(&key).and_then(VecDeque::pop_front)
                                {
                                    journal.push((RECV, packet.clone()));
                                    connection
                                        .output
                                        .extend_from_slice(&(packet.len() as u32).to_be_bytes());
                                    connection.output.extend_from_slice(&packet);
                                }
                            }
                            Some(JOURNAL) if input.len() == 1 => {
                                connection
                                    .output
                                    .extend_from_slice(&(journal.len() as u32).to_be_bytes());
                                for (operation, packet) in journal.iter() {
                                    connection.output.push(*operation);
                                    connection
                                        .output
                                        .extend_from_slice(&(packet.len() as u32).to_be_bytes());
                                    connection.output.extend_from_slice(packet);
                                }
                            }
                            Some(SEND | RECV) | None => {}
                            Some(_) => return Err(io::ErrorKind::InvalidData.into()),
                        }
                    }
                    if connection.output.is_empty()
                        || Instant::now() < connection.acknowledgement_at
                    {
                        return Ok(true);
                    }
                    while connection.written < connection.output.len() {
                        match connection
                            .stream
                            .write(&connection.output[connection.written..])
                        {
                            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                            Ok(count) => connection.written += count,
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                return Ok(true);
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(false)
                })();
                match result {
                    Ok(keep) => keep,
                    // Closing a failed connection cannot retract queued bytes.
                    Err(error) => {
                        std::eprintln!("TCP fixture connection: {error}");
                        false
                    }
                }
            });
            sleep(Duration::from_millis(1)).await?;
        }
    }
}

pub async fn serve(acknowledgement_delay: fn(FrameHeader) -> Duration) {
    let mut peer = Peer::bind(acknowledgement_delay);
    std::println!("inbox={}", peer.address);
    io::stdout().flush().unwrap();
    peer.serve().await.unwrap();
}

pub async fn line(input: &mut Io) -> io::Result<std::string::String> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        if input.read(&mut byte).await? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if byte[0] == b'\n' {
            return std::string::String::from_utf8(bytes)
                .map_err(|_| io::ErrorKind::InvalidData.into());
        }
        bytes.push(byte[0]);
    }
}

pub async fn read_exact(stream: &mut Io, bytes: &mut [u8]) -> io::Result<()> {
    let mut read = 0;
    while read != bytes.len() {
        let count = stream.read(&mut bytes[read..]).await?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        read += count;
    }
    Ok(())
}
async fn read_u8(stream: &mut Io) -> io::Result<u8> {
    let mut bytes = [0];
    read_exact(stream, &mut bytes).await?;
    Ok(bytes[0])
}
async fn read_u32(stream: &mut Io) -> io::Result<u32> {
    let mut bytes = [0; 4];
    read_exact(stream, &mut bytes).await?;
    Ok(u32::from_be_bytes(bytes))
}
async fn read_packet(stream: &mut Io) -> io::Result<Vec<u8>> {
    let length = read_u32(stream).await? as usize;
    if !(HEADER..=MAX_PACKET).contains(&length) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut packet = vec![0; length];
    read_exact(stream, &mut packet).await?;
    Ok(packet)
}
async fn write_packet(stream: &mut Io, packet: &[u8]) -> io::Result<()> {
    stream
        .write_all(&(packet.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(packet).await
}

pub struct Broker {
    pub address: SocketAddr,
    _process: Process,
    _output: Io,
}
impl Broker {
    pub async fn start(entrypoint: &str) -> Self {
        let mut process = Process::spawn(
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", entrypoint, "--nocapture"])
                .env("HIBANA_NETWORK_BROKER", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit()),
        )
        .unwrap();
        let (input, mut output) = process.io().unwrap();
        drop(input);
        let address = hibana_runtime::race(
            async {
                loop {
                    let line = line(&mut output).await?;
                    if let Some((_, address)) = line.split_once("inbox=") {
                        return address.parse::<SocketAddr>().map_err(io::Error::other);
                    }
                }
            },
            timeout(DEADLINE),
        )
        .await
        .unwrap();
        Self {
            address,
            _process: process,
            _output: output,
        }
    }
    pub async fn journal(&self) -> Vec<(u8, Vec<u8>)> {
        journal(self.address).await
    }
}

pub async fn journal(address: SocketAddr) -> Vec<(u8, Vec<u8>)> {
    hibana_runtime::race(
        async {
            let mut stream = Io::connect(address).await?;
            stream.write_all(&[JOURNAL]).await?;
            let length = read_u32(&mut stream).await?;
            let mut journal = Vec::new();
            for _ in 0..length {
                let operation = read_u8(&mut stream).await?;
                assert!(matches!(operation, SEND | RECV));
                journal.push((operation, read_packet(&mut stream).await?));
            }
            Ok::<_, io::Error>(journal)
        },
        timeout(DEADLINE),
    )
    .await
    .unwrap()
}

pub struct NetworkTransport<'a> {
    addresses: [SocketAddr; 256],
    initial: Option<(u8, &'a EndpointBinding)>,
}

impl<'a> NetworkTransport<'a> {
    pub fn new(addresses: [SocketAddr; 256], initial: Option<(u8, &'a EndpointBinding)>) -> Self {
        Self { addresses, initial }
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
    hibana_runtime::race(work, timeout(DEADLINE))
        .await
        .map_err(|error| {
            if error.kind() == io::ErrorKind::TimedOut {
                TransportError::Deadline
            } else {
                TransportError::Offline
            }
        })
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

    fn initial_endpoint_binding(
        &self,
        _session: hibana::runtime::ids::SessionId,
        role: u8,
    ) -> Option<&EndpointBinding> {
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
                || outgoing.payload().as_bytes().len() > MAX_PACKET - HEADER
            {
                return Poll::Ready(Err(TransportError::Capacity));
            }
            let mut packet = tx.port.session_id().raw().to_be_bytes().to_vec();
            packet.extend_from_slice(&[
                tx.port.lane(),
                tx.port.local_role(),
                outgoing.target_role(),
            ]);
            packet.extend_from_slice(&outgoing.frame_label().raw().to_be_bytes());
            packet.extend_from_slice(outgoing.payload().as_bytes());
            let address = self.addresses[usize::from(outgoing.target_role())];
            tx.state = SendIo::Pending(std::boxed::Box::pin(deadline(async move {
                let mut stream = Io::connect(address).await?;
                stream.write_all(&[SEND]).await?;
                write_packet(&mut stream, &packet).await?;
                if read_u8(&mut stream).await? != 0 {
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
            let address = self.addresses[usize::from(rx.port.local_role())];
            let port = rx.port;
            rx.state = ReceiveIo::Pending(std::boxed::Box::pin(deadline(async move {
                let mut stream = Io::connect(address).await?;
                stream.write_all(&[RECV]).await?;
                stream
                    .write_all(&port.session_id().raw().to_be_bytes())
                    .await?;
                stream.write_all(&[port.local_role()]).await?;
                stream.write_all(&[port.lane()]).await?;
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
            FrameHeader::from_bytes(packet[..HEADER].try_into().unwrap()),
            Payload::new(&packet[HEADER..]),
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
