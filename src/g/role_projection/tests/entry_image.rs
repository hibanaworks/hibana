//! Validate entry obligations against the source, not just their declared rows.
use super::*;

#[test]
fn missing_wrong_owner_and_unrelated_roll_entry_resources_are_rejected() {
    type Tick = g::Msg<39, ()>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 1, Tick>, g::Send<10, 3, Read>>>,
        g::Seq<g::Send<11, 4, Loan>, g::Seq<g::Send<4, 11, Loan>, g::Send<1, 2, OtherLoan>>>,
    >;
    const N: usize = RoleProjection::<1, Steps, 16>::IMAGE_REF
        .columns()
        .blob_len();
    const MUTANTS: [(crate::global::role_program::RoleImageColumns, [u8; N]); 4] = {
        let image = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
        let descriptor = RoleDescriptorRef::from_resident(image);
        let mut bytes = [0; N];
        let mut index = 0;
        while index < N {
            bytes[index] = descriptor.proof_byte_at(index);
            index += 1;
        }
        let columns = *image.columns();
        assert!(columns.roll_entry_view_count == 1);
        let entry = columns.roll_entry_views().offset as usize;
        assert!(bytes[entry] == 10 && bytes[entry + 1] == 1);
        let mut missing = columns;
        missing.roll_entry_view_count = 0;
        missing.owned_receipt_count = 0;
        let mut wrong_owner = bytes;
        wrong_owner[entry + 1] = 0;
        let mut unrelated = bytes;
        unrelated[entry] = 11;
        index = entry + 3;
        while index < entry + 7 {
            unrelated[index] = 0;
            index += 1;
        }
        let mut missing_roll = missing;
        missing_roll.roll_scopes.len = 0;
        [
            (missing, bytes),
            (columns, wrong_owner),
            (columns, unrelated),
            (missing_roll, bytes),
        ]
    };
    let image = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
    #[cfg(not(miri))]
    {
        use std::{format, string::String, vec::Vec};
        let mut generated = format!(
            "import Hibana.OwnedEntry\n\ndef generatedOwnedEntryImage : Hibana.RustDescriptorImage := {}\n\ntheorem generatedOwnedEntryAccepted : generatedOwnedEntryImage.ownedEntryResourcesCheck = true := by decide\n",
            crate::test_support::lean_descriptor::descriptor_source(
                RoleDescriptorRef::from_resident(image)
            )
        );
        for (index, (columns, bytes)) in MUTANTS.iter().enumerate() {
            let bytes = bytes
                .iter()
                .map(|byte| format!("{byte}"))
                .collect::<Vec<String>>()
                .join(", ");
            generated.push_str(&format!(
                "\ndef generatedOwnedEntryMutant{index} : Hibana.RustDescriptorImage :=\n  {{ generatedOwnedEntryImage with roleBytes := [{bytes}], rollScopeCount := {}, rollEntryViewCount := {}, ownedReceiptCount := {} }}\n\ntheorem generatedOwnedEntryMutant{index}Rejected : generatedOwnedEntryMutant{index}.ownedEntryResourcesCheck = false := by decide\n",
                columns.roll_scopes.len, columns.roll_entry_view_count, columns.owned_receipt_count
            ));
        }
        let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("OwnedEntryGenerated.lean"), generated).unwrap();
    }
    for (index, (columns, bytes)) in MUTANTS.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            RoleImageRef::new(image.program(), image.role(), image.facts(), columns, bytes)
        });
        assert!(
            result.is_err(),
            "corrupted entry resource {index} was published"
        );
    }
}
