use super::{Par, ProgramSourceData, ProgramTerm, Resolve, Roll, Route, Send, Seq};
use crate::global::steps::RoleLaneMask;

impl<const FROM: u8, const TO: u8, M> ProgramTerm for Send<FROM, TO, M>
where
    M: crate::global::Message,
{
    const PROGRAM_SOURCE: ProgramSourceData = {
        ProgramSourceData::from_parts(
            crate::global::const_dsl::const_send_typed::<FROM, TO, M, 0>(),
            RoleLaneMask::empty().with_role(FROM, 0).with_role(TO, 0),
            1,
        )
    };
}

impl<Left, Right> ProgramTerm for Seq<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        <Left as ProgramTerm>::PROGRAM_SOURCE.seq(<Right as ProgramTerm>::PROGRAM_SOURCE);
}

impl<Left, Right> ProgramTerm for Route<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData = {
        let left = <Left as ProgramTerm>::PROGRAM_SOURCE;
        let right = <Right as ProgramTerm>::PROGRAM_SOURCE;
        left.route(right)
    };
}

impl<Left, Right> ProgramTerm for Par<Left, Right>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        { <Left as ProgramTerm>::PROGRAM_SOURCE.par(<Right as ProgramTerm>::PROGRAM_SOURCE) };
}

impl<Left, Right, const RESOLVER_ID: u16> ProgramTerm for Resolve<Route<Left, Right>, RESOLVER_ID>
where
    Left: ProgramTerm,
    Right: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData =
        <Route<Left, Right> as ProgramTerm>::PROGRAM_SOURCE.resolve_route(RESOLVER_ID);
}

impl<Inner> ProgramTerm for Roll<Inner>
where
    Inner: ProgramTerm,
{
    const PROGRAM_SOURCE: ProgramSourceData = <Inner as ProgramTerm>::PROGRAM_SOURCE.roll();
}
