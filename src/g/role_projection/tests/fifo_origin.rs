//! A shared receive lane preserves causality across branches and reentry.
use super::*;

type LeftNotice = g::Msg<10, u32>;
type RightNotice = g::Msg<11, u32>;
type LeftData = g::Msg<12, u32>;
type RightData = g::Msg<13, u32>;
type Ack = g::Msg<14, u32>;
type Tail = g::Msg<15, u32>;
type Complete = g::Msg<16, u32>;
type Ready = g::Msg<17, u32>;
type Unordered = g::Roll<
    g::Seq<
        g::Route<
            g::Seq<g::Send<2, 0, LeftNotice>, g::Send<0, 1, LeftData>>,
            g::Seq<g::Send<2, 0, RightNotice>, g::Send<2, 1, RightData>>,
        >,
        g::Send<0, 1, Tail>,
    >,
>;
type OrderedBody = g::Seq<
    g::Route<
        g::Seq<g::Send<2, 0, LeftNotice>, g::Send<0, 1, LeftData>>,
        g::Seq<
            g::Send<2, 0, RightNotice>,
            g::Seq<g::Send<0, 2, Ready>, g::Seq<g::Send<2, 1, RightData>, g::Send<1, 0, Ack>>>,
        >,
    >,
    g::Send<0, 1, Tail>,
>;
type Ordered = g::Roll<OrderedBody>;
type Closed = g::Roll<g::Seq<OrderedBody, g::Send<1, 2, Complete>>>;

#[test]
fn branch_and_reentry_sender_changes_require_receive_causality() {
    const {
        let source = g::ProgramProjection::<Unordered, 32>::SOURCE_EFF_LIST;
        let mut merged = crate::global::const_dsl::EffList::<32>::new_partitioned(5, 6, 0);
        let mut event = 0;
        while event < source.len() {
            let mut atom = source.atom_at(event);
            atom.lane = 0;
            merged.push_event_mut(atom);
            event += 1;
        }
        merged.push_route_scope_mut(
            crate::global::const_dsl::ScopeId::route(0),
            0,
            2,
            4,
            crate::global::const_dsl::ReentryMark::Reentrant,
        );
        merged.push_roll_scope_mut(crate::global::const_dsl::ScopeId::roll_scope(0), 0, 5);
        assert!(!crate::global::const_dsl::validate_receive_lane_causality(
            &merged
        ));
        assert!(g::ProgramProjection::<Unordered, 32>::RECEIVE_CAUSAL);
        // The ACK orders the right branch's notification before its tail.
        assert!(g::ProgramProjection::<OrderedBody, 32>::RECEIVE_CAUSAL);
        // Distinct receive lanes also separate a later visit's notice from
        // the earlier acknowledgment. The public test delays that ACK.
        assert!(g::ProgramProjection::<Ordered, 32>::RECEIVE_CAUSAL);
        assert!(g::ProgramProjection::<Closed, 32>::ERROR.is_none());
        let () = g::ProgramProjection::<Closed, 32>::VALIDATION;
    }
}
