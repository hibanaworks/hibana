; Read-only source frontier, not a runtime owner/permission table.
; Two possible pending receipts suffice to expose the first-candidate defect.
(set-logic QF_UF)
(declare-const pending-a Bool)
(declare-const pending-b Bool)
(declare-const borrowed-a Bool)
(declare-const borrowed-b Bool)
(declare-const returns-a Bool)
(declare-const returns-b Bool)
(declare-const before-ab Bool)
(declare-const before-ba Bool)
(assert (not (and before-ab before-ba)))
(define-fun covers-a () Bool (and pending-b returns-b before-ba))
(define-fun covers-b () Bool (and pending-a returns-a before-ab))
(define-fun wait () Bool
  (and (or pending-a pending-b)
    (=> (and pending-a borrowed-a) covers-a)
    (=> (and pending-b borrowed-b) covers-b)))

; An independent alternative must not hide an uncovered self-receipt.
(push)
(assert wait)
(assert pending-a)
(assert (not borrowed-a))
(assert pending-b)
(assert borrowed-b)
(assert (not before-ab))
; EXPECT: unsat
(check-sat)
(pop)

; The previous first-candidate rule can accept exactly that self-wait.
(push)
(assert pending-a)
(assert (not borrowed-a))
(assert pending-b)
(assert borrowed-b)
(assert (not before-ab))
(assert (not wait))
; EXPECT: sat
(check-sat)
(pop)

; A real earlier return restores the resource before the later self-receipt.
(push)
(assert pending-a)
(assert returns-a)
(assert (not borrowed-a))
(assert before-ab)
(assert pending-b)
(assert borrowed-b)
(assert wait)
; EXPECT: sat
(check-sat)
(pop)

; Merely being earlier on the source is insufficient if it returns elsewhere.
(push)
(assert wait)
(assert pending-a)
(assert (not returns-a))
(assert before-ab)
(assert pending-b)
(assert borrowed-b)
; EXPECT: unsat
(check-sat)
(pop)
