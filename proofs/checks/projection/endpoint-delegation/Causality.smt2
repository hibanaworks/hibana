; Logical receive identity and physical causal evidence are different facts.
(set-logic QF_LIA)
(push)
(declare-const range_start Int)
(declare-const range_stop Int)
(declare-const occurrence Int)
(declare-const cutoff Int)
(assert (<= cutoff range_start))
(assert (and (<= range_start occurrence) (< occurrence range_stop)))
(assert (< occurrence cutoff))
; A subtree starting at the cutoff contains no queried occurrence.
; EXPECT: unsat
(check-sat)
(pop)

(push)
(declare-const range_start Int)
(declare-const range_stop Int)
(declare-const occurrence Int)
(declare-const cutoff Int)
(assert (and (<= 0 range_start) (< range_start cutoff) (<= cutoff range_stop)))
(assert (and (<= range_start occurrence) (< occurrence range_stop) (< occurrence cutoff)))
; Using the end instead of the start would skip an enabled prefix.
; EXPECT: sat
(check-sat)
(pop)

(push)
(declare-const start Int)
(declare-const following Int)
(declare-const stop Int)
(declare-const earlier Int)
(declare-const later_receive Int)
(assert (<= start following))
(assert (or (< earlier start) (< later_receive start)))
(assert (and (<= following earlier) (< earlier stop)
             (<= following later_receive) (< later_receive stop)))
; Ordered marker suffixes after either receive contain no enclosing roll.
; EXPECT: unsat
(check-sat)
(pop)
(declare-const receiver Int)
(declare-const first Int)
(declare-const second Int)
(declare-const relay_sender Int)
(declare-const later Int)
(declare-const left Bool)
(assert (and (<= 0 receiver) (<= receiver 255)
  (<= 0 first) (<= first 255) (<= 0 second) (<= second 255)
  (<= 0 relay_sender) (<= relay_sender 255) (<= 0 later) (<= later 255)))

; One relay extends exactly the seed actor and its reachable receiver.
(push)
(define-fun linear_known () Bool
  (or (= later receiver) (and (= relay_sender receiver) (= later first))))
(assert linear_known)
(assert (distinct later receiver))
(assert (or (distinct relay_sender receiver) (distinct later first)))
; EXPECT: unsat
(check-sat)
(pop)

; Selecting an arm selects both its relay and its holder, so every arm is safe.
(push)
(define-fun target () Int (ite left first second))
(define-fun taught () Int (ite left first second))
(assert (and (distinct target receiver) (distinct target taught)))
; EXPECT: unsat
(check-sat)
(pop)

; Parallel arms start with only receiver known. Joining can teach first, or
; second if its own relay was enabled from that shared input.
(push)
(define-fun joined_second () Bool
  (or (= second receiver) (= second first) (= first receiver)))
(assert joined_second)
(assert (distinct receiver first second))
; EXPECT: unsat
(check-sat)
(pop)

; Matching physical senders never establishes that the earlier logical
; receive completed. There is no reverse edge merely from sending twice.
(push)
(assert (= first second))
(assert (distinct first receiver))
(assert (= later first))
(assert (distinct later receiver))
; EXPECT: sat
(check-sat)
(pop)

; Wrong rule: a candidate-holder union authorizes the wrong actual holder.
(push)
(assert (distinct receiver first second))
(assert left)
(define-fun actual_holder () Int (ite left second first))
(assert (or (= actual_holder first) (= actual_holder second)))
(assert (distinct actual_holder (ite left first second)))
; EXPECT: sat
(check-sat)
(pop)

; Wrong rule: a relay in one parallel arm enables its sibling's relay.
(push)
(assert (distinct receiver first second))
(define-fun left_knows_first () Bool
  (or (= first receiver) (= receiver receiver)))
(define-fun wrong_sequential_second () Bool
  (or (= second receiver) left_knows_first))
(define-fun correct_parallel_second () Bool
  (or (= second receiver) (= second first) (= first receiver)))
(assert wrong_sequential_second)
(assert (not correct_parallel_second))
; EXPECT: sat
(check-sat)
(pop)

; Inner choices may differ between visits. Equating them would discard this
; legal schedule rather than proving its receive ordering.
(push)
(declare-const next_left Bool)
(assert (distinct left next_left))
; EXPECT: sat
(check-sat)
(pop)
