//! TCP Transport example using Tokio.
//!
//! Demonstrates the fundamentals of hibana:
//! 1. Defining a global protocol with `g::send`/`g::seq`
//! 2. Projecting to role-local programs at compile time
//! 3. Implementing a custom `Transport` for Tokio TCP streams
//! 4. Running a complete ping-pong protocol over the wire
//!
//! Run with:
//! ```bash
//! cargo run --example tcp_tokio --features std
//! ```

#![cfg(feature = "std")]

use std::{
    collections::VecDeque,
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
};

use hibana::{
    binding::NoBinding,
    control::{cluster::AttachError, CpError},
    endpoint::ControlOutcome,
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    observe::TapEvent,
    rendezvous::{Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, RING_EVENTS},
    },
    transport::{NoopMetrics, Transport, TransportError, wire::Payload},
    SendError, RecvError,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::yield_now,
};

// =============================================================================
// Protocol Definition
// =============================================================================

/// Client role (initiates ping)
type Client = Role<0>;
/// Server role (responds with pong)
type Server = Role<1>;

/// Ping message: label=1, payload=u32
type Ping = Msg<1, u32>;
/// Pong message: label=2, payload=u32
type Pong = Msg<2, u32>;

/// Protocol steps: Client sends Ping, Server sends Pong
type ProtocolSteps = StepCons<
    SendStep<Client, Server, Ping>,
    StepCons<SendStep<Server, Client, Pong>, StepNil>,
>;

/// Global protocol definition
const PROTOCOL: g::Program<ProtocolSteps> = g::seq(
    g::send::<Client, Server, Ping, 0>(),
    g::send::<Server, Client, Pong, 0>(),
);

/// Role-local projections (computed at compile time)
type ClientLocal = <ProtocolSteps as ProjectRole<Client>>::Output;
type ServerLocal = <ProtocolSteps as ProjectRole<Server>>::Output;

const CLIENT_PROGRAM: g::RoleProgram<'static, 0, ClientLocal> =
    g::project::<0, ProtocolSteps, _>(&PROTOCOL);
const SERVER_PROGRAM: g::RoleProgram<'static, 1, ServerLocal> =
    g::project::<1, ProtocolSteps, _>(&PROTOCOL);

// =============================================================================
// TCP Transport Implementation
// =============================================================================

/// Simple length-prefixed TCP transport for hibana.
///
/// Frame format: [4 bytes length (big-endian)] [payload bytes]
#[derive(Clone)]
struct TcpTransport {
    /// Frames received from the peer
    rx_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Channel to send frames to the network writer task
    tx_sender: mpsc::UnboundedSender<Vec<u8>>,
}

impl TcpTransport {
    /// Create a new transport from an established TCP stream.
    fn from_stream(stream: TcpStream) -> Self {
        let (reader, writer) = stream.into_split();
        let rx_queue = Arc::new(Mutex::new(VecDeque::new()));
        let (tx_sender, tx_receiver) = mpsc::unbounded_channel();

        // Spawn reader task
        let rx_queue_clone = Arc::clone(&rx_queue);
        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                // Read length prefix
                let mut len_buf = [0u8; 4];
                if reader.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(len_buf) as usize;

                // Read payload
                let mut payload = vec![0u8; len];
                if reader.read_exact(&mut payload).await.is_err() {
                    break;
                }

                // Queue for hibana to consume
                rx_queue_clone
                    .lock()
                    .expect("rx queue poisoned")
                    .push_back(payload);
            }
        });

        // Spawn writer task
        tokio::spawn(async move {
            let mut writer = writer;
            let mut rx: mpsc::UnboundedReceiver<Vec<u8>> = tx_receiver;
            while let Some(payload) = rx.recv().await {
                // Write length prefix
                let len_bytes = (payload.len() as u32).to_be_bytes();
                if writer.write_all(&len_bytes).await.is_err() {
                    break;
                }
                // Write payload
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
            }
        });

        Self {
            rx_queue,
            tx_sender,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TcpTransportError;

impl From<TcpTransportError> for TransportError {
    fn from(_: TcpTransportError) -> Self {
        TransportError::Failed
    }
}

impl Transport for TcpTransport {
    type Error = TcpTransportError;
    type Tx<'a> = ();
    type Rx<'a> = ();
    type Send<'a> = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'a>>;
    type Recv<'a> = Pin<Box<dyn Future<Output = Result<Payload<'a>, Self::Error>> + Send + 'a>>;
    type Metrics = NoopMetrics;

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        payload: Payload<'f>,
        _dest_role: u8,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        let bytes = payload.as_bytes().to_vec();
        let sender = self.tx_sender.clone();
        Box::pin(async move {
            sender.send(bytes).map_err(|_| TcpTransportError)?;
            Ok(())
        })
    }

    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        let rx_queue = Arc::clone(&self.rx_queue);
        Box::pin(async move {
            loop {
                // Check if we have a frame ready
                if let Some(frame) = rx_queue.lock().expect("rx queue poisoned").pop_front() {
                    // Leak for simplicity in this example
                    let bytes = Box::leak(frame.into_boxed_slice());
                    return Ok(Payload::new(bytes));
                }
                // Yield and try again
                yield_now().await;
            }
        })
    }
}

// =============================================================================
// Error Type
// =============================================================================

#[derive(Debug)]
enum DemoError {
    Attach(AttachError),
    Send(SendError),
    Recv(RecvError),
    Io(std::io::Error),
    Cp(CpError),
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemoError::Attach(e) => write!(f, "attach error: {:?}", e),
            DemoError::Send(e) => write!(f, "send error: {:?}", e),
            DemoError::Recv(e) => write!(f, "recv error: {:?}", e),
            DemoError::Io(e) => write!(f, "io error: {}", e),
            DemoError::Cp(e) => write!(f, "cp error: {:?}", e),
        }
    }
}

impl std::error::Error for DemoError {}

impl From<AttachError> for DemoError {
    fn from(e: AttachError) -> Self { DemoError::Attach(e) }
}

impl From<SendError> for DemoError {
    fn from(e: SendError) -> Self { DemoError::Send(e) }
}

impl From<RecvError> for DemoError {
    fn from(e: RecvError) -> Self { DemoError::Recv(e) }
}

impl From<std::io::Error> for DemoError {
    fn from(e: std::io::Error) -> Self { DemoError::Io(e) }
}

impl From<CpError> for DemoError {
    fn from(e: CpError) -> Self { DemoError::Cp(e) }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}

fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

type Cluster = SessionCluster<'static, TcpTransport, DefaultLabelUniverse, CounterClock, 2>;

// =============================================================================
// Server Logic
// =============================================================================

async fn run_server(addr: SocketAddr) -> Result<(), DemoError> {
    let listener = TcpListener::bind(addr).await?;
    println!("[server] listening on {addr}");

    let (stream, peer) = listener.accept().await?;
    println!("[server] accepted connection from {peer}");

    // Create transport and hibana infrastructure
    let transport = TcpTransport::from_stream(stream);
    let config = Config::new(leak_tap_storage(), leak_slab(4096));
    let rendezvous: Rendezvous<'_, '_, TcpTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport);

    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster.add_rendezvous(rendezvous)?;

    let sid = SessionId::new(1);

    // Attach server cursor with NoBinding (no transport binding layer)
    let server = cluster.attach_cursor::<1, _, _, _>(
        rv_id, sid, &SERVER_PROGRAM, NoBinding
    )?;

    // Receive ping
    let (server, ping_value) = server.recv::<Ping>().await?;
    println!("[server] received Ping({})", ping_value);

    // Send pong (echo back the value multiplied by 2)
    let pong_value = ping_value * 2;
    let (_server, outcome) = server.flow::<Pong>()?.send(&pong_value).await?;
    assert!(matches!(outcome, ControlOutcome::None));
    println!("[server] sent Pong({})", pong_value);

    // Verify terminal state
    #[cfg(feature = "test-utils")]
    server.phase_cursor().assert_terminal();
    println!("[server] protocol completed successfully");

    Ok(())
}

// =============================================================================
// Client Logic
// =============================================================================

async fn run_client(addr: SocketAddr) -> Result<(), DemoError> {
    // Give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let stream = TcpStream::connect(addr).await?;
    println!("[client] connected to {addr}");

    // Create transport and hibana infrastructure
    let transport = TcpTransport::from_stream(stream);
    let config = Config::new(leak_tap_storage(), leak_slab(4096));
    let rendezvous: Rendezvous<'_, '_, TcpTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport);

    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster.add_rendezvous(rendezvous)?;

    let sid = SessionId::new(1);

    // Attach client cursor with NoBinding
    let client = cluster.attach_cursor::<0, _, _, _>(
        rv_id, sid, &CLIENT_PROGRAM, NoBinding
    )?;

    // Send ping
    let ping_value = 42u32;
    let (client, outcome) = client.flow::<Ping>()?.send(&ping_value).await?;
    assert!(matches!(outcome, ControlOutcome::None));
    println!("[client] sent Ping({})", ping_value);

    // Receive pong
    let (_client, pong_value) = client.recv::<Pong>().await?;
    println!("[client] received Pong({})", pong_value);
    assert_eq!(pong_value, ping_value * 2);

    // Verify terminal state
    #[cfg(feature = "test-utils")]
    client.phase_cursor().assert_terminal();
    println!("[client] protocol completed successfully");

    Ok(())
}

// =============================================================================
// Main
// =============================================================================

fn run_with_large_stack<F>(f: F) -> Result<(), DemoError>
where
    F: FnOnce() -> Result<(), DemoError> + Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("hibana-tcp-demo".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || f())?;
    match handle.join() {
        Ok(res) => res,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn main() -> Result<(), DemoError> {
    run_with_large_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| DemoError::Io(e))?;
        rt.block_on(async_main())
    })
}

async fn async_main() -> Result<(), DemoError> {
    let addr: SocketAddr = "127.0.0.1:9999".parse().expect("valid address");

    println!("=== hibana TCP Ping-Pong Example ===");
    println!();
    println!("Protocol: Client --Ping(u32)--> Server --Pong(u32)--> Client");
    println!();

    // Run server and client concurrently
    let (server_result, client_result) =
        tokio::join!(run_server(addr), run_client(addr));

    server_result?;
    client_result?;

    println!();
    println!("=== Success! ===");
    Ok(())
}
