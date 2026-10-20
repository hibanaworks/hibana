; A single resource's source-derived receipt-to-target interval. Lean derives
; the causal partition and source prefix, rather than ordering unrelated par
; events by their numeric IDs. These are proof observations, not runtime flags.
(set-logic QF_UF)
(declare-const before-receipt Bool)
(declare-const same-receipt Bool)
(declare-const same-target Bool)
(declare-const after-target Bool)
(declare-const transfer-first-done Bool)
(declare-const transfer-cut-done Bool)
(declare-const target-cut-done Bool)
(declare-const target-final-done Bool)
(define-fun partition () Bool
  (or before-receipt same-receipt same-target after-target))
(define-fun receipt-prefix () Bool
  (=> (or before-receipt same-receipt) transfer-first-done))
(define-fun measured-prefix () Bool (=> transfer-first-done transfer-cut-done))
(define-fun measured-suffix () Bool (=> target-cut-done target-final-done))
(define-fun transfer-ready () Bool
  (and (not transfer-cut-done) (=> after-target target-cut-done)))

; No distinct handoff can accept while the target is still pending at the
; final cut. The target is not assumed ready throughout the interval.
(push)
(assert partition)
(assert receipt-prefix)
(assert measured-prefix)
(assert measured-suffix)
(assert (not same-target))
(assert (not target-final-done))
(assert transfer-ready)
; EXPECT: unsat
(check-sat)
(pop)

; The intended handoff itself remains permitted progress.
(push)
(assert partition)
(assert receipt-prefix)
(assert measured-prefix)
(assert measured-suffix)
(assert same-target)
(assert (not before-receipt))
(assert (not same-receipt))
(assert (not after-target))
(assert (not target-final-done))
(assert transfer-ready)
; EXPECT: sat
(check-sat)
(pop)

; A target can still lack another predecessor while independent par work
; progresses. Excluding competing handoffs does not serialize that work.
(push)
(declare-const target-other-parent-done Bool)
(declare-const independent-ready Bool)
(assert partition)
(assert receipt-prefix)
(assert measured-prefix)
(assert measured-suffix)
(assert before-receipt)
(assert (not same-target))
(assert (not target-final-done))
(assert (not target-other-parent-done))
(assert independent-ready)
(assert (not transfer-ready))
; EXPECT: sat
(check-sat)
(pop)

; Prefix closure at the actual selected receipt is necessary. Omitting it
; permits a countermodel; this model does not establish route correspondence.
(push)
(assert partition)
(assert measured-prefix)
(assert measured-suffix)
(assert before-receipt)
(assert (not same-receipt))
(assert (not same-target))
(assert (not after-target))
(assert (not transfer-first-done))
(assert (not target-final-done))
(assert transfer-ready)
; EXPECT: sat
(check-sat)
(pop)

; Likewise, a silently reset suffix can hide target completion. The measured
; no-unrecorded-reset history, not equal endpoint snapshots, excludes this.
(push)
(assert partition)
(assert receipt-prefix)
(assert measured-prefix)
(assert (not before-receipt))
(assert (not same-receipt))
(assert (not same-target))
(assert after-target)
(assert target-cut-done)
(assert (not target-final-done))
(assert transfer-ready)
; EXPECT: sat
(check-sat)
(pop)
