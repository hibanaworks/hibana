//! Static limits and universes shared across the crate.
//!
//! The values here intentionally favour predictability over configurability so
//! that they can be referenced inside `const` contexts without requiring
//! allocation or dynamic discovery.

/// Inclusive upper bound for labels supported by the default universe (`0..=127`).
/// Labels 0-38: Application protocol payloads
/// Labels 30-47: Management session (reply, load, observe, request)
/// Labels 48-63: Control labels (loop, splice, reroute, policy, checkpoint, etc.)
/// Labels 64-74: Internal management route heads
/// Labels 75-127: Extended application protocol payloads
pub const LABEL_MAX: u8 = 127;

/// Reserved label used for typed cancellation notifications.
pub const LABEL_CANCEL: u8 = 60;

/// Reserved label used for typed checkpoint proposals.
pub const LABEL_CHECKPOINT: u8 = 61;

/// Reserved label used for typed commit acknowledgements.
pub const LABEL_COMMIT: u8 = 62;

/// Reserved label used for typed rollback intents.
pub const LABEL_ROLLBACK: u8 = 63;

/// Management session label assignments (below control-plane range 48-63).
/// - Replies: 30-34
/// - Requests: 35-37, 40-45
/// - Streaming observe: 38-39
pub const LABEL_MGMT_REPLY_ERROR: u8 = 30;
pub const LABEL_MGMT_REPLY_LOADED: u8 = 31;
pub const LABEL_MGMT_REPLY_ACTIVATED: u8 = 32;
pub const LABEL_MGMT_REPLY_REVERTED: u8 = 33;
pub const LABEL_MGMT_REPLY_STATS: u8 = 34;
pub const LABEL_MGMT_ACTIVATE: u8 = 35;
pub const LABEL_MGMT_REVERT: u8 = 36;
pub const LABEL_MGMT_STATS: u8 = 37;
/// Streaming observe labels.
pub const LABEL_OBSERVE_BATCH: u8 = 38; // Batch of TapEvents (up to 50)
pub const LABEL_OBSERVE_STREAM_END: u8 = 39; // Reserved for EOS
pub const LABEL_MGMT_LOAD_BEGIN: u8 = 40; // For GenericCapToken<LoadBeginKind>
pub const LABEL_MGMT_LOAD_CHUNK: u8 = 42;
pub const LABEL_MGMT_LOAD_COMMIT: u8 = 43; // For GenericCapToken<LoadCommitKind>
pub const LABEL_MGMT_STAGE: u8 = 44;
pub const LABEL_OBSERVE_SUBSCRIBE: u8 = 45;
pub const LABEL_MGMT_LOAD_AND_ACTIVATE: u8 = 46;
pub const LABEL_MGMT_ROUTE_LOAD: u8 = 64;
pub const LABEL_MGMT_ROUTE_ACTIVATE: u8 = 65;
pub const LABEL_MGMT_ROUTE_REVERT: u8 = 66;
pub const LABEL_MGMT_ROUTE_STATS: u8 = 67;
pub const LABEL_MGMT_ROUTE_LOAD_FAMILY: u8 = 68;
pub const LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE: u8 = 69;
pub const LABEL_MGMT_ROUTE_REPLY_ERROR: u8 = 70;
pub const LABEL_MGMT_ROUTE_REPLY_LOADED: u8 = 71;
pub const LABEL_MGMT_ROUTE_REPLY_ACTIVATED: u8 = 72;
pub const LABEL_MGMT_ROUTE_REPLY_REVERTED: u8 = 73;
pub const LABEL_MGMT_ROUTE_REPLY_STATS: u8 = 74;
pub const LABEL_MGMT_ROUTE_COMMAND_FAMILY: u8 = 75;
pub const LABEL_MGMT_ROUTE_COMMAND_TAIL: u8 = 76;
pub const LABEL_MGMT_LOAD_FINAL_CHUNK: u8 = 77;
pub const LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY: u8 = 78;
pub const LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL: u8 = 79;
pub const LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL: u8 = 80;

// Control message label range (for route.case with GenericCapToken<ResourceKind>)
// These labels carry GenericCapToken<ResourceKind> payloads for control-plane operations
// expressed via route.case arms instead of bespoke combinators.
pub const LABEL_CONTROL_START: u8 = 48;
pub const LABEL_LOOP_CONTINUE: u8 = 48;
pub const LABEL_LOOP_BREAK: u8 = 49;
pub const LABEL_SPLICE_INTENT: u8 = 50;
pub const LABEL_SPLICE_ACK: u8 = 51;
pub const LABEL_REROUTE: u8 = 52;
pub const LABEL_ROUTE_DECISION: u8 = 57;
pub const LABEL_POLICY_LOAD: u8 = 53;
pub const LABEL_POLICY_ACTIVATE: u8 = 54;
pub const LABEL_POLICY_REVERT: u8 = 55;
pub const LABEL_POLICY_ANNOTATE: u8 = 56;
pub const LABEL_CONTROL_END: u8 = 58;

/// Maximum number of logical lanes per rendezvous.
///
/// Lanes are represented as `u8` throughout the crate (see
/// [`crate::control::types::Lane`]), so this bound never exceeds `u8::MAX`. Configuration
/// surfaces such as [`crate::runtime::config::Config::with_lane_range`] refer to this
/// constant when validating caller-provided ranges.
pub const LANES_MAX: u8 = 16;

/// Number of tap events maintained in the observation ring buffer.
pub const RING_EVENTS: usize = 2048;

/// Size of each individual ring buffer (User and Infra).
pub const RING_BUFFER_SIZE: usize = RING_EVENTS / 2;

/// Trait implemented by types that declare a label universe.
pub trait LabelUniverse {
    /// Inclusive upper bound for valid label identifiers.
    const MAX_LABEL: u8;
}

/// Default label universe (128 labels, 0..=127).
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultLabelUniverse;
impl LabelUniverse for DefaultLabelUniverse {
    const MAX_LABEL: u8 = LABEL_MAX;
}
