//! Tap event identifiers for the core observability surface.
//!
//! These constants define the tap event IDs used throughout the observability
//! infrastructure. Keep them in sync with the documentation in `observe::core`.
//!
//! # Dual-Ring Event ID Allocation
//!
//! Events are routed to separate ring buffers based on ID range:
//!
//! - **User Ring** (`0x0000..0x00FF`): Application/EPF events (TAP_OUT, custom events)
//! - **Infra Ring** (`0x0100..0xFFFF`): System events (ENDPOINT_SEND, LANE_ACQUIRE, etc.)
//!
//! This separation prevents Observer Effect feedback loops where streaming
//! infrastructure events flood the ring and trigger continuous wake cycles.

// ────────────── Event ID Range Boundaries ──────────────

/// Upper bound (exclusive) for User Ring events.
/// Events with `id < USER_EVENT_RANGE_END` are routed to the User Ring.
pub const USER_EVENT_RANGE_END: u16 = 0x0100;

// ────────────── Endpoint boundary (0x0200-0x020F) ──────────────

/// AMPST cancellation initiated (begin message emitted).
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Lane / reason payload (u32)
pub const CANCEL_BEGIN: u16 = 0x0200;

/// AMPST cancellation acknowledged.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Lane that observed the acknowledgement (u32)
pub const CANCEL_ACK: u16 = 0x0201;

/// Endpoint send operation observed at the tap boundary.
///
/// - `arg0`: Packed role/lane/label/flags (u32)
/// - `arg1`: Session identifier (u32)
pub const ENDPOINT_SEND: u16 = 0x0202;

/// Endpoint receive operation observed at the tap boundary.
///
/// - `arg0`: Packed role/lane/label/flags (u32)
/// - `arg1`: Session identifier (u32)
pub const ENDPOINT_RECV: u16 = 0x0203;

/// Endpoint control-plane event (checkpoint, rollback, cancel, ...).
///
/// - `arg0`: Packed role/lane/label/flags (u32)
///
pub const ENDPOINT_CONTROL: u16 = 0x0204;

/// Splice handshake initiated.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation / causal context (u32)
pub const SPLICE_BEGIN: u16 = 0x0208;

/// Splice handshake committed.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation acknowledged (u32)
pub const SPLICE_COMMIT: u16 = 0x0209;

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

/// Route arm selection resolved via dynamic policy.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: scope_id<<16 | arm (u32)
/// - `causal`: lane marker with decision encoded in the sequence field (0 = skip, 1 = send)
pub const ROUTE_DECISION: u16 = 0x0221;

/// Local action handler reported a failure.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: eff_index<<16 | reason (u32)
#[cfg(test)]
pub const LOCAL_ACTION_FAIL: u16 = 0x0226;

// ───────────── Capability lifecycle (0x0240-0x024F) ─────────────
///
/// Base identifier for capability mint events. Actual tap IDs are computed as
/// `CAP_MINT_BASE + ResourceKind::TAG as u16` to yield `CAP_MINT::<K>`.
///
/// Emitted when a new capability token is created via:
/// - `Rendezvous::mint_cap`
/// - endpoint local control send paths such as `flow().send()`
///
/// # Event Encoding
/// - `arg0`: Session ID (u32) or packed lane/role/kind/shot
/// - `arg1`: Capability identifier (u32) or 0 for nonce-based tokens
///
/// # Observable Properties
/// - Every CAP_MINT must eventually have a matching CAP_CLAIM
/// - One-shot: CAP_MINT → CAP_CLAIM → CAP_EXHAUST
/// - Many-shot: CAP_MINT → CAP_CLAIM (no EXHAUST)
pub const CAP_MINT_BASE: u16 = 0x0240;

/// Capability token claimed (validation succeeded).
///
/// Base identifier for capability claim events. Actual tap IDs are computed as
/// `CAP_CLAIM_BASE + ResourceKind::TAG as u16` to yield `CAP_CLAIM::<K>`.
///
/// Emitted when a capability token is successfully validated via:
/// - `Rendezvous::claim_cap`
///
/// # Event Encoding
/// - `arg0`: Session ID (u32) or packed lane/role/kind/shot
/// - `arg1`: Capability identifier (u32) or 0 for nonce-based tokens
///
/// # Observable Properties
/// - Must follow CAP_MINT for the same session
/// - One-shot: first claim succeeds, subsequent claims return Exhausted
/// - Many-shot: multiple claims succeed
pub const CAP_CLAIM_BASE: u16 = 0x0241;

/// One-shot capability exhausted (lifecycle complete).
pub const CAP_EXHAUST_BASE: u16 = 0x0242;

/// Session effect initialisation completed.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Number of control effects/materialised resources (u32)
pub const EFFECT_INIT: u16 = 0x0500;

/// Checkpoint request issued by the rollback subsystem.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation marker (u32)
pub const CHECKPOINT_REQ: u16 = 0x0130;

/// Rollback requested (transition to previous checkpoint).
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Target generation (u32)
pub const ROLLBACK_REQ: u16 = 0x0131;

/// Rollback completed successfully.
///
/// - `arg0`: Session identifier (u32)
/// - `arg1`: Generation restored (u32)
pub const ROLLBACK_OK: u16 = 0x0132;

/// Transport-level telemetry event (ACK / Loss notification).
///
/// - `arg0`: Lower 32 bits of the packet number
/// - `arg1`: Packed payload length / retransmission counters
pub const TRANSPORT_EVENT: u16 = 0x0212;

/// Transport-level congestion metrics snapshot.
///
/// - `arg0`: `[ algo | queue_depth | srtt_scaled ]`
/// - `arg1`: `[ congestion_window_kib | in_flight_kib ]`
pub const TRANSPORT_METRICS: u16 = 0x0213;

/// Transport-level congestion metrics extension payload.
///
/// - `arg0`: `[ retransmissions | congestion_marks ]`
/// - `arg1`: Pacing interval in microseconds (0 indicates absent)
pub const TRANSPORT_METRICS_EXT: u16 = 0x0214;

/// Delegation begins (tracks shot discipline and in-flight count).
///
/// - `arg0`: Service identifier (high 32 bits of 64-bit id)
/// - `arg1`: Low 32 bits | shot flag << 31 | in-flight count
///
/// # Observable Properties
/// - Every `DELEG_BEGIN` must be followed by `DELEG_SPLICE`
/// - Shot discipline: Only `Many` delegations can be re-routed
pub const DELEG_BEGIN: u16 = 0x0230;

/// Routing policy selected a target shard/node.
///
/// - `arg0`: Policy identifier
/// - `arg1`: Shard or node identifier (u32)
///
/// # Observable Properties
/// - Occurs between `DELEG_BEGIN` and `DELEG_SPLICE`
/// - Enables auditing of routing decisions
pub const ROUTE_PICK: u16 = 0x0231;

/// Delegation splice completed (session handed over to new lane).
///
/// - `arg0`: from_lane (u8) | to_lane (u8) << 8 | generation (u16) << 16
/// - `arg1`: Session identifier (u32)
///
/// # Observable Properties
/// - Marks successful completion of delegation
/// - Generation increments monotonically
pub const DELEG_SPLICE: u16 = 0x0232;

/// Policy VM requested a session abort (mapped to cancel_begin/ack).
///
/// - `arg0`: Abort reason (u16 promoted to u32)
/// - `arg1`: Session identifier when known, otherwise 0
pub const POLICY_ABORT: u16 = 0x0400;

/// Policy VM emitted an annotation via ACT_ANNOT.
///
/// - `arg0`: Annotation key (u16 promoted to u32)
/// - `arg1`: Annotation value (u32)
#[cfg(test)]
pub const POLICY_ANNOT: u16 = 0x0401;

/// Policy VM trapped (fuel exhausted, illegal opcode/syscall, verify failure).
///
/// - `arg0`: Trap kind discriminant
/// - `arg1`: Session identifier when known, otherwise 0
pub const POLICY_TRAP: u16 = 0x0402;

/// Policy VM dispatched a control-plane effect.
///
/// - `arg0`: ControlOp discriminant (u16 promoted to u32)
/// - `arg1`: Effect operand when present, otherwise 0
#[cfg(test)]
pub const POLICY_EFFECT: u16 = 0x0403;

/// Policy-requested control-plane effect completed successfully.
///
/// - `arg0`: ControlOp discriminant (u16 promoted to u32)
/// - `arg1`: Session identifier when known, otherwise 0
pub const POLICY_RA_OK: u16 = 0x0404;

/// Policy VM slot activation scheduled after Load→Commit.
///
/// - `arg0`: Slot identifier (0=Forward,1=EndpointRx,2=EndpointTx,3=Rendezvous)
/// - `arg1`: Activated version identifier
#[cfg(test)]
pub const POLICY_COMMIT: u16 = 0x0405;

/// Policy VM slot rolled back to a previous version.
///
/// - `arg0`: Slot identifier (0=Forward,1=EndpointRx,2=EndpointTx,3=Rendezvous)
/// - `arg1`: Version identifier restored
#[cfg(test)]
pub const POLICY_ROLLBACK: u16 = 0x0406;

/// Policy audit core digest tuple.
///
/// - `arg0`: policy_digest
/// - `arg1`: event_hash
/// - `arg2`: signals_input_hash
pub const POLICY_AUDIT: u16 = 0x0407;

/// Policy audit extension digest tuple.
///
/// - `arg0`: signals_attrs_hash
/// - `arg1`: transport_snapshot_hash
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

/// Policy replay input tuple (first three input words).
///
/// - `arg0`: policy_input[0]
/// - `arg1`: policy_input[1]
/// - `arg2`: policy_input[2]
pub(crate) const POLICY_REPLAY_INPUT0: u16 = 0x040B;

/// Policy replay input tuple (last input word).
///
/// - `arg0`: policy_input[3]
/// - `arg1`: reserved (0)
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_INPUT1: u16 = 0x040C;

/// Policy replay transport tuple (latency/queue/congestion).
///
/// - `arg0`: latency_us (saturated to u32, 0 when unavailable)
/// - `arg1`: queue_depth
/// - `arg2`: congestion_marks
pub(crate) const POLICY_REPLAY_TRANSPORT0: u16 = 0x040D;

/// Policy replay transport tuple (retry count).
///
/// - `arg0`: retransmissions
/// - `arg1`: presence bitmask (bit0 latency, bit1 queue, bit2 congestion, bit3 retry)
/// - `arg2`: reserved (0)
pub(crate) const POLICY_REPLAY_TRANSPORT1: u16 = 0x040E;

/// Policy replay event extension tuple.
///
/// - `arg0`: triggering event arg1
/// - `arg1`: triggering event arg2
/// - `arg2`: triggering event causal key (u16 promoted to u32)
pub(crate) const POLICY_REPLAY_EVENT_EXT: u16 = 0x040F;

/// Policy liveness/defer audit tuple.
///
/// - `arg0`: `defer_source<<24 | retry_hint<<16 | remaining_budget`
/// - `arg1`: `scope_slot<<16 | selected_arm<<8 | ready_arm_mask`
///            (`scope_slot=0xFFFF` means non-route frontier, `selected_arm=0xFF` means unknown)
/// - `arg2`: `defer_reason<<16 | hint<<8 | frontier<<4 | binding_ready<<1 | exhausted_flag`
pub(crate) const POLICY_AUDIT_DEFER: u16 = 0x0410;
