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
; Candidate lists use logical receivers, including every nonlocal event.
; Remapping lanes preserves those lists; actor ownership never selects them.
(declare-const sender-a (_ BitVec 8))
(declare-const receiver-a (_ BitVec 8))
(declare-const sender-b (_ BitVec 8))
(declare-const receiver-b (_ BitVec 8))
(define-fun indexed-candidate () Bool
  (and (distinct sender-b receiver-b) (= receiver-a receiver-b)))
(define-fun conflicting-candidate () Bool
  (and (distinct sender-a receiver-a) (distinct sender-b receiver-b)
       (= receiver-a receiver-b) (distinct sender-a sender-b)
       (= aligned-a aligned-b)))
(push)
(assert conflicting-candidate)
(assert (not indexed-candidate))
; Index lookup cannot omit a cross-boundary receive-order obligation.
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert indexed-candidate)
(assert (not conflicting-candidate))
; Index membership is only a candidate; causal and lane checks still apply.
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (= sender-b receiver-b))
(assert conflicting-candidate)
; Zero-byte local operations create no wire receive-order obligation.
; EXPECT: unsat
(check-sat)
(pop)
; A source occurrence inside a roll and an occurrence after its end need
; independent receive directions when their logical senders differ.
(declare-const roll-start (_ BitVec 16))
(declare-const roll-end (_ BitVec 16))
(declare-const event-a (_ BitVec 16))
(declare-const event-b (_ BitVec 16))
(define-fun reentry-crossing () Bool
  (and (bvule roll-start event-a) (bvult event-a roll-end)
       (bvule roll-end event-b)))
(define-fun receive-direction-needs-separation () Bool
  (and reentry-crossing (distinct sender-a receiver-a)
       (distinct sender-b receiver-b) (= receiver-a receiver-b)
       (distinct sender-a sender-b)))
(push)
(assert receive-direction-needs-separation)
(assert (= aligned-a aligned-b))
; Source order in one visit alone permits this erroneous coloring.
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (=> receive-direction-needs-separation (distinct aligned-a aligned-b)))
(assert receive-direction-needs-separation)
(assert (= aligned-a aligned-b))
; The reentry constraint excludes that coloring for the entire u16 domain.
; EXPECT: unsat
(check-sat)
(pop)
; Role sets are unchanged when intersection reads four 64-bit words.
(declare-const roles-left (_ BitVec 256))
(declare-const roles-right (_ BitVec 256))
(define-fun word-intersection-empty () Bool
  (and (= (bvand ((_ extract 63 0) roles-left) ((_ extract 63 0) roles-right)) (_ bv0 64))
       (= (bvand ((_ extract 127 64) roles-left) ((_ extract 127 64) roles-right)) (_ bv0 64))
       (= (bvand ((_ extract 191 128) roles-left) ((_ extract 191 128) roles-right)) (_ bv0 64))
       (= (bvand ((_ extract 255 192) roles-left) ((_ extract 255 192) roles-right)) (_ bv0 64))))
(push)
(assert (distinct word-intersection-empty (= (bvand roles-left roles-right) (_ bv0 256))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (= (bvand ((_ extract 191 0) roles-left) ((_ extract 191 0) roles-right)) (_ bv0 192)))
(assert (distinct (bvand roles-left roles-right) (_ bv0 256)))
; Dropping the final word loses an obligation in the full wire-role domain.
; EXPECT: sat
(check-sat)
(pop)
; A batch contains only one old lane class. Injectivity inside that selected
; set follows even when the candidate map is otherwise unrestricted.
(declare-const batch-lane (_ BitVec 8))
(push)
(assert (=> inside-a (= lane-a batch-lane)))
(assert (=> inside-b (= lane-b batch-lane)))
(assert (not injective))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert inside-a)
(assert inside-b)
(assert (= lane-a batch-lane))
(assert (not injective))
; Omitting the second membership check admits a collision between old classes.
; EXPECT: sat
(check-sat)
(pop)
; Compact vertices retain distinct active old lane classes.
(declare-const vertex-a (_ BitVec 8))
(declare-const vertex-b (_ BitVec 8))
(declare-fun vertex-color ((_ BitVec 8)) (_ BitVec 8))
(push)
(assert (=> (= vertex-a vertex-b) (= lane-a lane-b)))
(assert (=> (= (vertex-color vertex-a) (vertex-color vertex-b)) (= vertex-a vertex-b)))
(assert (distinct lane-a lane-b))
(assert (= (vertex-color vertex-a) (vertex-color vertex-b)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (=> (= (vertex-color vertex-a) (vertex-color vertex-b)) (= vertex-a vertex-b)))
(assert (distinct lane-a lane-b))
(assert (= (vertex-color vertex-a) (vertex-color vertex-b)))
; Omitting packing injectivity can collapse two old classes.
; EXPECT: sat
(check-sat)
(pop)
; Confirmed edges use the same packed row/bit address as the constraint table.
; A full wire-domain table has 65,536 bits. Ceil row width prevents aliasing
; even when the candidate palette ends in a partial byte.
(declare-const colors (_ BitVec 9))
(define-fun row-bytes () (_ BitVec 9)
  (bvlshr (bvadd colors (_ bv7 9)) (_ bv3 9)))
(define-fun packed-bit ((row (_ BitVec 8)) (color (_ BitVec 8)) (width (_ BitVec 9))) (_ BitVec 24)
  (bvadd (bvmul ((_ zero_extend 16) row)
    (bvmul ((_ zero_extend 15) width) (_ bv8 24))) ((_ zero_extend 16) color)))
(assert (bvugt colors (_ bv0 9)))
(assert (bvule colors (_ bv256 9)))
(assert (bvult ((_ zero_extend 1) lane-a) colors))
(assert (bvult ((_ zero_extend 1) lane-b) colors))
(push)
(assert (or (distinct vertex-a vertex-b) (distinct lane-a lane-b)))
(assert (= (packed-bit vertex-a lane-a row-bytes) (packed-bit vertex-b lane-b row-bytes)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (bvuge (packed-bit vertex-a lane-a row-bytes) (_ bv65536 24)))
; The complete wire row/color domain fits the compiler table.
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (or (distinct vertex-a vertex-b) (distinct lane-a lane-b)))
(assert (= (packed-bit vertex-a lane-a (bvlshr colors (_ bv3 9)))
           (packed-bit vertex-b lane-b (bvlshr colors (_ bv3 9)))))
; Truncating the partial row byte can certify a different edge by aliasing.
; EXPECT: sat
(check-sat)
(pop)
(exit)
