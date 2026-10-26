; Choice-arm remapping. All constraints are on finite wire lane values.
(set-logic QF_UFBV)
(declare-const inside-a Bool)
(declare-const inside-b Bool)
(declare-const lane-a (_ BitVec 8))
(declare-const lane-b (_ BitVec 8))
(declare-fun remap ((_ BitVec 8)) (_ BitVec 8))
(define-fun aligned-a () (_ BitVec 8) (ite inside-a (remap lane-a) lane-a))
(define-fun aligned-b () (_ BitVec 8) (ite inside-b (remap lane-b) lane-b))
; Incompatible pairs within the arm use distinct matched classes.
(define-fun injective () Bool
  (=> (and inside-a inside-b (= (remap lane-a) (remap lane-b))) (= lane-a lane-b)))
; Each incompatible cross-boundary pair forbids its outside color.
(define-fun cross () Bool
  (and (=> (and inside-a (not inside-b)) (distinct (remap lane-a) lane-b))
       (=> (and inside-b (not inside-a)) (distinct (remap lane-b) lane-a))))
(push)
(assert injective)
(assert (distinct lane-a lane-b))
(assert (= aligned-a aligned-b))
; An injective arm map alone does not prevent outside collisions.
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert injective)
(assert cross)
(assert (distinct lane-a lane-b))
(assert (= aligned-a aligned-b))
; No new incompatible pair shares a lane after a valid matching.
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert inside-a)
(assert (not inside-b))
(assert cross)
(assert (= aligned-a aligned-b))
; A selected cross-boundary witness is separated even if it collided before.
; EXPECT: unsat
(check-sat)
(pop)
(push)
(declare-fun compatible ((_ BitVec 8) (_ BitVec 8)) Bool)
(declare-fun allowed ((_ BitVec 8) (_ BitVec 8)) Bool)
(declare-const rejected-row (_ BitVec 8))
(declare-const rejected-color (_ BitVec 8))
(declare-const candidate-row (_ BitVec 8))
(assert (not (compatible rejected-row rejected-color)))
(assert (compatible candidate-row (remap candidate-row)))
(assert (allowed candidate-row (remap candidate-row)))
(assert (not (and (allowed candidate-row (remap candidate-row))
  (not (and (= candidate-row rejected-row) (= (remap candidate-row) rejected-color))))))
; Removing an incompatible edge preserves every valid selected edge.
; EXPECT: unsat
(check-sat)
(pop)
(exit)
