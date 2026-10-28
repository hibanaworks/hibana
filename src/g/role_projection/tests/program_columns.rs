//! Export actual ordinary and owned column headers for independent decoding.
//! Check grammar, source ownership, actor operations, dependency order and routes.
//! Complete progress, reentry and public qualification remain separate obligations.
use super::*;
use crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{format, fs, path::PathBuf, string::String, vec::Vec};

#[test]
fn a_delegated_nested_route_rejects_a_self_parent_in_its_resident_image() {
    type Routed = RoleProjection<1, routing::Steps, 32>;
    const LEN: usize = Routed::IMAGE_REF.columns().blob_len();
    static ORIGINAL: [u8; LEN] = {
        let mut bytes = [0; LEN];
        let image = &Routed::IMAGE_REF;
        let descriptor = RoleDescriptorRef::from_resident(image);
        let mut offset = 0;
        while offset < LEN {
            bytes[offset] = descriptor.proof_byte_at(offset);
            offset += 1;
        }
        bytes
    };
    static CYCLIC: [u8; LEN] = {
        let mut bytes = ORIGINAL;
        let image = &Routed::IMAGE_REF;
        let root = image.route_scope_by_slot(0).unwrap();
        let parent = (root.raw() * 2).to_le_bytes();
        let offset = image.columns().route_scope_conflicts.offset as usize;
        bytes[offset] = parent[0];
        bytes[offset + 1] = parent[1];
        bytes
    };
    let image = &Routed::IMAGE_REF;
    let _ = RoleImageRef::new(
        image.program(),
        image.role(),
        image.facts(),
        image.columns(),
        &ORIGINAL,
    );
    assert!(
        std::panic::catch_unwind(|| {
            RoleImageRef::new(
                image.program(),
                image.role(),
                image.facts(),
                image.columns(),
                &CYCLIC,
            )
        })
        .is_err()
    );
}

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
    source.push_str(&format!(
        "theorem {name}ActorRowsMatchSource :\n  \
         {name}Image.actorRowsCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}ActorOrderMatchesSource :\n  \
         {name}Image.actorOrderCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}ActorRoutesMatchSource :\n  \
         {name}Image.actorRouteCheck {name}Choreo = true := by\n  decide\n\n",
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

fn actor_certificate<const ROLE: u8, Steps: g::ProgramShape + LeanChoreo, const CAPACITY: usize>(
    name: &str,
) -> String {
    let () = g::ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    let descriptor =
        RoleDescriptorRef::from_resident(&RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF);
    assert!(descriptor.local_len() > 0);
    let choreo = Steps::lean_source();
    let image = descriptor_source(descriptor);
    let flags_offset = ROLE_IMAGE_EVENT_STRIDE - 1;
    format!(
        "def {name}Choreo : Hibana.Choreo := {choreo}\n\n\
         def {name}Image : Hibana.RustDescriptorImage := {image}\n\n\
         theorem {name}ActorRowsMatchSource :\n  \
         {name}Image.actorRowsCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}ActorOrderMatchesSource :\n  \
         {name}Image.actorOrderCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}ActorRoutesMatchSource :\n  \
         {name}Image.actorRouteCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}InvalidDirectionRejected :\n  \
         ({{ {name}Image with roleBytes := {name}Image.roleBytes.set {flags_offset} 6 }} : \
         Hibana.RustDescriptorImage).actorRowsCheck {name}Choreo = false := by\n  decide\n\n\
         theorem {name}MissingOperationsRejected :\n  \
         ({{ {name}Image with eventCount := 0 }} : \
         Hibana.RustDescriptorImage).actorRowsCheck {name}Choreo = false := by\n  decide\n\n",
    )
}

fn partial_choice_certificate() -> String {
    let mut source = String::from("import Hibana\n\nset_option maxRecDepth 10000\n\n");
    source.push_str(&certificate::<partial_choice::Steps, 32>(
        "ownedPartialChoice",
    ));
    let program = g::ProgramProjection::<partial_choice::Steps, 32>::PROGRAM_REF;
    let [reference, _] = program
        .actor_binding_at(program.columns.atom_count() - 2)
        .unwrap();
    let (outer_scope, [inner_reference, _]) = program
        .actor_decision_at(usize::from(reference) - 256)
        .unwrap();
    let (inner_scope, _) = program
        .actor_decision_at(usize::from(inner_reference) - 256)
        .unwrap();
    let arm = |value| match value {
        None => "none",
        Some(0) => "some .left",
        Some(1) => "some .right",
        _ => unreachable!(),
    };
    let inputs = partial_choice::CASES
        .iter()
        .map(|&(outer, inner, _)| format!("({}, {})", arm(outer), arm(inner)))
        .collect::<Vec<_>>()
        .join(", ");
    let observed = partial_choice::CASES
        .iter()
        .map(|&(outer, inner, expected)| {
            let observed = program.selected_actor(reference, &mut |scope| {
                if scope == outer_scope {
                    outer
                } else {
                    assert_eq!(scope, inner_scope);
                    inner
                }
            });
            assert_eq!(observed, expected);
            match observed {
                None => String::from("none"),
                Some(actor) => format!("some {actor}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    source.push_str(&format!(
        "def ownedPartialChoiceInputs : List (Option Hibana.RouteArm × Option Hibana.RouteArm) :=\n  \
         [{inputs}]\n\n\
         def ownedPartialChoiceRustActors : List (Option Nat) := [{observed}]\n\n\
         theorem ownedPartialChoiceActorsAgreeWithRust :\n  \
         ownedPartialChoiceInputs.map (fun choices =>\n    \
         ownedPartialChoiceColumns.selectedActor? (fun scope =>\n      \
         if scope == {} then choices.1 else if scope == {} then choices.2 else none) \
         {reference}) = ownedPartialChoiceRustActors := by\n  decide\n\n",
        outer_scope.raw(),
        inner_scope.raw(),
    ));
    source.push_str(&actor_certificate::<1, partial_choice::Steps, 32>(
        "ownedPartialFirstRows",
    ));
    source.push_str(&actor_certificate::<2, partial_choice::Steps, 32>(
        "ownedPartialSecondRows",
    ));
    source.push_str(&actor_certificate::<3, partial_choice::Steps, 32>(
        "ownedPartialThirdRows",
    ));
    source
}

#[test]
#[ignore = "host-only exact program-column proof artifact export"]
fn export_program_columns_for_lean() {
    type Ordinary = g::Seq<g::Send<0, 1, Read>, g::Send<1, 0, Value>>;
    type Linear = g::Seq<DbLoan, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    type Parallel = g::Par<g::Roll<DbLoan>, g::Roll<AuthLoan>>;
    let mut source = String::from("import Hibana.ActorRouting\n\nset_option maxRecDepth 10000\n\n");
    source.push_str(&certificate::<Ordinary, 8>("ordinaryProgramColumns"));
    source.push_str(&certificate::<Linear, 8>("ownedLinearColumns"));
    source.push_str(&certificate::<Parallel, 16>("ownedParallelColumns"));
    source.push_str(&certificate::<conditional_controller::Steps, 32>(
        "ownedConditionalColumns",
    ));
    source.push_str(&certificate::<composed::Global, 32>("ownedNativeColumns"));
    source.push_str(&actor_certificate::<1, Ordinary, 8>("ordinaryReceiverRows"));
    source.push_str(&actor_certificate::<1, Linear, 8>("ownedWorkerRows"));
    source.push_str(&actor_certificate::<3, Linear, 8>("ownedServiceRows"));
    source.push_str(&actor_certificate::<1, Parallel, 16>(
        "ownedParallelWorkerRows",
    ));
    source.push_str(&actor_certificate::<1, conditional_controller::Steps, 32>(
        "ownedConditionalFirstRows",
    ));
    source.push_str(&actor_certificate::<3, conditional_controller::Steps, 32>(
        "ownedConditionalSecondRows",
    ));
    source.push_str(&actor_certificate::<1, composed::Global, 32>(
        "ownedNativeWorkerRows",
    ));
    source.push_str(&actor_certificate::<2, composed::Global, 32>(
        "ownedNativeServiceRows",
    ));
    source.push_str(&actor_certificate::<3, composed::Global, 32>(
        "ownedNativeParallelRows",
    ));
    source.push_str(&actor_certificate::<4, composed::Global, 32>(
        "ownedNativeParallelPeerRows",
    ));
    type CoLocated = g::Seq<g::Send<0, 1, Loan>, g::Send<10, 1, Read>>;
    source.push_str(&actor_certificate::<1, CoLocated, 8>("coLocatedRows"));
    source.push_str(&actor_certificate::<1, order::Steps, 16>(
        "nestedActorOrder",
    ));
    source.push_str(&actor_certificate::<1, routing::Steps, 32>(
        "nestedActorRoutes",
    ));
    let routed = &RoleProjection::<1, routing::Steps, 32>::IMAGE_REF;
    assert_eq!(routed.footprint().route_scope_count, 2);
    let root = routed.route_scope_by_slot(0).unwrap();
    for (name, raw) in [
        ("falseRouteReentry", 0xc000),
        ("cyclicRouteParent", root.raw() * 2),
    ] {
        let low = raw & 255;
        let high = raw >> 8;
        source.push_str(&format!(
            "def {name}Image : Hibana.RustDescriptorImage := \
             {{ nestedActorRoutesImage with roleBytes := \
             (nestedActorRoutesImage.roleBytes.set nestedActorRoutesImage.roleRouteScopeConflictOffset {low}).set \
             (nestedActorRoutesImage.roleRouteScopeConflictOffset + 1) {high} }}\n\n\
             theorem {name}OrderStillMatches :\n  \
             {name}Image.actorOrderCheck nestedActorRoutesChoreo = true := by\n  decide\n\n\
             theorem {name}SourceRouteRejected :\n  \
             {name}Image.actorRouteCheck nestedActorRoutesChoreo = false := by\n  decide\n\n",
        ));
    }
    source.push_str(
        "theorem falseRouteReentryPriorRouteColumnsStillCoherent :\n  \
         (falseRouteReentryImage.routeArmColumnsCoherent &&\n  \
         falseRouteReentryImage.routeCommitColumnsCoherent &&\n  \
         falseRouteReentryImage.passiveChildColumnsCoherent) = true := by\n  decide\n\n\
         theorem cyclicRouteParentCommitColumnsRejected :\n  \
         cyclicRouteParentImage.routeCommitColumnsCoherent = false := by\n  decide\n\n",
    );
    let nested =
        RoleDescriptorRef::from_resident(&RoleProjection::<1, order::Steps, 16>::IMAGE_REF);
    assert_eq!(nested.local_len(), 5);
    let join_dependency = [
        nested.proof_byte_at(3 * ROLE_IMAGE_EVENT_STRIDE + 2),
        nested.proof_byte_at(3 * ROLE_IMAGE_EVENT_STRIDE + 3),
    ];
    let fork_dependency = [
        nested.proof_byte_at(4 * ROLE_IMAGE_EVENT_STRIDE + 2),
        nested.proof_byte_at(4 * ROLE_IMAGE_EVENT_STRIDE + 3),
    ];
    assert_ne!(join_dependency, [255, 255]);
    assert_ne!(fork_dependency, join_dependency);
    for (name, row, dependency) in [
        ("extraParallelWait", 4, join_dependency),
        ("missingParallelJoin", 3, [255, 255]),
    ] {
        source.push_str(&format!(
            "def {name}Image : Hibana.RustDescriptorImage := \
             {{ nestedActorOrderImage with roleBytes := \
             (nestedActorOrderImage.roleBytes.set {} {}).set {} {} }}\n\n\
             theorem {name}RowsStillMatch :\n  \
             {name}Image.actorRowsCheck nestedActorOrderChoreo = true := by\n  decide\n\n\
             theorem {name}LaneColumnsStillCoherent :\n  \
             {name}Image.laneColumnsCoherent = true := by\n  decide\n\n\
             theorem {name}OrderRejected :\n  \
             {name}Image.actorOrderCheck nestedActorOrderChoreo = false := by\n  decide\n\n",
            row * ROLE_IMAGE_EVENT_STRIDE + 2,
            dependency[0],
            row * ROLE_IMAGE_EVENT_STRIDE + 3,
            dependency[1],
        ));
    }
    let loan_schema = 0x1_0000_0000u64 + 10;
    let read_schema = <u32 as crate::transport::wire::WirePayload>::SCHEMA_ID;
    let co_located_flags = 2 * ROLE_IMAGE_EVENT_STRIDE - 1;
    source.push_str(&format!(
        "theorem coLocatedRowsRetainBothLogicalFacets :\n  \
         coLocatedRowsImage.decodeActions? = some \
         [.recv 0 1 {loan_schema}, .send 1 3 {read_schema}, .recv 10 3 {read_schema}] := by\n  \
         decide\n\n\
         theorem coLocatedRowsRetainLogicalViews :\n  \
         [coLocatedRowsImage.eventView? 0, coLocatedRowsImage.eventView? 1, \
         coLocatedRowsImage.eventView? 2] = [some 1, some 10, some 1] := by\n  decide\n\n\
         theorem coLocatedRowsCannotBecomeSelfSend :\n  \
         ({{ coLocatedRowsImage with roleBytes := coLocatedRowsImage.roleBytes.set {co_located_flags} 0 }} : \
         Hibana.RustDescriptorImage).actorRowsCheck coLocatedRowsChoreo = false := by\n  decide\n\n",
    ));
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/lean-proof/ProgramColumnsGenerated.lean");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, source).unwrap();
    fs::write(
        output.with_file_name("PartialChoiceGenerated.lean"),
        partial_choice_certificate(),
    )
    .unwrap();
    controller_columns::export();
    ingress_columns::export();
    reentry_columns::export();
}
