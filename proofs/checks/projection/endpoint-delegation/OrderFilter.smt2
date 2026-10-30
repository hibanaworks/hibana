(set-logic QF_BV)
(declare-const expected (_ BitVec 64))
(declare-const actual (_ BitVec 64))
(declare-const possible (_ BitVec 64))
(declare-const certain (_ BitVec 64))
(declare-const excluded (_ BitVec 64))
(define-fun pending () (_ BitVec 64)
  (bvand (bvor (bvand expected (bvnot certain))
               (bvand (bvnot expected) possible)) (bvnot excluded)))
(assert (= (bvand certain (bvnot actual)) #x0000000000000000))
(assert (= (bvand actual (bvnot possible)) #x0000000000000000))

; No compatible, differing answer can be omitted from the word comparison.
(push)
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot pending) (bvnot excluded))) #x0000000000000000)))
; EXPECT: unsat
(check-sat)
(pop)

; A four-row ordered source cut. Covers suffice for all positive obligations
; only when every extra actual predecessor is rejected. ActorProgress.lean
; proves this implication for arbitrary finite backward source orders.
(declare-const s01 Bool)
(declare-const s02 Bool)
(declare-const s03 Bool)
(declare-const s12 Bool)
(declare-const s13 Bool)
(declare-const s23 Bool)
(declare-const a0 Bool)
(declare-const a1 Bool)
(declare-const a2 Bool)
(declare-const k0 Bool)
(declare-const k1 Bool)
(declare-const k2 Bool)
(define-fun transitive-source () Bool (and
  (=> (and s01 s12) s02) (=> (and s01 s13) s03)
  (=> (and s02 s23) s03) (=> (and s12 s23) s13)))
(define-fun cover0 () Bool (and s03 (not (and s01 s13)) (not (and s02 s23))))
(define-fun cover1 () Bool (and s13 (not (and s12 s23))))
(define-fun covers-complete () Bool (and (=> cover0 k0) (=> cover1 k1) (=> s23 k2)))
(define-fun no-extra () Bool (and (=> a0 s03) (=> a1 s13) (=> a2 s23)))
(define-fun covered0 () Bool (or a0 (and a1 s01) (and a2 s02)))
(define-fun covered1 () Bool (or a1 (and a2 s12)))
(define-fun candidate-check () Bool (and
  (=> (and k0 s03) covered0) (=> (and k1 s13) covered1) (=> (and k2 s23) a2)))
(define-fun full-coverage () Bool (and no-extra
  (=> s03 covered0) (=> s13 covered1) (=> s23 a2)))
(push)
(assert (and transitive-source covers-complete no-extra candidate-check))
(assert (not full-coverage))
; EXPECT: unsat
(check-sat)
(pop)

; Sensitivity: omitting an actual cover loses a required predecessor.
(push)
(assert (and transitive-source no-extra candidate-check (not covers-complete)))
(assert (not full-coverage))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: a spurious intermediary can mask a missing direct cover.
(push)
(assert (and transitive-source covers-complete candidate-check))
(assert (and cover0 (not a0) (not no-extra)))
; EXPECT: sat
(check-sat)
(pop)

; Source-required target choices can settle a conditional dependency.
(declare-const target-present Bool)
(declare-const choice-known Bool)
(declare-const required-choice Bool)
(declare-const guard Bool)
(define-fun target-possible () Bool (or (not choice-known) required-choice))
(define-fun target-certain () Bool (and choice-known required-choice))
(push)
(assert target-present)
(assert (=> (and target-present choice-known) (= guard required-choice)))
(assert (or (and target-certain (not guard)) (and guard (not target-possible))))
; EXPECT: unsat
(check-sat)
(pop)

; Sensitivity: treating an unknown source choice as absent loses an edge.
(push)
(assert (and target-present (not choice-known) guard))
(assert (not (and choice-known required-choice)))
; EXPECT: sat
(check-sat)
(pop)

; A conditional pair is relevant exactly when both source events occur.
(declare-const first-present Bool)
(declare-const second-present Bool)
(declare-const selected Bool)
(declare-const valid Bool)
(push)
(assert (= selected (and first-present second-present)))
(assert (xor (and first-present second-present (not valid))
             (and selected (not valid))))
; EXPECT: unsat
(check-sat)
(pop)

; Sensitivity: dropping a common path can hide an invalid ordering.
(push)
(assert (and first-present second-present (not valid) (not selected)))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: including an inactive pair can reject a valid source.
(push)
(assert (and selected (not (and first-present second-present)) (not valid)))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: failing to inspect a missing expected edge is unsound.
(push)
(define-fun missing () (_ BitVec 64) (bvand (bvand (bvnot expected) possible) (bvnot excluded)))
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot missing) (bvnot excluded))) #x0000000000000000)))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: failing to inspect an unwanted actual edge is unsound.
(push)
(define-fun extra () (_ BitVec 64) (bvand (bvand expected (bvnot certain)) (bvnot excluded)))
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot extra) (bvnot excluded))) #x0000000000000000)))
; EXPECT: sat
(check-sat)
(pop)

; First-row source choices refine the target's interval bounds.
(declare-const inside Bool)
(declare-const same-lane Bool)
(declare-const lower-bound Bool)
(declare-const upper-bound Bool)
(declare-const first-known-true Bool)
(declare-const first-known-false Bool)
(define-fun direct-actual () Bool (or same-lane (and inside guard)))
(define-fun refined-possible () Bool
  (or same-lane (and inside upper-bound (not first-known-false))))
(define-fun refined-certain () Bool
  (or same-lane (and inside lower-bound) (and inside upper-bound first-known-true)))
(push)
(assert (=> lower-bound guard))
(assert (=> guard upper-bound))
(assert (=> first-known-true guard))
(assert (=> first-known-false (not guard)))
(assert (or (and refined-certain (not direct-actual))
            (and direct-actual (not refined-possible))))
; EXPECT: unsat
(check-sat)
(pop)

; Sensitivity: a known true guard alone must not extend its interval.
(push)
(assert (and (not inside) (not same-lane) guard upper-bound first-known-true))
(assert (and (or same-lane (and upper-bound first-known-true)) (not direct-actual)))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: an inactive guard must not remove same-lane order.
(push)
(assert (and same-lane first-known-false))
(assert (and direct-actual (not (and (or same-lane upper-bound) (not first-known-false)))))
; EXPECT: sat
(check-sat)
(pop)
