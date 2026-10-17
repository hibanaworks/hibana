//! Export actual controller witnesses, independently checked against source.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{format, fs, string::String, vec::Vec};

fn certificate<const ROLE: u8, Steps: g::ProgramShape + LeanChoreo, const CAPACITY: usize>(
    name: &str,
) -> String {
    let () = g::ProgramProjection::<Steps, CAPACITY>::VALIDATION;
    let image = &RoleProjection::<ROLE, Steps, CAPACITY>::IMAGE_REF;
    let mut observations = Vec::new();
    if image.footprint().route_scope_count != 0 {
        with_cursor(image, |cursor| {
            for slot in 0..image.footprint().route_scope_count {
                let scope = image.route_scope_by_slot(slot).unwrap();
                let controller = image.program().route_controller_role(scope);
                let present = cursor.contains_route_controller(scope);
                observations.push(format!(
                    "({slot}, {}, {controller}, {present})",
                    scope.raw()
                ));
            }
        });
    }
    let choreo = Steps::lean_source();
    let descriptor = descriptor_source(RoleDescriptorRef::from_resident(image));
    format!(
        "def {name}Choreo : Hibana.Choreo := {choreo}\n\n\
         def {name}Image : Hibana.RustDescriptorImage := {descriptor}\n\n\
         def {name}RustControllers : List (Nat × Nat × Nat × Bool) := [{}]\n\n\
         theorem {name}ControllersMatchSource :\n  \
         {name}Image.actorControllerCheck {name}Choreo = true := by\n  decide\n\n\
         theorem {name}ControllerWitnessesAgreeWithRust :\n  \
         {name}RustControllers.all (fun row =>\n    \
         {name}Image.decodeRouteScope? row.1 == some row.2.1 &&\n    \
         {name}Image.routeControllerAt? row.1 == some row.2.2.1 &&\n    \
         {name}Image.routeControllerPresent? row.1 row.2.2.1 == some row.2.2.2) = true := by\n  decide\n\n",
        observations.join(", "),
    )
}

pub(super) fn export() {
    type Ordinary = g::Seq<g::Send<0, 1, Read>, g::Send<1, 0, Value>>;
    type Linear = g::Seq<DbLoan, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    type Parallel = g::Par<g::Roll<DbLoan>, g::Roll<AuthLoan>>;
    type CoLocated = g::Seq<g::Send<0, 1, Loan>, g::Send<10, 1, Read>>;
    let mut source = String::from("import Hibana\n\nset_option maxRecDepth 10000\n\n");
    source.push_str(&certificate::<0, Ordinary, 8>("controllerOrdinaryOwner"));
    source.push_str(&certificate::<1, Ordinary, 8>("controllerOrdinaryPeer"));
    source.push_str(&certificate::<0, Linear, 8>("controllerLinearOwner"));
    source.push_str(&certificate::<1, Linear, 8>("controllerLinearWorker"));
    source.push_str(&certificate::<3, Linear, 8>("controllerLinearService"));
    source.push_str(&certificate::<0, Parallel, 16>("controllerParallelOwner"));
    source.push_str(&certificate::<1, Parallel, 16>("controllerParallelWorker"));
    source.push_str(&certificate::<0, conditional_controller::Steps, 32>(
        "controllerConditionalOwner",
    ));
    source.push_str(&certificate::<1, conditional_controller::Steps, 32>(
        "controllerConditionalFirst",
    ));
    source.push_str(&certificate::<3, conditional_controller::Steps, 32>(
        "controllerConditionalSecond",
    ));
    source.push_str(&certificate::<0, composed::Global, 32>(
        "controllerNativeOwner",
    ));
    source.push_str(&certificate::<1, composed::Global, 32>(
        "controllerNativeWorker",
    ));
    source.push_str(&certificate::<2, composed::Global, 32>(
        "controllerNativeService",
    ));
    source.push_str(&certificate::<3, composed::Global, 32>(
        "controllerNativeParallel",
    ));
    source.push_str(&certificate::<4, composed::Global, 32>(
        "controllerNativeParallelPeer",
    ));
    source.push_str(&certificate::<1, CoLocated, 8>("controllerCoLocated"));
    source.push_str(&certificate::<1, order::Steps, 16>("controllerNestedOrder"));
    source.push_str(&certificate::<1, routing::Steps, 32>(
        "controllerNestedRoutes",
    ));
    source.push_str(&certificate::<0, partial_choice::Steps, 32>(
        "controllerPartialOwner",
    ));
    source.push_str(&certificate::<1, partial_choice::Steps, 32>(
        "controllerPartialFirst",
    ));
    source.push_str(&certificate::<2, partial_choice::Steps, 32>(
        "controllerPartialSecond",
    ));
    source.push_str(&certificate::<3, partial_choice::Steps, 32>(
        "controllerPartialThird",
    ));
    source.push_str(&certificate::<0, conditional_controller::LocalSteps, 8>(
        "controllerLocalOwner",
    ));
    source.push_str(&certificate::<1, conditional_controller::LocalSteps, 8>(
        "controllerLocalWorker",
    ));
    source.push_str(
        "def ghostActorImage : Hibana.RustDescriptorImage :=\n  \
         { controllerCoLocatedImage with role := 2 }\n\n\
         theorem ghostActorOwnershipStillMatches :\n  \
         ghostActorImage.ownershipCheck controllerCoLocatedChoreo = true := by\n  decide\n\n\
         theorem ghostActorUnreachableRowsRejected :\n  \
         ghostActorImage.actorRowsCheck controllerCoLocatedChoreo = false := by\n  decide\n\n",
    );

    let program = g::ProgramProjection::<conditional_controller::Steps, 32>::PROGRAM_REF;
    let resolver_row = (0..program.columns.route_resolver_count())
        .find(|&row| {
            let (scope, _) = program.route_resolver_authority_at_row(row).unwrap();
            program.route_controller_role(scope) == 10
        })
        .unwrap();
    let resolver_offset = program.columns.route_resolvers().offset as usize + resolver_row * 8;
    for (name, offset, value) in [
        ("wrongController", resolver_offset + 4, 3),
        ("wrongResolverId", resolver_offset + 2, 99),
    ] {
        source.push_str(&format!(
            "def {name}Image : Hibana.RustDescriptorImage :=\n  \
             {{ controllerConditionalSecondImage with programBytes := \
             controllerConditionalSecondImage.programBytes.set {offset} {value} }}\n\n\
             theorem {name}PriorOperationsAndOrderStillMatch :\n  \
             ({name}Image.actorRowsCheck controllerConditionalSecondChoreo &&\n  \
             {name}Image.actorOrderCheck controllerConditionalSecondChoreo) = true := by\n  decide\n\n\
             theorem {name}ControllerSourceRejected :\n  \
             {name}Image.actorControllerCheck controllerConditionalSecondChoreo = false := by\n  decide\n\n",
        ));
    }
    let image = &RoleProjection::<3, conditional_controller::Steps, 32>::IMAGE_REF;
    let slot = (0..image.footprint().route_scope_count)
        .find(|&slot| {
            image
                .program()
                .route_controller_role(image.route_scope_by_slot(slot).unwrap())
                == 10
        })
        .unwrap();
    let mut shifted = String::from("controllerConditionalSecondImage.roleBytes");
    for arm in 0..=1 {
        let range = image.route_arm_event_row_by_slot(slot, arm);
        assert!(range.len() >= 2);
        assert!(
            image
                .local_step_node(range.start())
                .unwrap()
                .action()
                .is_send()
        );
        assert!(matches!(
            image.local_step_node(range.start() + 1).unwrap().action(),
            LocalAction::Recv { .. }
        ));
        let packed = ((range.start() + 1) << 16) | (range.len() - 1);
        for byte in 0..4 {
            shifted = format!(
                "({shifted}.set (controllerConditionalSecondImage.roleRouteArmOffset + {}) {})",
                (slot * 2 + usize::from(arm)) * 8 + byte,
                (packed >> (8 * byte)) & 255,
            );
        }
    }
    source.push_str(&format!(
        "def missingControllerWitnessImage : Hibana.RustDescriptorImage :=\n  \
         {{ controllerConditionalSecondImage with roleBytes := {shifted} }}\n\n\
         theorem missingControllerWitnessOperationsStillMatch :\n  \
         missingControllerWitnessImage.actorRowsCheck controllerConditionalSecondChoreo = true := by\n  decide\n\n\
         theorem missingControllerWitnessRangesStillDecode :\n  \
         ((missingControllerWitnessImage.decodeRouteArmEvents? {slot} 0).isSome &&\n  \
         (missingControllerWitnessImage.decodeRouteArmEvents? {slot} 1).isSome) = true := by\n  decide\n\n\
         theorem missingControllerWitnessSourceRejected :\n  \
         missingControllerWitnessImage.actorControllerCheck controllerConditionalSecondChoreo = false := by\n  decide\n\n",
    ));
    fs::write(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/lean-proof/ControllerGenerated.lean"),
        source,
    )
    .unwrap();
}
