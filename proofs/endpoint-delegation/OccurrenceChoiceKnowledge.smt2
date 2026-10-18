; Finite boundary model for committed choice observations and one accepted
; occurrence. A quiet gap differs from an explicit fresh reset/commit.
; Whole-path bits outside this occurrence are not participant knowledge.
(set-logic ALL)
(declare-datatypes () ((Choice Unknown Left Right)))
(declare-const progress-before (_ BitVec 8))
(declare-const progress-after (_ BitVec 8))
(declare-const writes-before Int)
(declare-const writes-after Int)
(declare-const inner-before Choice)
(declare-const inner-after Choice)
(declare-const outer-before Choice)
(declare-const outer-after Choice)
(declare-const sibling-before Choice)
(declare-const sibling-after Choice)
(declare-const frame-arm Choice)
(declare-const sender-unused-path Bool)
(declare-const receiver-unused-path Bool)
(define-fun old-quiet () Bool
  (and (= progress-before progress-after) (= writes-before writes-after)))
(define-fun quiet () Bool
  (and old-quiet (= inner-before inner-after) (= outer-before outer-after)
       (= sibling-before sibling-after)))
(define-fun publication () Bool
  (and (not (= frame-arm Unknown))
       (or (= inner-before Unknown) (= inner-before frame-arm))
       (= inner-after frame-arm) (= outer-after outer-before)
       (= sibling-after sibling-before)))

; The old observation rule admits invented knowledge without a write.
(push)
(assert old-quiet)
(assert (= sibling-before Unknown))
(assert (= sibling-after Right))
; EXPECT: sat
(check-sat)
(pop)

; The corrected quiet rule forbids changing any committed branch slot.
(push)
(assert quiet)
(assert (or (not (= inner-before inner-after))
            (not (= outer-before outer-after))
            (not (= sibling-before sibling-after))))
; EXPECT: unsat
(check-sat)
(pop)

; A source-checked reset/commit remains possible; it is not a quiet gap.
(push)
(assert (= writes-after (+ writes-before 5)))
(assert (= outer-before Left))
(assert (= outer-after Left))
(assert (= inner-before Left))
(assert (= inner-after Right))
(assert (= sibling-before Unknown))
(assert (= sibling-after Unknown))
; EXPECT: sat
(check-sat)
(pop)

; One accepted occurrence cannot publish the opposite inner arm.
(push)
(assert publication)
(assert (not (= inner-after frame-arm)))
; EXPECT: unsat
(check-sat)
(pop)

; Publication does not give knowledge of an unrelated sibling.
(push)
(assert publication)
(assert (= sibling-before Unknown))
(assert (not (= sibling-after Unknown)))
; EXPECT: unsat
(check-sat)
(pop)

; Participants may use different full witnesses for an unknown sibling.
(push)
(assert publication)
(assert (= inner-before Unknown))
(assert (= frame-arm Left))
(assert (= sibling-before Unknown))
(assert (not (= sender-unused-path receiver-unused-path)))
; EXPECT: sat
(check-sat)
(pop)
