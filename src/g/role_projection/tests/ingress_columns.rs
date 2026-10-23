//! Export receive metadata and the existing cursor after a committed loan.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{format, fs, string::ToString, vec::Vec};

pub(super) fn export() {
    type Steps = ingress::Steps;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let image = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
    let descriptor = descriptor_source(RoleDescriptorRef::from_resident(image));
    let choreo = Steps::lean_source();
    let mut ready = Vec::new();
    let mut completed = Vec::new();
    let mut receives = Vec::new();
    with_cursor(image, |cursor| {
        let loan = (0..image.local_step_count())
            .find(|&idx| {
                cursor
                    .try_recv_meta_at(idx)
                    .is_some_and(|meta| meta.label == 1)
            })
            .unwrap();
        let commit = cursor
            .event_enabled(loan, event(image, loan), &mut |_, _| None)
            .unwrap();
        cursor.advance_lane_to_relocatable_step(commit.progress_step());
        cursor.set_index(commit.cursor_after().as_usize());
        for idx in 0..image.local_step_count() {
            ready.push(
                cursor
                    .event_enabled(idx, event(image, idx), &mut |_, _| None)
                    .is_ok(),
            );
            let lane = cursor.event_lane_at(idx).unwrap();
            let step = cursor
                .relocatable_resident_lane_step_at_index(idx, usize::from(lane))
                .unwrap();
            completed.push(cursor.relocatable_step_done(step));
            if let Some(meta) = cursor.try_recv_meta_at(idx)
                && matches!(meta.label, 14 | 15)
            {
                receives.push((idx, cursor.logical_role_at(idx).unwrap(), meta));
            }
        }
    });
    assert_eq!(receives.len(), 2);
    let (worker, worker_view, left) = receives[0];
    let (resource, resource_view, right) = receives[1];
    assert_eq!((worker_view, resource_view), (1, 10));
    assert_eq!((left.peer, left.lane), (right.peer, right.lane));
    assert_ne!(left.frame_label, right.frame_label);
    for meta in [&left, &right] {
        assert_eq!(meta.frame_label, meta.eff_index.dense_ordinal() as u16);
    }
    assert!(ready[worker] && ready[resource]);
    assert!(completed.iter().filter(|&&done| done).count() == 1);
    let bools = |values: &[bool]| {
        values
            .iter()
            .map(bool::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut source = format!(
        "import Hibana\n\nset_option maxRecDepth 10000\n\n\
         def ingressChoreo : Hibana.Choreo := {choreo}\n\n\
         def ingressImage : Hibana.RustDescriptorImage := {descriptor}\n\n\
         def ingressRustReady : List Bool := [{}]\n\n\
         def ingressRustCompleted : List Bool := [{}]\n\n\
         def ingressAdmitted (event : Nat) : Bool := (ingressRustReady[event]?).getD false\n\n\
         def ingressSnapshot : Hibana.CommitState :=\n  \
         {{ Hibana.CommitState.initial with done := fun event =>\n    \
         (ingressRustCompleted[event]?).getD false }}\n\n\
         def ingressWorkerKey : Hibana.InboundOperationKey :=\n  \
         {{ source := {}, lane := {}, frameLabel := {} }}\n\n\
         def ingressResourceKey : Hibana.InboundOperationKey :=\n  \
         {{ source := {}, lane := {}, frameLabel := {} }}\n\n\
         theorem ingressSourceRowsMatch :\n  ingressImage.actorRowsCheck ingressChoreo = true := by\n  decide\n\n\
         theorem ingressSourceOrderMatches :\n  ingressImage.actorOrderCheck ingressChoreo = true := by\n  decide\n\n\
         theorem ingressSourceRoutesMatch :\n  ingressImage.actorRouteCheck ingressChoreo = true := by\n  decide\n\n\
         theorem ingressSourceControllerMatches :\n  ingressImage.actorControllerCheck ingressChoreo = true := by\n  decide\n\n\
         theorem ingressReadyAgreesWithSource :\n  \
         (match ingressImage.decodeActorOrder? ingressChoreo with\n  \
         | none => false\n  \
         | some data => (List.range ingressImage.eventCount).all fun event =>\n    \
         ingressAdmitted event == (!ingressSnapshot.done event &&\n      \
         data.normalized.paths.any fun path =>\n        \
         (data.activeRows path).any (fun row => row.eventId == event) &&\n        \
         (data.sourcePredecessors path event).all ingressSnapshot.done)) = true := by\n  decide\n\n\
         theorem ingressWorkerFrameSelectsWorker :\n  \
         (ingressImage.resolveActorInbound? {worker_view} ingressWorkerKey ingressAdmitted).map\n    \
         (·.eventId) = some {worker} := by\n  decide\n\n\
         theorem ingressResourceFrameSelectsResource :\n  \
         (ingressImage.resolveActorInbound? {resource_view} ingressResourceKey ingressAdmitted).map\n    \
         (·.eventId) = some {resource} := by\n  decide\n\n\
         theorem ingressBlockedFrameCannotSelect :\n  \
         ingressImage.resolveActorInbound? {worker_view} ingressWorkerKey (fun _ => false) = none := by\n  decide\n\n",
        bools(&ready),
        bools(&completed),
        left.peer,
        left.lane,
        left.frame_label,
        right.peer,
        right.lane,
        right.frame_label,
    );
    for (name, view, key) in [
        ("worker", worker_view, left),
        ("resource", resource_view, right),
    ] {
        for (field, source_role, lane, label) in [
            ("Source", key.peer ^ 128, key.lane, key.frame_label),
            ("Lane", key.peer, key.lane ^ 128, key.frame_label),
            ("FrameLabel", key.peer, key.lane, key.frame_label ^ 128),
        ] {
            source.push_str(&format!(
                "theorem {name}Wrong{field}Rejected :\n  \
                 ingressImage.resolveActorInbound? {view}\n    \
                 {{ source := {source_role}, lane := {lane}, frameLabel := {label} }} \
                 ingressAdmitted = none := by\n  decide\n\n",
            ));
        }
        source.push_str(&format!(
            "theorem {name}UnrelatedViewRejected :\n  \
             ingressImage.resolveActorInbound? 255 ingress{}Key ingressAdmitted = none := by\n  decide\n\n",
            if name == "worker" { "Worker" } else { "Resource" },
        ));
    }
    std::println!(
        "actor ingress metadata: worker=({worker_view},{},{},{}) resource=({resource_view},{},{},{}) ready={ready:?} completed={completed:?}",
        left.peer,
        left.lane,
        left.frame_label,
        right.peer,
        right.lane,
        right.frame_label,
    );
    fs::write(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/lean-proof/IngressGenerated.lean"),
        source,
    )
    .unwrap();
}
