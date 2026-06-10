//! Test local affine event-program witness semantics.
//!
//! This test module intentionally interprets the public choreography type AST
//! directly instead of reusing endpoint topology helpers. It keeps the
//! compiled-row `LocalEventProgram` honest without becoming runtime
//! authority.

use std::vec::Vec;

use crate::global::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceAction {
    Send { from: u8, to: u8, label: u8 },
    Recv { from: u8, to: u8, label: u8 },
    Local { role: u8, label: u8 },
}

impl ReferenceAction {
    const fn label(self) -> u8 {
        match self {
            Self::Send { label, .. } | Self::Recv { label, .. } | Self::Local { label, .. } => {
                label
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceConflictArm {
    pub(crate) conflict: usize,
    pub(crate) arm: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceEvent {
    pub(crate) id: usize,
    pub(crate) action: ReferenceAction,
    pub(crate) lane: u8,
    pub(crate) deps: Vec<usize>,
    pub(crate) conflicts: Vec<ReferenceConflictArm>,
}

impl ReferenceEvent {
    pub(crate) const fn label(&self) -> u8 {
        self.action.label()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceLocalProgram {
    events: Vec<ReferenceEvent>,
    conflict_count: usize,
}

impl ReferenceLocalProgram {
    pub(crate) fn from_steps<Steps, const ROLE: u8>() -> Self
    where
        Steps: ReferenceProject<ROLE>,
    {
        let mut builder = ReferenceBuilder::new();
        let root = ReferenceContext {
            incoming: Vec::new(),
            lane: builder.root_lane(),
            conflicts: Vec::new(),
        };
        let _ = Steps::project(&mut builder, root);
        builder.finish()
    }

    pub(crate) fn events(&self) -> &[ReferenceEvent] {
        &self.events
    }

    pub(crate) fn state(&self) -> ReferenceState<'_> {
        ReferenceState::new(self)
    }

    fn event_is_excluded(&self, event_id: usize, selected: &[Option<u8>]) -> bool {
        self.events[event_id]
            .conflicts
            .iter()
            .any(|membership| matches!(selected[membership.conflict], Some(arm) if arm != membership.arm))
    }

    fn event_allows_selected(&self, event: &ReferenceEvent, selected: &[Option<u8>]) -> bool {
        event
            .conflicts
            .iter()
            .all(|membership| selected[membership.conflict].is_none_or(|arm| arm == membership.arm))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceState<'a> {
    program: &'a ReferenceLocalProgram,
    done: Vec<bool>,
    selected: Vec<Option<u8>>,
}

impl<'a> ReferenceState<'a> {
    fn new(program: &'a ReferenceLocalProgram) -> Self {
        Self {
            program,
            done: std::vec![false; program.events.len()],
            selected: std::vec![None; program.conflict_count],
        }
    }

    pub(crate) fn enabled_labels(&self) -> Vec<u8> {
        self.program
            .events
            .iter()
            .filter(|event| self.event_enabled(event))
            .map(ReferenceEvent::label)
            .collect()
    }

    pub(crate) fn event_enabled(&self, event: &ReferenceEvent) -> bool {
        !self.done[event.id]
            && self.program.event_allows_selected(event, &self.selected)
            && event
                .deps
                .iter()
                .all(|dep| self.done[*dep] || self.program.event_is_excluded(*dep, &self.selected))
    }

    pub(crate) fn commit_label(&mut self, label: u8) -> Result<usize, ReferenceCommitError> {
        let Some(event_id) = self
            .program
            .events
            .iter()
            .find(|event| event.label() == label && self.event_enabled(event))
            .map(|event| event.id)
        else {
            return Err(ReferenceCommitError::NotEnabled { label });
        };
        self.commit_event(event_id)?;
        Ok(event_id)
    }

    pub(crate) fn commit_event(&mut self, event_id: usize) -> Result<(), ReferenceCommitError> {
        let Some(event) = self.program.events.get(event_id) else {
            return Err(ReferenceCommitError::MissingEvent { event_id });
        };
        if !self.event_enabled(event) {
            return Err(ReferenceCommitError::NotEnabled {
                label: event.label(),
            });
        }
        for membership in &event.conflicts {
            match self.selected[membership.conflict] {
                Some(arm) if arm != membership.arm => {
                    return Err(ReferenceCommitError::ConflictSelected {
                        conflict: membership.conflict,
                        selected: arm,
                        attempted: membership.arm,
                    });
                }
                Some(_selected) => {}
                None => self.selected[membership.conflict] = Some(membership.arm),
            }
        }
        self.done[event_id] = true;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceCommitError {
    NotEnabled {
        label: u8,
    },
    MissingEvent {
        event_id: usize,
    },
    ConflictSelected {
        conflict: usize,
        selected: u8,
        attempted: u8,
    },
}

pub(crate) trait ReferenceProject<const ROLE: u8> {
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceContext {
    incoming: Vec<usize>,
    lane: u8,
    conflicts: Vec<ReferenceConflictArm>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceBuilder {
    events: Vec<ReferenceEvent>,
    next_lane: u8,
    next_conflict: usize,
}

impl ReferenceBuilder {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            next_lane: 1,
            next_conflict: 0,
        }
    }

    const fn root_lane(&self) -> u8 {
        0
    }

    fn finish(self) -> ReferenceLocalProgram {
        ReferenceLocalProgram {
            events: self.events,
            conflict_count: self.next_conflict,
        }
    }

    fn add_event(
        &mut self,
        action: ReferenceAction,
        lane: u8,
        deps: Vec<usize>,
        conflicts: Vec<ReferenceConflictArm>,
    ) -> usize {
        let id = self.events.len();
        self.events.push(ReferenceEvent {
            id,
            action,
            lane,
            deps: unique_ids(deps),
            conflicts,
        });
        id
    }

    fn child_lane(&mut self) -> u8 {
        let lane = self.next_lane;
        self.next_lane = self
            .next_lane
            .checked_add(1)
            .expect("reference semantics lane id overflow");
        lane
    }

    fn conflict(&mut self) -> usize {
        let conflict = self.next_conflict;
        self.next_conflict = self
            .next_conflict
            .checked_add(1)
            .expect("reference semantics conflict id overflow");
        conflict
    }
}

impl<const ROLE: u8, const FROM: u8, const TO: u8, M> ReferenceProject<ROLE>
    for crate::g::Send<FROM, TO, M>
where
    M: Message,
{
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize> {
        let action = if ROLE == FROM && ROLE == TO {
            ReferenceAction::Local {
                role: ROLE,
                label: M::LOGICAL_LABEL,
            }
        } else if ROLE == FROM {
            ReferenceAction::Send {
                from: FROM,
                to: TO,
                label: M::LOGICAL_LABEL,
            }
        } else if ROLE == TO {
            ReferenceAction::Recv {
                from: FROM,
                to: TO,
                label: M::LOGICAL_LABEL,
            }
        } else {
            return context.incoming;
        };
        let id = builder.add_event(action, context.lane, context.incoming, context.conflicts);
        std::vec![id]
    }
}

impl<const ROLE: u8, Left, Right> ReferenceProject<ROLE> for crate::g::Seq<Left, Right>
where
    Left: ReferenceProject<ROLE>,
    Right: ReferenceProject<ROLE>,
{
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize> {
        let right_context = ReferenceContext {
            incoming: Left::project(builder, context.clone()),
            lane: context.lane,
            conflicts: context.conflicts,
        };
        Right::project(builder, right_context)
    }
}

impl<const ROLE: u8, Left, Right> ReferenceProject<ROLE> for crate::g::Par<Left, Right>
where
    Left: ReferenceProject<ROLE>,
    Right: ReferenceProject<ROLE>,
{
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize> {
        let left_context = ReferenceContext {
            incoming: context.incoming.clone(),
            lane: builder.child_lane(),
            conflicts: context.conflicts.clone(),
        };
        let right_context = ReferenceContext {
            incoming: context.incoming,
            lane: builder.child_lane(),
            conflicts: context.conflicts,
        };
        let left_exits = Left::project(builder, left_context);
        let right_exits = Right::project(builder, right_context);
        unique_ids(join_ids(left_exits, right_exits))
    }
}

impl<const ROLE: u8, Left, Right> ReferenceProject<ROLE> for crate::g::Route<Left, Right>
where
    Left: ReferenceProject<ROLE>,
    Right: ReferenceProject<ROLE>,
{
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize> {
        let conflict = builder.conflict();
        let mut left_conflicts = context.conflicts.clone();
        left_conflicts.push(ReferenceConflictArm { conflict, arm: 0 });
        let mut right_conflicts = context.conflicts.clone();
        right_conflicts.push(ReferenceConflictArm { conflict, arm: 1 });

        let left_context = ReferenceContext {
            incoming: context.incoming.clone(),
            lane: builder.child_lane(),
            conflicts: left_conflicts,
        };
        let right_context = ReferenceContext {
            incoming: context.incoming,
            lane: builder.child_lane(),
            conflicts: right_conflicts,
        };
        let left_exits = Left::project(builder, left_context);
        let right_exits = Right::project(builder, right_context);
        unique_ids(join_ids(left_exits, right_exits))
    }
}

impl<const ROLE: u8, Inner, const RESOLVER_ID: u16> ReferenceProject<ROLE>
    for crate::g::Resolve<Inner, RESOLVER_ID>
where
    Inner: ReferenceProject<ROLE>,
{
    fn project(builder: &mut ReferenceBuilder, context: ReferenceContext) -> Vec<usize> {
        Inner::project(builder, context)
    }
}

fn join_ids(mut left: Vec<usize>, right: Vec<usize>) -> Vec<usize> {
    left.extend(right);
    left
}

fn unique_ids(input: Vec<usize>) -> Vec<usize> {
    let mut output = Vec::new();
    for id in input {
        if !output.contains(&id) {
            output.push(id);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{ReferenceCommitError, ReferenceLocalProgram};
    use std::vec::Vec;

    type A = crate::g::Send<0, 1, crate::g::Msg<1, ()>>;
    type B = crate::g::Send<0, 1, crate::g::Msg<2, ()>>;
    type C = crate::g::Send<0, 1, crate::g::Msg<3, ()>>;
    type D = crate::g::Send<0, 1, crate::g::Msg<4, ()>>;
    type E = crate::g::Send<0, 1, crate::g::Msg<5, ()>>;
    type R = crate::g::Send<0, 1, crate::g::Msg<6, ()>>;
    type Post = crate::g::Send<0, 1, crate::g::Msg<7, ()>>;

    #[test]
    fn par_seq_par_join_requires_all_selected_dependencies() {
        type InnerJoin = crate::g::Par<A, B>;
        type Left = crate::g::Seq<InnerJoin, D>;
        type Program = crate::g::Seq<crate::g::Par<Left, E>, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut state = program.state();

        assert_enabled(&state, &[1, 2, 5]);
        state.commit_label(1).unwrap();
        assert_enabled(&state, &[2, 5]);
        assert!(matches!(
            state.commit_label(4),
            Err(ReferenceCommitError::NotEnabled { label: 4 })
        ));
        state.commit_label(2).unwrap();
        assert_enabled(&state, &[4, 5]);
        state.commit_label(4).unwrap();
        assert_enabled(&state, &[5]);
        state.commit_label(5).unwrap();
        assert_enabled(&state, &[7]);
    }

    #[test]
    fn route_selected_left_path_keeps_nested_parallel_join_live() {
        type NestedJoin = crate::g::Par<crate::g::Par<A, B>, C>;
        type Left = crate::g::Seq<NestedJoin, D>;
        type Choice = crate::g::Route<Left, R>;
        type Program = crate::g::Seq<Choice, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut state = program.state();

        assert_enabled(&state, &[1, 2, 3, 6]);
        state.commit_label(1).unwrap();
        assert_enabled(&state, &[2, 3]);
        assert!(matches!(
            state.commit_label(6),
            Err(ReferenceCommitError::NotEnabled { label: 6 })
        ));
        assert!(matches!(
            state.commit_label(4),
            Err(ReferenceCommitError::NotEnabled { label: 4 })
        ));
        state.commit_label(2).unwrap();
        assert_enabled(&state, &[3]);
        state.commit_label(3).unwrap();
        assert_enabled(&state, &[4]);
        state.commit_label(4).unwrap();
        assert_enabled(&state, &[7]);
    }

    #[test]
    fn route_unselected_arm_is_not_a_parallel_join_obligation() {
        type Choice = crate::g::Route<A, B>;
        type Program = crate::g::Seq<crate::g::Par<Choice, C>, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut left = program.state();
        assert_enabled(&left, &[1, 2, 3]);
        left.commit_label(1).unwrap();
        assert_enabled(&left, &[3]);
        left.commit_label(3).unwrap();
        assert_enabled(&left, &[7]);

        let mut right = program.state();
        right.commit_label(2).unwrap();
        assert_enabled(&right, &[3]);
        right.commit_label(3).unwrap();
        assert_enabled(&right, &[7]);
    }

    #[test]
    fn route_unselected_nested_parallel_arm_is_dead_not_join_obligation() {
        type Right = crate::g::Par<B, C>;
        type Choice = crate::g::Route<A, Right>;
        type Program = crate::g::Seq<Choice, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut left = program.state();
        assert_enabled(&left, &[1, 2, 3]);
        left.commit_label(1).unwrap();
        assert_enabled(&left, &[7]);
        assert!(matches!(
            left.commit_label(2),
            Err(ReferenceCommitError::NotEnabled { label: 2 })
        ));
        assert!(matches!(
            left.commit_label(3),
            Err(ReferenceCommitError::NotEnabled { label: 3 })
        ));
    }

    #[test]
    fn outer_left_selection_excludes_nested_right_route_and_parallel_events() {
        type InnerLeft = crate::g::Par<B, C>;
        type InnerRightRoute = crate::g::Route<InnerLeft, D>;
        type Choice = crate::g::Route<A, InnerRightRoute>;
        type Program = crate::g::Seq<Choice, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut left = program.state();
        assert_enabled(&left, &[1, 2, 3, 4]);
        left.commit_label(1).unwrap();
        assert_enabled(&left, &[7]);
        for label in [2, 3, 4] {
            assert!(matches!(
                left.commit_label(label),
                Err(ReferenceCommitError::NotEnabled { label: rejected }) if rejected == label
            ));
        }
    }

    #[test]
    fn alternating_route_parallel_nesting_uses_only_selected_arms_for_joins() {
        type InnerChoice = crate::g::Route<A, B>;
        type OuterLeft = crate::g::Par<InnerChoice, C>;
        type OuterChoice = crate::g::Route<OuterLeft, R>;
        type Program = crate::g::Seq<crate::g::Par<OuterChoice, E>, Post>;

        let program = ReferenceLocalProgram::from_steps::<Program, 0>();
        let mut left_inner_left = program.state();
        assert_enabled(&left_inner_left, &[1, 2, 3, 5, 6]);
        left_inner_left.commit_label(3).unwrap();
        assert_enabled(&left_inner_left, &[1, 2, 5]);
        left_inner_left.commit_label(5).unwrap();
        assert_enabled(&left_inner_left, &[1, 2]);
        left_inner_left.commit_label(1).unwrap();
        assert_enabled(&left_inner_left, &[7]);

        let mut left_inner_right = program.state();
        left_inner_right.commit_label(2).unwrap();
        assert_enabled(&left_inner_right, &[3, 5]);
        left_inner_right.commit_label(3).unwrap();
        assert_enabled(&left_inner_right, &[5]);
        left_inner_right.commit_label(5).unwrap();
        assert_enabled(&left_inner_right, &[7]);

        let mut outer_right = program.state();
        outer_right.commit_label(6).unwrap();
        assert_enabled(&outer_right, &[5]);
        outer_right.commit_label(5).unwrap();
        assert_enabled(&outer_right, &[7]);
    }

    fn assert_enabled(state: &super::ReferenceState<'_>, expected: &[u8]) {
        assert_sorted_eq(state.enabled_labels(), expected);
    }

    fn assert_sorted_eq(actual: Vec<u8>, expected: &[u8]) {
        assert_eq!(sorted(actual), sorted(expected.to_vec()));
    }

    fn sorted(mut labels: Vec<u8>) -> Vec<u8> {
        labels.sort_unstable();
        labels
    }
}
