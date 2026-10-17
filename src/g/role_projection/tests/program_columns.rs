//! Export actual ordinary and owned column headers for independent decoding.
//! This verifies the byte grammar, not actor progress or public qualification.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{format, fs, path::PathBuf, string::String, vec::Vec};

fn bytes(values: impl IntoIterator<Item = u8>) -> String {
    let entries = values
        .into_iter()
        .map(|value| format!("{value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

fn certificate<Steps: g::ProgramShape + LeanChoreo, const CAPACITY: usize>(name: &str) -> String {
    let () = g::ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    let program = g::ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF;
    let columns = program.columns;
    let header = bytes(columns.proof_header_bytes());
    let blob = bytes((0..program.proof_blob_len()).map(|offset| program.proof_byte_at(offset)));
    let bindings = (0..columns.atom_count())
        .filter_map(|event| program.actor_binding_at(event))
        .map(|[sender, receiver]| format!("({sender}, {receiver})"))
        .collect::<Vec<_>>();
    let decisions = (0..program.actor_decision_count())
        .map(|row| {
            let (scope, [left, right]) = program.actor_decision_at(row).unwrap();
            format!(
                "{{ scope := {}, left := {left}, right := {right} }}",
                scope.raw()
            )
        })
        .collect::<Vec<_>>();
    let initial = (0..=u8::MAX)
        .filter_map(|role| program.initial_actor_for(role).map(|actor| (role, actor)))
        .map(|(role, actor)| format!("({role}, {actor})"))
        .collect::<Vec<_>>();
    let prefix = columns.route_participants().end_offset(1);
    let scope_offset = prefix + if bindings.is_empty() { 0 } else { 4 };
    let choreo = Steps::lean_source();
    let image = descriptor_source(RoleDescriptorRef::from_resident(
        &RoleProjection::<0, Steps, CAPACITY>::IMAGE_REF,
    ));
    let mut source = format!(
        "def {name}Header : List Nat := {header}\n\n\
         def {name}Bytes : List Nat := {blob}\n\n\
         def {name}Columns : Hibana.DecodedProgramColumns := {{\n  \
         atomCount := {}\n  routeResolverCount := {}\n  routeParticipantCount := {}\n  \
         scopeMarkerCount := {}\n  scopeMarkerOffset := {scope_offset}\n  \
         actorBindings := [{}]\n  actorDecisions := [{}]\n  initialActors := [{}]\n}}\n\n\
         theorem {name}Decoded :\n  \
         Hibana.decodeProgramColumns? {name}Header {name}Bytes = some {name}Columns := by\n  decide\n\n\
         theorem {name}TruncatedHeaderRejected :\n  \
         Hibana.decodeProgramColumns? ({name}Header.take 7) {name}Bytes = none := by\n  decide\n\n",
        columns.atom_count(),
        columns.route_resolver_count(),
        columns.route_participant_count(),
        program.source_scope_marker_count(),
        bindings.join(", "),
        decisions.join(", "),
        initial.join(", "),
    );
    source.push_str(&format!(
        "def {name}Choreo : Hibana.Choreo := {choreo}\n\n\
         def {name}Image : Hibana.RustDescriptorImage := {image}\n\n\
         theorem {name}SourceOwnershipMatches :\n  \
         {name}Image.ownershipCheck {name}Choreo = true := by\n  decide\n\n",
    ));
    if !bindings.is_empty() {
        let wrong_length = columns.proof_header_bytes()[6] ^ 1;
        let untagged = columns.proof_header_bytes()[1] & 0x7f;
        let shortened = program.proof_blob_len() - 1;
        source.push_str(&format!(
            "theorem {name}WrongLengthRejected :\n  \
             Hibana.decodeProgramColumns? ({name}Header.set 6 {wrong_length}) {name}Bytes = none := by\n  decide\n\n\
             theorem {name}ClearedTagRejected :\n  \
             Hibana.decodeProgramColumns? ({name}Header.set 1 {untagged}) {name}Bytes = none := by\n  decide\n\n\
             theorem {name}OversizedDecisionsRejected :\n  \
             Hibana.decodeProgramColumns? {name}Header \
             (({name}Bytes.set {} 255).set {} 255) = none := by\n  decide\n\n\
             theorem {name}OddIssuerTailRejected :\n  \
             Hibana.decodeProgramColumns? \
             (({name}Header.set 6 {}).set 7 {}) \
             ({name}Bytes.take {shortened}) = none := by\n  decide\n\n",
            prefix + 2,
            prefix + 3,
            shortened & 255,
            shortened >> 8,
        ));
        let binding_offset = scope_offset + program.source_scope_marker_count() * 5;
        let wrong_sender = program.proof_byte_at(binding_offset) ^ 1;
        let issuer_actor_offset = program.proof_blob_len() - 1;
        let wrong_issuer = program.proof_byte_at(issuer_actor_offset) ^ 1;
        source.push_str(&format!(
            "theorem {name}UnqualifiedTopologyRejected :\n  \
             {name}Image.check = false := by\n  decide\n\n\
             theorem {name}WrongActorBindingRejected :\n  \
             ({{ {name}Image with programBytes := {name}Bytes.set {binding_offset} {wrong_sender} }} : \
             Hibana.RustDescriptorImage).ownershipCheck {name}Choreo = false := by\n  decide\n\n\
             theorem {name}WrongInitialIssuerRejected :\n  \
             ({{ {name}Image with programBytes := {name}Bytes.set {issuer_actor_offset} {wrong_issuer} }} : \
             Hibana.RustDescriptorImage).ownershipCheck {name}Choreo = false := by\n  decide\n\n",
        ));
        if let Some((_scope, [left, right])) = program.actor_decision_at(0) {
            let decision_offset = binding_offset + columns.atom_count() * 4;
            source.push_str(&format!(
                "theorem {name}SwappedChoiceActorsRejected :\n  \
                 ({{ {name}Image with programBytes := \
                 ((({name}Bytes.set {} {}).set {} {}).set {} {}).set {} {} }} : \
                 Hibana.RustDescriptorImage).ownershipCheck {name}Choreo = false := by\n  decide\n\n",
                decision_offset + 2,
                right & 255,
                decision_offset + 3,
                right >> 8,
                decision_offset + 4,
                left & 255,
                decision_offset + 5,
                left >> 8,
            ));
        }
    }
    source
}

#[test]
#[ignore = "host-only exact program-column proof artifact export"]
fn export_program_columns_for_lean() {
    type Ordinary = g::Seq<g::Send<0, 1, Read>, g::Send<1, 0, Value>>;
    type Linear = g::Seq<DbLoan, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    type Parallel = g::Par<g::Roll<DbLoan>, g::Roll<AuthLoan>>;
    let mut source = String::from("import Hibana\n\nset_option maxRecDepth 10000\n\n");
    source.push_str(&certificate::<Ordinary, 8>("ordinaryProgramColumns"));
    source.push_str(&certificate::<Linear, 8>("ownedLinearColumns"));
    source.push_str(&certificate::<Parallel, 16>("ownedParallelColumns"));
    source.push_str(&certificate::<conditional_controller::Steps, 32>(
        "ownedConditionalColumns",
    ));
    source.push_str(&certificate::<composed::Global, 32>("ownedNativeColumns"));
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/lean-proof/ProgramColumnsGenerated.lean");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(output, source).unwrap();
}
