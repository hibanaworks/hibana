; Standalone SMT obligations; every SAT witness and UNSAT claim is replayed.
; Query 1: owner_none_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalOwner 0)) (((none) (some (span Int) (owner Int)))))
(declare-fun selected_owner () OptionalOwner)
(declare-fun explicit_owner () Int)
(assert
 (let (($x14 (and (>= (span selected_owner) 0) (>= (owner selected_owner) 0))))
 (let (($x8 ((_ is some ) selected_owner)))
 (=> $x8 $x14))))
(assert
 (or (and (= selected_owner none) (= explicit_owner 0)) (and ((_ is some ) selected_owner) (= explicit_owner (owner selected_owner)))))
(assert
 (= selected_owner none))
; EXPECT: sat
(check-sat)
; Query 2: owner_some_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalOwner 0)) (((none) (some (span Int) (owner Int)))))
(declare-fun selected_owner () OptionalOwner)
(declare-fun explicit_owner () Int)
(assert
 (let (($x14 (and (>= (span selected_owner) 0) (>= (owner selected_owner) 0))))
 (let (($x8 ((_ is some ) selected_owner)))
 (=> $x8 $x14))))
(assert
 (let (($x19 (and (= selected_owner none) (= explicit_owner 0))))
 (or $x19 (and ((_ is some ) selected_owner) (= explicit_owner (owner selected_owner))))))
(assert
 ((_ is some ) selected_owner))
(assert
 (let ((?x12 (owner selected_owner)))
(= ?x12 3)))
; EXPECT: sat
(check-sat)
; Query 3: owner_match_equivalence
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalOwner 0)) (((none) (some (span Int) (owner Int)))))
(declare-fun selected_owner () OptionalOwner)
(declare-fun explicit_owner () Int)
(assert
 (let (($x14 (and (>= (span selected_owner) 0) (>= (owner selected_owner) 0))))
 (let (($x8 ((_ is some ) selected_owner)))
 (=> $x8 $x14))))
(assert
 (or (and (= selected_owner none) (= explicit_owner 0)) (and ((_ is some ) selected_owner) (= explicit_owner (owner selected_owner)))))
(assert
 (let ((?x12 (owner selected_owner)))
(let (($x8 ((_ is some ) selected_owner)))
(let ((?x16 (ite $x8 ?x12 0)))
(and (distinct explicit_owner ?x16) true)))))
; EXPECT: unsat
(check-sat)
; Query 4: color_none_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalColor 0)) (((none) (some (value Int)))))
(declare-fun selected_color () OptionalColor)
(declare-fun explicit_color () Int)
(assert
 (let ((?x24 (value selected_color)))
 (let (($x39 (>= ?x24 0)))
 (let (($x40 ((_ is some ) selected_color)))
 (=> $x40 $x39)))))
(assert
 (or (and (= selected_color none) (= explicit_color 256)) (and ((_ is some ) selected_color) (= explicit_color (value selected_color)))))
(assert
 (= selected_color none))
; EXPECT: sat
(check-sat)
; Query 5: color_some_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalColor 0)) (((none) (some (value Int)))))
(declare-fun selected_color () OptionalColor)
(declare-fun explicit_color () Int)
(assert
 (let ((?x24 (value selected_color)))
 (let (($x39 (>= ?x24 0)))
 (let (($x40 ((_ is some ) selected_color)))
 (=> $x40 $x39)))))
(assert
 (let (($x14 (and (= selected_color none) (= explicit_color 256))))
 (or $x14 (and ((_ is some ) selected_color) (= explicit_color (value selected_color))))))
(assert
 ((_ is some ) selected_color))
(assert
 (let ((?x24 (value selected_color)))
(= ?x24 0)))
; EXPECT: sat
(check-sat)
; Query 6: color_match_equivalence
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalColor 0)) (((none) (some (value Int)))))
(declare-fun selected_color () OptionalColor)
(declare-fun explicit_color () Int)
(assert
 (let ((?x24 (value selected_color)))
 (let (($x39 (>= ?x24 0)))
 (let (($x40 ((_ is some ) selected_color)))
 (=> $x40 $x39)))))
(assert
 (or (and (= selected_color none) (= explicit_color 256)) (and ((_ is some ) selected_color) (= explicit_color (value selected_color)))))
(assert
 (let ((?x24 (value selected_color)))
(let (($x40 ((_ is some ) selected_color)))
(let ((?x11 (ite $x40 ?x24 256)))
(and (distinct explicit_color ?x11) true)))))
; EXPECT: unsat
(check-sat)
; Query 7: exhaustion_stays_invalid_256
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalColor 0)) (((none) (some (value Int)))))
(declare-fun selected_color () OptionalColor)
(declare-fun explicit_color () Int)
(assert
 (let ((?x24 (value selected_color)))
 (let (($x39 (>= ?x24 0)))
 (let (($x40 ((_ is some ) selected_color)))
 (=> $x40 $x39)))))
(assert
 (or (and (= selected_color none) (= explicit_color 256)) (and ((_ is some ) selected_color) (= explicit_color (value selected_color)))))
(assert
 (= selected_color none))
(assert
 (and (distinct explicit_color 256) true))
; EXPECT: unsat
(check-sat)
; Query 8: last_byte_255_retained
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-datatypes ((OptionalColor 0)) (((none) (some (value Int)))))
(declare-fun selected_color () OptionalColor)
(declare-fun explicit_color () Int)
(assert
 (let ((?x24 (value selected_color)))
 (let (($x39 (>= ?x24 0)))
 (let (($x40 ((_ is some ) selected_color)))
 (=> $x40 $x39)))))
(assert
 (or (and (= selected_color none) (= explicit_color 256)) (and ((_ is some ) selected_color) (= explicit_color (value selected_color)))))
(assert
 (let ((?x44 (some 255)))
 (= selected_color ?x44)))
(assert
 (= explicit_color 255))
; EXPECT: sat
(check-sat)
