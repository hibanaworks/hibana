//! Tap event identifiers for the core observability surface.
//!
//! These constants define the canonical runtime evidence IDs emitted by Hibana
//! internals. Application telemetry belongs in choreography, not in the tap
//! ring.

// ────────────── Endpoint boundary (0x0200-0x020F) ──────────────

/// Endpoint send operation observed at the tap boundary.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Packed role/lane/label/zero (u32)
pub const ENDPOINT_SEND: u16 = 0x0202;

/// Endpoint receive operation observed at the tap boundary.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Packed role/lane/label/zero (u32)
pub const ENDPOINT_RECV: u16 = 0x0203;

/// Endpoint event whose committed choreography row is session-originated.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Packed role/lane/label/zero (u32)
pub const ENDPOINT_SESSION: u16 = 0x0204;

/// Transport frame observed but not delivered because its header did not match
/// the endpoint descriptor.
///
/// - `causal_key`: expected lane in high byte, mismatch reason in low byte
/// - `arg0`: Expected session identifier (u32)
/// - `arg1`: Observed session for session mismatch; otherwise observed
///   lane/source/target/label packed as `lane<<24 | source<<16 | target<<8 | label`
pub const TRANSPORT_MISMATCH: u16 = 0x0205;

pub const TRANSPORT_MISMATCH_SESSION: u8 = 1;
pub const TRANSPORT_MISMATCH_LANE: u8 = 2;
pub const TRANSPORT_MISMATCH_SOURCE_ROLE: u8 = 3;
pub const TRANSPORT_MISMATCH_PEER_ROLE: u8 = 4;
pub const TRANSPORT_MISMATCH_LABEL: u8 = 5;

/// Transport frame metadata observed before endpoint progress committed.
///
/// This is staged frame evidence, not accepted-frame evidence. Endpoint commit
/// remains represented by `ENDPOINT_RECV` / `ENDPOINT_SESSION`.
///
/// - `arg0`: Observed session identifier (u32)
/// - `arg1`: observed_lane<<24 | source_role<<16 | target_role<<8 | frame_label
pub const TRANSPORT_FRAME: u16 = 0x0206;

/// Transport operation reached a carrier-local terminal condition.
///
/// - `causal_key`: lane in high byte, fault reason in low byte
/// - `arg0`: Session identifier if available (u32)
/// - `arg1`: Lane index encoded on the wire (u8 promoted to u32)
pub const TRANSPORT_FAULT: u16 = 0x0207;

pub const TRANSPORT_FAULT_OFFLINE: u8 = 1;
pub const TRANSPORT_FAULT_DEADLINE: u8 = 2;
pub const TRANSPORT_FAULT_CAPACITY: u8 = 3;
pub const TRANSPORT_FAULT_FAILED: u8 = 4;

// ───────────── Lane lifecycle (0x0210-0x021F) ─────────────

/// Session/lane association acquired when the resident association count moves
/// from zero to one.
///
/// - `arg0`: Rendezvous identifier (u32)
/// - `arg1`: Packed session/lane (u32)
///
/// # Observable Properties
/// - Every LANE_ACQUIRE must eventually have a matching LANE_RELEASE
/// - Multiple LANE_ACQUIRE for the same session/lane without RELEASE indicates violation
pub const LANE_ACQUIRE: u16 = 0x0210;

/// Session/lane association released when the resident association count moves
/// from one to zero.
///
/// - `arg0`: Rendezvous identifier (u32)
/// - `arg1`: Packed session/lane (u32)
///
/// # Observable Properties
/// - Must follow LANE_ACQUIRE for the same lane
/// - Enables streaming verification of lane lifecycle correctness
pub const LANE_RELEASE: u16 = 0x0211;

// ───────────── Route decision (0x0220-0x022F) ─────────────

/// Route arm selection resolved via dynamic resolver.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: scope_id<<16 | arm (u32)
/// - `causal`: lane marker with authority token encoded in the sequence field
///   (1 = ack, 2 = resolver, 3 = poll)
pub const ROUTE_ARM_SELECTION: u16 = 0x0221;

/// Resolver audit summary tuple.
///
/// - `arg0`: triggering event hash
/// - `arg1`: resolver slot tag in high half, triggering event id in low half
pub const RESOLVER_AUDIT: u16 = 0x0407;

/// Resolver replay event tuple.
///
/// - `arg0`: triggering event timestamp
/// - `arg1`: triggering event id (u16 promoted to u32)
pub const RESOLVER_REPLAY_EVENT: u16 = 0x0408;

/// Resolver replay event extension tuple.
///
/// - `arg0`: triggering event arg1
/// - `arg1`: triggering event causal key (u16 promoted to u32)
pub const RESOLVER_REPLAY_EVENT_EXT: u16 = 0x0409;

/// Resolver progress/defer audit tuple.
///
/// - `arg0`: `defer_source<<24 | scope_slot<<8 | pending_flag`
///   (`defer_source` is not a route-arm token tap sequence)
/// - `arg1`: `selected_arm<<24 | defer_reason<<16 | frontier<<12 | hint_present<<2 | ingress_ready<<1 | pending_flag`
///   (`scope_slot=0xFFFF` means non-route frontier, `selected_arm=0xFF` means no selected arm)
pub const RESOLVER_AUDIT_DEFER: u16 = 0x040A;
