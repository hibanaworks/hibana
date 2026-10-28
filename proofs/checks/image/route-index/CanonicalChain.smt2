(set-logic QF_UFLIA)
(declare-fun parent (Int) Int)
(declare-const a Int)
(declare-const b Int)
(declare-const c Int)
(declare-const fresh0 Bool)
(declare-const fresh1 Bool)
(declare-const fresh2 Bool)
(declare-const same0 Bool)
(declare-const same1 Bool)
(declare-const same2 Bool)

(assert (and (>= a 0) (>= b 0) (>= c 0)))
(define-fun canonical () Bool
  (and (= (parent a) (- 1)) (= (parent b) a) (= (parent c) b)))
(define-fun ancestor ((root Int) (node Int)) Bool
  (or (= root node) (= root (parent node))
      (= root (parent (parent node)))))

; -1 means no new visit, -2 means rejection, and 0..2 name its first row.
(define-fun full-step ((start Int) (index Int) (fresh Bool)
                      (same Bool) (descendant Bool)) Int
  (ite (= start (- 2)) (- 2)
    (ite (>= start 0) (ite descendant start (- 2))
      (ite fresh index (ite same (- 1) (- 2))))))
(define-fun full0 () Int (full-step (- 1) 0 fresh0 same0 true))
(define-fun full1 () Int
  (full-step full0 1 fresh1 same1 (ancestor a b)))
(define-fun full () Int
  (full-step full1 2 fresh2 same2
    (ite (= full1 0) (ancestor a c) (ancestor b c))))
(define-fun short () Int
  (ite fresh0 0 (ite (not same0) (- 2)
    (ite fresh1 1 (ite (not same1) (- 2)
      (ite fresh2 2 (ite same2 (- 1) (- 2))))))))

; A complete chain cannot repeat a scope, regardless of its numeric labels.
(push)
(assert canonical)
(assert (or (= a b) (= b c) (= a c)))
; EXPECT: unsat
(check-sat)
(pop)

; All freshness and retained-choice assignments produce the same result.
(push)
(assert canonical)
(assert (not (= full short)))
; EXPECT: unsat
(check-sat)
(pop)

; Without the complete-chain check, an unrelated suffix would be accepted.
(push)
(assert (= (parent a) (- 1)))
(assert (= (parent b) a))
(assert (= (parent c) (- 1)))
(assert (distinct a b c))
(assert fresh0)
(assert (not (= full short)))
; EXPECT: sat
(check-sat)
(pop)

; A mismatched retained ancestor still rejects before a later fresh child.
(push)
(assert canonical)
(assert (not fresh0))
(assert same0)
(assert (not fresh1))
(assert (not same1))
(assert (not (= short (- 2))))
; EXPECT: unsat
(check-sat)
(pop)

; A fresh outer visit owns the suffix even if an old child chose another arm.
(push)
(assert canonical)
(assert fresh0)
(assert (not same1))
(assert (not same2))
(assert (or (not (= full 0)) (not (= short 0))))
; EXPECT: unsat
(check-sat)
(pop)
