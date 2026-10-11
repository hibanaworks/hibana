use super::*;
use crate::{
    rendezvous::{SessionFaultKind, error::RendezvousError},
    session::types::{Lane, SessionId},
};
use std::{format, fs, path::PathBuf, string::String, vec::Vec};

const PROOF_ENDPOINT_ALIGN: usize = 16;

fn lean_region(kind: &str, offset: usize, bytes: usize, align: usize) -> String {
    format!("    {{ kind := .{kind}, offset := {offset}, bytes := {bytes}, align := {align} }}")
}

fn allocator_snapshot_source(
    name: &str,
    rv: &Rendezvous<'_, '_, FailingTransport>,
    association_witnesses: &[(SessionId, Lane)],
) -> String {
    let table = rv.endpoint_lease_storage.get();
    let mut slots = Vec::new();
    let mut index = 0usize;
    while index < rv.endpoint_lease_slot_count() {
        let slot = crate::invariant_some(rv.endpoint_lease_slot_by_index(index));
        let state = match slot.state {
            crate::rendezvous::core::EndpointLeaseState::Vacant => 0,
            crate::rendezvous::core::EndpointLeaseState::Reserved => 1,
            crate::rendezvous::core::EndpointLeaseState::Published
            | crate::rendezvous::core::EndpointLeaseState::MembershipSealed => 2,
        };
        slots.push(format!(
            "    {{ generation := {}, session := {}, role := {}, offset := {}, bytes := {}, state := {state} }}",
            slot.generation,
            slot.sid.raw(),
            slot.role,
            slot.offset,
            slot.len,
        ));
        index += 1;
    }
    let association_witnesses = association_witnesses
        .iter()
        .map(|(sid, lane)| {
            format!(
                "    {{ session := {}, lane := {}, attached := {} }}",
                sid.raw(),
                lane.raw(),
                rv.has_lane_attachment(*sid, *lane),
            )
        })
        .collect::<Vec<_>>();
    format!(
        "def {name} : Hibana.LeaseAllocatorSnapshot := {{\n  generation := {},\n  \
         tableBytes := {},\n  assocBytes := {},\n  \
         resolverBytes := {},\n  \
         imageFrontier := {},\n  \
         workspaceBytes := {},\n  endpointFloor := {},\n  activeLaneAttachments := {},\n  associationWitnesses := [\n{}\n  ],\n  slots := [\n{}\n  ]\n}}\n",
        rv.endpoint_lease_generation.get(),
        table.bytes(),
        rv.assoc_storage.get().bytes(),
        rv.resolver_storage_sidecar().bytes(),
        rv.image_frontier.get(),
        rv.frontier_workspace_bytes.get(),
        rv.endpoint_storage_floor(),
        rv.active_lane_attachment_count(),
        association_witnesses.join(",\n"),
        slots.join(",\n"),
    )
}

fn allocation_failure_certificate_source() -> String {
    let initial = {
        let mut slab = [0u8; 4096];
        let slab_bytes = slab.len();
        let rv = init_test_rendezvous(&mut slab);
        let before = allocator_snapshot_source("generatedInitialFailureBefore", rv, &[]);
        assert_eq!(
            rv.allocate_endpoint_lease(
                SessionId::new(1),
                0,
                slab_bytes,
                1,
                crate::rendezvous::core::EndpointResidentBudget::ZERO,
            ),
            Err(ResourceScope::EndpointLease)
        );
        let after = allocator_snapshot_source("generatedInitialFailureAfter", rv, &[]);
        format!(
            "{before}\n{after}\ndef generatedInitialAllocationFailure : \
             Hibana.LeaseAllocationFailureCertificate := {{\n  before := \
             generatedInitialFailureBefore,\n  after := generatedInitialFailureAfter\n}}\n"
        )
    };
    let growth = {
        let mut slab = [0u8; 4096];
        let slab_bytes = slab.len();
        let rv = init_test_rendezvous(&mut slab);
        rv.allocate_endpoint_lease(
            SessionId::new(1),
            0,
            64,
            core::mem::align_of::<usize>(),
            crate::rendezvous::core::EndpointResidentBudget::ZERO,
        )
        .expect("existing endpoint lease");
        populate_non_endpoint_sidecars(rv);
        let before = allocator_snapshot_source("generatedGrowthFailureBefore", rv, &[]);
        assert_eq!(
            rv.allocate_endpoint_lease(
                SessionId::new(2),
                0,
                slab_bytes,
                1,
                crate::rendezvous::core::EndpointResidentBudget::ZERO,
            ),
            Err(ResourceScope::EndpointLease)
        );
        let after = allocator_snapshot_source("generatedGrowthFailureAfter", rv, &[]);
        format!(
            "{before}\n{after}\ndef generatedGrowthAllocationFailure : \
             Hibana.LeaseAllocationFailureCertificate := {{\n  before := \
             generatedGrowthFailureBefore,\n  after := generatedGrowthFailureAfter\n}}\n"
        )
    };
    let abort = {
        let mut slab = [0u8; 4096];
        let rv = init_test_rendezvous(&mut slab);
        let (first_slot, first_generation, _, _) = rv
            .allocate_endpoint_lease(
                SessionId::new(1),
                0,
                64,
                core::mem::align_of::<usize>(),
                crate::rendezvous::core::EndpointResidentBudget::ZERO,
            )
            .expect("existing endpoint lease");
        rv.publish_endpoint_lease(first_slot, first_generation);
        let before = allocator_snapshot_source("generatedAbortFailureBefore", rv, &[]);
        let (aborted_slot, aborted_generation, _, _) = rv
            .allocate_endpoint_lease(
                SessionId::new(2),
                0,
                64,
                core::mem::align_of::<usize>(),
                crate::rendezvous::core::EndpointResidentBudget::ZERO,
            )
            .expect("aborted endpoint lease");
        rv.abort_endpoint_lease_reservation(aborted_slot, aborted_generation);
        let after = allocator_snapshot_source("generatedAbortFailureAfter", rv, &[]);
        format!(
            "{before}\n{after}\ndef generatedAbortedAllocation : \
             Hibana.LeaseAllocationFailureCertificate := {{\n  before := \
             generatedAbortFailureBefore,\n  after := generatedAbortFailureAfter\n}}\n"
        )
    };
    let compacting_abort = {
        let mut slab = [0u8; 4096];
        let rv = init_test_rendezvous(&mut slab);
        let sid = SessionId::new(1);
        let lane = Lane::new(0);
        let (first_slot, first_generation, _, _) = rv
            .allocate_endpoint_lease(
                sid,
                0,
                64,
                core::mem::align_of::<usize>(),
                crate::rendezvous::core::EndpointResidentBudget {
                    frontier_workspace_bytes: 0,
                },
            )
            .expect("existing endpoint lease");
        rv.publish_endpoint_lease(first_slot, first_generation);
        populate_non_endpoint_sidecars(rv);
        rv.activate_lane_attachment(sid, lane)
            .expect("existing lane authority");
        let association_witnesses = [(sid, lane)];
        let before =
            allocator_snapshot_source("generatedCompactingAbortBefore", rv, &association_witnesses);
        let (aborted_slot, aborted_generation, _, _) = rv
            .allocate_endpoint_lease(
                SessionId::new(2),
                0,
                64,
                core::mem::align_of::<usize>(),
                crate::rendezvous::core::EndpointResidentBudget::ZERO,
            )
            .expect("aborted endpoint lease");
        rv.abort_endpoint_lease_reservation(aborted_slot, aborted_generation);
        let after =
            allocator_snapshot_source("generatedCompactingAbortAfter", rv, &association_witnesses);
        format!(
            "{before}\n{after}\ndef generatedCompactingAbort : \
             Hibana.LeaseAllocationAbortCertificate := {{\n  before := \
             generatedCompactingAbortBefore,\n  after := generatedCompactingAbortAfter\n}}\n"
        )
    };
    format!(
        "{initial}\n{growth}\n{abort}\n{compacting_abort}\n\
         theorem generatedInitialAllocationFailureAccepted :\n  \
         generatedInitialAllocationFailure.check = true := by decide\n\n\
         theorem generatedInitialAllocationFailurePreservesState :\n  \
         generatedInitialAllocationFailure.PreservesState :=\n  \
         Hibana.lease_allocation_failure_certificate_sound\n    \
         generatedInitialAllocationFailureAccepted\n\n\
         theorem generatedGrowthAllocationFailureAccepted :\n  \
         generatedGrowthAllocationFailure.check = true := by decide\n\n\
         theorem generatedGrowthAllocationFailurePreservesState :\n  \
         generatedGrowthAllocationFailure.PreservesState :=\n  \
         Hibana.lease_allocation_failure_certificate_sound\n    \
         generatedGrowthAllocationFailureAccepted\n\n\
         theorem generatedAbortedAllocationAccepted :\n  \
         generatedAbortedAllocation.check = true := by decide\n\n\
         theorem generatedAbortedAllocationPreservesState :\n  \
         generatedAbortedAllocation.PreservesState :=\n  \
         Hibana.lease_allocation_failure_certificate_sound\n    \
         generatedAbortedAllocationAccepted\n\n\
         theorem generatedCompactingAbortAccepted :\n  \
         generatedCompactingAbort.check = true := by decide\n\n\
         theorem generatedCompactingAbortPreservesAuthorityAndCapacity :\n  \
         generatedCompactingAbort.PreservesAuthorityAndCapacity :=\n  \
         Hibana.lease_allocation_abort_certificate_sound\n    \
         generatedCompactingAbortAccepted\n"
    )
}

fn layout_certificate_source(rv: &Rendezvous<'_, '_, FailingTransport>) -> (String, usize) {
    let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
    let mut regions = Vec::new();
    for resident in rv.live_sidecars() {
        if let Some((start, end)) = rv.sidecar_range(resident.storage) {
            regions.push(lean_region("resident", start, end - start, resident.align));
        }
    }
    let mut endpoint_regions = 0usize;
    let mut index = 0usize;
    while index < rv.endpoint_lease_slot_count() {
        let slot = crate::invariant_some(rv.endpoint_lease_slot_by_index(index));
        if slot.is_occupied() {
            endpoint_regions += 1;
            regions.push(lean_region(
                "endpoint",
                slot.offset as usize,
                slot.len as usize,
                PROOF_ENDPOINT_ALIGN,
            ));
        }
        index += 1;
    }
    assert_eq!(
        endpoint_regions, 1,
        "runtime layout proof fixture must own exactly one endpoint region"
    );
    let region_count = regions.len();
    let source = format!(
        "def generatedSlabLayout : Hibana.SlabLayoutCertificate := {{\n  \
         base := {},\n  capacity := {slab_len},\n  imageFrontier := {},\n  \
         workspaceBytes := {},\n  endpointFloor := {},\n  regions := [\n{}\n  ]\n}}\n\n\
         theorem generatedSlabLayoutAccepted : generatedSlabLayout.check = true := by\n  \
         decide\n\n\
         theorem generatedSlabLayoutWellFormed : generatedSlabLayout.WellFormed :=\n  \
         Hibana.slab_layout_certificate_sound generatedSlabLayoutAccepted\n",
        slab_ptr.addr(),
        rv.image_frontier.get(),
        rv.frontier_workspace_bytes.get(),
        rv.endpoint_storage_floor(),
        regions.join(",\n"),
    );
    (source, region_count)
}

fn assert_production_poison_generation(rv: &Rendezvous<'_, '_, FailingTransport>) {
    let sid = SessionId::new(77);
    let lane0 = Lane::new(0);
    let lane1 = Lane::new(1);
    let lane2 = Lane::new(2);
    rv.ensure_core_lane_tables_for_assoc_entries(3, 3)
        .expect("poison proof association storage");
    rv.activate_lane_attachment(sid, lane0)
        .expect("poison proof lane 0");
    rv.activate_lane_attachment(sid, lane1)
        .expect("poison proof lane 1");

    assert_eq!(
        rv.poison_session(sid, 0, SessionFaultKind::TransportClosed),
        SessionFaultKind::TransportClosed
    );
    assert_eq!(
        rv.poison_session(sid, 1, SessionFaultKind::DecodeFailed),
        SessionFaultKind::TransportClosed,
        "the first fault must remain generation authority"
    );
    assert_eq!(
        rv.activate_lane_attachment(sid, lane2),
        Err(RendezvousError::SessionPoisoned { sid }),
        "a poisoned generation must reject new lane attachment"
    );
    assert_eq!(
        rv.release_lane(sid, lane1),
        crate::rendezvous::core::LaneRelease::Released
    );
    assert_eq!(
        rv.session_fault(sid),
        Some(SessionFaultKind::TransportClosed),
        "poison must survive while any lane in the generation remains"
    );
    assert_eq!(
        rv.release_lane(sid, lane0),
        crate::rendezvous::core::LaneRelease::Released
    );
    assert_eq!(rv.session_fault(sid), None);
}

const GENERATION_SOURCE: &str = r#"
def generatedPoisonTail : List Hibana.SessionGenerationAction := [
  .poison .decodeFailed,
  .releaseRemaining,
  .releaseLast
]

theorem generatedPoisonTailAccepted :
    Hibana.checkSessionGenerationTrace (.poisoned .transportClosed)
      generatedPoisonTail = true := by
  decide

theorem generatedPoisonTailRetires :
    Hibana.runSessionGenerationTrace (.poisoned .transportClosed)
      generatedPoisonTail = some .retired := by
  decide

theorem generatedPoisonTailTrace :
    Hibana.SessionGenerationTrace (.poisoned .transportClosed)
      generatedPoisonTail .retired :=
  Hibana.session_generation_run_sound generatedPoisonTailRetires

theorem generatedPoisonedAttachRejected :
    Hibana.applySessionGenerationAction (.poisoned .transportClosed) .attach = none :=
  Hibana.poisoned_generation_attach_rejected .transportClosed

theorem generatedNextLeaseGenerationMaximumAccepted :
    Hibana.nextLeaseGeneration? 4294967294 = some 4294967295 := by
  decide

theorem generatedMaximumLeaseGenerationExhausted :
    Hibana.nextLeaseGeneration? 4294967295 = none :=
  Hibana.max_lease_generation_is_exhausted
"#;

#[test]
#[ignore = "host-only Lean runtime certificate export"]
fn export_runtime_certificates_for_lean() {
    let mut slab = [0u8; 8192];
    let rv = init_test_rendezvous(&mut slab);
    let resident_budget = crate::rendezvous::core::EndpointResidentBudget {
        frontier_workspace_bytes: 32,
    };
    let (_lease, _generation, endpoint_offset, endpoint_bytes) = rv
        .allocate_endpoint_lease(
            SessionId::new(1),
            0,
            128,
            PROOF_ENDPOINT_ALIGN,
            resident_budget,
        )
        .expect("proof endpoint lease");
    let (runtime_slab, _) = rv.slab_ptr_and_len();
    assert_eq!(
        (runtime_slab.addr() + endpoint_offset) % PROOF_ENDPOINT_ALIGN,
        0
    );
    assert_eq!(endpoint_bytes, 128);
    rv.ensure_endpoint_resident_capacity()
        .expect("proof workspace storage");
    rv.ensure_core_lane_tables_for_assoc_entries(3, 3)
        .expect("proof association storage");
    rv.ensure_dynamic_resolver_capacity(2)
        .expect("proof resolver storage");

    let (layout, region_count) = layout_certificate_source(rv);
    assert_production_poison_generation(rv);
    let allocation_failures = allocation_failure_certificate_source();

    rv.endpoint_lease_generation.set(u32::MAX - 1);
    assert_eq!(rv.next_endpoint_lease_generation(), Some(u32::MAX));
    assert_eq!(
        rv.next_endpoint_lease_generation(),
        None,
        "production lease generation must fail closed at exhaustion"
    );

    let generated = format!(
        "import Hibana.MainTheorems\n\n{layout}\n{GENERATION_SOURCE}\n{allocation_failures}\n\
         #eval IO.println \"hibana Lean runtime proof passed regions={region_count} poison=1 generation=1 atomic-failures=4\"\n"
    );
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&output_dir).expect("create generated Lean proof artifact directory");
    fs::write(output_dir.join("RuntimeGenerated.lean"), generated)
        .expect("write generated Lean runtime certificate");
}
