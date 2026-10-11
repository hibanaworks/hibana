use super::{
    Par, ProgramShape, ProgramSourceNode, Resolve, Roll, Route, Send, Seq, SourceRouteResolver,
    checked_source_count,
};

impl<const FROM: u8, const TO: u8, M> ProgramShape for Send<FROM, TO, M>
where
    M: crate::global::Message,
    M::Payload: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Send(crate::eff::EffAtom {
        from: FROM,
        to: TO,
        label: <M as crate::global::Message>::LOGICAL_LABEL,
        payload_schema: crate::global::payload_schema::<M>(),
        origin: crate::eff::EventOrigin::User,
        lane: 0,
    });
    const EVENT_COUNT: usize = 1;
    const SCOPE_MARKER_COUNT: usize = 0;
    const RESOLVER_MARKER_COUNT: usize = 0;
}

impl<Left, Right> ProgramShape for Seq<Left, Right>
where
    Left: ProgramShape,
    Right: ProgramShape,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Seq {
        left: &Left::SOURCE_NODE,
        right: &Right::SOURCE_NODE,
    };
    const EVENT_COUNT: usize = checked_source_count(Left::EVENT_COUNT, Right::EVENT_COUNT, 0);
    const SCOPE_MARKER_COUNT: usize =
        checked_source_count(Left::SCOPE_MARKER_COUNT, Right::SCOPE_MARKER_COUNT, 0);
    const RESOLVER_MARKER_COUNT: usize =
        checked_source_count(Left::RESOLVER_MARKER_COUNT, Right::RESOLVER_MARKER_COUNT, 0);
}

impl<Left, Right> ProgramShape for Route<Left, Right>
where
    Left: ProgramShape,
    Right: ProgramShape,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Route {
        left: &Left::SOURCE_NODE,
        right: &Right::SOURCE_NODE,
        resolver: SourceRouteResolver::Intrinsic,
    };
    const EVENT_COUNT: usize = checked_source_count(Left::EVENT_COUNT, Right::EVENT_COUNT, 0);
    const SCOPE_MARKER_COUNT: usize =
        checked_source_count(Left::SCOPE_MARKER_COUNT, Right::SCOPE_MARKER_COUNT, 4);
    const RESOLVER_MARKER_COUNT: usize =
        checked_source_count(Left::RESOLVER_MARKER_COUNT, Right::RESOLVER_MARKER_COUNT, 0);
}

impl<Left, Right> ProgramShape for Par<Left, Right>
where
    Left: ProgramShape,
    Right: ProgramShape,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Parallel {
        left: &Left::SOURCE_NODE,
        right: &Right::SOURCE_NODE,
    };
    const EVENT_COUNT: usize = checked_source_count(Left::EVENT_COUNT, Right::EVENT_COUNT, 0);
    const SCOPE_MARKER_COUNT: usize =
        checked_source_count(Left::SCOPE_MARKER_COUNT, Right::SCOPE_MARKER_COUNT, 3);
    const RESOLVER_MARKER_COUNT: usize =
        checked_source_count(Left::RESOLVER_MARKER_COUNT, Right::RESOLVER_MARKER_COUNT, 0);
}

impl<Left, Right, const RESOLVER_ID: u16> ProgramShape for Resolve<Route<Left, Right>, RESOLVER_ID>
where
    Left: ProgramShape,
    Right: ProgramShape,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Route {
        left: &Left::SOURCE_NODE,
        right: &Right::SOURCE_NODE,
        resolver: SourceRouteResolver::Dynamic(RESOLVER_ID),
    };
    const EVENT_COUNT: usize = <Route<Left, Right> as ProgramShape>::EVENT_COUNT;
    const SCOPE_MARKER_COUNT: usize = <Route<Left, Right> as ProgramShape>::SCOPE_MARKER_COUNT;
    const RESOLVER_MARKER_COUNT: usize = checked_source_count(
        <Route<Left, Right> as ProgramShape>::RESOLVER_MARKER_COUNT,
        0,
        1,
    );
}

impl<Inner> ProgramShape for Roll<Inner>
where
    Inner: ProgramShape,
{
    const SOURCE_NODE: ProgramSourceNode = ProgramSourceNode::Roll(&Inner::SOURCE_NODE);
    const EVENT_COUNT: usize = Inner::EVENT_COUNT;
    const SCOPE_MARKER_COUNT: usize = checked_source_count(Inner::SCOPE_MARKER_COUNT, 0, 2);
    const RESOLVER_MARKER_COUNT: usize = Inner::RESOLVER_MARKER_COUNT;
}
