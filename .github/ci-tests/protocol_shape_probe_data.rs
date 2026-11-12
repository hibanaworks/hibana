//! Immutable coordinates for scoped public-call trials.
use std::{format, string::String};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LocalProbe {
    Admission {
        at: u8,
        at_receive: bool,
        after: bool,
        occurrence: usize,
        actor: u8,
        role: u8,
        label: u8,
        receive: bool,
        choice: usize,
    },
    Borrow {
        group: (u8, u8),
        part: u8,
        occurrence: usize,
        actor: u8,
        role: u8,
        label: u8,
        receive: bool,
    },
    BeforeReceive {
        before: u8,
        occurrence: usize,
        actor: u8,
        role: u8,
        label: u8,
    },
}

impl LocalProbe {
    pub(crate) fn json(self) -> String {
        match self {
            Self::Admission {
                at,
                at_receive,
                after,
                occurrence,
                actor,
                role,
                label,
                receive,
                choice,
            } => format!(
                r#"{{"kind":"admission","at":{at},"atReceive":{at_receive},"after":{after},"occurrence":{occurrence},"actor":{actor},"role":{role},"label":{label},"receive":{receive},"choice":{choice}}}"#
            ),
            Self::Borrow {
                group,
                part,
                occurrence,
                actor,
                role,
                label,
                receive,
            } => format!(
                r#"{{"kind":"borrow","group":[{},{}],"part":{part},"occurrence":{occurrence},"actor":{actor},"role":{role},"label":{label},"receive":{receive},"accepted":false}}"#,
                group.0, group.1
            ),
            Self::BeforeReceive {
                before,
                occurrence,
                actor,
                role,
                label,
            } => format!(
                r#"{{"kind":"causal","before":{before},"occurrence":{occurrence},"actor":{actor},"role":{role},"label":{label},"accepted":false}}"#
            ),
        }
    }
}
