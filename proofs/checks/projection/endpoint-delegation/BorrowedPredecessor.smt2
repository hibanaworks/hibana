; Source dependency coverage and a future's borrowed prerequisite.
; These arrays interpret immutable guards and existing completion, not another
; runtime progress/ownership store. This abstraction is not Rust refinement.
(set-logic AUFBV)
(declare-const required (Array (_ BitVec 16) Bool))
(declare-const guards (Array (_ BitVec 16) Bool))
(declare-const cover (Array (_ BitVec 16) (_ BitVec 16)))
(declare-const done (Array (_ BitVec 16) Bool))
(declare-const usesBorrow (Array (_ BitVec 16) Bool))
(declare-fun sourceBefore ((_ BitVec 16) (_ BitVec 16)) Bool)
(declare-const prior (_ BitVec 16))
(declare-const middle (_ BitVec 16))
(define-fun coverage () Bool
  (and
    (forall ((row (_ BitVec 16)))
      (=> (select guards row) (select required row)))
    (forall ((row (_ BitVec 16)))
      (=> (select required row)
        (and (select guards (select cover row))
          (or (= (select cover row) row)
            (sourceBefore row (select cover row))))))))
(define-fun closed () Bool
  (forall ((row (_ BitVec 16)) (ancestor (_ BitVec 16)))
    (=> (and (select required row) (select done row) (sourceBefore ancestor row))
      (select done ancestor))))
(define-fun ready () Bool
  (forall ((row (_ BitVec 16))) (=> (select guards row) (select done row))))
(define-fun directBorrow () Bool
  (exists ((row (_ BitVec 16)))
    (and (select guards row) (not (select done row)) (select usesBorrow row))))

; The old direct-only classification can miss a borrowed transitive ancestor.
(push)
(assert (distinct prior middle))
(assert (= required (store (store ((as const (Array (_ BitVec 16) Bool)) false) prior true) middle true)))
(assert (= guards (store ((as const (Array (_ BitVec 16) Bool)) false) middle true)))
(assert (= cover ((as const (Array (_ BitVec 16) (_ BitVec 16))) middle)))
(assert (= done ((as const (Array (_ BitVec 16) Bool)) false)))
(assert (= usesBorrow (store ((as const (Array (_ BitVec 16) Bool)) false) prior true)))
(assert (forall ((left (_ BitVec 16)) (right (_ BitVec 16)))
  (= (sourceBefore left right) (and (= left prior) (= right middle)))))
(assert coverage)
(assert closed)
(assert (not directBorrow))
; EXPECT: sat
(check-sat)
(pop)

; A pending source prerequisite cannot be hidden by completed compact guards.
(push)
(assert coverage)
(assert closed)
(assert (select required prior))
(assert (not (select done prior)))
(assert (select usesBorrow prior))
(assert ready)
; EXPECT: unsat
(check-sat)
(pop)

; An unfinished independent sibling has no source dependency on the candidate.
(push)
(assert (= required ((as const (Array (_ BitVec 16) Bool)) false)))
(assert (= guards ((as const (Array (_ BitVec 16) Bool)) false)))
(assert (not (select done prior)))
(assert (select usesBorrow prior))
(assert coverage)
(assert closed)
(assert ready)
(assert (not directBorrow))
; EXPECT: sat
(check-sat)
(pop)
