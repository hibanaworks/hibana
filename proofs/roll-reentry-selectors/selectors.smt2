; EXPECT: unsat
; EXPECT: unsat
; EXPECT: sat
; Check the public send selector independently of peer and lane.
(set-logic QF_BV)
(declare-const role_a (_ BitVec 8))
(declare-const role_b (_ BitVec 8))
(declare-const label_a (_ BitVec 8))
(declare-const label_b (_ BitVec 8))
(declare-const schema_a (_ BitVec 32))
(declare-const schema_b (_ BitVec 32))
(define-fun key_a () (_ BitVec 64) (concat #x00 role_a label_a #x00 schema_a))
(define-fun key_b () (_ BitVec 64) (concat #x00 role_b label_b #x00 schema_b))
(push)
(assert (= key_a key_b))
(assert (or (distinct role_a role_b) (distinct label_a label_b) (distinct schema_a schema_b)))
(check-sat) ; unsat: no packed-key collision.
(pop)
(push)
(assert (and (= role_a role_b) (= label_a label_b) (= schema_a schema_b)))
(assert (distinct key_a key_b))
(check-sat) ; unsat: equal public contracts cannot encode a different occurrence.
(pop)
(push)
(assert (and (= role_a role_b) (distinct label_a label_b)))
(assert (distinct key_a key_b))
(check-sat) ; sat: distinct labels can keep independent heads distinguishable.
(pop)
