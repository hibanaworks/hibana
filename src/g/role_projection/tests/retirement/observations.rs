//! Copied retirement evidence. No Endpoint operation or coordination lives here.
use crate::{
    endpoint::proof_snapshot::{ActorSnapshot, owned_future::OwnedFutureBoundary},
    rendezvous::SessionFaultKind,
};
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
    pub(in crate::g::role_projection::tests) constructor: Option<OwnedFutureBoundary>,
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
        r#"import Hibana.OwnedCallHistory

set_option maxRecDepth 10000

def retirementChoreo : Hibana.Choreo := {choreography}

def retirementImage : Hibana.RustDescriptorImage := {image}

open Hibana.EndpointTransfer

abbrev RetirementAuthority := Observation (Nat × Nat × Nat × Nat) Unit
def retirementResource : Hibana.EndpointDelegation.EndpointKey := {{ component := 0, role := 10 }}

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
            constructor,
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
        let (event, before_cut, boundary) = match origin {
            RetirementOrigin::Future => {
                let Some(OwnedFutureBoundary::Seal {
                    carrier,
                    payload,
                    label,
                    region,
                }) = *constructor
                else {
                    panic!("missing actual owned-future constructor");
                };
                assert_eq!(carrier.actor_address, payload.actor_address);
                for identity in [carrier, payload] {
                    let observed = before
                        .views
                        .iter()
                        .find(|candidate| candidate.role == identity.role)
                        .unwrap();
                    assert_eq!(
                        (identity.run, identity.nonce),
                        (observed.run, observed.nonce)
                    );
                }
                assert_eq!(payload.role, view.role);
                let bounds = region.bounds();
                source.push_str(&std::format!(
                    "def {name}Request : Hibana.OwnedCallHistory.Request := {{ \
                     carrier := ({}, {}, {}, {}), payload := ({}, {}, {}, {}), \
                     label := {label}, borrow := ({}, {}) }}\n\n",
                    before.actor,
                    carrier.role,
                    carrier.run,
                    carrier.nonce,
                    before.actor,
                    payload.role,
                    payload.run,
                    payload.nonce,
                    bounds.start,
                    bounds.end,
                ));
                (
                    std::format!(".abandon {name}Request {name}"),
                    std::format!(
                        "{{ requests := [{name}Request], custody := {{ usable := [], preparing := [{name}Value], acceptedFutures := [], inFlight := [] }} }}"
                    ),
                    std::format!(
                        ".abandon events {{ usable := [], preparing := [], acceptedFutures := [], inFlight := [] }} [] [] [] [] {name}Request {name} (by decide)"
                    ),
                )
            }
            RetirementOrigin::Application => {
                assert!(constructor.is_none());
                (
                    std::format!(".dropValue {name}Value {name}"),
                    std::format!(
                        "{{ requests := [], custody := {{ usable := [{name}Value], preparing := [], acceptedFutures := [], inFlight := [] }} }}"
                    ),
                    std::format!(
                        ".dropValue events {{ usable := [], preparing := [], acceptedFutures := [], inFlight := [] }} [] [] [] {name}Value {name} (by decide)"
                    ),
                )
            }
        };
        source.push_str(&std::format!(
            r#"def {name}Event : Hibana.OwnedCallHistory.OwnedCallEvent := {event}
def {name}BeforeCut : Hibana.OwnedCallHistory.Cut := {before_cut}
def {name}AfterCut : Hibana.OwnedCallHistory.Cut :=
  {{ requests := [], custody := {{ usable := [], preparing := [], acceptedFutures := [], inFlight := [] }} }}

theorem {name}MatchedValue :
  Hibana.OwnedCallHistory.retirementMatches retirementResource {name}Value {name}
    retirementImage retirementChoreo = true := by decide

theorem {name}WrongValueRejected :
  Hibana.OwnedCallHistory.retirementMatches retirementResource
    ({actor}, 10, {run}, {nonce} + 2) {name} retirementImage retirementChoreo = false := by decide

theorem {name}WrongResourceRejected :
  Hibana.OwnedCallHistory.retirementMatches {{ retirementResource with role := 11 }} {name}Value
    {name} retirementImage retirementChoreo = false := by decide

theorem {name}MergedBoundary (events : List Hibana.OwnedCallHistory.OwnedCallEvent) :
  Hibana.OwnedCallHistory.Boundary (fun _ => retirementImage) retirementResource retirementChoreo
    events {name}Event {name}BeforeCut {name}AfterCut := {boundary}

theorem {name}RetirementIsRecorded : {name}Event.retirement? = some ({name}Value, {name}) := rfl

theorem {name}MergedOutcome (events : List Hibana.OwnedCallHistory.OwnedCallEvent) :
  ({name}.afterFault = none ∧
    Hibana.sourceViewComplete? {name}.before retirementImage retirementChoreo {name}.view = some true ∧
    ∃ current, Hibana.observedActorView? {name}.before {name}.view = some current ∧ current.operation = .idle) ∨
  ∃ cause, {name}.afterFault = some cause ∧
    (Hibana.observedGeneration {name}.afterFault).publicationPermitted = false :=
  Hibana.a_joint_value_retirement_is_complete_or_faulted ({name}MergedBoundary events) {name}RetirementIsRecorded

theorem {name}SourceWordRetainsRetirement (events : List Hibana.OwnedCallHistory.OwnedCallEvent) :
  Hibana.OwnedCallHistory.sourceWord (events ++ [{name}Event]) =
    Hibana.OwnedCallHistory.sourceWord events ++ [.retireView {name}] := by
  simp only [Hibana.OwnedCallHistory.sourceWord, List.filterMap_append, List.filterMap_cons,
    List.filterMap_nil, {name}Event, Hibana.OwnedCallHistory.OwnedCallEvent.source?]

theorem {name}ActualZeroWrites : ({after_writes} : Nat) = {before_writes} := by decide

theorem {name}QuietCannotHideRetirement :
  Hibana.actorQuietHeadersCheck {name}.before {name}.after = false := by decide

theorem {name}PhysicalCutExtension
    {{images : Nat → Hibana.RustDescriptorImage}} {{first cut : Hibana.DistributedSourceCut}}
    {{word entries published pending}} {{session : Nat}}
    (imageAt : images {name}.before.actor = retirementImage)
    (history : Hibana.DistributedPhysicalExecution images retirementChoreo session
      first cut word entries published pending)
    (atCut : cut.actors {name}.before.actor = ({name}.before, {before_writes})) :
  Hibana.DistributedPhysicalExecution images retirementChoreo session first
    (cut.actorAt {name}.before.actor ({name}.after, {after_writes}))
    (word ++ [.retireView {name}]) entries published pending := by
  have advanced := Hibana.DistributedPhysicalExecution.retireView (sample := {name}) history
    (by simpa only [imageAt] using {name}Accepted) (congrArg Prod.fst atCut)
  simpa only [atCut] using advanced

"#,
            actor = before.actor,
            run = view.run,
            nonce = view.nonce,
            before_writes = before.completion_mutations,
            after_writes = after.completion_mutations,
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
