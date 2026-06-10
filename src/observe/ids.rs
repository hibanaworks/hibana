//! Tap event identifiers for the core observability surface.
//!
//! These constants define the tap event IDs used throughout the observability
//! infrastructure. Keep them in sync with the documentation in `observe::core`.
//!
//! # Dual-Ring Event ID Allocation
//!
//! Events are routed to separate ring buffers based on ID range:
//!
//! - **User Ring** (`0x0000..0x00FF`): application/custom events
//! - **Infra Ring** (`0x0100..0xFFFF`): System events (ENDPOINT_SEND, LANE_ACQUIRE, etc.)
//!
//! This separation prevents Observer Effect feedback loops where streaming
//! infrastructure events flood the ring and trigger continuous wake cycles.

// ────────────── Event ID Range Boundaries ──────────────

/// Upper bound (exclusive) for User Ring events.
/// Events with `id < USER_EVENT_RANGE_END` are routed to the User Ring.
pub const USER_EVENT_RANGE_END: u16 = 0x0100;

// ────────────── Endpoint boundary (0x0200-0x020F) ──────────────

/// Control abort initiated.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Lane / reason payload (u32)
pub const ABORT_BEGIN: u16 = 0x0200;

/// Control abort acknowledged.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Lane that observed the acknowledgement (u32)
pub const ABORT_ACK: u16 = 0x0201;

/// Endpoint send operation observed at the tap boundary.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Packed role/lane/label/flags (u32)
pub const ENDPOINT_SEND: u16 = 0x0202;

/// Endpoint receive operation observed at the tap boundary.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Packed role/lane/label/flags (u32)
pub const ENDPOINT_RECV: u16 = 0x0203;

/// Endpoint control-plane event (state snapshot, state restore, abort, ...).
///
/// - `arg0`: Packed role/lane/label/flags (u32)
///
pub const ENDPOINT_CONTROL: u16 = 0x0204;

/// Transport frame observed but not delivered because its header did not match
/// the endpoint descriptor.
///
/// - `causal_key`: expected lane in high byte, mismatch reason in low byte
/// - `arg0`: Expected session identifier (u32)
/// - `arg1`: Observed session identifier (u32)
/// - `arg2`: observed_lane<<24 | source_role<<16 | peer_role<<8 | frame_label
pub const TRANSPORT_MISMATCH: u16 = 0x0205;

pub const TRANSPORT_MISMATCH_SESSION: u8 = 1;
pub const TRANSPORT_MISMATCH_LANE: u8 = 2;
pub const TRANSPORT_MISMATCH_SOURCE_ROLE: u8 = 3;
pub const TRANSPORT_MISMATCH_PEER_ROLE: u8 = 4;
pub const TRANSPORT_MISMATCH_LABEL: u8 = 5;

/// Transport frame metadata observed before endpoint progress committed.
///
/// This is staged frame evidence, not accepted-frame evidence. Endpoint commit
/// remains represented by `ENDPOINT_RECV` / `ENDPOINT_CONTROL`.
///
/// - `causal_key`: zero in the core runtime ABI
/// - `arg0`: Observed session identifier (u32)
/// - `arg1`: observed_lane<<24 | source_role<<16 | peer_role<<8 | frame_label
/// - `arg2`: Optional adapter-local compact status, zero in core transports
pub const TRANSPORT_FRAME: u16 = 0x0206;

/// Transport operation reached a carrier-local terminal condition.
///
/// - `causal_key`: lane in high byte, fault reason in low byte
/// - `arg0`: Session identifier if available (u32)
/// - `arg1`: Reserved, zero in the core runtime ABI
/// - `arg2`: Reserved, zero in the core runtime ABI
pub const TRANSPORT_FAULT: u16 = 0x0207;

pub const TRANSPORT_FAULT_OFFLINE: u8 = 1;
pub const TRANSPORT_FAULT_DEADLINE: u8 = 2;
pub const TRANSPORT_FAULT_CAPACITY: u8 = 3;
pub const TRANSPORT_FAULT_FAILED: u8 = 4;

/// Topology handshake initiated.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation / causal context (u32)
pub const TOPOLOGY_BEGIN: u16 = 0x0208;

/// Topology handshake acknowledged.
///
/// - `arg0`: from_lane (u8) | to_lane (u8) << 8 | generation (u16) << 16
/// - `arg1`: Session identifier (u32)
pub const TOPOLOGY_ACK: u16 = 0x0209;

/// Topology handshake committed.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation acknowledged (u32)
pub const TOPOLOGY_COMMIT: u16 = 0x020A;

// ───────────── Lane lifecycle (0x0210-0x021F) ─────────────

/// Lane acquired via LaneLease (RAII lifecycle start).
///
/// - `arg0`: Rendezvous identifier (u32)
/// - `arg1`: Packed session/lane (u32)
///
/// # Observable Properties
/// - Every LANE_ACQUIRE must eventually have a matching LANE_RELEASE
/// - Multiple LANE_ACQUIRE for the same lane without RELEASE indicates violation
pub const LANE_ACQUIRE: u16 = 0x0210;

/// Lane released via LaneLease::Drop (RAII lifecycle end).
///
/// - `arg0`: Rendezvous identifier (u32)
/// - `arg1`: Packed session/lane (u32)
///
/// # Observable Properties
/// - Must follow LANE_ACQUIRE for the same lane
/// - Enables streaming verification of lane lifecycle correctness
pub const LANE_RELEASE: u16 = 0x0211;

// ───────────── Route / Loop control (0x0220-0x022F) ─────────────

/// Loop decision recorded (continue/break).
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: lane<<16 | idx<<8 | disposition (1 = continue, 0 = break)
pub const LOOP_DECISION: u16 = 0x0220;

/// Route arm selection resolved via dynamic resolver.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: scope_id<<16 | arm (u32)
/// - `causal`: lane marker with decision encoded in the sequence field (0 = skip, 1 = send)
pub const ROUTE_ARM_SELECTION: u16 = 0x0221;

/// Session effect initialisation completed.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Number of control effects/materialised resources (u32)
pub const EFFECT_INIT: u16 = 0x0500;

/// State snapshot request issued by the state-restore subsystem.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation marker (u32)
pub const STATE_SNAPSHOT_REQ: u16 = 0x0130;

/// State restore requested.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Target generation (u32)
pub const STATE_RESTORE_REQ: u16 = 0x0131;

/// State restore completed successfully.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation restored (u32)
pub const STATE_RESTORE_OK: u16 = 0x0132;

/// Policy transaction abort requested.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Snapshot generation restored (u32)
pub const POLICY_TX_ABORT: u16 = 0x0411;

/// Policy-requested control-plane effect completed successfully.
///
/// - `arg0`: ControlOp discriminant (u16 promoted to u32)
/// - `arg1`: Session identifier when known, otherwise 0
pub const POLICY_RA_OK: u16 = 0x0404;

/// Policy VM slot activation scheduled after Load→Commit.
///
/// - `arg0`: Slot identifier (0=Forward,1=EndpointRx,2=EndpointTx,3=Rendezvous)
/// - `arg1`: Activated version identifier
pub const POLICY_COMMIT: u16 = 0x0405;

/// Policy audit core digest tuple.
///
/// - `arg0`: policy_digest
/// - `arg1`: event_hash
/// - `arg2`: signals_input_hash
pub const POLICY_AUDIT: u16 = 0x0407;

/// Policy audit extension digest tuple.
///
/// - `arg0`: signals_attrs_hash
/// - `arg1`: policy_attrs_replay_hash
/// - `arg2`: slot/mode metadata
pub const POLICY_AUDIT_EXT: u16 = 0x0408;

/// Policy audit verdict tuple.
///
/// - `arg0`: verdict metadata (`tag<<24 | arm<<16`)
/// - `arg1`: reject reason (0 unless `Reject`)
/// - `arg2`: fuel_used
pub const POLICY_AUDIT_RESULT: u16 = 0x0409;

/// Policy replay event tuple.
///
/// - `arg0`: triggering event id (u16 promoted to u32)
/// - `arg1`: triggering event arg0
/// - `arg2`: triggering event arg1
pub(crate) const POLICY_REPLAY_EVENT: u16 = 0x040A;

/// Policy replay input tuple.
///
/// - `arg0`: policy_input.primary
/// - `arg1`: reserved (0)
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_INPUT0: u16 = 0x040B;

/// Reserved policy replay input tuple.
///
/// - `arg0`: reserved (0)
/// - `arg1`: reserved (0)
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_INPUT1: u16 = 0x040C;

/// Policy replay attribute tuple (latency/queue).
///
/// - `arg0`: latency_us (saturated to u32, 0 when unavailable)
/// - `arg1`: queue_depth
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_ATTRS0: u16 = 0x040D;

/// Policy replay attribute tuple (presence).
///
/// - `arg0`: reserved (0)
/// - `arg1`: presence bitmask (bit0 latency, bit1 queue)
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_ATTRS1: u16 = 0x040E;

/// Policy replay event extension tuple.
///
/// - `arg0`: triggering event arg1
/// - `arg1`: triggering event arg2
/// - `arg2`: triggering event causal key (u16 promoted to u32)
pub(crate) const POLICY_REPLAY_EVENT_EXT: u16 = 0x040F;

/// Policy progress/defer audit tuple.
///
/// - `arg0`: `defer_source<<24 | pending_flag`
///   (`defer_source` is not a route-arm token tap sequence)
/// - `arg1`: `scope_slot<<16 | selected_arm<<8 | ready_arm_mask`
///   (`scope_slot=0xFFFF` means non-route frontier, `selected_arm=0xFF` means unknown)
/// - `arg2`: `defer_reason<<16 | hint<<8 | frontier<<4 | ingress_ready<<1 | pending_flag`
pub(crate) const POLICY_AUDIT_DEFER: u16 = 0x0410;
