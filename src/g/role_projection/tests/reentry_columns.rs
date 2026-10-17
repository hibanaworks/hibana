//! Bind source roll ranges and actual done-bit preparation to actor images.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{format, fs, string::String, string::ToString, vec::Vec};

fn certificate<const ROLE: u8, Steps: g::ProgramShape + LeanChoreo, const CAPACITY: usize>(
    name: &str,
) -> String {
    let () = g::ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    let image = &RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF;
    let rows = (0..usize::from(image.columns().roll_scopes.len))
        .map(|slot| {
            let row = image.roll_scope_row(slot).unwrap();
            let range = row.event_row();
            format!(
                "({}, {}, {})",
                row.scope().raw() % 8192,
                range.start(),
                range.end() - range.start(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let descriptor = descriptor_source(RoleDescriptorRef::from_resident(image));
    let choreo = Steps::lean_source();
    let mut choice_resets = Vec::new();
    with_cursor(image, |cursor| {
        for slot in 0..usize::from(image.columns().roll_scopes.len) {
            let roll = image.roll_scope_row(slot).unwrap().scope();
            let choices = (0..usize::from(image.columns().route_scopes.len))
                .map(|route_slot| {
                    let route = image.route_scope_by_slot(route_slot).unwrap();
                    format!(
                        "some {}",
                        cursor.route_scope_contained_in_roll_scope(route, roll)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            choice_resets.push(format!("[{choices}]"));
        }
    });
    let choice_resets = choice_resets.join(", ");
    format!(
        "def {name}Choreo : Hibana.Choreo := {choreo}\n\n\
         def {name}Image : Hibana.RustDescriptorImage := {descriptor}\n\n\
         theorem {name}SourceRanges :\n  \
         {name}Image.actorRollCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}SourceOrder :\n  \
         {name}Image.actorOrderCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}RustRanges :\n  \
         ({name}Image.decodeRollRows?).map\n    \
         (fun rows => rows.map fun row => (row.scope, row.eventStart, row.eventLength)) =\n    \
         some [{rows}] := by\n  decide\n\n\
         theorem {name}RustChoiceReset :\n  \
         ({name}Image.decodeRollRows?).map (fun rows => rows.map fun roll =>\n    \
         (List.range {name}Image.routeScopeCount).map\n      \
         ({name}Image.rollResetsRoute? roll)) = some [{choice_resets}] := by\n  decide\n\n",
    )
}

pub(super) fn export() {
    type Parallel =
        g::Seq<g::Par<g::Roll<DbLoan>, g::Roll<AuthLoan>>, g::Send<1, 0, reentry::Done>>;
    type Nested = g::Roll<g::Roll<DbLoan>>;
    type ParallelTail =
        g::Par<g::Seq<g::Roll<DbLoan>, g::Send<1, 0, g::Msg<22, ()>>>, g::Roll<AuthLoan>>;
    let mut source = String::from("import Hibana\n\nset_option maxRecDepth 10000\n\n");
    source.push_str(&certificate::<0, reentry::Steps, 16>("rollOwner"));
    source.push_str(&certificate::<1, reentry::Steps, 16>("rollWorker"));
    source.push_str(&certificate::<0, Parallel, 32>("parallelRollOwner"));
    source.push_str(&certificate::<1, Parallel, 32>("parallelRollWorker"));
    source.push_str(&certificate::<0, ParallelTail, 32>("parallelTailOwner"));
    source.push_str(&certificate::<1, ParallelTail, 32>("parallelTailWorker"));
    source.push_str(&certificate::<0, Nested, 16>("nestedRollOwner"));
    source.push_str(&certificate::<1, Nested, 16>("nestedRollWorker"));
    source.push_str(&certificate::<3, Nested, 16>("nestedRollService"));
    source.push_str(&certificate::<0, composed::Global, 32>("nativeRollOwner"));
    source.push_str(&certificate::<1, composed::Global, 32>("nativeRollWorker"));
    source.push_str(&certificate::<2, composed::Global, 32>("nativeRollService"));
    source.push_str(&certificate::<3, composed::Global, 32>("nativeRollSide"));
    source.push_str(&certificate::<4, composed::Global, 32>(
        "nativeRollSidePeer",
    ));
    source.push_str(&certificate::<0, conditional_controller::Steps, 32>(
        "conditionalRollOwner",
    ));
    source.push_str(&certificate::<1, conditional_controller::Steps, 32>(
        "conditionalRollWorker",
    ));
    source.push_str(&certificate::<3, conditional_controller::Steps, 32>(
        "conditionalRollService",
    ));
    source.push_str(&certificate::<0, reentry_choice::Steps, 32>(
        "enclosedRollOwner",
    ));
    source.push_str(&certificate::<1, reentry_choice::Steps, 32>(
        "enclosedRollWorker",
    ));
    source.push_str(&certificate::<3, reentry_choice::Steps, 32>(
        "enclosedRollService",
    ));
    for (name, offset, value) in [
        ("rollTooShort", 2, 2),
        ("rollIncludesContinuation", 2, 6),
        ("rollSkipsLoan", 4, 1),
        ("rollWrongScope", 0, 7),
    ] {
        source.push_str(&format!(
            "def {name}Image : Hibana.RustDescriptorImage :=\n  \
             {{ rollWorkerImage with roleBytes := (rollWorkerImage.roleBytes.set\n    \
             (rollWorkerImage.roleRollScopeOffset + {offset}) {value}) }}\n\n\
             theorem {name}PriorRowsAndOrder :\n  \
             ({name}Image.actorRowsCheck rollWorkerChoreo &&\n    \
             {name}Image.actorOrderCheck rollWorkerChoreo) = true := by\n  decide\n\n\
             theorem {name}StructurallyCoherent :\n  \
             {name}Image.rollColumnsCoherent = true := by\n  decide\n\n\
             theorem {name}SourceRejected :\n  \
             {name}Image.actorRollCheck rollWorkerChoreo = false := by\n  decide\n\n",
        ));
    }

    let image = &RoleProjection::<1, reentry::Steps, 16>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 6);
    let range = image.roll_scope_row(0).unwrap().event_row();
    assert_eq!((range.start(), range.end()), (0, 4));
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut ready = Vec::new();
    with_cursor(image, |cursor| {
        for step in 0..image.local_step_count() {
            let commit = cursor
                .event_enabled(step, event(image, step), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
            cursor.set_index(commit.cursor_after().as_usize());
        }
        for step in 0..image.local_step_count() {
            let lane = cursor.event_lane_at(step).unwrap();
            let progress = cursor
                .relocatable_resident_lane_step_at_index(step, usize::from(lane))
                .unwrap();
            before.push(cursor.relocatable_step_done(progress));
        }
        let head = cursor
            .relocatable_resident_lane_step_at_index(
                0,
                usize::from(cursor.event_lane_at(0).unwrap()),
            )
            .unwrap();
        let scope = cursor
            .roll_body_reentry_scope_for_step(head, &mut |_| None)
            .unwrap();
        assert_eq!(scope, image.roll_scope_row(0).unwrap().scope());
        cursor.clear_reentry_scope_events(scope);
        for step in 0..image.local_step_count() {
            let lane = cursor.event_lane_at(step).unwrap();
            let progress = cursor
                .relocatable_resident_lane_step_at_index(step, usize::from(lane))
                .unwrap();
            after.push(cursor.relocatable_step_done(progress));
            ready.push(
                cursor
                    .event_enabled(step, event(image, step), &mut |_, _| None)
                    .is_ok(),
            );
        }
    });
    assert!(before.iter().all(|done| *done));
    assert_eq!(after, [false, false, false, false, true, true]);
    assert_eq!(ready, [true, false, false, false, false, false]);
    let bools = |values: &[bool]| {
        values
            .iter()
            .map(bool::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    source.push_str(&format!(
        "def rollBefore : Hibana.CommitState := {{\n  \
         Hibana.CommitState.initial with done := fun event => ([{}][event]?).getD false }}\n\n\
         def rollAfter : Hibana.CommitState := {{\n  \
         Hibana.CommitState.initial with done := fun event => ([{}][event]?).getD false }}\n\n\
         def rollRustReady : List Bool := [{}]\n\n\
         def preparedRoll : Hibana.RollInfo := {{\n  \
         events := Hibana.eventRange 0 4, entries := [0], conflicts := [] }}\n\n\
         theorem rollDoneResetAgreesWithRust :\n  \
         (List.range 6).all (fun event =>\n    \
         (Hibana.resetRoll rollBefore preparedRoll).done event == rollAfter.done event) = true := by\n  decide\n\n\
         theorem rollReadyAgreesWithSource :\n  \
         (match rollWorkerImage.decodeActorOrder? rollWorkerChoreo with\n  \
         | none => false\n  \
         | some data => (List.range 6).all fun event =>\n    \
         (rollRustReady[event]?).getD false == (!rollAfter.done event &&\n      \
         data.normalized.paths.any fun path =>\n        \
         (data.activeRows path).any (fun row => row.eventId == event) &&\n        \
         (data.sourcePredecessors path event).all rollAfter.done)) = true := by\n  decide\n\n",
        bools(&before), bools(&after), bools(&ready),
    ));
    let parallel = &RoleProjection::<1, ParallelTail, 32>::IMAGE_REF;
    assert_eq!(parallel.local_step_count(), 9);
    let mut parallel_done = Vec::new();
    let mut parallel_ready = Vec::new();
    with_cursor(parallel, |cursor| {
        for step in 0..parallel.local_step_count() {
            let commit = cursor
                .event_enabled(step, event(parallel, step), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
            cursor.set_index(commit.cursor_after().as_usize());
        }
        for (slot, first) in [(0, 0), (1, 5)] {
            let head = cursor
                .relocatable_resident_lane_step_at_index(
                    first,
                    usize::from(cursor.event_lane_at(first).unwrap()),
                )
                .unwrap();
            let scope = cursor
                .roll_body_reentry_scope_for_step(head, &mut |_| None)
                .unwrap();
            assert_eq!(scope, parallel.roll_scope_row(slot).unwrap().scope());
            cursor.clear_reentry_scope_events(scope);
        }
        for step in 0..parallel.local_step_count() {
            let lane = cursor.event_lane_at(step).unwrap();
            let progress = cursor
                .relocatable_resident_lane_step_at_index(step, usize::from(lane))
                .unwrap();
            parallel_done.push(cursor.relocatable_step_done(progress));
            parallel_ready.push(
                cursor
                    .event_enabled(step, event(parallel, step), &mut |_, _| None)
                    .is_ok(),
            );
        }
    });
    assert_eq!(
        parallel_done,
        [false, false, false, false, true, false, false, false, false]
    );
    assert!(parallel_ready[0] && parallel_ready[5]);
    std::println!("parallel roll actual cursor: after={parallel_done:?} ready={parallel_ready:?}");
    source.push_str(&format!(
        "def parallelTailAfter : Hibana.CommitState := {{\n  \
         Hibana.CommitState.initial with done := fun event => ([{}][event]?).getD false }}\n\n\
         def parallelTailRustReady : List Bool := [{}]\n\n\
         theorem parallelTailOldPrefixIsNotClosed :\n  \
         (match parallelTailWorkerImage.decodeActorOrder? parallelTailWorkerChoreo with\n  \
         | none => true\n  \
         | some data => data.normalized.paths.all fun path =>\n    \
         (List.range 5).all fun event => !parallelTailAfter.done event ||\n      \
         (data.sourcePredecessors path event).all parallelTailAfter.done) = false := by\n  decide\n\n\
         theorem parallelTailCandidateCutIsClosed :\n  \
         (match parallelTailWorkerImage.decodeActorOrder? parallelTailWorkerChoreo with\n  \
         | none => false\n  \
         | some data => data.normalized.paths.all fun path =>\n    \
         (data.sourcePredecessors path 5).all fun event => !parallelTailAfter.done event ||\n      \
         (data.sourcePredecessors path event).all parallelTailAfter.done) = true := by\n  decide\n\n\
         theorem parallelTailReadyAgreesWithSource :\n  \
         (match parallelTailWorkerImage.decodeActorOrder? parallelTailWorkerChoreo with\n  \
         | none => false\n  \
         | some data => (List.range 9).all fun event =>\n    \
         (parallelTailRustReady[event]?).getD false == (!parallelTailAfter.done event &&\n      \
         data.normalized.paths.any fun path =>\n        \
         (data.activeRows path).any (fun row => row.eventId == event) &&\n        \
         (data.sourcePredecessors path event).all parallelTailAfter.done)) = true := by\n  decide\n\n",
        bools(&parallel_done), bools(&parallel_ready),
    ));
    std::println!(
        "roll preparation actual cursor: before={before:?} after={after:?} ready={ready:?}"
    );
    fs::write(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/lean-proof/ReentryGenerated.lean"),
        source,
    )
    .unwrap();
}
