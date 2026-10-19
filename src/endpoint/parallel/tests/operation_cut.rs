//! Serialization of passive cuts from the existing direct Tokio localsides.
use crate::{
    endpoint::proof_snapshot::{OperationCutSnapshot, operation_cut_source},
    global::compiled::images::RoleDescriptorRef,
    runtime::program::RoleProgram,
    test_support::lean_descriptor::descriptor_source,
};

pub(super) fn write_observations(program: &RoleProgram<0>, cuts: &[OperationCutSnapshot]) {
    let image = RoleDescriptorRef::from_resident(program.role_image_ref());
    let mut source = std::format!(
        "import Hibana.PublicOperationCut\n\nset_option maxRecDepth 10000\n\n\
         def parallelOperationImage : Hibana.RustDescriptorImage := {}\n\n",
        descriptor_source(image)
    );
    for (index, cut) in cuts.iter().enumerate() {
        source.push_str(&std::format!(
            "def parallelOperationCut{index} : Hibana.PublicOperationCut := {}\n\n\
             theorem parallel_operation_cut_{index}_matches :\n\
               parallelOperationCut{index}.matches parallelOperationImage = true := by decide\n\n",
            operation_cut_source(cut),
        ));
    }
    source.push_str(
        r#"
theorem primary_idle_does_not_mean_the_parent_is_clear :
    parallelOperationCut0.targetPhase? = some .idle ∧
      parallelOperationCut0.target.otherActive = true := by decide

theorem the_independent_send_can_start_while_its_sibling_receives :
    parallelOperationCut1.permitsFreshCall .beginSend = true := by
  exact Hibana.an_idle_independent_borrow_can_begin_its_send
    parallel_operation_cut_1_matches (by decide) (by decide)

theorem the_parent_cannot_ignore_its_pending_non_primary_slot :
    parallelOperationCut0.permitsFreshCall .beginSend = false := by
  exact Hibana.a_busy_borrow_cannot_begin_a_fresh_call
    parallel_operation_cut_0_matches (by decide) .beginSend

theorem a_non_primary_observation_retains_the_primary_idle_phase :
    parallelOperationCut2.targetPhase? = some .recv ∧
      parallelOperationCut2.slots[0]?.map (·.phase) = some .idle := by decide

theorem omitting_the_pending_slot_is_rejected :
    ({ parallelOperationCut1 with slots := parallelOperationCut1.slots.take 1 }).matches
      parallelOperationImage = false := by decide

theorem duplicating_a_slot_is_rejected :
    ({ parallelOperationCut1 with slots := parallelOperationCut1.slots ++
      parallelOperationCut1.slots.take 1 }).matches parallelOperationImage = false := by decide

theorem an_invented_borrow_mask_is_rejected :
    ({ parallelOperationCut1 with target := { parallelOperationCut1.target with
      ownedLanes := [] } }).matches parallelOperationImage = false := by decide

theorem claiming_the_whole_parent_is_clear_is_rejected :
    ({ parallelOperationCut0 with target := { parallelOperationCut0.target with
      otherActive := false } }).matches parallelOperationImage = false := by decide
"#,
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ParallelOperationCutGenerated.lean"), source).unwrap();
}
