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
        assert!(!g::ProgramProjection::<Unordered, 32>::RECEIVE_CAUSAL);
        // The ACK orders the right branch's notification before its tail.
        assert!(g::ProgramProjection::<OrderedBody, 32>::RECEIVE_CAUSAL);
        // Repeating also requires that next visit's chooser cannot overtake the
        // previous ACK at role 0. Completion communicates that dependency.
        assert!(!g::ProgramProjection::<Ordered, 32>::RECEIVE_CAUSAL);
        assert!(g::ProgramProjection::<Closed, 32>::ERROR.is_none());
        let () = g::ProgramProjection::<Closed, 32>::VALIDATION;
    }
}
