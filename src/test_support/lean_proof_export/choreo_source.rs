use crate::{g, global::Message};
use std::{format, string::String};

pub(super) trait LeanChoreo {
    fn lean_source() -> String;
}

impl<const FROM: u8, const TO: u8, M> LeanChoreo for g::Send<FROM, TO, M>
where
    M: Message,
    M::Payload: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.send {FROM} {TO} {} {}",
            M::LOGICAL_LABEL,
            crate::global::payload_schema::<M>()
        )
    }
}

impl<Left, Right> LeanChoreo for g::Seq<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.seq ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right> LeanChoreo for g::Par<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.par ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right> LeanChoreo for g::Route<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.route .intrinsic ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right, const RESOLVER_ID: u16> LeanChoreo
    for g::Resolve<g::Route<Left, Right>, RESOLVER_ID>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.route (.dynamic {RESOLVER_ID}) ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Inner> LeanChoreo for g::Roll<Inner>
where
    Inner: LeanChoreo,
{
    fn lean_source() -> String {
        format!("Hibana.Choreo.roll ({})", Inner::lean_source())
    }
}
