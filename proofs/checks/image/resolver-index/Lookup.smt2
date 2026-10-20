; Standalone SMT obligations; every SAT witness and UNSAT claim is replayed.
; Query 1: first-match-equivalence-0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(assert
 (let ((?x8 (ite and (- 1) (- 1))))
(and (distinct (- 1) ?x8) true)))
; EXPECT: unsat
(check-sat)
; Query 2: first-match-equivalence-1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_1_0 () Int)
(declare-fun query_1 () Int)
(assert
 (let (($x17 (>= scope_1_0 0)))
 (and $x17 (< scope_1_0 8192))))
(assert
 (let (($x20 (= scope_1_0 query_1)))
(let ((?x18 (ite $x20 0 (- 1))))
(let ((?x21 (ite (< scope_1_0 query_1) (- 1) (ite (< query_1 scope_1_0) (- 1) 0))))
(let ((?x16 (ite and ?x21 ?x18)))
(and (distinct ?x16 ?x18) true))))))
; EXPECT: unsat
(check-sat)
; Query 3: first-match-equivalence-2
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_2_0 () Int)
(declare-fun scope_2_1 () Int)
(declare-fun query_2 () Int)
(assert
 (let (($x42 (>= scope_2_0 0)))
 (and $x42 (< scope_2_0 8192))))
(assert
 (let (($x48 (>= scope_2_1 0)))
 (and $x48 (< scope_2_1 8192))))
(assert
 (let ((?x43 (ite (= scope_2_0 query_2) 0 (ite (= scope_2_1 query_2) 1 (- 1)))))
(let ((?x62 (ite (< scope_2_0 query_2) (- 1) (ite (< query_2 scope_2_0) (- 1) 0))))
(let ((?x64 (ite (< scope_2_1 query_2) (- 1) (ite (< query_2 scope_2_1) ?x62 1))))
(let (($x53 (and (< scope_2_0 scope_2_1))))
(let ((?x41 (ite $x53 ?x64 ?x43)))
(and (distinct ?x41 ?x43) true)))))))
; EXPECT: unsat
(check-sat)
; Query 4: first-match-equivalence-3
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_3_0 () Int)
(declare-fun scope_3_1 () Int)
(declare-fun scope_3_2 () Int)
(declare-fun query_3 () Int)
(assert
 (let (($x69 (>= scope_3_0 0)))
 (and $x69 (< scope_3_0 8192))))
(assert
 (let (($x33 (>= scope_3_1 0)))
 (and $x33 (< scope_3_1 8192))))
(assert
 (let (($x88 (>= scope_3_2 0)))
 (and $x88 (< scope_3_2 8192))))
(assert
 (let ((?x31 (ite (= scope_3_1 query_3) 1 (ite (= scope_3_2 query_3) 2 (- 1)))))
(let ((?x36 (ite (= scope_3_0 query_3) 0 ?x31)))
(let ((?x94 (ite (< scope_3_0 query_3) (- 1) (ite (< query_3 scope_3_0) (- 1) 0))))
(let ((?x96 (ite (< scope_3_2 query_3) (- 1) (ite (< query_3 scope_3_2) (- 1) 2))))
(let ((?x6 (ite (< scope_3_1 query_3) ?x96 (ite (< query_3 scope_3_1) ?x94 1))))
(let (($x67 (and (< scope_3_0 scope_3_1) (< scope_3_1 scope_3_2))))
(let ((?x38 (ite $x67 ?x6 ?x36)))
(and (distinct ?x38 ?x36) true)))))))))
; EXPECT: unsat
(check-sat)
; Query 5: first-match-equivalence-4
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_4_0 () Int)
(declare-fun scope_4_1 () Int)
(declare-fun scope_4_2 () Int)
(declare-fun scope_4_3 () Int)
(declare-fun query_4 () Int)
(assert
 (let (($x115 (>= scope_4_0 0)))
 (and $x115 (< scope_4_0 8192))))
(assert
 (let (($x109 (>= scope_4_1 0)))
 (and $x109 (< scope_4_1 8192))))
(assert
 (let (($x106 (>= scope_4_2 0)))
 (and $x106 (< scope_4_2 8192))))
(assert
 (let (($x16 (>= scope_4_3 0)))
 (and $x16 (< scope_4_3 8192))))
(assert
 (let ((?x79 (ite (= scope_4_2 query_4) 2 (ite (= scope_4_3 query_4) 3 (- 1)))))
(let ((?x47 (ite (= scope_4_0 query_4) 0 (ite (= scope_4_1 query_4) 1 ?x79))))
(let ((?x110 (ite (< scope_4_0 query_4) (- 1) (ite (< query_4 scope_4_0) (- 1) 0))))
(let ((?x143 (ite (< scope_4_1 query_4) (- 1) (ite (< query_4 scope_4_1) ?x110 1))))
(let ((?x276 (ite (< scope_4_3 query_4) (- 1) (ite (< query_4 scope_4_3) (- 1) 3))))
(let ((?x178 (ite (< scope_4_2 query_4) ?x276 (ite (< query_4 scope_4_2) ?x143 2))))
(let (($x23 (and (< scope_4_0 scope_4_1) (< scope_4_1 scope_4_2) (< scope_4_2 scope_4_3))))
(let ((?x98 (ite $x23 ?x178 ?x47)))
(and (distinct ?x98 ?x47) true))))))))))
; EXPECT: unsat
(check-sat)
; Query 6: first-match-equivalence-5
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_5_0 () Int)
(declare-fun scope_5_1 () Int)
(declare-fun scope_5_2 () Int)
(declare-fun scope_5_3 () Int)
(declare-fun scope_5_4 () Int)
(declare-fun query_5 () Int)
(assert
 (let (($x180 (>= scope_5_0 0)))
 (and $x180 (< scope_5_0 8192))))
(assert
 (let (($x288 (>= scope_5_1 0)))
 (and $x288 (< scope_5_1 8192))))
(assert
 (let (($x90 (>= scope_5_2 0)))
 (and $x90 (< scope_5_2 8192))))
(assert
 (let (($x51 (>= scope_5_3 0)))
 (and $x51 (< scope_5_3 8192))))
(assert
 (let (($x277 (>= scope_5_4 0)))
 (and $x277 (< scope_5_4 8192))))
(assert
 (let ((?x360 (ite (= scope_5_3 query_5) 3 (ite (= scope_5_4 query_5) 4 (- 1)))))
(let ((?x181 (ite (= scope_5_1 query_5) 1 (ite (= scope_5_2 query_5) 2 ?x360))))
(let ((?x43 (ite (= scope_5_0 query_5) 0 ?x181)))
(let ((?x132 (ite (< scope_5_0 query_5) (- 1) (ite (< query_5 scope_5_0) (- 1) 0))))
(let ((?x62 (ite (< scope_5_1 query_5) (- 1) (ite (< query_5 scope_5_1) ?x132 1))))
(let ((?x156 (ite (< scope_5_3 query_5) (- 1) (ite (< query_5 scope_5_3) (- 1) 3))))
(let ((?x222 (ite (< scope_5_4 query_5) (- 1) (ite (< query_5 scope_5_4) ?x156 4))))
(let ((?x271 (ite (< scope_5_2 query_5) ?x222 (ite (< query_5 scope_5_2) ?x62 2))))
(let (($x19 (and (< scope_5_0 scope_5_1) (< scope_5_1 scope_5_2) (< scope_5_2 scope_5_3) (< scope_5_3 scope_5_4))))
(let ((?x45 (ite $x19 ?x271 ?x43)))
(and (distinct ?x45 ?x43) true))))))))))))
; EXPECT: unsat
(check-sat)
; Query 7: first-match-equivalence-6
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_6_0 () Int)
(declare-fun scope_6_1 () Int)
(declare-fun scope_6_2 () Int)
(declare-fun scope_6_3 () Int)
(declare-fun scope_6_4 () Int)
(declare-fun scope_6_5 () Int)
(declare-fun query_6 () Int)
(assert
 (let (($x33 (>= scope_6_0 0)))
 (and $x33 (< scope_6_0 8192))))
(assert
 (let (($x393 (>= scope_6_1 0)))
 (and $x393 (< scope_6_1 8192))))
(assert
 (let (($x105 (>= scope_6_2 0)))
 (and $x105 (< scope_6_2 8192))))
(assert
 (let (($x251 (>= scope_6_3 0)))
 (and $x251 (< scope_6_3 8192))))
(assert
 (let (($x55 (>= scope_6_4 0)))
 (and $x55 (< scope_6_4 8192))))
(assert
 (let (($x72 (>= scope_6_5 0)))
 (and $x72 (< scope_6_5 8192))))
(assert
 (let ((?x339 (ite (= scope_6_4 query_6) 4 (ite (= scope_6_5 query_6) 5 (- 1)))))
(let ((?x170 (ite (= scope_6_2 query_6) 2 (ite (= scope_6_3 query_6) 3 ?x339))))
(let ((?x394 (ite (= scope_6_0 query_6) 0 (ite (= scope_6_1 query_6) 1 ?x170))))
(let ((?x94 (ite (< scope_6_0 query_6) (- 1) (ite (< query_6 scope_6_0) (- 1) 0))))
(let ((?x354 (ite (< scope_6_2 query_6) (- 1) (ite (< query_6 scope_6_2) (- 1) 2))))
(let ((?x97 (ite (< scope_6_1 query_6) ?x354 (ite (< query_6 scope_6_1) ?x94 1))))
(let ((?x391 (ite (< scope_6_4 query_6) (- 1) (ite (< query_6 scope_6_4) (- 1) 4))))
(let ((?x361 (ite (< scope_6_5 query_6) (- 1) (ite (< query_6 scope_6_5) ?x391 5))))
(let ((?x12 (ite (< scope_6_3 query_6) ?x361 (ite (< query_6 scope_6_3) ?x97 3))))
(let (($x259 (and (< scope_6_0 scope_6_1) (< scope_6_1 scope_6_2) (< scope_6_2 scope_6_3) (< scope_6_3 scope_6_4) (< scope_6_4 scope_6_5))))
(let ((?x244 (ite $x259 ?x12 ?x394)))
(and (distinct ?x244 ?x394) true)))))))))))))
; EXPECT: unsat
(check-sat)
; Query 8: first-match-equivalence-7
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_7_0 () Int)
(declare-fun scope_7_1 () Int)
(declare-fun scope_7_2 () Int)
(declare-fun scope_7_3 () Int)
(declare-fun scope_7_4 () Int)
(declare-fun scope_7_5 () Int)
(declare-fun scope_7_6 () Int)
(declare-fun query_7 () Int)
(assert
 (let (($x331 (>= scope_7_0 0)))
 (and $x331 (< scope_7_0 8192))))
(assert
 (let (($x187 (>= scope_7_1 0)))
 (and $x187 (< scope_7_1 8192))))
(assert
 (let (($x108 (>= scope_7_2 0)))
 (and $x108 (< scope_7_2 8192))))
(assert
 (let (($x438 (>= scope_7_3 0)))
 (and $x438 (< scope_7_3 8192))))
(assert
 (let (($x24 (>= scope_7_4 0)))
 (and $x24 (< scope_7_4 8192))))
(assert
 (let (($x112 (>= scope_7_5 0)))
 (and $x112 (< scope_7_5 8192))))
(assert
 (let (($x114 (>= scope_7_6 0)))
 (and $x114 (< scope_7_6 8192))))
(assert
 (let ((?x23 (ite (= scope_7_5 query_7) 5 (ite (= scope_7_6 query_7) 6 (- 1)))))
(let ((?x276 (ite (= scope_7_3 query_7) 3 (ite (= scope_7_4 query_7) 4 ?x23))))
(let ((?x494 (ite (= scope_7_1 query_7) 1 (ite (= scope_7_2 query_7) 2 ?x276))))
(let ((?x91 (ite (= scope_7_0 query_7) 0 ?x494)))
(let ((?x478 (ite (< scope_7_0 query_7) (- 1) (ite (< query_7 scope_7_0) (- 1) 0))))
(let ((?x497 (ite (< scope_7_2 query_7) (- 1) (ite (< query_7 scope_7_2) (- 1) 2))))
(let ((?x439 (ite (< scope_7_1 query_7) ?x497 (ite (< query_7 scope_7_1) ?x478 1))))
(let ((?x18 (ite (< scope_7_4 query_7) (- 1) (ite (< query_7 scope_7_4) (- 1) 4))))
(let ((?x47 (ite (< scope_7_6 query_7) (- 1) (ite (< query_7 scope_7_6) (- 1) 6))))
(let ((?x92 (ite (< scope_7_5 query_7) ?x47 (ite (< query_7 scope_7_5) ?x18 5))))
(let ((?x177 (ite (< scope_7_3 query_7) ?x92 (ite (< query_7 scope_7_3) ?x439 3))))
(let (($x14 (and (< scope_7_0 scope_7_1) (< scope_7_1 scope_7_2) (< scope_7_2 scope_7_3) (< scope_7_3 scope_7_4) (< scope_7_4 scope_7_5) (< scope_7_5 scope_7_6))))
(let ((?x113 (ite $x14 ?x177 ?x91)))
(and (distinct ?x113 ?x91) true)))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 9: first-match-equivalence-8
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_8_0 () Int)
(declare-fun scope_8_1 () Int)
(declare-fun scope_8_2 () Int)
(declare-fun scope_8_3 () Int)
(declare-fun scope_8_4 () Int)
(declare-fun scope_8_5 () Int)
(declare-fun scope_8_6 () Int)
(declare-fun scope_8_7 () Int)
(declare-fun query_8 () Int)
(assert
 (let (($x72 (>= scope_8_0 0)))
 (and $x72 (< scope_8_0 8192))))
(assert
 (let (($x260 (>= scope_8_1 0)))
 (and $x260 (< scope_8_1 8192))))
(assert
 (let (($x66 (>= scope_8_2 0)))
 (and $x66 (< scope_8_2 8192))))
(assert
 (let (($x297 (>= scope_8_3 0)))
 (and $x297 (< scope_8_3 8192))))
(assert
 (let (($x73 (>= scope_8_4 0)))
 (and $x73 (< scope_8_4 8192))))
(assert
 (let (($x290 (>= scope_8_5 0)))
 (and $x290 (< scope_8_5 8192))))
(assert
 (let (($x168 (>= scope_8_6 0)))
 (and $x168 (< scope_8_6 8192))))
(assert
 (let (($x96 (>= scope_8_7 0)))
 (and $x96 (< scope_8_7 8192))))
(assert
 (let ((?x182 (ite (= scope_8_6 query_8) 6 (ite (= scope_8_7 query_8) 7 (- 1)))))
(let ((?x284 (ite (= scope_8_4 query_8) 4 (ite (= scope_8_5 query_8) 5 ?x182))))
(let ((?x132 (ite (= scope_8_2 query_8) 2 (ite (= scope_8_3 query_8) 3 ?x284))))
(let ((?x601 (ite (= scope_8_0 query_8) 0 (ite (= scope_8_1 query_8) 1 ?x132))))
(let ((?x493 (ite (< scope_8_0 query_8) (- 1) (ite (< query_8 scope_8_0) (- 1) 0))))
(let ((?x258 (ite (< scope_8_1 query_8) (- 1) (ite (< query_8 scope_8_1) ?x493 1))))
(let ((?x613 (ite (< scope_8_3 query_8) (- 1) (ite (< query_8 scope_8_3) (- 1) 3))))
(let ((?x222 (ite (< scope_8_2 query_8) ?x613 (ite (< query_8 scope_8_2) ?x258 2))))
(let ((?x11 (ite (< scope_8_5 query_8) (- 1) (ite (< query_8 scope_8_5) (- 1) 5))))
(let ((?x173 (ite (< scope_8_7 query_8) (- 1) (ite (< query_8 scope_8_7) (- 1) 7))))
(let ((?x238 (ite (< scope_8_6 query_8) ?x173 (ite (< query_8 scope_8_6) ?x11 6))))
(let ((?x75 (ite (< scope_8_4 query_8) ?x238 (ite (< query_8 scope_8_4) ?x222 4))))
(let (($x89 (and (< scope_8_0 scope_8_1) (< scope_8_1 scope_8_2) (< scope_8_2 scope_8_3) (< scope_8_3 scope_8_4) (< scope_8_4 scope_8_5) (< scope_8_5 scope_8_6) (< scope_8_6 scope_8_7))))
(let ((?x657 (ite $x89 ?x75 ?x601)))
(and (distinct ?x657 ?x601) true))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 10: first-match-equivalence-9
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_9_0 () Int)
(declare-fun scope_9_1 () Int)
(declare-fun scope_9_2 () Int)
(declare-fun scope_9_3 () Int)
(declare-fun scope_9_4 () Int)
(declare-fun scope_9_5 () Int)
(declare-fun scope_9_6 () Int)
(declare-fun scope_9_7 () Int)
(declare-fun scope_9_8 () Int)
(declare-fun query_9 () Int)
(assert
 (let (($x34 (>= scope_9_0 0)))
 (and $x34 (< scope_9_0 8192))))
(assert
 (let (($x110 (>= scope_9_1 0)))
 (and $x110 (< scope_9_1 8192))))
(assert
 (let (($x259 (>= scope_9_2 0)))
 (and $x259 (< scope_9_2 8192))))
(assert
 (let (($x414 (>= scope_9_3 0)))
 (and $x414 (< scope_9_3 8192))))
(assert
 (let (($x572 (>= scope_9_4 0)))
 (and $x572 (< scope_9_4 8192))))
(assert
 (let (($x227 (>= scope_9_5 0)))
 (and $x227 (< scope_9_5 8192))))
(assert
 (let (($x353 (>= scope_9_6 0)))
 (and $x353 (< scope_9_6 8192))))
(assert
 (let (($x23 (>= scope_9_7 0)))
 (and $x23 (< scope_9_7 8192))))
(assert
 (let (($x495 (>= scope_9_8 0)))
 (and $x495 (< scope_9_8 8192))))
(assert
 (let ((?x226 (ite (= scope_9_7 query_9) 7 (ite (= scope_9_8 query_9) 8 (- 1)))))
(let ((?x278 (ite (= scope_9_5 query_9) 5 (ite (= scope_9_6 query_9) 6 ?x226))))
(let ((?x480 (ite (= scope_9_3 query_9) 3 (ite (= scope_9_4 query_9) 4 ?x278))))
(let ((?x292 (ite (= scope_9_1 query_9) 1 (ite (= scope_9_2 query_9) 2 ?x480))))
(let ((?x706 (ite (= scope_9_0 query_9) 0 ?x292)))
(let ((?x31 (ite (< scope_9_0 query_9) (- 1) (ite (< query_9 scope_9_0) (- 1) 0))))
(let ((?x69 (ite (< scope_9_1 query_9) (- 1) (ite (< query_9 scope_9_1) ?x31 1))))
(let ((?x469 (ite (< scope_9_3 query_9) (- 1) (ite (< query_9 scope_9_3) (- 1) 3))))
(let ((?x762 (ite (< scope_9_2 query_9) ?x469 (ite (< query_9 scope_9_2) ?x69 2))))
(let ((?x220 (ite (< scope_9_5 query_9) (- 1) (ite (< query_9 scope_9_5) (- 1) 5))))
(let ((?x81 (ite (< scope_9_6 query_9) (- 1) (ite (< query_9 scope_9_6) ?x220 6))))
(let ((?x395 (ite (< scope_9_8 query_9) (- 1) (ite (< query_9 scope_9_8) (- 1) 8))))
(let ((?x591 (ite (< scope_9_7 query_9) ?x395 (ite (< query_9 scope_9_7) ?x81 7))))
(let ((?x54 (ite (< scope_9_4 query_9) ?x591 (ite (< query_9 scope_9_4) ?x762 4))))
(let (($x134 (and (< scope_9_0 scope_9_1) (< scope_9_1 scope_9_2) (< scope_9_2 scope_9_3) (< scope_9_3 scope_9_4) (< scope_9_4 scope_9_5) (< scope_9_5 scope_9_6) (< scope_9_6 scope_9_7) (< scope_9_7 scope_9_8))))
(let ((?x48 (ite $x134 ?x54 ?x706)))
(and (distinct ?x48 ?x706) true))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 11: first-match-equivalence-10
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_10_0 () Int)
(declare-fun scope_10_1 () Int)
(declare-fun scope_10_2 () Int)
(declare-fun scope_10_3 () Int)
(declare-fun scope_10_4 () Int)
(declare-fun scope_10_5 () Int)
(declare-fun scope_10_6 () Int)
(declare-fun scope_10_7 () Int)
(declare-fun scope_10_8 () Int)
(declare-fun scope_10_9 () Int)
(declare-fun query_10 () Int)
(assert
 (let (($x437 (>= scope_10_0 0)))
 (and $x437 (< scope_10_0 8192))))
(assert
 (let (($x673 (>= scope_10_1 0)))
 (and $x673 (< scope_10_1 8192))))
(assert
 (let (($x72 (>= scope_10_2 0)))
 (and $x72 (< scope_10_2 8192))))
(assert
 (let (($x388 (>= scope_10_3 0)))
 (and $x388 (< scope_10_3 8192))))
(assert
 (let (($x389 (>= scope_10_4 0)))
 (and $x389 (< scope_10_4 8192))))
(assert
 (let (($x288 (>= scope_10_5 0)))
 (and $x288 (< scope_10_5 8192))))
(assert
 (let (($x176 (>= scope_10_6 0)))
 (and $x176 (< scope_10_6 8192))))
(assert
 (let (($x703 (>= scope_10_7 0)))
 (and $x703 (< scope_10_7 8192))))
(assert
 (let (($x118 (>= scope_10_8 0)))
 (and $x118 (< scope_10_8 8192))))
(assert
 (let (($x35 (>= scope_10_9 0)))
 (and $x35 (< scope_10_9 8192))))
(assert
 (let ((?x112 (ite (= scope_10_8 query_10) 8 (ite (= scope_10_9 query_10) 9 (- 1)))))
(let ((?x385 (ite (= scope_10_6 query_10) 6 (ite (= scope_10_7 query_10) 7 ?x112))))
(let ((?x449 (ite (= scope_10_4 query_10) 4 (ite (= scope_10_5 query_10) 5 ?x385))))
(let ((?x16 (ite (= scope_10_2 query_10) 2 (ite (= scope_10_3 query_10) 3 ?x449))))
(let ((?x448 (ite (= scope_10_0 query_10) 0 (ite (= scope_10_1 query_10) 1 ?x16))))
(let ((?x90 (ite (< scope_10_0 query_10) (- 1) (ite (< query_10 scope_10_0) (- 1) 0))))
(let ((?x488 (ite (< scope_10_1 query_10) (- 1) (ite (< query_10 scope_10_1) ?x90 1))))
(let ((?x732 (ite (< scope_10_3 query_10) (- 1) (ite (< query_10 scope_10_3) (- 1) 3))))
(let ((?x736 (ite (< scope_10_4 query_10) (- 1) (ite (< query_10 scope_10_4) ?x732 4))))
(let ((?x801 (ite (< scope_10_2 query_10) ?x736 (ite (< query_10 scope_10_2) ?x488 2))))
(let ((?x178 (ite (< scope_10_6 query_10) (- 1) (ite (< query_10 scope_10_6) (- 1) 6))))
(let ((?x177 (ite (< scope_10_7 query_10) (- 1) (ite (< query_10 scope_10_7) ?x178 7))))
(let ((?x53 (ite (< scope_10_9 query_10) (- 1) (ite (< query_10 scope_10_9) (- 1) 9))))
(let ((?x98 (ite (< scope_10_8 query_10) ?x53 (ite (< query_10 scope_10_8) ?x177 8))))
(let ((?x289 (ite (< scope_10_5 query_10) ?x98 (ite (< query_10 scope_10_5) ?x801 5))))
(let (($x297 (and (< scope_10_0 scope_10_1) (< scope_10_1 scope_10_2) (< scope_10_2 scope_10_3) (< scope_10_3 scope_10_4) (< scope_10_4 scope_10_5) (< scope_10_5 scope_10_6) (< scope_10_6 scope_10_7) (< scope_10_7 scope_10_8) (< scope_10_8 scope_10_9))))
(let ((?x205 (ite $x297 ?x289 ?x448)))
(and (distinct ?x205 ?x448) true)))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 12: first-match-equivalence-11
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_11_0 () Int)
(declare-fun scope_11_1 () Int)
(declare-fun scope_11_2 () Int)
(declare-fun scope_11_3 () Int)
(declare-fun scope_11_4 () Int)
(declare-fun scope_11_5 () Int)
(declare-fun scope_11_6 () Int)
(declare-fun scope_11_7 () Int)
(declare-fun scope_11_8 () Int)
(declare-fun scope_11_9 () Int)
(declare-fun scope_11_10 () Int)
(declare-fun query_11 () Int)
(assert
 (let (($x459 (>= scope_11_0 0)))
 (and $x459 (< scope_11_0 8192))))
(assert
 (let (($x10 (>= scope_11_1 0)))
 (and $x10 (< scope_11_1 8192))))
(assert
 (let (($x258 (>= scope_11_2 0)))
 (and $x258 (< scope_11_2 8192))))
(assert
 (let (($x294 (>= scope_11_3 0)))
 (and $x294 (< scope_11_3 8192))))
(assert
 (let (($x55 (>= scope_11_4 0)))
 (and $x55 (< scope_11_4 8192))))
(assert
 (let (($x46 (>= scope_11_5 0)))
 (and $x46 (< scope_11_5 8192))))
(assert
 (let (($x614 (>= scope_11_6 0)))
 (and $x614 (< scope_11_6 8192))))
(assert
 (let (($x117 (>= scope_11_7 0)))
 (and $x117 (< scope_11_7 8192))))
(assert
 (let (($x560 (>= scope_11_8 0)))
 (and $x560 (< scope_11_8 8192))))
(assert
 (let (($x287 (>= scope_11_9 0)))
 (and $x287 (< scope_11_9 8192))))
(assert
 (let (($x340 (>= scope_11_10 0)))
 (and $x340 (< scope_11_10 8192))))
(assert
 (let ((?x337 (ite (= scope_11_9 query_11) 9 (ite (= scope_11_10 query_11) 10 (- 1)))))
(let ((?x757 (ite (= scope_11_7 query_11) 7 (ite (= scope_11_8 query_11) 8 ?x337))))
(let ((?x283 (ite (= scope_11_5 query_11) 5 (ite (= scope_11_6 query_11) 6 ?x757))))
(let ((?x659 (ite (= scope_11_3 query_11) 3 (ite (= scope_11_4 query_11) 4 ?x283))))
(let ((?x660 (ite (= scope_11_1 query_11) 1 (ite (= scope_11_2 query_11) 2 ?x659))))
(let ((?x613 (ite (= scope_11_0 query_11) 0 ?x660)))
(let ((?x89 (ite (< scope_11_0 query_11) (- 1) (ite (< query_11 scope_11_0) (- 1) 0))))
(let ((?x105 (ite (< scope_11_1 query_11) (- 1) (ite (< query_11 scope_11_1) ?x89 1))))
(let ((?x257 (ite (< scope_11_3 query_11) (- 1) (ite (< query_11 scope_11_3) (- 1) 3))))
(let ((?x451 (ite (< scope_11_4 query_11) (- 1) (ite (< query_11 scope_11_4) ?x257 4))))
(let ((?x52 (ite (< scope_11_2 query_11) ?x451 (ite (< query_11 scope_11_2) ?x105 2))))
(let ((?x170 (ite (< scope_11_6 query_11) (- 1) (ite (< query_11 scope_11_6) (- 1) 6))))
(let ((?x390 (ite (< scope_11_7 query_11) (- 1) (ite (< query_11 scope_11_7) ?x170 7))))
(let ((?x348 (ite (< scope_11_9 query_11) (- 1) (ite (< query_11 scope_11_9) (- 1) 9))))
(let ((?x789 (ite (< scope_11_10 query_11) (- 1) (ite (< query_11 scope_11_10) ?x348 10))))
(let ((?x418 (ite (< scope_11_8 query_11) ?x789 (ite (< query_11 scope_11_8) ?x390 8))))
(let ((?x490 (ite (< scope_11_5 query_11) ?x418 (ite (< query_11 scope_11_5) ?x52 5))))
(let (($x285 (and (< scope_11_0 scope_11_1) (< scope_11_1 scope_11_2) (< scope_11_2 scope_11_3) (< scope_11_3 scope_11_4) (< scope_11_4 scope_11_5) (< scope_11_5 scope_11_6) (< scope_11_6 scope_11_7) (< scope_11_7 scope_11_8) (< scope_11_8 scope_11_9) (< scope_11_9 scope_11_10))))
(let ((?x551 (ite $x285 ?x490 ?x613)))
(and (distinct ?x551 ?x613) true)))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 13: first-match-equivalence-12
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_12_0 () Int)
(declare-fun scope_12_1 () Int)
(declare-fun scope_12_2 () Int)
(declare-fun scope_12_3 () Int)
(declare-fun scope_12_4 () Int)
(declare-fun scope_12_5 () Int)
(declare-fun scope_12_6 () Int)
(declare-fun scope_12_7 () Int)
(declare-fun scope_12_8 () Int)
(declare-fun scope_12_9 () Int)
(declare-fun scope_12_10 () Int)
(declare-fun scope_12_11 () Int)
(declare-fun query_12 () Int)
(assert
 (let (($x938 (>= scope_12_0 0)))
 (and $x938 (< scope_12_0 8192))))
(assert
 (let (($x388 (>= scope_12_1 0)))
 (and $x388 (< scope_12_1 8192))))
(assert
 (let (($x915 (>= scope_12_2 0)))
 (and $x915 (< scope_12_2 8192))))
(assert
 (let (($x61 (>= scope_12_3 0)))
 (and $x61 (< scope_12_3 8192))))
(assert
 (let (($x109 (>= scope_12_4 0)))
 (and $x109 (< scope_12_4 8192))))
(assert
 (let (($x408 (>= scope_12_5 0)))
 (and $x408 (< scope_12_5 8192))))
(assert
 (let (($x386 (>= scope_12_6 0)))
 (and $x386 (< scope_12_6 8192))))
(assert
 (let (($x417 (>= scope_12_7 0)))
 (and $x417 (< scope_12_7 8192))))
(assert
 (let (($x438 (>= scope_12_8 0)))
 (and $x438 (< scope_12_8 8192))))
(assert
 (let (($x57 (>= scope_12_9 0)))
 (and $x57 (< scope_12_9 8192))))
(assert
 (let (($x498 (>= scope_12_10 0)))
 (and $x498 (< scope_12_10 8192))))
(assert
 (let (($x332 (>= scope_12_11 0)))
 (and $x332 (< scope_12_11 8192))))
(assert
 (let ((?x12 (ite (= scope_12_10 query_12) 10 (ite (= scope_12_11 query_12) 11 (- 1)))))
(let ((?x187 (ite (= scope_12_8 query_12) 8 (ite (= scope_12_9 query_12) 9 ?x12))))
(let ((?x156 (ite (= scope_12_6 query_12) 6 (ite (= scope_12_7 query_12) 7 ?x187))))
(let ((?x591 (ite (= scope_12_4 query_12) 4 (ite (= scope_12_5 query_12) 5 ?x156))))
(let ((?x54 (ite (= scope_12_2 query_12) 2 (ite (= scope_12_3 query_12) 3 ?x591))))
(let ((?x778 (ite (= scope_12_0 query_12) 0 (ite (= scope_12_1 query_12) 1 ?x54))))
(let ((?x51 (ite (< scope_12_0 query_12) (- 1) (ite (< query_12 scope_12_0) (- 1) 0))))
(let ((?x8 (ite (< scope_12_2 query_12) (- 1) (ite (< query_12 scope_12_2) (- 1) 2))))
(let ((?x277 (ite (< scope_12_1 query_12) ?x8 (ite (< query_12 scope_12_1) ?x51 1))))
(let ((?x937 (ite (< scope_12_4 query_12) (- 1) (ite (< query_12 scope_12_4) (- 1) 4))))
(let ((?x876 (ite (< scope_12_5 query_12) (- 1) (ite (< query_12 scope_12_5) ?x937 5))))
(let ((?x829 (ite (< scope_12_3 query_12) ?x876 (ite (< query_12 scope_12_3) ?x277 3))))
(let ((?x692 (ite (< scope_12_7 query_12) (- 1) (ite (< query_12 scope_12_7) (- 1) 7))))
(let ((?x191 (ite (< scope_12_8 query_12) (- 1) (ite (< query_12 scope_12_8) ?x692 8))))
(let ((?x350 (ite (< scope_12_10 query_12) (- 1) (ite (< query_12 scope_12_10) (- 1) 10))))
(let ((?x437 (ite (< scope_12_11 query_12) (- 1) (ite (< query_12 scope_12_11) ?x350 11))))
(let ((?x425 (ite (< scope_12_9 query_12) ?x437 (ite (< query_12 scope_12_9) ?x191 9))))
(let ((?x88 (ite (< scope_12_6 query_12) ?x425 (ite (< query_12 scope_12_6) ?x829 6))))
(let (($x40 (and (< scope_12_0 scope_12_1) (< scope_12_1 scope_12_2) (< scope_12_2 scope_12_3) (< scope_12_3 scope_12_4) (< scope_12_4 scope_12_5) (< scope_12_5 scope_12_6) (< scope_12_6 scope_12_7) (< scope_12_7 scope_12_8) (< scope_12_8 scope_12_9) (< scope_12_9 scope_12_10) (< scope_12_10 scope_12_11))))
(let ((?x694 (ite $x40 ?x88 ?x778)))
(and (distinct ?x694 ?x778) true))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 14: first-match-equivalence-13
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_13_0 () Int)
(declare-fun scope_13_1 () Int)
(declare-fun scope_13_2 () Int)
(declare-fun scope_13_3 () Int)
(declare-fun scope_13_4 () Int)
(declare-fun scope_13_5 () Int)
(declare-fun scope_13_6 () Int)
(declare-fun scope_13_7 () Int)
(declare-fun scope_13_8 () Int)
(declare-fun scope_13_9 () Int)
(declare-fun scope_13_10 () Int)
(declare-fun scope_13_11 () Int)
(declare-fun scope_13_12 () Int)
(declare-fun query_13 () Int)
(assert
 (let (($x43 (>= scope_13_0 0)))
 (and $x43 (< scope_13_0 8192))))
(assert
 (let (($x70 (>= scope_13_1 0)))
 (and $x70 (< scope_13_1 8192))))
(assert
 (let (($x260 (>= scope_13_2 0)))
 (and $x260 (< scope_13_2 8192))))
(assert
 (let (($x460 (>= scope_13_3 0)))
 (and $x460 (< scope_13_3 8192))))
(assert
 (let (($x188 (>= scope_13_4 0)))
 (and $x188 (< scope_13_4 8192))))
(assert
 (let (($x122 (>= scope_13_5 0)))
 (and $x122 (< scope_13_5 8192))))
(assert
 (let (($x754 (>= scope_13_6 0)))
 (and $x754 (< scope_13_6 8192))))
(assert
 (let (($x225 (>= scope_13_7 0)))
 (and $x225 (< scope_13_7 8192))))
(assert
 (let (($x1088 (>= scope_13_8 0)))
 (and $x1088 (< scope_13_8 8192))))
(assert
 (let (($x613 (>= scope_13_9 0)))
 (and $x613 (< scope_13_9 8192))))
(assert
 (let (($x52 (>= scope_13_10 0)))
 (and $x52 (< scope_13_10 8192))))
(assert
 (let (($x62 (>= scope_13_11 0)))
 (and $x62 (< scope_13_11 8192))))
(assert
 (let (($x426 (>= scope_13_12 0)))
 (and $x426 (< scope_13_12 8192))))
(assert
 (let ((?x31 (ite (= scope_13_11 query_13) 11 (ite (= scope_13_12 query_13) 12 (- 1)))))
(let ((?x755 (ite (= scope_13_9 query_13) 9 (ite (= scope_13_10 query_13) 10 ?x31))))
(let ((?x44 (ite (= scope_13_7 query_13) 7 (ite (= scope_13_8 query_13) 8 ?x755))))
(let ((?x138 (ite (= scope_13_5 query_13) 5 (ite (= scope_13_6 query_13) 6 ?x44))))
(let ((?x39 (ite (= scope_13_3 query_13) 3 (ite (= scope_13_4 query_13) 4 ?x138))))
(let ((?x738 (ite (= scope_13_1 query_13) 1 (ite (= scope_13_2 query_13) 2 ?x39))))
(let ((?x479 (ite (= scope_13_0 query_13) 0 ?x738)))
(let ((?x481 (ite (< scope_13_0 query_13) (- 1) (ite (< query_13 scope_13_0) (- 1) 0))))
(let ((?x1014 (ite (< scope_13_2 query_13) (- 1) (ite (< query_13 scope_13_2) (- 1) 2))))
(let ((?x705 (ite (< scope_13_1 query_13) ?x1014 (ite (< query_13 scope_13_1) ?x481 1))))
(let ((?x772 (ite (< scope_13_4 query_13) (- 1) (ite (< query_13 scope_13_4) (- 1) 4))))
(let ((?x1004 (ite (< scope_13_5 query_13) (- 1) (ite (< query_13 scope_13_5) ?x772 5))))
(let ((?x686 (ite (< scope_13_3 query_13) ?x1004 (ite (< query_13 scope_13_3) ?x705 3))))
(let ((?x397 (ite (< scope_13_7 query_13) (- 1) (ite (< query_13 scope_13_7) (- 1) 7))))
(let ((?x244 (ite (< scope_13_9 query_13) (- 1) (ite (< query_13 scope_13_9) (- 1) 9))))
(let ((?x34 (ite (< scope_13_8 query_13) ?x244 (ite (< query_13 scope_13_8) ?x397 8))))
(let ((?x391 (ite (< scope_13_11 query_13) (- 1) (ite (< query_13 scope_13_11) (- 1) 11))))
(let ((?x258 (ite (< scope_13_12 query_13) (- 1) (ite (< query_13 scope_13_12) ?x391 12))))
(let ((?x832 (ite (< scope_13_10 query_13) ?x258 (ite (< query_13 scope_13_10) ?x34 10))))
(let ((?x468 (ite (< scope_13_6 query_13) ?x832 (ite (< query_13 scope_13_6) ?x686 6))))
(let (($x560 (and (< scope_13_0 scope_13_1) (< scope_13_1 scope_13_2) (< scope_13_2 scope_13_3) (< scope_13_3 scope_13_4) (< scope_13_4 scope_13_5) (< scope_13_5 scope_13_6) (< scope_13_6 scope_13_7) (< scope_13_7 scope_13_8) (< scope_13_8 scope_13_9) (< scope_13_9 scope_13_10) (< scope_13_10 scope_13_11) (< scope_13_11 scope_13_12))))
(let ((?x753 (ite $x560 ?x468 ?x479)))
(and (distinct ?x753 ?x479) true))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 15: first-match-equivalence-14
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_14_0 () Int)
(declare-fun scope_14_1 () Int)
(declare-fun scope_14_2 () Int)
(declare-fun scope_14_3 () Int)
(declare-fun scope_14_4 () Int)
(declare-fun scope_14_5 () Int)
(declare-fun scope_14_6 () Int)
(declare-fun scope_14_7 () Int)
(declare-fun scope_14_8 () Int)
(declare-fun scope_14_9 () Int)
(declare-fun scope_14_10 () Int)
(declare-fun scope_14_11 () Int)
(declare-fun scope_14_12 () Int)
(declare-fun scope_14_13 () Int)
(declare-fun query_14 () Int)
(assert
 (let (($x13 (>= scope_14_0 0)))
 (and $x13 (< scope_14_0 8192))))
(assert
 (let (($x499 (>= scope_14_1 0)))
 (and $x499 (< scope_14_1 8192))))
(assert
 (let (($x478 (>= scope_14_2 0)))
 (and $x478 (< scope_14_2 8192))))
(assert
 (let (($x763 (>= scope_14_3 0)))
 (and $x763 (< scope_14_3 8192))))
(assert
 (let (($x1228 (>= scope_14_4 0)))
 (and $x1228 (< scope_14_4 8192))))
(assert
 (let (($x63 (>= scope_14_5 0)))
 (and $x63 (< scope_14_5 8192))))
(assert
 (let (($x709 (>= scope_14_6 0)))
 (and $x709 (< scope_14_6 8192))))
(assert
 (let (($x208 (>= scope_14_7 0)))
 (and $x208 (< scope_14_7 8192))))
(assert
 (let (($x12 (>= scope_14_8 0)))
 (and $x12 (< scope_14_8 8192))))
(assert
 (let (($x297 (>= scope_14_9 0)))
 (and $x297 (< scope_14_9 8192))))
(assert
 (let (($x23 (>= scope_14_10 0)))
 (and $x23 (< scope_14_10 8192))))
(assert
 (let (($x915 (>= scope_14_11 0)))
 (and $x915 (< scope_14_11 8192))))
(assert
 (let (($x88 (>= scope_14_12 0)))
 (and $x88 (< scope_14_12 8192))))
(assert
 (let (($x82 (>= scope_14_13 0)))
 (and $x82 (< scope_14_13 8192))))
(assert
 (let ((?x181 (ite (= scope_14_12 query_14) 12 (ite (= scope_14_13 query_14) 13 (- 1)))))
(let ((?x66 (ite (= scope_14_10 query_14) 10 (ite (= scope_14_11 query_14) 11 ?x181))))
(let ((?x758 (ite (= scope_14_8 query_14) 8 (ite (= scope_14_9 query_14) 9 ?x66))))
(let ((?x84 (ite (= scope_14_6 query_14) 6 (ite (= scope_14_7 query_14) 7 ?x758))))
(let ((?x494 (ite (= scope_14_4 query_14) 4 (ite (= scope_14_5 query_14) 5 ?x84))))
(let ((?x222 (ite (= scope_14_2 query_14) 2 (ite (= scope_14_3 query_14) 3 ?x494))))
(let ((?x791 (ite (= scope_14_0 query_14) 0 (ite (= scope_14_1 query_14) 1 ?x222))))
(let ((?x99 (ite (< scope_14_0 query_14) (- 1) (ite (< query_14 scope_14_0) (- 1) 0))))
(let ((?x1158 (ite (< scope_14_2 query_14) (- 1) (ite (< query_14 scope_14_2) (- 1) 2))))
(let ((?x992 (ite (< scope_14_1 query_14) ?x1158 (ite (< query_14 scope_14_1) ?x99 1))))
(let ((?x261 (ite (< scope_14_4 query_14) (- 1) (ite (< query_14 scope_14_4) (- 1) 4))))
(let ((?x48 (ite (< scope_14_6 query_14) (- 1) (ite (< query_14 scope_14_6) (- 1) 6))))
(let ((?x1146 (ite (< scope_14_5 query_14) ?x48 (ite (< query_14 scope_14_5) ?x261 5))))
(let ((?x276 (ite (< scope_14_3 query_14) ?x1146 (ite (< query_14 scope_14_3) ?x992 3))))
(let ((?x1176 (ite (< scope_14_8 query_14) (- 1) (ite (< query_14 scope_14_8) (- 1) 8))))
(let ((?x190 (ite (< scope_14_10 query_14) (- 1) (ite (< query_14 scope_14_10) (- 1) 10))))
(let ((?x1177 (ite (< scope_14_9 query_14) ?x190 (ite (< query_14 scope_14_9) ?x1176 9))))
(let ((?x417 (ite (< scope_14_12 query_14) (- 1) (ite (< query_14 scope_14_12) (- 1) 12))))
(let ((?x112 (ite (< scope_14_13 query_14) (- 1) (ite (< query_14 scope_14_13) ?x417 13))))
(let ((?x227 (ite (< scope_14_11 query_14) ?x112 (ite (< query_14 scope_14_11) ?x1177 11))))
(let ((?x674 (ite (< scope_14_7 query_14) ?x227 (ite (< query_14 scope_14_7) ?x276 7))))
(let (($x332 (and (< scope_14_0 scope_14_1) (< scope_14_1 scope_14_2) (< scope_14_2 scope_14_3) (< scope_14_3 scope_14_4) (< scope_14_4 scope_14_5) (< scope_14_5 scope_14_6) (< scope_14_6 scope_14_7) (< scope_14_7 scope_14_8) (< scope_14_8 scope_14_9) (< scope_14_9 scope_14_10) (< scope_14_10 scope_14_11) (< scope_14_11 scope_14_12) (< scope_14_12 scope_14_13))))
(let ((?x348 (ite $x332 ?x674 ?x791)))
(and (distinct ?x348 ?x791) true)))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 16: first-match-equivalence-15
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_15_0 () Int)
(declare-fun scope_15_1 () Int)
(declare-fun scope_15_2 () Int)
(declare-fun scope_15_3 () Int)
(declare-fun scope_15_4 () Int)
(declare-fun scope_15_5 () Int)
(declare-fun scope_15_6 () Int)
(declare-fun scope_15_7 () Int)
(declare-fun scope_15_8 () Int)
(declare-fun scope_15_9 () Int)
(declare-fun scope_15_10 () Int)
(declare-fun scope_15_11 () Int)
(declare-fun scope_15_12 () Int)
(declare-fun scope_15_13 () Int)
(declare-fun scope_15_14 () Int)
(declare-fun query_15 () Int)
(assert
 (let (($x52 (>= scope_15_0 0)))
 (and $x52 (< scope_15_0 8192))))
(assert
 (let (($x80 (>= scope_15_1 0)))
 (and $x80 (< scope_15_1 8192))))
(assert
 (let (($x60 (>= scope_15_2 0)))
 (and $x60 (< scope_15_2 8192))))
(assert
 (let (($x191 (>= scope_15_3 0)))
 (and $x191 (< scope_15_3 8192))))
(assert
 (let (($x224 (>= scope_15_4 0)))
 (and $x224 (< scope_15_4 8192))))
(assert
 (let (($x178 (>= scope_15_5 0)))
 (and $x178 (< scope_15_5 8192))))
(assert
 (let (($x449 (>= scope_15_6 0)))
 (and $x449 (< scope_15_6 8192))))
(assert
 (let (($x904 (>= scope_15_7 0)))
 (and $x904 (< scope_15_7 8192))))
(assert
 (let (($x1088 (>= scope_15_8 0)))
 (and $x1088 (< scope_15_8 8192))))
(assert
 (let (($x1301 (>= scope_15_9 0)))
 (and $x1301 (< scope_15_9 8192))))
(assert
 (let (($x947 (>= scope_15_10 0)))
 (and $x947 (< scope_15_10 8192))))
(assert
 (let (($x703 (>= scope_15_11 0)))
 (and $x703 (< scope_15_11 8192))))
(assert
 (let (($x597 (>= scope_15_12 0)))
 (and $x597 (< scope_15_12 8192))))
(assert
 (let (($x59 (>= scope_15_13 0)))
 (and $x59 (< scope_15_13 8192))))
(assert
 (let (($x468 (>= scope_15_14 0)))
 (and $x468 (< scope_15_14 8192))))
(assert
 (let ((?x989 (ite (= scope_15_13 query_15) 13 (ite (= scope_15_14 query_15) 14 (- 1)))))
(let ((?x1379 (ite (= scope_15_11 query_15) 11 (ite (= scope_15_12 query_15) 12 ?x989))))
(let ((?x692 (ite (= scope_15_9 query_15) 9 (ite (= scope_15_10 query_15) 10 ?x1379))))
(let ((?x570 (ite (= scope_15_7 query_15) 7 (ite (= scope_15_8 query_15) 8 ?x692))))
(let ((?x655 (ite (= scope_15_5 query_15) 5 (ite (= scope_15_6 query_15) 6 ?x570))))
(let ((?x91 (ite (= scope_15_3 query_15) 3 (ite (= scope_15_4 query_15) 4 ?x655))))
(let ((?x81 (ite (= scope_15_1 query_15) 1 (ite (= scope_15_2 query_15) 2 ?x91))))
(let ((?x1147 (ite (= scope_15_0 query_15) 0 ?x81)))
(let ((?x230 (ite (< scope_15_0 query_15) (- 1) (ite (< query_15 scope_15_0) (- 1) 0))))
(let ((?x1065 (ite (< scope_15_2 query_15) (- 1) (ite (< query_15 scope_15_2) (- 1) 2))))
(let ((?x98 (ite (< scope_15_1 query_15) ?x1065 (ite (< query_15 scope_15_1) ?x230 1))))
(let ((?x1322 (ite (< scope_15_4 query_15) (- 1) (ite (< query_15 scope_15_4) (- 1) 4))))
(let ((?x489 (ite (< scope_15_6 query_15) (- 1) (ite (< query_15 scope_15_6) (- 1) 6))))
(let ((?x898 (ite (< scope_15_5 query_15) ?x489 (ite (< query_15 scope_15_5) ?x1322 5))))
(let ((?x288 (ite (< scope_15_3 query_15) ?x898 (ite (< query_15 scope_15_3) ?x98 3))))
(let ((?x17 (ite (< scope_15_8 query_15) (- 1) (ite (< query_15 scope_15_8) (- 1) 8))))
(let ((?x460 (ite (< scope_15_10 query_15) (- 1) (ite (< query_15 scope_15_10) (- 1) 10))))
(let ((?x1089 (ite (< scope_15_9 query_15) ?x460 (ite (< query_15 scope_15_9) ?x17 9))))
(let ((?x238 (ite (< scope_15_12 query_15) (- 1) (ite (< query_15 scope_15_12) (- 1) 12))))
(let ((?x1015 (ite (< scope_15_14 query_15) (- 1) (ite (< query_15 scope_15_14) (- 1) 14))))
(let ((?x122 (ite (< scope_15_13 query_15) ?x1015 (ite (< query_15 scope_15_13) ?x238 13))))
(let ((?x1318 (ite (< scope_15_11 query_15) ?x122 (ite (< query_15 scope_15_11) ?x1089 11))))
(let ((?x211 (ite (< scope_15_7 query_15) ?x1318 (ite (< query_15 scope_15_7) ?x288 7))))
(let (($x426 (and (< scope_15_0 scope_15_1) (< scope_15_1 scope_15_2) (< scope_15_2 scope_15_3) (< scope_15_3 scope_15_4) (< scope_15_4 scope_15_5) (< scope_15_5 scope_15_6) (< scope_15_6 scope_15_7) (< scope_15_7 scope_15_8) (< scope_15_8 scope_15_9) (< scope_15_9 scope_15_10) (< scope_15_10 scope_15_11) (< scope_15_11 scope_15_12) (< scope_15_12 scope_15_13) (< scope_15_13 scope_15_14))))
(let ((?x93 (ite $x426 ?x211 ?x1147)))
(and (distinct ?x93 ?x1147) true)))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 17: first-match-equivalence-16
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_16_0 () Int)
(declare-fun scope_16_1 () Int)
(declare-fun scope_16_2 () Int)
(declare-fun scope_16_3 () Int)
(declare-fun scope_16_4 () Int)
(declare-fun scope_16_5 () Int)
(declare-fun scope_16_6 () Int)
(declare-fun scope_16_7 () Int)
(declare-fun scope_16_8 () Int)
(declare-fun scope_16_9 () Int)
(declare-fun scope_16_10 () Int)
(declare-fun scope_16_11 () Int)
(declare-fun scope_16_12 () Int)
(declare-fun scope_16_13 () Int)
(declare-fun scope_16_14 () Int)
(declare-fun scope_16_15 () Int)
(declare-fun query_16 () Int)
(assert
 (let (($x717 (>= scope_16_0 0)))
 (and $x717 (< scope_16_0 8192))))
(assert
 (let (($x194 (>= scope_16_1 0)))
 (and $x194 (< scope_16_1 8192))))
(assert
 (let (($x1290 (>= scope_16_2 0)))
 (and $x1290 (< scope_16_2 8192))))
(assert
 (let (($x625 (>= scope_16_3 0)))
 (and $x625 (< scope_16_3 8192))))
(assert
 (let (($x459 (>= scope_16_4 0)))
 (and $x459 (< scope_16_4 8192))))
(assert
 (let (($x1323 (>= scope_16_5 0)))
 (and $x1323 (< scope_16_5 8192))))
(assert
 (let (($x393 (>= scope_16_6 0)))
 (and $x393 (< scope_16_6 8192))))
(assert
 (let (($x131 (>= scope_16_7 0)))
 (and $x131 (< scope_16_7 8192))))
(assert
 (let (($x756 (>= scope_16_8 0)))
 (and $x756 (< scope_16_8 8192))))
(assert
 (let (($x832 (>= scope_16_9 0)))
 (and $x832 (< scope_16_9 8192))))
(assert
 (let (($x1014 (>= scope_16_10 0)))
 (and $x1014 (< scope_16_10 8192))))
(assert
 (let (($x876 (>= scope_16_11 0)))
 (and $x876 (< scope_16_11 8192))))
(assert
 (let (($x1017 (>= scope_16_12 0)))
 (and $x1017 (< scope_16_12 8192))))
(assert
 (let (($x95 (>= scope_16_13 0)))
 (and $x95 (< scope_16_13 8192))))
(assert
 (let (($x452 (>= scope_16_14 0)))
 (and $x452 (< scope_16_14 8192))))
(assert
 (let (($x768 (>= scope_16_15 0)))
 (and $x768 (< scope_16_15 8192))))
(assert
 (let ((?x205 (ite (= scope_16_14 query_16) 14 (ite (= scope_16_15 query_16) 15 (- 1)))))
(let ((?x734 (ite (= scope_16_12 query_16) 12 (ite (= scope_16_13 query_16) 13 ?x205))))
(let ((?x1312 (ite (= scope_16_10 query_16) 10 (ite (= scope_16_11 query_16) 11 ?x734))))
(let ((?x1029 (ite (= scope_16_8 query_16) 8 (ite (= scope_16_9 query_16) 9 ?x1312))))
(let ((?x716 (ite (= scope_16_6 query_16) 6 (ite (= scope_16_7 query_16) 7 ?x1029))))
(let ((?x1027 (ite (= scope_16_4 query_16) 4 (ite (= scope_16_5 query_16) 5 ?x716))))
(let ((?x559 (ite (= scope_16_2 query_16) 2 (ite (= scope_16_3 query_16) 3 ?x1027))))
(let ((?x1004 (ite (= scope_16_0 query_16) 0 (ite (= scope_16_1 query_16) 1 ?x559))))
(let ((?x42 (ite (< scope_16_0 query_16) (- 1) (ite (< query_16 scope_16_0) (- 1) 0))))
(let ((?x353 (ite (< scope_16_1 query_16) (- 1) (ite (< query_16 scope_16_1) ?x42 1))))
(let ((?x1403 (ite (< scope_16_3 query_16) (- 1) (ite (< query_16 scope_16_3) (- 1) 3))))
(let ((?x601 (ite (< scope_16_2 query_16) ?x1403 (ite (< query_16 scope_16_2) ?x353 2))))
(let ((?x929 (ite (< scope_16_5 query_16) (- 1) (ite (< query_16 scope_16_5) (- 1) 5))))
(let ((?x769 (ite (< scope_16_7 query_16) (- 1) (ite (< query_16 scope_16_7) (- 1) 7))))
(let ((?x1487 (ite (< scope_16_6 query_16) ?x769 (ite (< query_16 scope_16_6) ?x929 6))))
(let ((?x649 (ite (< scope_16_4 query_16) ?x1487 (ite (< query_16 scope_16_4) ?x601 4))))
(let ((?x345 (ite (< scope_16_9 query_16) (- 1) (ite (< query_16 scope_16_9) (- 1) 9))))
(let ((?x493 (ite (< scope_16_11 query_16) (- 1) (ite (< query_16 scope_16_11) (- 1) 11))))
(let ((?x110 (ite (< scope_16_10 query_16) ?x493 (ite (< query_16 scope_16_10) ?x345 10))))
(let ((?x12 (ite (< scope_16_13 query_16) (- 1) (ite (< query_16 scope_16_13) (- 1) 13))))
(let ((?x650 (ite (< scope_16_15 query_16) (- 1) (ite (< query_16 scope_16_15) (- 1) 15))))
(let ((?x19 (ite (< scope_16_14 query_16) ?x650 (ite (< query_16 scope_16_14) ?x12 14))))
(let ((?x990 (ite (< scope_16_12 query_16) ?x19 (ite (< query_16 scope_16_12) ?x110 12))))
(let ((?x418 (ite (< scope_16_8 query_16) ?x990 (ite (< query_16 scope_16_8) ?x649 8))))
(let (($x451 (and (< scope_16_0 scope_16_1) (< scope_16_1 scope_16_2) (< scope_16_2 scope_16_3) (< scope_16_3 scope_16_4) (< scope_16_4 scope_16_5) (< scope_16_5 scope_16_6) (< scope_16_6 scope_16_7) (< scope_16_7 scope_16_8) (< scope_16_8 scope_16_9) (< scope_16_9 scope_16_10) (< scope_16_10 scope_16_11) (< scope_16_11 scope_16_12) (< scope_16_12 scope_16_13) (< scope_16_13 scope_16_14) (< scope_16_14 scope_16_15))))
(let ((?x820 (ite $x451 ?x418 ?x1004)))
(and (distinct ?x820 ?x1004) true))))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 18: first-match-equivalence-32
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_32_0 () Int)
(declare-fun scope_32_1 () Int)
(declare-fun scope_32_2 () Int)
(declare-fun scope_32_3 () Int)
(declare-fun scope_32_4 () Int)
(declare-fun scope_32_5 () Int)
(declare-fun scope_32_6 () Int)
(declare-fun scope_32_7 () Int)
(declare-fun scope_32_8 () Int)
(declare-fun scope_32_9 () Int)
(declare-fun scope_32_10 () Int)
(declare-fun scope_32_11 () Int)
(declare-fun scope_32_12 () Int)
(declare-fun scope_32_13 () Int)
(declare-fun scope_32_14 () Int)
(declare-fun scope_32_15 () Int)
(declare-fun scope_32_16 () Int)
(declare-fun scope_32_17 () Int)
(declare-fun scope_32_18 () Int)
(declare-fun scope_32_19 () Int)
(declare-fun scope_32_20 () Int)
(declare-fun scope_32_21 () Int)
(declare-fun scope_32_22 () Int)
(declare-fun scope_32_23 () Int)
(declare-fun scope_32_24 () Int)
(declare-fun scope_32_25 () Int)
(declare-fun scope_32_26 () Int)
(declare-fun scope_32_27 () Int)
(declare-fun scope_32_28 () Int)
(declare-fun scope_32_29 () Int)
(declare-fun scope_32_30 () Int)
(declare-fun scope_32_31 () Int)
(declare-fun query_32 () Int)
(assert
 (let (($x358 (>= scope_32_0 0)))
 (and $x358 (< scope_32_0 8192))))
(assert
 (let (($x936 (>= scope_32_1 0)))
 (and $x936 (< scope_32_1 8192))))
(assert
 (let (($x498 (>= scope_32_2 0)))
 (and $x498 (< scope_32_2 8192))))
(assert
 (let (($x409 (>= scope_32_3 0)))
 (and $x409 (< scope_32_3 8192))))
(assert
 (let (($x360 (>= scope_32_4 0)))
 (and $x360 (< scope_32_4 8192))))
(assert
 (let (($x777 (>= scope_32_5 0)))
 (and $x777 (< scope_32_5 8192))))
(assert
 (let (($x811 (>= scope_32_6 0)))
 (and $x811 (< scope_32_6 8192))))
(assert
 (let (($x852 (>= scope_32_7 0)))
 (and $x852 (< scope_32_7 8192))))
(assert
 (let (($x258 (>= scope_32_8 0)))
 (and $x258 (< scope_32_8 8192))))
(assert
 (let (($x1532 (>= scope_32_9 0)))
 (and $x1532 (< scope_32_9 8192))))
(assert
 (let (($x89 (>= scope_32_10 0)))
 (and $x89 (< scope_32_10 8192))))
(assert
 (let (($x230 (>= scope_32_11 0)))
 (and $x230 (< scope_32_11 8192))))
(assert
 (let (($x247 (>= scope_32_12 0)))
 (and $x247 (< scope_32_12 8192))))
(assert
 (let (($x32 (>= scope_32_13 0)))
 (and $x32 (< scope_32_13 8192))))
(assert
 (let (($x465 (>= scope_32_14 0)))
 (and $x465 (< scope_32_14 8192))))
(assert
 (let (($x834 (>= scope_32_15 0)))
 (and $x834 (< scope_32_15 8192))))
(assert
 (let (($x989 (>= scope_32_16 0)))
 (and $x989 (< scope_32_16 8192))))
(assert
 (let (($x830 (>= scope_32_17 0)))
 (and $x830 (< scope_32_17 8192))))
(assert
 (let (($x91 (>= scope_32_18 0)))
 (and $x91 (< scope_32_18 8192))))
(assert
 (let (($x18 (>= scope_32_19 0)))
 (and $x18 (< scope_32_19 8192))))
(assert
 (let (($x597 (>= scope_32_20 0)))
 (and $x597 (< scope_32_20 8192))))
(assert
 (let (($x812 (>= scope_32_21 0)))
 (and $x812 (< scope_32_21 8192))))
(assert
 (let (($x40 (>= scope_32_22 0)))
 (and $x40 (< scope_32_22 8192))))
(assert
 (let (($x902 (>= scope_32_23 0)))
 (and $x902 (< scope_32_23 8192))))
(assert
 (let (($x449 (>= scope_32_24 0)))
 (and $x449 (< scope_32_24 8192))))
(assert
 (let (($x68 (>= scope_32_25 0)))
 (and $x68 (< scope_32_25 8192))))
(assert
 (let (($x736 (>= scope_32_26 0)))
 (and $x736 (< scope_32_26 8192))))
(assert
 (let (($x35 (>= scope_32_27 0)))
 (and $x35 (< scope_32_27 8192))))
(assert
 (let (($x52 (>= scope_32_28 0)))
 (and $x52 (< scope_32_28 8192))))
(assert
 (let (($x738 (>= scope_32_29 0)))
 (and $x738 (< scope_32_29 8192))))
(assert
 (let (($x261 (>= scope_32_30 0)))
 (and $x261 (< scope_32_30 8192))))
(assert
 (let (($x391 (>= scope_32_31 0)))
 (and $x391 (< scope_32_31 8192))))
(assert
 (let ((?x156 (ite (= scope_32_30 query_32) 30 (ite (= scope_32_31 query_32) 31 (- 1)))))
(let ((?x978 (ite (= scope_32_28 query_32) 28 (ite (= scope_32_29 query_32) 29 ?x156))))
(let ((?x753 (ite (= scope_32_26 query_32) 26 (ite (= scope_32_27 query_32) 27 ?x978))))
(let ((?x715 (ite (= scope_32_24 query_32) 24 (ite (= scope_32_25 query_32) 25 ?x753))))
(let ((?x429 (ite (= scope_32_22 query_32) 22 (ite (= scope_32_23 query_32) 23 ?x715))))
(let ((?x244 (ite (= scope_32_20 query_32) 20 (ite (= scope_32_21 query_32) 21 ?x429))))
(let ((?x51 (ite (= scope_32_18 query_32) 18 (ite (= scope_32_19 query_32) 19 ?x244))))
(let ((?x31 (ite (= scope_32_16 query_32) 16 (ite (= scope_32_17 query_32) 17 ?x51))))
(let ((?x74 (ite (= scope_32_14 query_32) 14 (ite (= scope_32_15 query_32) 15 ?x31))))
(let ((?x38 (ite (= scope_32_12 query_32) 12 (ite (= scope_32_13 query_32) 13 ?x74))))
(let ((?x546 (ite (= scope_32_10 query_32) 10 (ite (= scope_32_11 query_32) 11 ?x38))))
(let ((?x1569 (ite (= scope_32_8 query_32) 8 (ite (= scope_32_9 query_32) 9 ?x546))))
(let ((?x1614 (ite (= scope_32_6 query_32) 6 (ite (= scope_32_7 query_32) 7 ?x1569))))
(let ((?x1342 (ite (= scope_32_4 query_32) 4 (ite (= scope_32_5 query_32) 5 ?x1614))))
(let ((?x1621 (ite (= scope_32_2 query_32) 2 (ite (= scope_32_3 query_32) 3 ?x1342))))
(let ((?x1581 (ite (= scope_32_0 query_32) 0 (ite (= scope_32_1 query_32) 1 ?x1621))))
(let ((?x1324 (ite (< scope_32_0 query_32) (- 1) (ite (< query_32 scope_32_0) (- 1) 0))))
(let ((?x657 (ite (< scope_32_1 query_32) (- 1) (ite (< query_32 scope_32_1) ?x1324 1))))
(let ((?x445 (ite (< scope_32_3 query_32) (- 1) (ite (< query_32 scope_32_3) (- 1) 3))))
(let ((?x212 (ite (< scope_32_2 query_32) ?x445 (ite (< query_32 scope_32_2) ?x657 2))))
(let ((?x1289 (ite (< scope_32_5 query_32) (- 1) (ite (< query_32 scope_32_5) (- 1) 5))))
(let ((?x479 (ite (< scope_32_7 query_32) (- 1) (ite (< query_32 scope_32_7) (- 1) 7))))
(let ((?x1381 (ite (< scope_32_6 query_32) ?x479 (ite (< query_32 scope_32_6) ?x1289 6))))
(let ((?x114 (ite (< scope_32_4 query_32) ?x1381 (ite (< query_32 scope_32_4) ?x212 4))))
(let ((?x17 (ite (< scope_32_9 query_32) (- 1) (ite (< query_32 scope_32_9) (- 1) 9))))
(let ((?x284 (ite (< scope_32_11 query_32) (- 1) (ite (< query_32 scope_32_11) (- 1) 11))))
(let ((?x732 (ite (< scope_32_10 query_32) ?x284 (ite (< query_32 scope_32_10) ?x17 10))))
(let ((?x53 (ite (< scope_32_13 query_32) (- 1) (ite (< query_32 scope_32_13) (- 1) 13))))
(let ((?x23 (ite (< scope_32_15 query_32) (- 1) (ite (< query_32 scope_32_15) (- 1) 15))))
(let ((?x492 (ite (< scope_32_14 query_32) ?x23 (ite (< query_32 scope_32_14) ?x53 14))))
(let ((?x79 (ite (< scope_32_12 query_32) ?x492 (ite (< query_32 scope_32_12) ?x732 12))))
(let ((?x898 (ite (< scope_32_8 query_32) ?x79 (ite (< query_32 scope_32_8) ?x114 8))))
(let ((?x69 (ite (< scope_32_17 query_32) (- 1) (ite (< query_32 scope_32_17) (- 1) 17))))
(let ((?x780 (ite (< scope_32_19 query_32) (- 1) (ite (< query_32 scope_32_19) (- 1) 19))))
(let ((?x251 (ite (< scope_32_18 query_32) ?x780 (ite (< query_32 scope_32_18) ?x69 18))))
(let ((?x937 (ite (< scope_32_21 query_32) (- 1) (ite (< query_32 scope_32_21) (- 1) 21))))
(let ((?x781 (ite (< scope_32_23 query_32) (- 1) (ite (< query_32 scope_32_23) (- 1) 23))))
(let ((?x14 (ite (< scope_32_22 query_32) ?x781 (ite (< query_32 scope_32_22) ?x937 22))))
(let ((?x190 (ite (< scope_32_20 query_32) ?x14 (ite (< query_32 scope_32_20) ?x251 20))))
(let ((?x573 (ite (< scope_32_25 query_32) (- 1) (ite (< query_32 scope_32_25) (- 1) 25))))
(let ((?x76 (ite (< scope_32_27 query_32) (- 1) (ite (< query_32 scope_32_27) (- 1) 27))))
(let ((?x615 (ite (< scope_32_26 query_32) ?x76 (ite (< query_32 scope_32_26) ?x573 26))))
(let ((?x57 (ite (< scope_32_29 query_32) (- 1) (ite (< query_32 scope_32_29) (- 1) 29))))
(let ((?x223 (ite (< scope_32_31 query_32) (- 1) (ite (< query_32 scope_32_31) (- 1) 31))))
(let ((?x203 (ite (< scope_32_30 query_32) ?x223 (ite (< query_32 scope_32_30) ?x57 30))))
(let ((?x399 (ite (< scope_32_28 query_32) ?x203 (ite (< query_32 scope_32_28) ?x615 28))))
(let ((?x610 (ite (< scope_32_24 query_32) ?x399 (ite (< query_32 scope_32_24) ?x190 24))))
(let ((?x1303 (ite (< scope_32_16 query_32) ?x610 (ite (< query_32 scope_32_16) ?x898 16))))
(let (($x772 (and (< scope_32_0 scope_32_1) (< scope_32_1 scope_32_2) (< scope_32_2 scope_32_3) (< scope_32_3 scope_32_4) (< scope_32_4 scope_32_5) (< scope_32_5 scope_32_6) (< scope_32_6 scope_32_7) (< scope_32_7 scope_32_8) (< scope_32_8 scope_32_9) (< scope_32_9 scope_32_10) (< scope_32_10 scope_32_11) (< scope_32_11 scope_32_12) (< scope_32_12 scope_32_13) (< scope_32_13 scope_32_14) (< scope_32_14 scope_32_15) (< scope_32_15 scope_32_16) (< scope_32_16 scope_32_17) (< scope_32_17 scope_32_18) (< scope_32_18 scope_32_19) (< scope_32_19 scope_32_20) (< scope_32_20 scope_32_21) (< scope_32_21 scope_32_22) (< scope_32_22 scope_32_23) (< scope_32_23 scope_32_24) (< scope_32_24 scope_32_25) (< scope_32_25 scope_32_26) (< scope_32_26 scope_32_27) (< scope_32_27 scope_32_28) (< scope_32_28 scope_32_29) (< scope_32_29 scope_32_30) (< scope_32_30 scope_32_31))))
(let ((?x1076 (ite $x772 ?x1303 ?x1581)))
(and (distinct ?x1076 ?x1581) true))))))))))))))))))))))))))))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 19: first-match-equivalence-64
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun scope_64_0 () Int)
(declare-fun scope_64_1 () Int)
(declare-fun scope_64_2 () Int)
(declare-fun scope_64_3 () Int)
(declare-fun scope_64_4 () Int)
(declare-fun scope_64_5 () Int)
(declare-fun scope_64_6 () Int)
(declare-fun scope_64_7 () Int)
(declare-fun scope_64_8 () Int)
(declare-fun scope_64_9 () Int)
(declare-fun scope_64_10 () Int)
(declare-fun scope_64_11 () Int)
(declare-fun scope_64_12 () Int)
(declare-fun scope_64_13 () Int)
(declare-fun scope_64_14 () Int)
(declare-fun scope_64_15 () Int)
(declare-fun scope_64_16 () Int)
(declare-fun scope_64_17 () Int)
(declare-fun scope_64_18 () Int)
(declare-fun scope_64_19 () Int)
(declare-fun scope_64_20 () Int)
(declare-fun scope_64_21 () Int)
(declare-fun scope_64_22 () Int)
(declare-fun scope_64_23 () Int)
(declare-fun scope_64_24 () Int)
(declare-fun scope_64_25 () Int)
(declare-fun scope_64_26 () Int)
(declare-fun scope_64_27 () Int)
(declare-fun scope_64_28 () Int)
(declare-fun scope_64_29 () Int)
(declare-fun scope_64_30 () Int)
(declare-fun scope_64_31 () Int)
(declare-fun scope_64_32 () Int)
(declare-fun scope_64_33 () Int)
(declare-fun scope_64_34 () Int)
(declare-fun scope_64_35 () Int)
(declare-fun scope_64_36 () Int)
(declare-fun scope_64_37 () Int)
(declare-fun scope_64_38 () Int)
(declare-fun scope_64_39 () Int)
(declare-fun scope_64_40 () Int)
(declare-fun scope_64_41 () Int)
(declare-fun scope_64_42 () Int)
(declare-fun scope_64_43 () Int)
(declare-fun scope_64_44 () Int)
(declare-fun scope_64_45 () Int)
(declare-fun scope_64_46 () Int)
(declare-fun scope_64_47 () Int)
(declare-fun scope_64_48 () Int)
(declare-fun scope_64_49 () Int)
(declare-fun scope_64_50 () Int)
(declare-fun scope_64_51 () Int)
(declare-fun scope_64_52 () Int)
(declare-fun scope_64_53 () Int)
(declare-fun scope_64_54 () Int)
(declare-fun scope_64_55 () Int)
(declare-fun scope_64_56 () Int)
(declare-fun scope_64_57 () Int)
(declare-fun scope_64_58 () Int)
(declare-fun scope_64_59 () Int)
(declare-fun scope_64_60 () Int)
(declare-fun scope_64_61 () Int)
(declare-fun scope_64_62 () Int)
(declare-fun scope_64_63 () Int)
(declare-fun query_64 () Int)
(assert
 (let (($x1341 (>= scope_64_0 0)))
 (and $x1341 (< scope_64_0 8192))))
(assert
 (let (($x1125 (>= scope_64_1 0)))
 (and $x1125 (< scope_64_1 8192))))
(assert
 (let (($x1801 (>= scope_64_2 0)))
 (and $x1801 (< scope_64_2 8192))))
(assert
 (let (($x1583 (>= scope_64_3 0)))
 (and $x1583 (< scope_64_3 8192))))
(assert
 (let (($x1826 (>= scope_64_4 0)))
 (and $x1826 (< scope_64_4 8192))))
(assert
 (let (($x1739 (>= scope_64_5 0)))
 (and $x1739 (< scope_64_5 8192))))
(assert
 (let (($x1218 (>= scope_64_6 0)))
 (and $x1218 (< scope_64_6 8192))))
(assert
 (let (($x1612 (>= scope_64_7 0)))
 (and $x1612 (< scope_64_7 8192))))
(assert
 (let (($x1036 (>= scope_64_8 0)))
 (and $x1036 (< scope_64_8 8192))))
(assert
 (let (($x1331 (>= scope_64_9 0)))
 (and $x1331 (< scope_64_9 8192))))
(assert
 (let (($x523 (>= scope_64_10 0)))
 (and $x523 (< scope_64_10 8192))))
(assert
 (let (($x1738 (>= scope_64_11 0)))
 (and $x1738 (< scope_64_11 8192))))
(assert
 (let (($x1168 (>= scope_64_12 0)))
 (and $x1168 (< scope_64_12 8192))))
(assert
 (let (($x1743 (>= scope_64_13 0)))
 (and $x1743 (< scope_64_13 8192))))
(assert
 (let (($x180 (>= scope_64_14 0)))
 (and $x180 (< scope_64_14 8192))))
(assert
 (let (($x1283 (>= scope_64_15 0)))
 (and $x1283 (< scope_64_15 8192))))
(assert
 (let (($x475 (>= scope_64_16 0)))
 (and $x475 (< scope_64_16 8192))))
(assert
 (let (($x322 (>= scope_64_17 0)))
 (and $x322 (< scope_64_17 8192))))
(assert
 (let (($x1270 (>= scope_64_18 0)))
 (and $x1270 (< scope_64_18 8192))))
(assert
 (let (($x1316 (>= scope_64_19 0)))
 (and $x1316 (< scope_64_19 8192))))
(assert
 (let (($x1627 (>= scope_64_20 0)))
 (and $x1627 (< scope_64_20 8192))))
(assert
 (let (($x925 (>= scope_64_21 0)))
 (and $x925 (< scope_64_21 8192))))
(assert
 (let (($x131 (>= scope_64_22 0)))
 (and $x131 (< scope_64_22 8192))))
(assert
 (let (($x92 (>= scope_64_23 0)))
 (and $x92 (< scope_64_23 8192))))
(assert
 (let (($x705 (>= scope_64_24 0)))
 (and $x705 (< scope_64_24 8192))))
(assert
 (let (($x577 (>= scope_64_25 0)))
 (and $x577 (< scope_64_25 8192))))
(assert
 (let (($x790 (>= scope_64_26 0)))
 (and $x790 (< scope_64_26 8192))))
(assert
 (let (($x717 (>= scope_64_27 0)))
 (and $x717 (< scope_64_27 8192))))
(assert
 (let (($x1567 (>= scope_64_28 0)))
 (and $x1567 (< scope_64_28 8192))))
(assert
 (let (($x1388 (>= scope_64_29 0)))
 (and $x1388 (< scope_64_29 8192))))
(assert
 (let (($x1526 (>= scope_64_30 0)))
 (and $x1526 (< scope_64_30 8192))))
(assert
 (let (($x1677 (>= scope_64_31 0)))
 (and $x1677 (< scope_64_31 8192))))
(assert
 (let (($x1059 (>= scope_64_32 0)))
 (and $x1059 (< scope_64_32 8192))))
(assert
 (let (($x1674 (>= scope_64_33 0)))
 (and $x1674 (< scope_64_33 8192))))
(assert
 (let (($x1337 (>= scope_64_34 0)))
 (and $x1337 (< scope_64_34 8192))))
(assert
 (let (($x1133 (>= scope_64_35 0)))
 (and $x1133 (< scope_64_35 8192))))
(assert
 (let (($x1375 (>= scope_64_36 0)))
 (and $x1375 (< scope_64_36 8192))))
(assert
 (let (($x976 (>= scope_64_37 0)))
 (and $x976 (< scope_64_37 8192))))
(assert
 (let (($x428 (>= scope_64_38 0)))
 (and $x428 (< scope_64_38 8192))))
(assert
 (let (($x1199 (>= scope_64_39 0)))
 (and $x1199 (< scope_64_39 8192))))
(assert
 (let (($x1759 (>= scope_64_40 0)))
 (and $x1759 (< scope_64_40 8192))))
(assert
 (let (($x1519 (>= scope_64_41 0)))
 (and $x1519 (< scope_64_41 8192))))
(assert
 (let (($x526 (>= scope_64_42 0)))
 (and $x526 (< scope_64_42 8192))))
(assert
 (let (($x922 (>= scope_64_43 0)))
 (and $x922 (< scope_64_43 8192))))
(assert
 (let (($x165 (>= scope_64_44 0)))
 (and $x165 (< scope_64_44 8192))))
(assert
 (let (($x1752 (>= scope_64_45 0)))
 (and $x1752 (< scope_64_45 8192))))
(assert
 (let (($x169 (>= scope_64_46 0)))
 (and $x169 (< scope_64_46 8192))))
(assert
 (let (($x1748 (>= scope_64_47 0)))
 (and $x1748 (< scope_64_47 8192))))
(assert
 (let (($x553 (>= scope_64_48 0)))
 (and $x553 (< scope_64_48 8192))))
(assert
 (let (($x1746 (>= scope_64_49 0)))
 (and $x1746 (< scope_64_49 8192))))
(assert
 (let (($x1507 (>= scope_64_50 0)))
 (and $x1507 (< scope_64_50 8192))))
(assert
 (let (($x512 (>= scope_64_51 0)))
 (and $x512 (< scope_64_51 8192))))
(assert
 (let (($x1821 (>= scope_64_52 0)))
 (and $x1821 (< scope_64_52 8192))))
(assert
 (let (($x1820 (>= scope_64_53 0)))
 (and $x1820 (< scope_64_53 8192))))
(assert
 (let (($x1732 (>= scope_64_54 0)))
 (and $x1732 (< scope_64_54 8192))))
(assert
 (let (($x1720 (>= scope_64_55 0)))
 (and $x1720 (< scope_64_55 8192))))
(assert
 (let (($x1802 (>= scope_64_56 0)))
 (and $x1802 (< scope_64_56 8192))))
(assert
 (let (($x1669 (>= scope_64_57 0)))
 (and $x1669 (< scope_64_57 8192))))
(assert
 (let (($x256 (>= scope_64_58 0)))
 (and $x256 (< scope_64_58 8192))))
(assert
 (let (($x1891 (>= scope_64_59 0)))
 (and $x1891 (< scope_64_59 8192))))
(assert
 (let (($x1483 (>= scope_64_60 0)))
 (and $x1483 (< scope_64_60 8192))))
(assert
 (let (($x1522 (>= scope_64_61 0)))
 (and $x1522 (< scope_64_61 8192))))
(assert
 (let (($x1102 (>= scope_64_62 0)))
 (and $x1102 (< scope_64_62 8192))))
(assert
 (let (($x1599 (>= scope_64_63 0)))
 (and $x1599 (< scope_64_63 8192))))
(assert
 (let ((?x959 (ite (= scope_64_62 query_64) 62 (ite (= scope_64_63 query_64) 63 (- 1)))))
(let ((?x1887 (ite (= scope_64_60 query_64) 60 (ite (= scope_64_61 query_64) 61 ?x959))))
(let ((?x1878 (ite (= scope_64_58 query_64) 58 (ite (= scope_64_59 query_64) 59 ?x1887))))
(let ((?x19 (ite (= scope_64_56 query_64) 56 (ite (= scope_64_57 query_64) 57 ?x1878))))
(let ((?x598 (ite (= scope_64_54 query_64) 54 (ite (= scope_64_55 query_64) 55 ?x19))))
(let ((?x823 (ite (= scope_64_52 query_64) 52 (ite (= scope_64_53 query_64) 53 ?x598))))
(let ((?x1414 (ite (= scope_64_50 query_64) 50 (ite (= scope_64_51 query_64) 51 ?x823))))
(let ((?x476 (ite (= scope_64_48 query_64) 48 (ite (= scope_64_49 query_64) 49 ?x1414))))
(let ((?x1628 (ite (= scope_64_46 query_64) 46 (ite (= scope_64_47 query_64) 47 ?x476))))
(let ((?x176 (ite (= scope_64_44 query_64) 44 (ite (= scope_64_45 query_64) 45 ?x1628))))
(let ((?x1010 (ite (= scope_64_42 query_64) 42 (ite (= scope_64_43 query_64) 43 ?x176))))
(let ((?x168 (ite (= scope_64_40 query_64) 40 (ite (= scope_64_41 query_64) 41 ?x1010))))
(let ((?x20 (ite (= scope_64_38 query_64) 38 (ite (= scope_64_39 query_64) 39 ?x168))))
(let ((?x213 (ite (= scope_64_36 query_64) 36 (ite (= scope_64_37 query_64) 37 ?x20))))
(let ((?x559 (ite (= scope_64_34 query_64) 34 (ite (= scope_64_35 query_64) 35 ?x213))))
(let ((?x84 (ite (= scope_64_32 query_64) 32 (ite (= scope_64_33 query_64) 33 ?x559))))
(let ((?x47 (ite (= scope_64_30 query_64) 30 (ite (= scope_64_31 query_64) 31 ?x84))))
(let ((?x41 (ite (= scope_64_28 query_64) 28 (ite (= scope_64_29 query_64) 29 ?x47))))
(let ((?x87 (ite (= scope_64_26 query_64) 26 (ite (= scope_64_27 query_64) 27 ?x41))))
(let ((?x172 (ite (= scope_64_24 query_64) 24 (ite (= scope_64_25 query_64) 25 ?x87))))
(let ((?x434 (ite (= scope_64_22 query_64) 22 (ite (= scope_64_23 query_64) 23 ?x172))))
(let ((?x1451 (ite (= scope_64_20 query_64) 20 (ite (= scope_64_21 query_64) 21 ?x434))))
(let ((?x568 (ite (= scope_64_18 query_64) 18 (ite (= scope_64_19 query_64) 19 ?x1451))))
(let ((?x1077 (ite (= scope_64_16 query_64) 16 (ite (= scope_64_17 query_64) 17 ?x568))))
(let ((?x1249 (ite (= scope_64_14 query_64) 14 (ite (= scope_64_15 query_64) 15 ?x1077))))
(let ((?x1373 (ite (= scope_64_12 query_64) 12 (ite (= scope_64_13 query_64) 13 ?x1249))))
(let ((?x1726 (ite (= scope_64_10 query_64) 10 (ite (= scope_64_11 query_64) 11 ?x1373))))
(let ((?x1257 (ite (= scope_64_8 query_64) 8 (ite (= scope_64_9 query_64) 9 ?x1726))))
(let ((?x1329 (ite (= scope_64_6 query_64) 6 (ite (= scope_64_7 query_64) 7 ?x1257))))
(let ((?x1272 (ite (= scope_64_4 query_64) 4 (ite (= scope_64_5 query_64) 5 ?x1329))))
(let ((?x299 (ite (= scope_64_2 query_64) 2 (ite (= scope_64_3 query_64) 3 ?x1272))))
(let ((?x377 (ite (= scope_64_0 query_64) 0 (ite (= scope_64_1 query_64) 1 ?x299))))
(let ((?x243 (ite (< scope_64_0 query_64) (- 1) (ite (< query_64 scope_64_0) (- 1) 0))))
(let ((?x1832 (ite (< scope_64_1 query_64) (- 1) (ite (< query_64 scope_64_1) ?x243 1))))
(let ((?x1312 (ite (< scope_64_3 query_64) (- 1) (ite (< query_64 scope_64_3) (- 1) 3))))
(let ((?x393 (ite (< scope_64_2 query_64) ?x1312 (ite (< query_64 scope_64_2) ?x1832 2))))
(let ((?x95 (ite (< scope_64_5 query_64) (- 1) (ite (< query_64 scope_64_5) (- 1) 5))))
(let ((?x293 (ite (< scope_64_7 query_64) (- 1) (ite (< query_64 scope_64_7) (- 1) 7))))
(let ((?x276 (ite (< scope_64_6 query_64) ?x293 (ite (< query_64 scope_64_6) ?x95 6))))
(let ((?x1107 (ite (< scope_64_4 query_64) ?x276 (ite (< query_64 scope_64_4) ?x393 4))))
(let ((?x944 (ite (< scope_64_9 query_64) (- 1) (ite (< query_64 scope_64_9) (- 1) 9))))
(let ((?x82 (ite (< scope_64_11 query_64) (- 1) (ite (< query_64 scope_64_11) (- 1) 11))))
(let ((?x614 (ite (< scope_64_10 query_64) ?x82 (ite (< query_64 scope_64_10) ?x944 10))))
(let ((?x1017 (ite (< scope_64_13 query_64) (- 1) (ite (< query_64 scope_64_13) (- 1) 13))))
(let ((?x1691 (ite (< scope_64_15 query_64) (- 1) (ite (< query_64 scope_64_15) (- 1) 15))))
(let ((?x291 (ite (< scope_64_14 query_64) ?x1691 (ite (< query_64 scope_64_14) ?x1017 14))))
(let ((?x8 (ite (< scope_64_12 query_64) ?x291 (ite (< query_64 scope_64_12) ?x614 12))))
(let ((?x325 (ite (< scope_64_8 query_64) ?x8 (ite (< query_64 scope_64_8) ?x1107 8))))
(let ((?x954 (ite (< scope_64_17 query_64) (- 1) (ite (< query_64 scope_64_17) (- 1) 17))))
(let ((?x493 (ite (< scope_64_19 query_64) (- 1) (ite (< query_64 scope_64_19) (- 1) 19))))
(let ((?x1074 (ite (< scope_64_18 query_64) ?x493 (ite (< query_64 scope_64_18) ?x954 18))))
(let ((?x929 (ite (< scope_64_21 query_64) (- 1) (ite (< query_64 scope_64_21) (- 1) 21))))
(let ((?x248 (ite (< scope_64_23 query_64) (- 1) (ite (< query_64 scope_64_23) (- 1) 23))))
(let ((?x769 (ite (< scope_64_22 query_64) ?x248 (ite (< query_64 scope_64_22) ?x929 22))))
(let ((?x1864 (ite (< scope_64_20 query_64) ?x769 (ite (< query_64 scope_64_20) ?x1074 20))))
(let ((?x418 (ite (< scope_64_25 query_64) (- 1) (ite (< query_64 scope_64_25) (- 1) 25))))
(let ((?x63 (ite (< scope_64_27 query_64) (- 1) (ite (< query_64 scope_64_27) (- 1) 27))))
(let ((?x272 (ite (< scope_64_26 query_64) ?x63 (ite (< query_64 scope_64_26) ?x418 26))))
(let ((?x1219 (ite (< scope_64_29 query_64) (- 1) (ite (< query_64 scope_64_29) (- 1) 29))))
(let ((?x1487 (ite (< scope_64_31 query_64) (- 1) (ite (< query_64 scope_64_31) (- 1) 31))))
(let ((?x1575 (ite (< scope_64_30 query_64) ?x1487 (ite (< query_64 scope_64_30) ?x1219 30))))
(let ((?x30 (ite (< scope_64_28 query_64) ?x1575 (ite (< query_64 scope_64_28) ?x272 28))))
(let ((?x467 (ite (< scope_64_24 query_64) ?x30 (ite (< query_64 scope_64_24) ?x1864 24))))
(let ((?x584 (ite (< scope_64_16 query_64) ?x467 (ite (< query_64 scope_64_16) ?x325 16))))
(let ((?x1345 (ite (< scope_64_33 query_64) (- 1) (ite (< query_64 scope_64_33) (- 1) 33))))
(let ((?x1874 (ite (< scope_64_35 query_64) (- 1) (ite (< query_64 scope_64_35) (- 1) 35))))
(let ((?x1104 (ite (< scope_64_34 query_64) ?x1874 (ite (< query_64 scope_64_34) ?x1345 34))))
(let ((?x469 (ite (< scope_64_37 query_64) (- 1) (ite (< query_64 scope_64_37) (- 1) 37))))
(let ((?x1085 (ite (< scope_64_39 query_64) (- 1) (ite (< query_64 scope_64_39) (- 1) 39))))
(let ((?x583 (ite (< scope_64_38 query_64) ?x1085 (ite (< query_64 scope_64_38) ?x469 38))))
(let ((?x1376 (ite (< scope_64_36 query_64) ?x583 (ite (< query_64 scope_64_36) ?x1104 36))))
(let ((?x353 (ite (< scope_64_41 query_64) (- 1) (ite (< query_64 scope_64_41) (- 1) 41))))
(let ((?x1210 (ite (< scope_64_43 query_64) (- 1) (ite (< query_64 scope_64_43) (- 1) 43))))
(let ((?x1892 (ite (< scope_64_42 query_64) ?x1210 (ite (< query_64 scope_64_42) ?x353 42))))
(let ((?x519 (ite (< scope_64_45 query_64) (- 1) (ite (< query_64 scope_64_45) (- 1) 45))))
(let ((?x1880 (ite (< scope_64_47 query_64) (- 1) (ite (< query_64 scope_64_47) (- 1) 47))))
(let ((?x397 (ite (< scope_64_46 query_64) ?x1880 (ite (< query_64 scope_64_46) ?x519 46))))
(let ((?x1749 (ite (< scope_64_44 query_64) ?x397 (ite (< query_64 scope_64_44) ?x1892 44))))
(let ((?x43 (ite (< scope_64_40 query_64) ?x1749 (ite (< query_64 scope_64_40) ?x1376 40))))
(let ((?x157 (ite (< scope_64_49 query_64) (- 1) (ite (< query_64 scope_64_49) (- 1) 49))))
(let ((?x1110 (ite (< scope_64_51 query_64) (- 1) (ite (< query_64 scope_64_51) (- 1) 51))))
(let ((?x1866 (ite (< scope_64_50 query_64) ?x1110 (ite (< query_64 scope_64_50) ?x157 50))))
(let ((?x323 (ite (< scope_64_53 query_64) (- 1) (ite (< query_64 scope_64_53) (- 1) 53))))
(let ((?x524 (ite (< scope_64_55 query_64) (- 1) (ite (< query_64 scope_64_55) (- 1) 55))))
(let ((?x1099 (ite (< scope_64_54 query_64) ?x524 (ite (< query_64 scope_64_54) ?x323 54))))
(let ((?x1535 (ite (< scope_64_52 query_64) ?x1099 (ite (< query_64 scope_64_52) ?x1866 52))))
(let ((?x1485 (ite (< scope_64_57 query_64) (- 1) (ite (< query_64 scope_64_57) (- 1) 57))))
(let ((?x945 (ite (< scope_64_59 query_64) (- 1) (ite (< query_64 scope_64_59) (- 1) 59))))
(let ((?x536 (ite (< scope_64_58 query_64) ?x945 (ite (< query_64 scope_64_58) ?x1485 58))))
(let ((?x676 (ite (< scope_64_61 query_64) (- 1) (ite (< query_64 scope_64_61) (- 1) 61))))
(let ((?x912 (ite (< scope_64_63 query_64) (- 1) (ite (< query_64 scope_64_63) (- 1) 63))))
(let ((?x744 (ite (< scope_64_62 query_64) ?x912 (ite (< query_64 scope_64_62) ?x676 62))))
(let ((?x1856 (ite (< scope_64_60 query_64) ?x744 (ite (< query_64 scope_64_60) ?x536 60))))
(let ((?x1416 (ite (< scope_64_56 query_64) ?x1856 (ite (< query_64 scope_64_56) ?x1535 56))))
(let ((?x716 (ite (< scope_64_48 query_64) ?x1416 (ite (< query_64 scope_64_48) ?x43 48))))
(let ((?x808 (ite (< scope_64_32 query_64) ?x716 (ite (< query_64 scope_64_32) ?x584 32))))
(let (($x636 (and (< scope_64_0 scope_64_1) (< scope_64_1 scope_64_2) (< scope_64_2 scope_64_3) (< scope_64_3 scope_64_4) (< scope_64_4 scope_64_5) (< scope_64_5 scope_64_6) (< scope_64_6 scope_64_7) (< scope_64_7 scope_64_8) (< scope_64_8 scope_64_9) (< scope_64_9 scope_64_10) (< scope_64_10 scope_64_11) (< scope_64_11 scope_64_12) (< scope_64_12 scope_64_13) (< scope_64_13 scope_64_14) (< scope_64_14 scope_64_15) (< scope_64_15 scope_64_16) (< scope_64_16 scope_64_17) (< scope_64_17 scope_64_18) (< scope_64_18 scope_64_19) (< scope_64_19 scope_64_20) (< scope_64_20 scope_64_21) (< scope_64_21 scope_64_22) (< scope_64_22 scope_64_23) (< scope_64_23 scope_64_24) (< scope_64_24 scope_64_25) (< scope_64_25 scope_64_26) (< scope_64_26 scope_64_27) (< scope_64_27 scope_64_28) (< scope_64_28 scope_64_29) (< scope_64_29 scope_64_30) (< scope_64_30 scope_64_31) (< scope_64_31 scope_64_32) (< scope_64_32 scope_64_33) (< scope_64_33 scope_64_34) (< scope_64_34 scope_64_35) (< scope_64_35 scope_64_36) (< scope_64_36 scope_64_37) (< scope_64_37 scope_64_38) (< scope_64_38 scope_64_39) (< scope_64_39 scope_64_40) (< scope_64_40 scope_64_41) (< scope_64_41 scope_64_42) (< scope_64_42 scope_64_43) (< scope_64_43 scope_64_44) (< scope_64_44 scope_64_45) (< scope_64_45 scope_64_46) (< scope_64_46 scope_64_47) (< scope_64_47 scope_64_48) (< scope_64_48 scope_64_49) (< scope_64_49 scope_64_50) (< scope_64_50 scope_64_51) (< scope_64_51 scope_64_52) (< scope_64_52 scope_64_53) (< scope_64_53 scope_64_54) (< scope_64_54 scope_64_55) (< scope_64_55 scope_64_56) (< scope_64_56 scope_64_57) (< scope_64_57 scope_64_58) (< scope_64_58 scope_64_59) (< scope_64_59 scope_64_60) (< scope_64_60 scope_64_61) (< scope_64_61 scope_64_62) (< scope_64_62 scope_64_63))))
(let ((?x154 (ite $x636 ?x808 ?x377)))
(and (distinct ?x154 ?x377) true))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 20: complete-scope-kind-not-aliased
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun resolver () Int)
(declare-fun packed () Int)
(assert
 (let (($x644 (or (>= packed 32768) (= resolver 0))))
 (let (($x908 (<= resolver 65535)))
 (let (($x3251 (>= resolver 0)))
 (let (($x2566 (<= packed 65535)))
 (let (($x3714 (>= packed 0)))
 (and $x3714 $x2566 (< (mod packed 32768) 8192) $x3251 $x908 $x644)))))))
(assert
 (let ((?x2002 (mod packed 32768)))
(>= ?x2002 8192)))
; EXPECT: unsat
(check-sat)
; Query 21: intrinsic-authority-remains-canonical
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun resolver () Int)
(declare-fun packed () Int)
(assert
 (let (($x3605 (= resolver 0)))
 (let (($x2747 (>= packed 32768)))
 (let (($x644 (or $x2747 $x3605)))
 (let (($x908 (<= resolver 65535)))
 (let (($x3251 (>= resolver 0)))
 (let (($x2566 (<= packed 65535)))
 (let (($x3714 (>= packed 0)))
 (and $x3714 $x2566 (< (mod packed 32768) 8192) $x3251 $x908 $x644)))))))))
(assert
 (< packed 32768))
(assert
 (and (distinct resolver 0) true))
; EXPECT: unsat
(check-sat)
; Query 22: maximum-dynamic-resolver-remains-valid
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun resolver () Int)
(declare-fun packed () Int)
(assert
 (let (($x644 (or (>= packed 32768) (= resolver 0))))
 (let (($x908 (<= resolver 65535)))
 (let (($x3251 (>= resolver 0)))
 (let (($x2566 (<= packed 65535)))
 (let (($x3714 (>= packed 0)))
 (and $x3714 $x2566 (< (mod packed 32768) 8192) $x3251 $x908 $x644)))))))
(assert
 (= packed 32768))
(assert
 (= resolver 65535))
; EXPECT: sat
(check-sat)
; Query 23: unsorted-first-match-remains-valid
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(assert
 (= (ite true 0 (ite false 1 (ite true 2 (- 1)))) 0))
; EXPECT: sat
(check-sat)
; Query 24: immutable-validation-reuse
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun canonical_before () Bool)
(declare-fun canonical_after () Bool)
(assert
 canonical_before)
(assert
 (= canonical_after canonical_before))
(assert
 (not canonical_after))
; EXPECT: unsat
(check-sat)
; Query 25: mutation-would-break-certificate
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun canonical_before () Bool)
(declare-fun canonical_after () Bool)
(assert
 canonical_before)
(assert
 (not canonical_after))
; EXPECT: sat
(check-sat)
