//! Serialize read-only observations after the localside tasks have completed.
use super::*;
use crate::{
    endpoint::proof_snapshot::ActorSnapshot,
    global::typestate::LocalAction,
    test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source},
};

pub(super) fn write(
    outer: DecisionArm,
    remote: &[(ActorSnapshot, ActorSnapshot, ActorSnapshot)],
    local: &[(ActorSnapshot, ActorSnapshot, ActorSnapshot)],
    observer: &[(ActorSnapshot, ActorSnapshot, ActorSnapshot)],
) {
    let (name, outer_arm, controller, controller_image) = match outer {
        DecisionArm::Left => (
            "Left",
            0,
            remote,
            &RoleProjection::<1, Steps, 32>::IMAGE_REF,
        ),
        DecisionArm::Right => (
            "Right",
            1,
            local,
            &RoleProjection::<3, Steps, 32>::IMAGE_REF,
        ),
    };
    let observer_image = &RoleProjection::<3, Steps, 32>::IMAGE_REF;
    assert_eq!(controller.len(), 5);
    assert_eq!(observer.len(), 5);
    let mut source = std::format!(
        "import Hibana.ActorReentryCommit\n\nset_option maxRecDepth 10000\n\n\
         def routeReentryChoreo : Hibana.Choreo := {}\n\n\
         def controllerImage : Hibana.RustDescriptorImage := {}\n\n\
         def observerImage : Hibana.RustDescriptorImage := {}\n\n",
        Steps::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(controller_image)),
        descriptor_source(RoleDescriptorRef::from_resident(observer_image)),
    );
    for (role, samples, image, view) in [
        ("controller", controller, controller_image, 10),
        ("observer", observer, observer_image, 3),
    ] {
        for (visit, ((_, before, after), number)) in
            samples.iter().zip([41, 43, 42, 44, 45]).enumerate()
        {
            assert_eq!(before.actor, image.role());
            assert_eq!(before.counter, after.counter);
            assert!(before.view_headers().eq(after.view_headers()));
            let arm = usize::from(number % 2 == 0);
            let label = if arm == 0 { 3 } else { 6 };
            let event = before
                .steps
                .iter()
                .position(|step| {
                    step.view == view
                        && match step.action {
                            LocalAction::Send { label: actual, .. }
                            | LocalAction::Recv { label: actual, .. } => actual == label,
                            LocalAction::Local { .. } | LocalAction::Terminate => false,
                        }
                })
                .unwrap();
            assert!(after.steps[event].done);
            assert_eq!(
                after.choices.iter().find(|row| row.0 == 0).unwrap().1,
                Some(outer_arm)
            );
            assert_eq!(
                after.choices.iter().find(|row| row.0 == 2).unwrap().1,
                Some(arm as u8)
            );
            let next_path = usize::from(outer_arm) * 2 + arm;
            let sample = std::format!("{role}Visit{visit}");
            let image_name = std::format!("{role}Image");
            let before_source = super::super::commit::observation_source(before);
            let after_source = super::super::commit::observation_source(after);
            if let Some(scope) = before.steps[event].reentry_scope {
                assert!(visit > 0);
                let previous_arm = usize::from([41, 43, 42, 44, 45][visit - 1] % 2 == 0);
                let previous_path = usize::from(outer_arm) * 2 + previous_arm;
                let previous_head = before
                    .steps
                    .iter()
                    .position(|step| {
                        step.view == view
                            && match step.action {
                                LocalAction::Send { label, .. }
                                | LocalAction::Recv { label, .. } => {
                                    label == if previous_arm == 0 { 3 } else { 6 }
                                }
                                LocalAction::Local { .. } | LocalAction::Terminate => false,
                            }
                    })
                    .unwrap();
                assert!(before.steps[previous_head].done);
                source.push_str(&std::format!(
                    "def {sample} : Hibana.ActorReentryCommitObservation := {{\n  \
                     previousPath := {previous_path}, nextPath := {next_path}, scope := {scope},\n  \
                     event := {event}, view := {view},\n  before := {before_source},\n  after := {after_source}\n}}\n\n\
                     theorem {sample}Accepted :\n  {sample}.check {image_name} routeReentryChoreo = true := by\n  decide\n\n\
                     theorem {sample}SourceOrdered :\n  {sample}.SourceOrdered {image_name} routeReentryChoreo :=\n  \
                     Hibana.accepted_body_reentry_restores_source_progress {sample}Accepted\n\n"
                ));
                for (suffix, change) in [
                    (
                        "MissingReset",
                        std::format!(
                            "after := {{ {sample}.after with steps := {sample}.before.steps }}"
                        ),
                    ),
                    (
                        "LostOuterChoice",
                        std::format!(
                            "after := {{ {sample}.after with choices := {sample}.after.choices.map (fun (scope, arm) => (scope, if scope = 0 then none else arm)) }}"
                        ),
                    ),
                    (
                        "MissingFreshChoice",
                        std::format!(
                            "after := {{ {sample}.after with choices := {sample}.after.choices.map (fun (scope, arm) => (scope, if scope = 2 then none else arm)) }}"
                        ),
                    ),
                    (
                        "WrongViewCount",
                        std::format!(
                            "after := {{ {sample}.after with liveViews := {sample}.after.liveViews + 1 }}"
                        ),
                    ),
                    (
                        "IncompleteVisit",
                        std::format!(
                            "before := {{ {sample}.before with steps := {sample}.before.steps.mapIdx (fun event step => if event = {previous_head} then {{ step with done := false }} else step) }}"
                        ),
                    ),
                    ("WrongScope", "scope := 0".into()),
                ] {
                    source.push_str(&std::format!(
                        "theorem {sample}{suffix}Rejected :\n  \
                         ({{ {sample} with {change} }} : Hibana.ActorReentryCommitObservation).check \
                         {image_name} routeReentryChoreo = false := by\n  decide\n\n"
                    ));
                }
            } else {
                assert!(!before.steps[event].done);
                assert_eq!(
                    before
                        .steps
                        .iter()
                        .zip(&after.steps)
                        .filter(|(old, new)| old.done != new.done)
                        .count(),
                    1
                );
                source.push_str(&std::format!(
                    "def {sample} : Hibana.ActorCommitObservation := {{\n  \
                     pathIndex := {next_path}, event := {event}, view := {view},\n  \
                     before := {before_source},\n  after := {after_source}\n}}\n\n\
                     theorem {sample}Accepted :\n  {sample}.check {image_name} routeReentryChoreo = true := by\n  decide\n\n\
                     theorem {sample}SourceOrdered :\n  {sample}.SourceOrdered {image_name} routeReentryChoreo :=\n  \
                     Hibana.accepted_actor_commit_matches_source_progress {sample}Accepted\n\n\
                     theorem {sample}MissingProgressRejected :\n  \
                     ({{ {sample} with after := {{ {sample}.after with steps := {sample}.before.steps }} }} : \
                     Hibana.ActorCommitObservation).check {image_name} routeReentryChoreo = false := by\n  decide\n\n\
                     theorem {sample}WrongViewCountRejected :\n  \
                     ({{ {sample} with after := {{ {sample}.after with liveViews := {sample}.after.liveViews + 1 }} }} : \
                     Hibana.ActorCommitObservation).check {image_name} routeReentryChoreo = false := by\n  decide\n\n\
                     theorem {sample}ChoicesPublished :\n  \
                     ({image_name}.eventEffIndex? {event}).bind (fun eff => \
                     Hibana.publishedObservedChoices? (Hibana.canonicalProgramSource routeReentryChoreo) \
                     eff {sample}.before.choices) = some {sample}.after.choices := by\n  decide\n\n"
                ));
                for position in ["before", "after"] {
                    source.push_str(&std::format!(
                        "theorem {sample}Contradictory{position}ChoiceRejected :\n  \
                         ({{ {sample} with {position} := {{ {sample}.{position} with choices := \
                         {sample}.{position}.choices.map (fun (scope, arm) => \
                         (scope, if scope = 0 then some (if arm = some .left then .right else .left) else arm)) }} }} : \
                         Hibana.ActorCommitObservation).check {image_name} routeReentryChoreo = false := by\n  decide\n\n"
                    ));
                    source.push_str(&std::format!(
                        "theorem {sample}Forgotten{position}ChoiceRejected :\n  \
                         ({{ {sample} with {position} := {{ {sample}.{position} with choices := \
                         {sample}.{position}.choices.map (fun (scope, arm) => \
                         (scope, if scope = 0 then none else arm)) }} }} : \
                         Hibana.ActorCommitObservation).check {image_name} routeReentryChoreo = false := by\n  decide\n\n"
                    ));
                }
            }
        }
    }
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(std::format!("ActorRoute{name}ReentryGenerated.lean")),
        source,
    )
    .unwrap();
}
