//! Serialize measured call boundaries after execution; never drive localside.
use super::{Global, SID};
use crate::{
    endpoint::proof_snapshot::{ActorSnapshot, OperationCutSnapshot, operation_cut_source},
    global::{compiled::images::RoleDescriptorRef, typestate::LocalAction},
    runtime::transport::FrameHeader,
    test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source},
};
use std::{fmt::Write, vec::Vec};

pub(super) fn write_observations(
    snapshots: &[ActorSnapshot; 4],
    journal: &[(u8, Vec<u8>)],
    operations: &[OperationCutSnapshot; 2],
) {
    assert_eq!(snapshots[0].completion_mutations, 0);
    assert_eq!(snapshots[1].completion_mutations, 0);
    assert_eq!(snapshots[2].completion_mutations, 1);
    assert_eq!(snapshots[3].completion_mutations, 2);
    assert_eq!(snapshots[0].actor, 0);
    assert!(snapshots.iter().all(|cut| cut.actor == 0));

    let program = crate::runtime::program::project::<0>(&crate::g::Program::<Global>::new());
    let image = RoleDescriptorRef::from_resident(program.role_image_ref());
    let mut source = std::format!(
        "import Hibana.SourcePublicationPrefix\n\nset_option maxRecDepth 10000\n\
         set_option maxHeartbeats 0\n\nopen Hibana Hibana.EndpointDelegation\n\n\
         def asyncChoreo : Choreo := {}\n\n\
         def asyncImage : RustDescriptorImage := {}\n\n",
        Global::lean_source(),
        descriptor_source(image),
    );
    for (index, snapshot) in snapshots.iter().enumerate() {
        writeln!(
            source,
            "def asyncCut{index} : ActorProgressObservation := {}\n",
            super::super::super::super::commit::observation_source(snapshot)
        )
        .unwrap();
    }

    let sends: Vec<_> = journal
        .iter()
        .enumerate()
        .filter_map(|(witness, (operation, packet))| {
            let header = FrameHeader::from_bytes(packet[..8].try_into().unwrap());
            (*operation == 1 && header.source_role() == 0).then_some((witness, header, packet))
        })
        .collect();
    assert_eq!(
        sends.len(),
        2,
        "both physical acceptances precede loan publication"
    );
    assert_eq!(sends[0].1.target_role(), 1);
    assert_eq!(sends[1].1.target_role(), 2);
    for (index, (witness, header, packet)) in sends.iter().enumerate() {
        let label = if index == 0 { 1 } else { 93 };
        let event = snapshots[0]
            .steps
            .iter()
            .position(|step| {
                step.view == 0
                    && matches!(step.action, LocalAction::Send { label: actual, .. } if actual == label)
            })
            .unwrap();
        let LocalAction::Send { eff_index, .. } = snapshots[0].steps[event].action else {
            unreachable!()
        };
        let (base, before, after) = if index == 0 { (0, 2, 3) } else { (1, 1, 2) };
        writeln!(source, "def asyncWire{index} : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n\
            def asyncEntry{index} : SourceAcceptance := {{ sample := {{ image := asyncImage, base := asyncCut{base}, pathIndex := 0, event := {event}, wire := asyncWire{index} }}, channel := {{ session := {}, generation := 0, lane := {}, sender := {}, receiver := {} }}, event := {}, mutation := {}, carrierWitness := {witness} }}\n\n\
            def asyncCommit{index} : ActorCommitRecord := {{ step := {{ pathIndex := 0, event := {event}, view := 0, before := asyncCut{before}, after := asyncCut{after} }}, wire := asyncWire{index} }}\n",
            SID.raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(), &packet[8..],
            SID.raw(), header.lane(), header.source_role(), header.target_role(), eff_index.dense_ordinal(), snapshots[base].completion_mutations).unwrap();
    }
    source.push_str(include_str!("publication.lean"));
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("AsyncSourcePublicationGenerated.lean"),
        source,
    )
    .unwrap();
    let mut source = std::format!(
        "import Hibana.PublicOperationCut\n\nset_option maxRecDepth 10000\n\n\
         def asyncOperationImage : Hibana.RustDescriptorImage := {}\n\n",
        descriptor_source(image)
    );
    for (index, cut) in operations.iter().enumerate() {
        writeln!(
            source,
            "def asyncOperationCut{index} : Hibana.PublicOperationCut := {}\n\n\
            theorem async_operation_cut_{index}_matches :\n\
            asyncOperationCut{index}.matches asyncOperationImage = true := by decide\n",
            operation_cut_source(cut)
        )
        .unwrap();
    }
    source.push_str(include_str!("operation_cut.lean"));
    std::fs::write(directory.join("AsyncOperationCutGenerated.lean"), source).unwrap();
}
