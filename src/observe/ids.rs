//! Tap event identifiers for the core observability surface.
//!
//! These constants define the canonical runtime evidence IDs emitted by Hibana
//! internals. Application telemetry belongs in choreography, not in the tap
//! ring.

// ────────────── Endpoint boundary (0x0200-0x020F) ──────────────

/// Endpoint send after its progress and any Endpoint transfer have committed.
///
/// All three `ENDPOINT_*` commit records have this layout:
///
/// - `causal_key`: Executing actor in the high byte; API direction in the low
///   byte (send = 1, receive = 2).
/// - `arg0`: Session identifier (u32)
/// - `arg1`: `logical_view<<24 | wire_lane<<16 | source_event`.
///   `source_event` retains the complete 16-bit wire label. Obtain the logical
///   message label from the compiled operation with that identity.
///
/// The executing actor can differ from the logical view after delegation.
/// Repeated occurrences of an event in a roll have the same source-event
/// identity. These fields do not encode nested roll visits or establish a
/// complete dynamic audit on their own.
pub const ENDPOINT_SEND: u16 = 0x0202;

/// Endpoint receive after payload validation and progress commit, with the
/// layout documented by [`ENDPOINT_SEND`]. A staged frame alone emits no such
/// record. A terminal route arm with no source operation is represented by
/// route selection records rather than a receive of an invented source event.
pub const ENDPOINT_RECV: u16 = 0x0203;

/// Endpoint event whose committed choreography row is session-originated.
///
/// Uses the layout documented by [`ENDPOINT_SEND`], including the API direction.
pub const ENDPOINT_SESSION: u16 = 0x0204;

/// Transport frame observed but not delivered because its header did not match
/// the endpoint descriptor.
///
/// - `causal_key`: expected lane in high byte, mismatch reason in low byte
/// - `arg0`: Expected session identifier (u32)
/// - `arg1`: Observed session for session mismatch; observed lane for lane
///   mismatch; otherwise `source<<24 | target<<16 | frame_label`, retaining
///   the full 16-bit label
pub const TRANSPORT_MISMATCH: u16 = 0x0205;

/// Frame session ID differs from the port's session.
pub const TRANSPORT_MISMATCH_SESSION: u8 = 1;
/// Frame lane differs from the projected lane.
pub const TRANSPORT_MISMATCH_LANE: u8 = 2;
/// Frame sender differs from the expected logical source role.
pub const TRANSPORT_MISMATCH_SOURCE_ROLE: u8 = 3;
/// Frame destination differs from the receiving logical role.
pub const TRANSPORT_MISMATCH_PEER_ROLE: u8 = 4;
/// Wire frame label differs from the permitted message.
pub const TRANSPORT_MISMATCH_LABEL: u8 = 5;

/// Transport frame metadata observed before endpoint progress committed.
///
/// This is staged frame evidence, not accepted-frame evidence. Endpoint commit
/// remains represented by `ENDPOINT_RECV` / `ENDPOINT_SESSION`.
///
/// - `arg0`: Observed session identifier (u32)
/// - `causal_key`: Observed lane in the high byte; zero in the low byte
/// - `arg1`: `source_role<<24 | target_role<<16 | frame_label`
pub const TRANSPORT_FRAME: u16 = 0x0206;

/// Transport operation reached a carrier-local terminal condition.
///
/// - `causal_key`: lane in high byte, fault reason in low byte
/// - `arg0`: Session identifier if available (u32)
/// - `arg1`: Lane index encoded on the wire (u8 promoted to u32)
pub const TRANSPORT_FAULT: u16 = 0x0207;

/// A transport operation returned `TransportError::Offline`.
pub const TRANSPORT_FAULT_OFFLINE: u8 = 1;
/// A transport operation returned `TransportError::Deadline`.
pub const TRANSPORT_FAULT_DEADLINE: u8 = 2;
/// A transport operation returned `TransportError::Capacity`.
pub const TRANSPORT_FAULT_CAPACITY: u8 = 3;
/// A transport operation returned `TransportError::Failed`.
pub const TRANSPORT_FAULT_FAILED: u8 = 4;

// ───────────── Lane lifecycle (0x0210-0x021F) ─────────────

/// Session/lane association acquired when the resident association count moves
/// from zero to one.
///
/// - `arg0`: Full session identifier (u32)
/// - `arg1`: Rendezvous identifier in the high 16 bits, lane in the low 16 bits
///
/// # Observable Properties
/// - Every LANE_ACQUIRE must eventually have a matching LANE_RELEASE
/// - Multiple LANE_ACQUIRE for the same session/lane without RELEASE indicates violation
pub const LANE_ACQUIRE: u16 = 0x0210;

/// Session/lane association released when the resident association count moves
/// from one to zero.
///
/// - `arg0`: Full session identifier (u32)
/// - `arg1`: Rendezvous identifier in the high 16 bits, lane in the low 16 bits
///
/// # Observable Properties
/// - Must follow LANE_ACQUIRE for the same lane
/// - Enables streaming verification of lane lifecycle correctness
pub const LANE_RELEASE: u16 = 0x0211;

// ───────────── Route decision (0x0220-0x022F) ─────────────

/// Route arm selection resolved by route authority.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: route_site<<16 | arm (u32)
/// - `causal_key`: lane marker with authority token encoded in the sequence field
///   (1 = ack, 2 = resolver, 3 = poll)
pub const ROUTE_ARM_SELECTION: u16 = 0x0221;

/// Compiled local operation returned by [`projected`](super::projected::projected).
///
/// All three `PROJECTED_*` events have this layout:
///
/// - `ts`: Zero-based local row ordinal.
/// - `causal_key`: Initial physical actor in the high byte, requested program
///   role in the low byte. Delegated views can share a physical actor's image.
/// - `arg0`: `wire_label<<16 | lane<<8 | message_label`.
/// - `arg1`: `source_role<<24 | target_role<<16 | payload_kind<<8 | resource_role`.
///   `payload_kind` is zero for a wire payload and one for an owned Endpoint;
///   `resource_role` is zero for wire payloads.
///
/// These describe static rows, including alternative branches and operations
/// on delegated views. They do not report a committed or currently enabled step.
pub const PROJECTED_LOCAL: u16 = 0x0300;

/// Compiled send operation, with the layout documented by [`PROJECTED_LOCAL`].
pub const PROJECTED_SEND: u16 = 0x0301;

/// Compiled receive operation, with the layout documented by [`PROJECTED_LOCAL`].
pub const PROJECTED_RECV: u16 = 0x0302;

/// Dynamic resolver decision evidence.
///
/// Endpoint facts use `ENDPOINT_*`, transport facts use `TRANSPORT_*`, lane
/// lifecycle facts use `LANE_*`, and dynamic route authority decisions use this
/// event. It is emitted once per dynamic resolver decision.
///
/// - `arg0`: full [`SessionId`](crate::runtime::ids::SessionId) raw value
/// - `arg1`: route_site<<16 | resolver_id
/// - `causal_key`: lane<<8 | result
/// - `result`: Left = 0, Right = 1, Reject = 0xff
pub const RESOLVER_AUDIT: u16 = 0x0407;
