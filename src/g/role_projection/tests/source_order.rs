//! Byte-level counterexamples for the source/local dependency boundary.
use super::*;
use crate::global::role_program::{ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE};

#[test]
fn source_order_rejects_a_missing_join_and_spurious_parallel_order() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Par<g::Send<1, 2, Read>, g::Send<10, 3, Read>>,
            g::Seq<g::Send<1, 2, Value>, g::Send<1, 0, Loan>>,
        >,
    >;
    const N: usize = RoleProjection::<1, Steps, 16>::IMAGE_REF
        .columns()
        .blob_len();
    const BYTES: [[u8; N]; 2] = {
        let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
        let descriptor = RoleDescriptorRef::from_resident(original);
        let columns = original.columns();
        let mut bytes = [0; N];
        let mut offset = 0;
        while offset < N {
            bytes[offset] = descriptor.proof_byte_at(offset);
            offset += 1;
        }
        let mut worker_read = None;
        let mut db_read = None;
        let mut after_join = None;
        let mut row = 0;
        while row < original.local_step_count() {
            match original.local_step_node(row).unwrap().action() {
                LocalAction::Send {
                    label: 3, peer: 2, ..
                } => worker_read = Some(row),
                LocalAction::Send {
                    label: 3, peer: 3, ..
                } => db_read = Some(row),
                LocalAction::Send {
                    label: 4, peer: 2, ..
                } => after_join = Some(row),
                LocalAction::Send { .. }
                | LocalAction::Recv { .. }
                | LocalAction::Local { .. }
                | LocalAction::Terminate => (),
            }
            row += 1;
        }
        let worker_read = worker_read.unwrap();
        let db_read = db_read.unwrap();
        let after_join = after_join.unwrap();
        assert!(worker_read < db_read && db_read < after_join);
        let join_reference =
            columns.events.offset as usize + after_join * ROLE_IMAGE_EVENT_STRIDE + 2;
        let mut missing_join = bytes;
        missing_join[join_reference] = 255;
        missing_join[join_reference + 1] = 255;
        let db_reference = columns.events.offset as usize + db_read * ROLE_IMAGE_EVENT_STRIDE + 2;
        let dependency = u16::from_le_bytes([bytes[db_reference], bytes[db_reference + 1]]);
        assert!(dependency != u16::MAX);
        let end = columns.dependencies.offset as usize
            + dependency as usize * ROLE_IMAGE_DEPENDENCY_STRIDE
            + 2;
        let mut extra_order = bytes;
        let encoded = ((worker_read + 1) as u16).to_le_bytes();
        extra_order[end] = encoded[0];
        extra_order[end + 1] = encoded[1];
        [missing_join, extra_order]
    };
    const IMAGES: [RoleImageRef; 2] = {
        let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
        [
            RoleImageRef::new(
                original.program(),
                original.role(),
                original.facts(),
                original.columns(),
                &BYTES[0],
            ),
            RoleImageRef::new(
                original.program(),
                original.role(),
                original.facts(),
                original.columns(),
                &BYTES[1],
            ),
        ]
    };
    let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
    assert!(crate::g::role_projection::source_order::check::<Steps, 16>(
        original
    ));
    // Structural image validity alone cannot prove source dependency coverage.
    // The typed projection additionally runs source_order before publication.
    let missing_join = &IMAGES[0];
    let extra_order = &IMAGES[1];
    assert!(!crate::g::role_projection::source_order::check::<Steps, 16>(missing_join));

    assert!(!crate::g::role_projection::source_order::check::<Steps, 16>(extra_order));
    #[cfg(not(miri))]
    {
        use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
        let descriptor = RoleDescriptorRef::from_resident(original);
        let source = std::format!(
            "import Hibana\n\nset_option maxRecDepth 10000\n\n\
             def sourceOrderChoreo : Hibana.Choreo := {}\n\n\
             def sourceOrderValid : Hibana.RustDescriptorImage := {}\n\n\
             def sourceOrderMissingJoin : Hibana.RustDescriptorImage := {}\n\n\
             def sourceOrderExtraOrder : Hibana.RustDescriptorImage := {}\n\n\
             theorem sourceOrderValidAccepted :\n  \
             sourceOrderValid.actorOrderCheck sourceOrderChoreo = true := by decide\n\n\
             theorem sourceOrderMissingJoinRejected :\n  \
             sourceOrderMissingJoin.actorRowsCheck sourceOrderChoreo = true ∧\n  \
             sourceOrderMissingJoin.actorOrderCheck sourceOrderChoreo = false := by decide\n\n\
             theorem sourceOrderExtraOrderRejected :\n  \
             sourceOrderExtraOrder.actorRowsCheck sourceOrderChoreo = true ∧\n  \
             sourceOrderExtraOrder.actorOrderCheck sourceOrderChoreo = false := by decide\n",
            Steps::lean_source(),
            descriptor_source(descriptor),
            descriptor_source(RoleDescriptorRef::from_resident(missing_join)),
            descriptor_source(RoleDescriptorRef::from_resident(extra_order)),
        );
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("SourceOrderGenerated.lean"), source).unwrap();
    }
}
