
private def distributedPreparedReceivedSourceFacts :=
  (distributedImage1.ownedSourceFacts? distributedChoreo 0 0).get (by decide)

/-- The new receive has not been performed. Its pure source preparation and
actual accepted FIFO head determine the exact owned-wire preflight for ANY
proposed bytes, including malformed or changed bytes. No after-view or current
receive-pair check is used to establish the result. -/
theorem distributedFreshOwnedReceiveChecksActualAcceptedBytesBeforeCommit
    (bytes : List Nat) :
    distributedPreparedReceivedSourceFacts.wireMatches 1
        { distributedEntry6.sample.wire with payload := bytes } 9062 = true ↔
      bytes = distributedEntry6.sample.wire.payload := by
  have senderAt := (Option.some_get
    (x := distributedEntry6.sample.ownedFacts? distributedChoreo) (by decide)).symm
  have receiverAt := (Option.some_get
    (x := distributedImage1.ownedSourceFacts? distributedChoreo 0 0) (by decide)).symm
  have senderColumns := decoded_owned_source_facts_bind_wire_columns senderAt
  have receiverColumns := decoded_owned_source_facts_bind_wire_columns receiverAt
  have matching : distributedFreshSentFacts.effIndex = distributedPreparedReceivedSourceFacts.effIndex :=
    Option.some.inj (senderColumns.1.symm.trans
      (distributedFreshPreparedSourceIdentity.trans receiverColumns.1))
  have receiveIdentity : sourceOperationIdentity? distributedImage1 distributedChoreo 0 0 =
      some (distributedFreshSentFacts.effIndex, distributedFreshSentFacts.binding, .receive) := by
    rw [matching]
    decide
  obtain ⟨_, _, _, checked⟩ := a_source_matched_owned_head_has_its_exact_receive_wire_check
    (sender := distributedEntry6.sample) (receiverImage := distributedImage1)
    (session := 9062) (choreo := distributedChoreo)
    (pathIndex := 0) (event := 0) (source := distributedFreshSentFacts.effIndex)
    (binding := distributedFreshSentFacts.binding)
    (sentFacts := distributedFreshSentFacts) (receivedFacts := distributedPreparedReceivedSourceFacts)
    (wire := { distributedEntry6.sample.wire with payload := bytes })
    (frame := ((distributedCut13.carriers distributedChannel8).frames[2]?).get (by decide))
    (by decide) senderAt receiverAt (by decide) receiveIdentity (by decide)
    (by unfold OwnedWireObservation.identifies; decide)
    (by dsimp only [OwnedWireObservation.identifies]; decide)
    (by dsimp only; decide) (by dsimp only; decide)
  exact checked
