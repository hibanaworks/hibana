use super::{
    FragmentShape, LabelMarker, LoopControlMeaning, Message, MessageControlSpec, MessageSpec,
    NonEmptyParallelArm, RoleMarker, RouteArmHead, RouteArmLoopHead, SameRouteControllerRole,
    SendableLabel, TailLoopControl,
};

#[diagnostic::do_not_recommend]
impl<RouteController, const LOGICAL_LABEL: u8, Payload, Control, const LANE: u8> RouteArmHead
    for crate::g::Send<
        RouteController,
        RouteController,
        Message<LabelMarker<LOGICAL_LABEL>, Payload, Control>,
        LANE,
    >
where
    RouteController: RoleMarker,
    Message<LabelMarker<LOGICAL_LABEL>, Payload, Control>: MessageSpec + SendableLabel,
{
    type Controller = RouteController;
    type Label = LabelMarker<LOGICAL_LABEL>;
}

#[diagnostic::do_not_recommend]
impl<RouteController, const LOGICAL_LABEL: u8, Payload, Control, const LANE: u8> RouteArmLoopHead
    for crate::g::Send<
        RouteController,
        RouteController,
        Message<LabelMarker<LOGICAL_LABEL>, Payload, Control>,
        LANE,
    >
where
    RouteController: RoleMarker,
    Message<LabelMarker<LOGICAL_LABEL>, Payload, Control>:
        MessageSpec + MessageControlSpec + SendableLabel,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = LoopControlMeaning::from_control_spec(
        <Message<LabelMarker<LOGICAL_LABEL>, Payload, Control> as MessageControlSpec>::CONTROL,
    );
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmHead for crate::g::Seq<Left, Right>
where
    Left: RouteArmHead,
{
    type Controller = <Left as RouteArmHead>::Controller;
    type Label = <Left as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmLoopHead for crate::g::Seq<Left, Right>
where
    Left: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Left as RouteArmLoopHead>::LOOP_MEANING;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> RouteArmHead for crate::g::Policy<Inner, POLICY_ID>
where
    Inner: RouteArmHead,
{
    type Controller = <Inner as RouteArmHead>::Controller;
    type Label = <Inner as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> RouteArmLoopHead for crate::g::Policy<Inner, POLICY_ID>
where
    Inner: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Inner as RouteArmLoopHead>::LOOP_MEANING;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmHead for crate::g::Route<Left, Right>
where
    Left: RouteArmHead,
{
    type Controller = <Left as RouteArmHead>::Controller;
    type Label = <Left as RouteArmHead>::Label;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> RouteArmLoopHead for crate::g::Route<Left, Right>
where
    Left: RouteArmLoopHead,
{
    const LOOP_MEANING: Option<LoopControlMeaning> = <Left as RouteArmLoopHead>::LOOP_MEANING;
}

#[diagnostic::do_not_recommend]
impl<Controller> SameRouteControllerRole<Controller> for Controller where Controller: RoleMarker {}

#[diagnostic::do_not_recommend]
impl<From, To, Msg, const LANE: u8> NonEmptyParallelArm for crate::g::Send<From, To, Msg, LANE> {}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for crate::g::Seq<Left, Right> where Left: NonEmptyParallelArm {}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for crate::g::Route<Left, Right> where
    Left: NonEmptyParallelArm
{
}

#[diagnostic::do_not_recommend]
impl<Left, Right> NonEmptyParallelArm for crate::g::Par<Left, Right> where Left: NonEmptyParallelArm {}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> NonEmptyParallelArm for crate::g::Policy<Inner, POLICY_ID> where
    Inner: NonEmptyParallelArm
{
}

#[diagnostic::do_not_recommend]
impl<From, To, Msg, const LANE: u8> FragmentShape for crate::g::Send<From, To, Msg, LANE> {
    const IS_EMPTY: bool = false;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for crate::g::Seq<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for crate::g::Route<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> FragmentShape for crate::g::Par<Left, Right>
where
    Left: FragmentShape,
    Right: FragmentShape,
{
    const IS_EMPTY: bool = <Left as FragmentShape>::IS_EMPTY && <Right as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> FragmentShape for crate::g::Policy<Inner, POLICY_ID>
where
    Inner: FragmentShape,
{
    const IS_EMPTY: bool = <Inner as FragmentShape>::IS_EMPTY;
}

#[diagnostic::do_not_recommend]
impl<From, To, Msg, const LANE: u8> TailLoopControl for crate::g::Send<From, To, Msg, LANE>
where
    Msg: MessageSpec + MessageControlSpec,
{
    const IS_LOOP_CONTROL: bool =
        LoopControlMeaning::from_control_spec(<Msg as MessageControlSpec>::CONTROL).is_some();
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for crate::g::Seq<Left, Right>
where
    Left: TailLoopControl,
    Right: FragmentShape + TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = if <Right as FragmentShape>::IS_EMPTY {
        <Left as TailLoopControl>::IS_LOOP_CONTROL
    } else {
        <Right as TailLoopControl>::IS_LOOP_CONTROL
    };
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for crate::g::Route<Left, Right>
where
    Right: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Right as TailLoopControl>::IS_LOOP_CONTROL;
}

#[diagnostic::do_not_recommend]
impl<Left, Right> TailLoopControl for crate::g::Par<Left, Right>
where
    Right: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Right as TailLoopControl>::IS_LOOP_CONTROL;
}

#[diagnostic::do_not_recommend]
impl<Inner, const POLICY_ID: u16> TailLoopControl for crate::g::Policy<Inner, POLICY_ID>
where
    Inner: TailLoopControl,
{
    const IS_LOOP_CONTROL: bool = <Inner as TailLoopControl>::IS_LOOP_CONTROL;
}
