; Logical receive identity and physical causal evidence are different facts.
(set-logic QF_LIA)

; Merging path failures preserves acceptance and the first rejected position.
(push)
(declare-const left_failed Bool)
(declare-const right_failed Bool)
(declare-const left_event Int)
(declare-const right_event Int)
(assert (and (<= 0 left_event) (<= 0 right_event)))
(define-fun failed () Bool (or left_failed right_failed))
(define-fun first_event () Int
  (ite left_failed
    (ite right_failed (ite (< left_event right_event) left_event right_event) left_event)
    right_event))
(assert (or
  (distinct (not failed) (and (not left_failed) (not right_failed)))
  (and failed
    (or (not (or (and left_failed (= first_event left_event))
                 (and right_failed (= first_event right_event))))
        (and left_failed (> first_event left_event))
        (and right_failed (> first_event right_event))))))
; EXPECT: unsat
(check-sat)
(pop)

; Taking only the left path loses an earlier rejection on the right.
(push)
(declare-const left_event Int)
(declare-const right_event Int)
(assert (and (<= 0 right_event) (< right_event left_event)))
(assert (> left_event right_event))
; EXPECT: sat
(check-sat)
(pop)

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

; A known receiver cannot gain another fact, regardless of sender knowledge.
(push)
(declare-const sender-known Bool)
(declare-const receiver-known Bool)
(assert receiver-known)
(assert (distinct (or receiver-known sender-known) receiver-known))
; EXPECT: unsat
(check-sat)
(pop)

; Hoisting the cutoff into source coordinates preserves occurrence order.
(push)
(declare-const source-position Int)
(declare-const body-start Int)
(declare-const iteration-start Int)
(declare-const cutoff Int)
(assert (and (<= 0 body-start) (<= body-start source-position)
             (<= 0 iteration-start) (<= iteration-start cutoff)))
(assert (distinct (< (+ iteration-start (- source-position body-start)) cutoff)
                  (< source-position (+ body-start (- cutoff iteration-start)))))
; EXPECT: unsat
(check-sat)
(pop)

; The source-order lookup can ignore all markers outside (before, after].
(declare-const scope-start Int)
(declare-const scope-split Int)
(declare-const scope-stop Int)
(declare-const before-event Int)
(declare-const after-event Int)
(define-fun split-between () Bool
  (and (< before-event scope-split) (<= scope-split after-event)))
(define-fun start-between () Bool
  (and (< before-event scope-start) (<= scope-start after-event)))
(define-fun stop-between () Bool
  (and (< before-event scope-stop) (<= scope-stop after-event)))
(define-fun scope-separates () Bool
  (and (<= scope-start before-event) split-between (< after-event scope-stop)))
(push)
(assert (and (<= 0 scope-start) (< scope-start scope-split)
             (< scope-split scope-stop) (<= 0 before-event) (< before-event after-event)))
(assert (distinct scope-separates (and split-between (not start-between) (not stop-between))))
; EXPECT: unsat
(check-sat)
(pop)

; Omitting either enclosing boundary can falsely remove a sequential edge.
(push)
(assert (and (<= 0 scope-start) (< scope-start scope-split)
             (< scope-split scope-stop) (<= 0 before-event) (< before-event after-event)))
(assert (and split-between (not stop-between) (not scope-separates)))
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (and (<= 0 scope-start) (< scope-start scope-split)
             (< scope-split scope-stop) (<= 0 before-event) (< before-event after-event)))
(assert (and split-between (not start-between) (not scope-separates)))
; EXPECT: sat
(check-sat)
(pop)

; Entered or closed scopes rule out separating splits at later preorder positions.
(push)
(declare-const later-start Int)
(declare-const later-split Int)
(declare-const later-stop Int)
(assert (and (<= 0 scope-start) (< scope-start scope-stop)
             (<= scope-start later-start) (< later-start later-split)
             (< later-split later-stop) (<= 0 before-event) (< before-event after-event)))
(assert (=> (< later-start scope-stop) (<= later-stop scope-stop)))
(assert (or (< before-event scope-start)
            (and (< before-event scope-stop) (<= scope-stop after-event))))
(assert (and (<= later-start before-event) (< before-event later-split)
             (<= later-split after-event) (< after-event later-stop)))
; EXPECT: unsat
(check-sat)
(pop)

; Crossing intervals would invalidate preorder elision; nesting is essential.
(push)
(declare-const crossing-start Int)
(declare-const crossing-split Int)
(declare-const crossing-stop Int)
(assert (and (<= 0 scope-start) (<= scope-start crossing-start)
             (<= crossing-start before-event) (< before-event scope-stop)
             (<= scope-stop after-event) (< before-event crossing-split)
             (<= crossing-split after-event) (< after-event crossing-stop)))
; EXPECT: sat
(check-sat)
(pop)

; The last different sender bounds all obligations; keep every target before it.
(push)
(declare-const first-query Int)
(declare-const stop-query Int)
(declare-const cutoff-query Int)
(declare-const target-query Int)
(declare-const changed-sender Bool)
(declare-const causal-witness Bool)
(assert (and (<= 0 first-query) (<= first-query cutoff-query)
             (<= cutoff-query stop-query) (<= 0 target-query)))
(define-fun full-target () Bool
  (and (<= first-query target-query) (< target-query stop-query) changed-sender))
(define-fun prefix-target () Bool
  (and (<= first-query target-query) (< target-query cutoff-query) changed-sender))
(assert (=> full-target (< target-query cutoff-query)))
(assert (distinct (=> full-target causal-witness) (=> prefix-target causal-witness)))
; EXPECT: unsat
(check-sat)
(pop)

; Removing the last sender change itself would discard a real obligation.
(push)
(declare-const last-change Int)
(declare-const source-end Int)
(declare-const last-witness Bool)
(assert (and (<= 0 last-change) (< last-change source-end)))
(assert (not last-witness))
(assert (not (=> (< last-change source-end) last-witness)))
(assert (=> (< last-change last-change) last-witness))
; EXPECT: sat
(check-sat)
(pop)
