(set-logic QF_ABV)
(declare-const words (Array (_ BitVec 3) (_ BitVec 32)))
(declare-const lane (_ BitVec 8))
(declare-const query (_ BitVec 8))
(declare-const eligible Bool)
(define-fun word ((x (_ BitVec 8))) (_ BitVec 3) ((_ extract 7 5) x))
(define-fun bit ((x (_ BitVec 8))) (_ BitVec 32)
  (bvshl #x00000001 ((_ zero_extend 27) ((_ extract 4 0) x))))
(define-fun has ((w (Array (_ BitVec 3) (_ BitVec 32))) (x (_ BitVec 8))) Bool
  (not (= (bvand (select w (word x)) (bit x)) #x00000000)))
(define-fun next () (Array (_ BitVec 3) (_ BitVec 32))
  (ite eligible
    (store words (word lane) (bvor (select words (word lane)) (bit lane)))
    words))

; One insertion is exactly old membership OR this eligible occurrence.
(push)
(assert (not (= (has next query)
  (or (has words query) (and eligible (= lane query))))))
(check-sat)
(pop)

; A different lane never loses or gains membership through this insertion.
(push)
(assert (not (= lane query)))
(assert (not (= (has next query) (has words query))))
(check-sat)
(pop)

; An eligible lane is included, including wire lane 255.
(push)
(assert eligible)
(assert (not (has next lane)))
(check-sat)
(pop)

; Ineligible events do not change any word.
(push)
(assert (not eligible))
(assert (not (= next words)))
(check-sat)
(pop)

; A fresh all-zero census has no member without an eligible occurrence.
(push)
(assert (= words ((as const (Array (_ BitVec 3) (_ BitVec 32))) #x00000000)))
(assert (has next query))
(assert (not (and eligible (= lane query))))
(check-sat)
(pop)

; Removing eligibility creates a concrete spurious member: negative witness.
(push)
(assert (= words ((as const (Array (_ BitVec 3) (_ BitVec 32))) #x00000000)))
(assert (not eligible))
(assert (has (store words (word lane) (bit lane)) lane))
(check-sat)
(pop)
