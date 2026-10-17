//! Serialize real body reentries, including retained enclosing choices.
use super::*;
use crate::{
    endpoint::proof_snapshot::ActorSnapshot,
    test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source},
};
use std::{format, fs};

pub(super) fn write_observations(observations: &[(u8, ActorSnapshot, ActorSnapshot)]) {
    let images = [
        &RoleProjection::<0, reentry_choice::Steps, 32>::IMAGE_REF,
        &RoleProjection::<1, reentry_choice::Steps, 32>::IMAGE_REF,
        &RoleProjection::<3, reentry_choice::Steps, 32>::IMAGE_REF,
    ];
    let mut source = format!(
        "import Hibana.ActorReentryCommit\n\nset_option maxRecDepth 10000\n\n\
         def reentryCommitChoreo : Hibana.Choreo := {}\n\n",
        reentry_choice::Steps::lean_source(),
    );
    for image in images {
        source.push_str(&format!(
            "def reentryActor{} : Hibana.RustDescriptorImage := {}\n\n",
            image.role(),
            descriptor_source(RoleDescriptorRef::from_resident(image)),
        ));
    }
    let mut count = 0;
    for (choice, before, after) in observations {
        let (label, direction) = match before.actor {
            0 => (*choice, true),
            1 => (*choice, false),
            3 => (if *choice == 1 { 3 } else { 24 }, false),
            _ => unreachable!(),
        };
        let mut entries = before
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| match step.action {
                LocalAction::Send { label: actual, .. } => direction && actual == label,
                LocalAction::Recv { label: actual, .. } => !direction && actual == label,
                LocalAction::Local { .. } | LocalAction::Terminate => false,
            });
        let (event, entry) = entries.next().unwrap();
        assert!(entries.next().is_none());
        let Some(scope) = entry.reentry_scope else {
            // The first owner send belongs to the initial visit, not a reset.
            assert_eq!(before.actor, 0);
            continue;
        };
        assert!(entry.done);
        assert_eq!(before.choices, after.choices);
        assert!(before.choices.iter().any(|(_, arm)| arm.is_some()));
        let path = usize::from(*choice == 23);
        let actor = before.actor;
        let image = images
            .into_iter()
            .find(|image| image.role() == actor)
            .unwrap();
        let wrong_scope = (0..usize::from(image.columns().roll_scopes.len))
            .map(|slot| image.roll_scope_row(slot).unwrap().scope().raw() % 8192)
            .find(|candidate| *candidate != scope)
            .unwrap();
        let name = format!("reentry{path}Actor{actor}");
        source.push_str(&format!(
            "def {name} : Hibana.ActorReentryCommitObservation := {{\n  \
             previousPath := {path}, nextPath := {path}, scope := {scope},\n  \
             event := {event}, view := {},\n  before := {},\n  after := {}\n}}\n\n\
             theorem {name}Accepted :\n  \
             {name}.check reentryActor{actor} reentryCommitChoreo = true := by\n  decide\n\n\
             theorem {name}SourceOrdered :\n  \
             {name}.SourceOrdered reentryActor{actor} reentryCommitChoreo :=\n  \
             Hibana.accepted_body_reentry_restores_source_progress {name}Accepted\n\n\
             theorem {name}MissingResetRejected :\n  \
             ({{ {name} with after := {{ {name}.after with steps := {name}.before.steps }} }} : \
             Hibana.ActorReentryCommitObservation).check reentryActor{actor} reentryCommitChoreo = false := by\n  decide\n\n\
             theorem {name}ClearedOuterChoiceRejected :\n  \
             ({{ {name} with after := {{ {name}.after with choices := \
             {name}.after.choices.map (fun entry => (entry.1, none)) }} }} : \
             Hibana.ActorReentryCommitObservation).check reentryActor{actor} reentryCommitChoreo = false := by\n  decide\n\n\
             theorem {name}WrongViewCountRejected :\n  \
             ({{ {name} with after := {{ {name}.after with liveViews := {name}.after.liveViews + 1 }} }} : \
             Hibana.ActorReentryCommitObservation).check reentryActor{actor} reentryCommitChoreo = false := by\n  decide\n\n\
             theorem {name}WrongScopeRejected :\n  \
             ({{ {name} with scope := {wrong_scope} }} : \
             Hibana.ActorReentryCommitObservation).check reentryActor{actor} reentryCommitChoreo = false := by\n  decide\n\n",
            entry.view,
            super::commit::observation_source(before),
            super::commit::observation_source(after),
        ));
        count += 1;
    }
    assert_eq!(count, 6);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("ActorReentryCommitGenerated.lean"), source).unwrap();
}
