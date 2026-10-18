
private def distributedPreparedReceivedSourceFacts :=
  (distributedImage1.ownedSourceFacts? distributedChoreo 0 0).get (by decide)

private theorem distributedFreshOwnedSenderAndWire
    (bytes : List Nat) :
    ∃ sentFacts, distributedEntry6.sample.ownedFacts? distributedChoreo = some sentFacts ∧
      sentFacts.resource = distributedPreparedReceivedSourceFacts.resource ∧ sentFacts.side = .send ∧
      (distributedPreparedReceivedSourceFacts.wireMatches 1
          { distributedEntry6.sample.wire with payload := bytes } 9062 = true ↔
        bytes = distributedEntry6.sample.wire.payload) := by
  have receiverAt := (Option.some_get
    (x := distributedImage1.ownedSourceFacts? distributedChoreo 0 0) (by decide)).symm
  have atomsAt := (Option.some_get (x := distributedImage0.decodeAtoms?) (by decide)).symm
  have receiverColumns := decoded_owned_source_facts_bind_wire_columns receiverAt
  have matching : distributedEntry6.sample.effIndex? =
      some distributedPreparedReceivedSourceFacts.effIndex :=
    distributedFreshPreparedSourceIdentity.trans receiverColumns.1
  exact a_matched_owned_receive_recovers_its_accepted_sender
    (sender := distributedEntry6.sample) (receiverImage := distributedImage1)
    (session := 9062) (choreo := distributedChoreo)
    (pathIndex := 0) (event := 0) (receivedFacts := distributedPreparedReceivedSourceFacts)
    (atoms := (distributedImage0.decodeAtoms?).get (by decide))
    (wire := { distributedEntry6.sample.wire with payload := bytes })
    (frame := ((distributedCut13.carriers distributedChannel8).frames[2]?).get (by decide))
    (by decide) receiverAt atomsAt (by decide) matching (by decide) (by decide)
    (by unfold OwnedWireObservation.identifies; decide)
    (by dsimp only [OwnedWireObservation.identifies]; decide)
    (by dsimp only; decide) (by dsimp only; decide)

/-- The new receive has not been performed. Its pure source preparation and
actual accepted FIFO head determine the exact owned-wire preflight for ANY
proposed bytes. The accepted sender's owned kind is derived, not assumed.
No after-view or current receive-pair check supplies the result. -/
theorem distributedFreshOwnedReceiveChecksActualAcceptedBytesBeforeCommit
    (bytes : List Nat) :
    distributedPreparedReceivedSourceFacts.wireMatches 1
        { distributedEntry6.sample.wire with payload := bytes } 9062 = true ↔
      bytes = distributedEntry6.sample.wire.payload := by
  obtain ⟨_, _, _, _, checked⟩ := distributedFreshOwnedSenderAndWire bytes
  exact checked

theorem distributedFreshOwnedHeadKindComesFromItsPreparedSource :
    ∃ sentFacts, distributedEntry6.sample.ownedFacts? distributedChoreo = some sentFacts ∧
      sentFacts.resource = distributedPreparedReceivedSourceFacts.resource ∧ sentFacts.side = .send := by
  obtain ⟨facts, decoded, resource, sending, _⟩ := distributedFreshOwnedSenderAndWire []
  exact ⟨facts, decoded, resource, sending⟩
