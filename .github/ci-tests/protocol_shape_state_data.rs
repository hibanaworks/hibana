//! Lossless indexing of passive observations, never Endpoint progress state.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OperationState {
    pub id: usize,
    pub cut: Value,
    pub progress: Value,
    pub borrow_reentry: Option<(u64, (u64, u64))>,
    pub retained_rolls: Vec<u64>,
    pub credited_rolls: Vec<u64>,
}

impl OperationState {
    pub fn key(&self) -> Result<String> {
        Ok(serde_json::to_string(&(
            &self.cut,
            &self.progress,
            self.borrow_reentry,
            &self.retained_rolls,
            &self.credited_rolls,
        ))?)
    }

    pub fn decode(value: Value) -> Result<Self> {
        let state: Self = serde_json::from_value(value.clone())?;
        if serde_json::to_value(&state)? != value
            || !state.cut.is_object()
            || !state.progress.is_object()
            || state
                .borrow_reentry
                .is_some_and(|(scope, _)| scope >> 13 != 1)
            || state.retained_rolls.iter().any(|&scope| scope >= 8192)
            || state.credited_rolls.iter().any(|&scope| scope >= 8192)
        {
            return Err("invalid passive operation-state data".into());
        }
        Ok(state)
    }
}

pub fn read(path: &Path) -> Result<Vec<OperationState>> {
    let mut states = Vec::new();
    let mut unique = BTreeSet::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let state = OperationState::decode(serde_json::from_str(&line?)?)?;
        if state.id != states.len() || !unique.insert(state.key()?) {
            return Err("missing, duplicate or reordered operation state".into());
        }
        states.push(state);
    }
    Ok(states)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_identity_keeps_borrow_epochs_and_every_observed_value() {
        let value = serde_json::json!({"id":0,"cut":{"target":0},"progress":{"done":true},"borrowReentry":[8192,[0,1]],"retainedRolls":[0],"creditedRolls":[0]});
        let original = OperationState::decode(value.clone()).unwrap();
        let mut changed = original.clone();
        changed.id = 27;
        assert_eq!(original.key().unwrap(), changed.key().unwrap());
        changed.borrow_reentry = Some((8192, (1, 1)));
        assert_ne!(original.key().unwrap(), changed.key().unwrap());
        changed = original.clone();
        changed.credited_rolls.clear();
        assert_ne!(original.key().unwrap(), changed.key().unwrap());
        for field in [
            "cut",
            "progress",
            "borrowReentry",
            "retainedRolls",
            "creditedRolls",
        ] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(OperationState::decode(missing).is_err());
        }
        let mut wrong_kind = value;
        wrong_kind["borrowReentry"] = serde_json::json!([16384, [0, 1]]);
        assert!(OperationState::decode(wrong_kind).is_err());
    }
}
