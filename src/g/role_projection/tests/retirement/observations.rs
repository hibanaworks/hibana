//! Copied retirement evidence. No Endpoint operation or coordination lives here.
use crate::{endpoint::proof_snapshot::ActorSnapshot, rendezvous::SessionFaultKind};
use std::string::String;

/// Location of the actual moved Rust value immediately before its drop.
pub(in crate::g::role_projection::tests) enum RetirementOrigin {
    Future,
    Application,
}

pub(in crate::g::role_projection::tests) struct RetirementSample {
    pub(in crate::g::role_projection::tests) name: &'static str,
    pub(in crate::g::role_projection::tests) before: ActorSnapshot,
    pub(in crate::g::role_projection::tests) after: ActorSnapshot,
    pub(in crate::g::role_projection::tests) before_fault: Option<SessionFaultKind>,
    pub(in crate::g::role_projection::tests) after_fault: Option<SessionFaultKind>,
    pub(in crate::g::role_projection::tests) origin: RetirementOrigin,
    /// Fixture expectation, independently checked from the global in Lean.
    pub(in crate::g::role_projection::tests) complete: bool,
}

fn fault_source(fault: Option<SessionFaultKind>) -> &'static str {
    match fault {
        None => "none",
        Some(SessionFaultKind::EndpointDropped) => "some .endpointDropped",
        Some(SessionFaultKind::TransportClosed) => "some .transportClosed",
        Some(SessionFaultKind::DecodeFailed) => "some .decodeFailed",
        Some(SessionFaultKind::ProtocolViolation) => "some .protocolViolation",
        Some(SessionFaultKind::ProgressInvariantViolated) => "some .progressInvariantViolated",
    }
}

pub(in crate::g::role_projection::tests) fn write_retirements(
    filename: &str,
    choreography: String,
    image: String,
    samples: &[RetirementSample],
) {
    let mut source = std::format!(
        r#"import Hibana.OwnedRetirement

set_option maxRecDepth 10000

def retirementChoreo : Hibana.Choreo := {choreography}

def retirementImage : Hibana.RustDescriptorImage := {image}

open Hibana.EndpointTransfer

abbrev RetirementAuthority := Observation (Nat × Nat × Nat × Nat) Unit

"#
    );
    for sample in samples {
        let RetirementSample {
            name,
            before,
            after,
            before_fault,
            after_fault,
            origin,
            complete,
        } = sample;
        let view = before.views.iter().find(|view| view.role == 10).unwrap();
        let (before_authority, action, prefix) = match origin {
            RetirementOrigin::Future => (
                std::format!(
                    "{{ usable := [], preparing := [{name}Value], acceptedFutures := [], inFlight := [] }}"
                ),
                "abandonPreparation",
                std::format!(".tail (.refl _) (Step.seal (initial {name}Value) [] [] {name}Value)"),
            ),
            RetirementOrigin::Application => (
                std::format!("initial {name}Value"),
                "dropValue",
                ".refl _".into(),
            ),
        };
        source.push_str(&std::format!(
            r#"def {name} : Hibana.ViewRetirementObservation := {{
  view := 10
  before := {before}
  after := {after}
  beforeFault := {before_fault}
  afterFault := {after_fault}
}}

def {name}Value : Nat × Nat × Nat × Nat := ({actor}, 10, {run}, {nonce})

def {name}BeforeAuthority : RetirementAuthority := {before_authority}

def {name}AfterAuthority : RetirementAuthority :=
  {{ usable := [], preparing := [], acceptedFutures := [], inFlight := [] }}

theorem {name}BeforeValid : {name}.before.valid = true := by decide

theorem {name}AfterValid : {name}.after.valid = true := by decide

theorem {name}SourceCompletion :
  Hibana.sourceViewComplete? {name}.before retirementImage retirementChoreo 10 = some {complete} := by decide

theorem {name}Accepted : {name}.check retirementImage retirementChoreo = true := by decide

theorem {name}GenerationRealized :
  ∃ complete current,
    Hibana.sourceViewComplete? {name}.before retirementImage retirementChoreo 10 = some complete ∧
    Hibana.observedActorView? {name}.before 10 = some current ∧
    Hibana.applySessionGenerationAction (Hibana.observedGeneration {name}.beforeFault)
      (Hibana.retirementGenerationAction current.operation complete) =
        some (Hibana.observedGeneration {name}.afterFault) :=
  Hibana.retirement_realizes_existing_generation_action {name}Accepted

theorem {name}Retired : {name}.after.viewIssued 10 = false := by decide

theorem {name}ProgressPreserved : {name}.progressMatches = true := by decide

theorem {name}MissingSourceRowsRejected :
  Hibana.sourceViewComplete? {{ {name}.before with steps := [] }} retirementImage retirementChoreo 10 = none := by decide

theorem {name}MissingRetirementRejected :
  ({{ {name} with after := {{ {name}.after with views := {name}.before.views }} }} :
    Hibana.ViewRetirementObservation).check retirementImage retirementChoreo = false := by decide

theorem {name}GhostProgressRejected :
  ({{ {name} with after := {{ {name}.after with steps := {name}.after.steps.map (fun step => {{ step with done := true }}) }} }} :
    Hibana.ViewRetirementObservation).check retirementImage retirementChoreo = false := by decide

theorem {name}AuthorityRetirement : Step .{action} {name}BeforeAuthority {name}AfterAuthority :=
  Step.{action} {name}BeforeAuthority [] [] {name}Value

theorem {name}NoAuthorityRestored : {name}AfterAuthority.authorityCount = 0 := by decide

theorem {name}BeforeAuthorityLocated : Hibana.TransferAuthorityLocated {name}BeforeAuthority :=
  Hibana.count_one_transfer_authority_has_one_location (by decide)

theorem {name}AuthorityHistory : Execution (initial {name}Value) {name}AfterAuthority :=
  .tail ({prefix}) {name}AuthorityRetirement

theorem {name}LossBoundary :
  ∃ middle following action,
    Execution (initial {name}Value) middle ∧
    Step action middle following ∧
    (action = .abandonPreparation ∨ action = .dropValue ∨ action = .fenceFrame) ∧
    Execution following {name}AfterAuthority := by
  rcases Hibana.initial_transfer_execution_is_located_or_has_retired {name}AuthorityHistory with
    located | retired
  · have count := Hibana.located_transfer_authority_has_count_one located
    rw [{name}NoAuthorityRestored] at count
    contradiction
  · exact retired

"#,
            before = super::super::commit::observation_source(before),
            after = super::super::commit::observation_source(after),
            before_fault = fault_source(*before_fault),
            after_fault = fault_source(*after_fault),
            actor = before.actor, run = view.run, nonce = view.nonce,
        ));
        if *complete {
            source.push_str(&std::format!(
                r#"theorem {name}LiveSourceCompleted :
  Hibana.sourceViewComplete? {name}.before retirementImage retirementChoreo 10 = some true ∧
  ∃ current, Hibana.observedActorView? {name}.before 10 = some current ∧ current.operation = .idle :=
  Hibana.live_retirement_requires_source_completion {name}Accepted (by decide)

theorem {name}PublicationPermitted :
  (Hibana.observedGeneration {name}.afterFault).publicationPermitted = true := by decide

theorem {name}SpuriousFaultRejected :
  ({{ {name} with afterFault := some .endpointDropped }} : Hibana.ViewRetirementObservation).check
    retirementImage retirementChoreo = false := by decide

"#,
            ));
        } else {
            source.push_str(&std::format!(
                r#"theorem {name}PublicationBlocked :
  (Hibana.observedGeneration {name}.afterFault).publicationPermitted = false :=
  Hibana.retirement_with_pending_source_disables_publication {name}Accepted {name}SourceCompletion

theorem {name}ReattachmentBlocked :
  Hibana.applySessionGenerationAction (Hibana.observedGeneration {name}.afterFault) .attach = none := by decide

theorem {name}MissingFaultRejected :
  ({{ {name} with afterFault := none }} : Hibana.ViewRetirementObservation).check
    retirementImage retirementChoreo = false := by decide

"#,
            ));
        }
    }
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(std::format!("{filename}.lean")), source).unwrap();
}
