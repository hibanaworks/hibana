; Algebra of the source-derived resource cut. These are proof observations,
; not a runtime owner table. Lean derives the causal alternative from source.
(set-logic QF_UF)
(declare-const use-done Bool)
(declare-const transfer-done Bool)
(declare-const use-before-transfer Bool)
(declare-const transfer-before-use Bool)
(declare-const same-occurrence Bool)
(define-fun use-ready () Bool
  (and (not use-done) (=> transfer-before-use transfer-done)))
(define-fun transfer-ready () Bool
  (and (not transfer-done) (=> use-before-transfer use-done)))

; A same-resource causal alternative excludes simultaneous readiness.
(push)
(assert (or use-before-transfer transfer-before-use))
(assert use-ready)
(assert transfer-ready)
; EXPECT: unsat
(check-sat)
(pop)

; At a ready owned handoff, a different source occurrence cannot also accept.
(push)
(assert (or same-occurrence use-before-transfer transfer-before-use))
(assert (not same-occurrence))
(assert use-ready)
(assert transfer-ready)
; EXPECT: unsat
(check-sat)
(pop)

; Accepting the intended source occurrence is permitted progress.
(push)
(assert same-occurrence)
(assert (not use-before-transfer))
(assert (not transfer-before-use))
(assert use-ready)
(assert transfer-ready)
; EXPECT: sat
(check-sat)
(pop)

; Genuinely independent work can remain ready concurrently.
(push)
(assert (not use-before-transfer))
(assert (not transfer-before-use))
(assert use-ready)
(assert transfer-ready)
; EXPECT: sat
(check-sat)
(pop)

; A checked live retirement needs source completion, including this use.
(push)
(declare-const source-complete Bool)
(declare-const retirement-live Bool)
(assert (=> source-complete use-done))
(assert (=> retirement-live source-complete))
(assert retirement-live)
(assert (not use-done))
; EXPECT: unsat
(check-sat)
(pop)

; Moving a value into an unpolled future is permitted. It is not a second
; usable value, and cannot authorize transport acceptance before its use.
(push)
(declare-const application-value Bool)
(declare-const private-future-value Bool)
(assert use-before-transfer)
(assert use-ready)
(assert (not application-value))
(assert private-future-value)
(assert (not transfer-ready))
; EXPECT: sat
(check-sat)
(pop)
