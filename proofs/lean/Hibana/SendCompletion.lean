import Hibana.ResourceValueCorrespondence

namespace Hibana

/-- Results at the existing non-generic send loop's prepared-I/O boundary.
Success means the endpoint supplied its checked commit plan. This is not a
new carrier method, runtime phase, or application coordination state. -/
inductive SendPollClass where
  | pending
  | success
  | failure
  deriving DecidableEq

/-- Read-only result of executing the production kernel_send function with a
boundary double. The plan is opaque to that loop; source preparation and the
real publication body require their separate implementation evidence. -/
structure SendCompletionObservation where
  boundary : SendPollClass
  returned : SendPollClass
  polls : Nat
  publications : Nat
  done : Bool
  retained : Bool
  deriving DecidableEq

def SendCompletionObservation.Exact (sample : SendCompletionObservation) : Prop :=
  sample.returned = sample.boundary ∧ sample.polls = 1 ∧
    match sample.boundary with
    | .pending => sample.publications = 0 ∧ sample.done = false ∧ sample.retained = true
    | .success => sample.publications = 1 ∧ sample.done = true ∧ sample.retained = false
    | .failure => sample.publications = 0 ∧ sample.done = true ∧ sample.retained = false

instance (sample : SendCompletionObservation) : Decidable sample.Exact := by
  unfold SendCompletionObservation.Exact
  cases sample.boundary <;> infer_instance

def SendCompletionObservation.check (sample : SendCompletionObservation) : Bool :=
  decide sample.Exact

theorem checked_send_completion_has_its_exact_boundary
    {sample : SendCompletionObservation} (checked : sample.check = true) : sample.Exact :=
  of_decide_eq_true checked

theorem a_checked_send_can_park_only_while_its_boundary_is_pending
    {sample : SendCompletionObservation} (checked : sample.check = true)
    (parked : sample.returned = .pending) : sample.boundary = .pending :=
  (checked_send_completion_has_its_exact_boundary checked).1.symm.trans parked

theorem a_checked_successful_send_publishes_before_returning
    {sample : SendCompletionObservation} (checked : sample.check = true)
    (successful : sample.boundary = .success) :
    sample.returned = .success ∧ sample.polls = 1 ∧ sample.publications = 1 ∧
      sample.done = true ∧ sample.retained = false := by
  have exactAt := checked_send_completion_has_its_exact_boundary checked
  exact ⟨exactAt.1.trans successful, exactAt.2.1, by simpa only [successful] using exactAt.2.2⟩

theorem a_checked_failed_send_finishes_without_publication
    {sample : SendCompletionObservation} (checked : sample.check = true)
    (failed : sample.boundary = .failure) :
    sample.returned = .failure ∧ sample.polls = 1 ∧ sample.publications = 0 ∧
      sample.done = true ∧ sample.retained = false := by
  have exactAt := checked_send_completion_has_its_exact_boundary checked
  exact ⟨exactAt.1.trans failed, exactAt.2.1, by simpa only [failed] using exactAt.2.2⟩

theorem a_checked_pending_send_retains_its_plan_without_publication
    {sample : SendCompletionObservation} (checked : sample.check = true)
    (pending : sample.boundary = .pending) :
    sample.returned = .pending ∧ sample.polls = 1 ∧ sample.publications = 0 ∧
      sample.done = false ∧ sample.retained = true := by
  have exactAt := checked_send_completion_has_its_exact_boundary checked
  exact ⟨exactAt.1.trans pending, exactAt.2.1, by simpa only [pending] using exactAt.2.2⟩

def checkSendCompletionTable (samples : List SendCompletionObservation) : Bool :=
  decide (samples.map (·.boundary) = [.pending, .success, .failure]) &&
    samples.all SendCompletionObservation.check

theorem a_checked_send_completion_table_covers_every_boundary
    {samples : List SendCompletionObservation}
    (checked : checkSendCompletionTable samples = true) (boundary : SendPollClass) :
    ∃ sample, sample ∈ samples ∧ sample.boundary = boundary ∧ sample.check = true := by
  have fields := Bool.and_eq_true_iff.mp checked
  have classes : samples.map (·.boundary) = [.pending, .success, .failure] :=
    of_decide_eq_true fields.1
  have present : boundary ∈ samples.map (·.boundary) := by
    rw [classes]
    cases boundary <;> decide
  obtain ⟨sample, member, same⟩ := List.mem_map.mp present
  exact ⟨sample, member, same, List.all_eq_true.mp fields.2 sample member⟩

/-- Compose the successful loop contract and an owned local commit's exact
tag retirement. Identifying that call's publication with this checked source
commit is an implementation correspondence obligation. No second wire send
or receiver acknowledgment is introduced, and callback/executor termination
is not inferred from a completed observation. -/
theorem checked_owned_send_publication_finishes_without_another_wait
    {poll : SendCompletionObservation} {sample : ActorCommitObservation}
    {image : RustDescriptorImage} {choreo : Choreo} {wire : OwnedWireObservation}
    {session : Nat} {facts : OwnedCommitFacts}
    (pollChecked : poll.check = true) (accepted : poll.boundary = .success)
    (commitChecked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (sending : facts.side = .send) :
    poll.returned = .success ∧ poll.publications = 1 ∧ poll.done = true ∧
      observedResourceTag? sample.after facts.resource.role = none := by
  obtain ⟨returned, _, publications, done, _⟩ :=
    a_checked_successful_send_publishes_before_returning pollChecked accepted
  obtain ⟨_, _, _, retired⟩ :=
    checked_owned_send_retires_its_exact_observed_tag commitChecked decoded sending
  exact ⟨returned, publications, done, retired⟩

end Hibana
