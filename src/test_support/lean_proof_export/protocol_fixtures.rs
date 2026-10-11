use crate::g;

pub(super) const RESOLVED_ROUTE: u16 = 901;
pub(super) const NESTED_OUTER_RESOLVER: u16 = 902;
pub(super) const NESTED_INNER_RESOLVER: u16 = 903;
pub(super) const ROLLED_RESOLVER: u16 = 904;
pub(super) const REJECTING_RESOLVER: u16 = 905;
pub(super) const FULL_ROLE_DOMAIN_RESOLVER: u16 = u16::MAX;

pub(super) type A = g::Send<0, 1, g::Msg<11, u32>>;
pub(super) type B = g::Send<0, 2, g::Msg<12, i32>>;
type LeftHead = g::Send<0, 1, g::Msg<21, ()>>;
type LeftTail = g::Send<1, 0, g::Msg<22, ()>>;
type Left = g::Seq<LeftHead, LeftTail>;
type Right = g::Send<0, 1, g::Msg<23, ()>>;
type Choice = g::Route<Left, Right>;
type Post = g::Send<0, 3, g::Msg<31, ()>>;
pub(super) type Steps = g::Seq<g::Par<A, B>, g::Seq<Choice, Post>>;
type RollLeft = g::Send<0, 1, g::Msg<41, ()>>;
type RollRight = g::Send<0, 1, g::Msg<42, ()>>;
type RollPost = g::Send<0, 1, g::Msg<43, ()>>;
pub(super) type RolledSteps = g::Roll<g::Seq<g::Route<RollLeft, RollRight>, RollPost>>;
type NestedHead = g::Send<0, 1, g::Msg<71, ()>>;
type NestedInnerTail = g::Send<0, 1, g::Msg<72, ()>>;
type NestedOuterTail = g::Send<0, 1, g::Msg<73, ()>>;
pub(super) type NestedRolledSteps =
    g::Roll<g::Seq<g::Roll<g::Seq<NestedHead, NestedInnerTail>>, NestedOuterTail>>;
type ResolvedLeft = g::Send<0, 1, g::Msg<51, u32>>;
type ResolvedRight = g::Send<0, 1, g::Msg<51, i32>>;
pub(super) type ResolvedSteps = g::Resolve<g::Route<ResolvedLeft, ResolvedRight>, RESOLVED_ROUTE>;
type NestedResolvedPrefix = g::Send<0, 1, g::Msg<61, ()>>;
type NestedResolvedLeft = g::Send<0, 1, g::Msg<62, ()>>;
type NestedResolvedRight = g::Send<0, 1, g::Msg<63, ()>>;
type NestedResolvedTail = g::Send<0, 1, g::Msg<64, ()>>;
type NestedResolvedInner =
    g::Resolve<g::Route<NestedResolvedLeft, NestedResolvedRight>, NESTED_INNER_RESOLVER>;
type NestedResolvedOuterLeft = g::Seq<NestedResolvedPrefix, NestedResolvedInner>;
pub(super) type NestedResolvedSteps =
    g::Resolve<g::Route<NestedResolvedOuterLeft, NestedResolvedTail>, NESTED_OUTER_RESOLVER>;
type RolledResolvedLeft = g::Send<0, 1, g::Msg<81, ()>>;
type RolledResolvedRight = g::Send<0, 1, g::Msg<82, ()>>;
pub(super) type RolledResolvedSteps =
    g::Roll<g::Resolve<g::Route<RolledResolvedLeft, RolledResolvedRight>, ROLLED_RESOLVER>>;
type RejectLeft = g::Send<0, 1, g::Msg<91, ()>>;
type RejectRight = g::Send<0, 1, g::Msg<92, ()>>;
pub(super) type RejectSteps = g::Resolve<g::Route<RejectLeft, RejectRight>, REJECTING_RESOLVER>;
type FullRoleDomainLeft = g::Send<254, 255, g::Msg<101, u32>>;
type FullRoleDomainRight = g::Send<254, 255, g::Msg<102, u32>>;
pub(super) type FullRoleDomainSteps = g::Roll<
    g::Resolve<g::Route<FullRoleDomainLeft, FullRoleDomainRight>, FULL_ROLE_DOMAIN_RESOLVER>,
>;
type MatchingLeftZero = g::Send<0, 1, g::Msg<111, ()>>;
type MatchingLeftOne = g::Send<0, 2, g::Msg<112, ()>>;
type MatchingRightZero = g::Send<3, 4, g::Msg<113, ()>>;
type MatchingRightOne = g::Send<2, 3, g::Msg<114, ()>>;
pub(super) type MatchingSteps =
    g::Par<g::Par<MatchingLeftZero, MatchingLeftOne>, g::Par<MatchingRightZero, MatchingRightOne>>;
