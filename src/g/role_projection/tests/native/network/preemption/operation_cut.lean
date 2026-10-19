theorem an_actual_independent_call_can_start_during_the_pending_owned_send :
    asyncOperationCut0.permitsFreshCall .beginSend = true := by
  exact Hibana.an_idle_independent_borrow_can_begin_its_send
    async_operation_cut_0_matches (by decide) (by decide)

theorem both_pending_sender_and_payload_slots_remain_visible :
    asyncOperationCut0.slots.any (fun slot => slot.view == 0 && slot.phase == .send) = true ∧
      asyncOperationCut0.slots.any
        (fun slot => slot.view == 10 && slot.phase == .sendPayload) = true := by decide

theorem independent_publication_does_not_erase_the_other_pending_call :
    asyncOperationCut1.slots = asyncOperationCut0.slots := by decide

theorem omitting_a_whole_resource_view_cannot_hide_its_borrow :
    ({ asyncOperationCut0 with
      views := asyncOperationCut0.views.filter (· != 10),
      slots := asyncOperationCut0.slots.filter (fun slot => slot.view != 10) }).matches
        asyncOperationImage = false := by decide
