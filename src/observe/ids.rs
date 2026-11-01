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

/// A checked Endpoint send is about to enter one transport send poll.
///
/// Emitted after source admission and payload encoding, immediately before
/// `Transport::poll_send`, including polls that subsequently return Pending or
/// an error. It does not report frame acceptance, progress commit or a new visit.
/// A resumed future can produce several records for the same operation.
///
/// - `causal_key`: Executing actor in the high byte, logical view in the low byte.
/// - `arg0`: Session identifier (u32).
/// - `arg1`: Prepared repetition-reset scope in the high 16 bits, source event
///   in the low 16 bits. Scope `0xffff` means no repetition reset is prepared.
///   The actual reset, if committed, is recorded by [`SCOPE_REENTRY`].
///
/// The compiled source identifies the logical peers, wire lane and payload.
/// Local unit operations never poll a transport and emit no such record.
pub const SEND_POLL: u16 = 0x0208;

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
/// - `causal_key`: Executing actor in the high byte; selection mechanism in
///   the low byte (1 = ack, 2 = resolver, 3 = poll).
///
/// Match the route site against `PROJECTED_CHOICE` to obtain its controlling
/// logical role. Delegation can make that role differ from the executing actor.
pub const ROUTE_ARM_SELECTION: u16 = 0x0221;

/// Progress commit reused an already completed source scope.
///
/// - `causal_key`: Executing actor in the high byte, logical view in the low byte.
/// - `arg0`: Session identifier (u32).
/// - `arg1`: Full scope identifier in the high 16 bits. For a roll scope the
///   low 16 bits identify the source event entering it again; for a route scope
///   they contain the newly selected arm.
///
/// The scope is the one actually reset by progress commit. This does not assign
/// a shared visit number to participants or report a preview as a committed
/// operation. Nested scopes retained by that reset are not separate reentries.
/// Route-only progress can emit this without an Endpoint commit; the record
/// never stands in for a successful payload receive.
pub const SCOPE_REENTRY: u16 = 0x0222;

/// Compiled local operation returned by [`projected`](super::projected::projected).
///
/// The three local-row events have this layout:
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

/// First record of a compiled protocol snapshot from [`projected`](super::projected::projected).
///
/// `ts` is format version 1, with bit 16 set when actor bindings are present.
/// `causal_key` is `initial_actor<<8 | requested_role`. `arg0` contains global
/// event count in the high 16 bits and scope marker count in the low 16 bits;
/// `arg1` contains actor decision count and initial actor count in that order.
pub const PROJECTED_PROGRAM: u16 = 0x0310;

/// Second snapshot header. `ts` is local row count, `arg0` is choice row count
/// and `arg1` is the largest logical role. The key matches [`PROJECTED_PROGRAM`].
pub const PROJECTED_ROLE: u16 = 0x0311;

/// One global source operation, including operations outside this actor.
///
/// `ts` is its full source-event ordinal. `causal_key` contains origin (user 0,
/// session 1) in the low byte and payload kind (wire 0, Endpoint 1) in the high
/// byte. `arg0` is `source<<24 | target<<16 | lane<<8 | message_label`.
/// `arg1` is the wire schema identifier or delegated logical role.
pub const PROJECTED_SOURCE: u16 = 0x0312;

/// One source scope marker, in the original marker order.
///
/// `ts` is marker ordinal, `causal_key` is the scope identifier shared with
/// route observations, `arg0` is the event boundary, and `arg1` is
/// `reentrant<<2 | boundary` (enter 0, parallel split 1, exit 2). A scope ID is
/// `kind<<13 | ordinal`, with kind route 0, roll 1 or parallel 2. Marker order
/// preserves nesting even when several scopes have identical event bounds.
pub const PROJECTED_SCOPE: u16 = 0x0313;

/// One compiled choice authority. `ts` is row ordinal, `causal_key` is its
/// scope, `arg0` is the controlling logical role, and `arg1` is zero for an
/// intrinsic choice or `resolver_id + 1` for a registered resolver.
pub const PROJECTED_CHOICE: u16 = 0x0314;

/// Immutable actor references for one source event. `ts` is source-event
/// ordinal and `arg0`/`arg1` are sender/receiver references. Values below 256
/// name an actor; larger values name a [`PROJECTED_ACTOR_DECISION`] record.
/// The key matches [`PROJECTED_PROGRAM`].
pub const PROJECTED_BINDING: u16 = 0x0315;

/// One conditional actor reference. `ts` is its reference (256 plus row
/// ordinal), `causal_key` is its choice scope and `arg0`/`arg1` are the left
/// and right actor references. Both references precede this decision.
pub const PROJECTED_ACTOR_DECISION: u16 = 0x0316;

/// One initial delegated-role binding. `ts` is row ordinal, `arg0` is the
/// logical role and `arg1` is its initial actor. Other roles initially name
/// themselves. The key matches [`PROJECTED_PROGRAM`].
pub const PROJECTED_INITIAL_ACTOR: u16 = 0x0317;

/// Dynamic resolver decision evidence.
///
/// Endpoint facts use `ENDPOINT_*`, transport facts use `TRANSPORT_*`, lane
/// lifecycle facts use `LANE_*`, and dynamic route authority decisions use this
/// event. It is emitted once per dynamic resolver decision.
///
/// - `arg0`: full [`SessionId`](crate::runtime::ids::SessionId) raw value
/// - `arg1`: route_site<<16 | resolver_id
/// - `causal_key`: executing_actor<<8 | result
/// - `result`: Left = 0, Right = 1, Reject = 0xff
///
/// `PROJECTED_CHOICE` identifies the controlling logical role for this site.
/// The actor is recorded for rejected decisions too, even when no Endpoint
/// operation subsequently commits.
pub const RESOLVER_AUDIT: u16 = 0x0407;
