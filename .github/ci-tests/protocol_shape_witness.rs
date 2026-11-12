//! Untrusted source evidence exchanged by the generator and EPF host codec.
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePlan {
    pub script: usize,
    /// EPF Unfold rows: tag and six integer arguments, zero when unused.
    pub unfolding: Vec<[u64; 7]>,
    /// Expanded route span and selected arm.
    pub choices: Vec<[u64; 3]>,
    /// Actor, logical view, original event, receive bit, expanded event.
    pub calls: Vec<[u64; 5]>,
}
