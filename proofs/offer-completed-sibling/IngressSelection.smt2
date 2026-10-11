; Standalone SMT obligations; every SAT witness and UNSAT claim is replayed.
; Query 1: 
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 0))
(assert
 (let (($x23 (<= frame 1)))
 (let (($x22 (>= frame 0)))
 (let (($x11 (= current_lane observed_lane)))
 (let ((?x12 (ite $x11 current pending_head)))
 (let (($x27 (>= ?x12 5)))
 (let (($x25 (= observed_lane 1)))
 (let (($x19 (>= ?x12 1)))
 (let (($x17 (= observed_lane 0)))
 (let ((?x33 (ite (and $x17 $x19 (< ?x12 4) $x22 $x23) 0 (ite (and $x25 $x27 (< ?x12 8) $x22 $x23) 1 (- 1)))))
 (= ?x33 (- 1))))))))))))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x54 (>= pending_head 5)))
(let (($x25 (= observed_lane 1)))
(let (($x51 (>= pending_head 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x58 (ite (and $x17 $x51 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x54 (< pending_head 8) $x22 $x23) 1 (- 1)))))
(= ?x58 1)))))))))
; EXPECT: sat
(check-sat)
; Query 2: exact_existing_rejection_frame_0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 0))
; EXPECT: sat
(check-sat)
; Query 3: exact_existing_rejection_frame_0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 0))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x11 (= current_lane observed_lane)))
(let ((?x12 (ite $x11 current pending_head)))
(let (($x27 (>= ?x12 5)))
(let (($x25 (= observed_lane 1)))
(let (($x19 (>= ?x12 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x33 (ite (and $x17 $x19 (< ?x12 4) $x22 $x23) 0 (ite (and $x25 $x27 (< ?x12 8) $x22 $x23) 1 (- 1)))))
(and (distinct ?x33 (- 1)) true)))))))))))
; EXPECT: unsat
(check-sat)
; Query 4: candidate_acceptance_frame_0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 0))
; EXPECT: sat
(check-sat)
; Query 5: candidate_acceptance_frame_0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 0))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let ((?x15 (ite (and (= current_lane observed_lane) (not current_done)) current pending_head)))
(let (($x25 (= observed_lane 1)))
(let ((?x40 (ite (and $x25 (>= ?x15 5) (< ?x15 8) $x22 $x23) 1 (- 1))))
(let (($x17 (= observed_lane 0)))
(let ((?x41 (ite (and $x17 (>= ?x15 1) (< ?x15 4) $x22 $x23) 0 ?x40)))
(and (distinct ?x41 1) true)))))))))
; EXPECT: unsat
(check-sat)
; Query 6: 
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 1))
(assert
 (let (($x23 (<= frame 1)))
 (let (($x22 (>= frame 0)))
 (let (($x11 (= current_lane observed_lane)))
 (let ((?x12 (ite $x11 current pending_head)))
 (let (($x27 (>= ?x12 5)))
 (let (($x25 (= observed_lane 1)))
 (let (($x19 (>= ?x12 1)))
 (let (($x17 (= observed_lane 0)))
 (let ((?x33 (ite (and $x17 $x19 (< ?x12 4) $x22 $x23) 0 (ite (and $x25 $x27 (< ?x12 8) $x22 $x23) 1 (- 1)))))
 (= ?x33 (- 1))))))))))))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x55 (>= pending_head 5)))
(let (($x25 (= observed_lane 1)))
(let (($x52 (>= pending_head 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
(= ?x59 1)))))))))
; EXPECT: sat
(check-sat)
; Query 7: exact_existing_rejection_frame_1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 1))
; EXPECT: sat
(check-sat)
; Query 8: exact_existing_rejection_frame_1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 1))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x11 (= current_lane observed_lane)))
(let ((?x12 (ite $x11 current pending_head)))
(let (($x27 (>= ?x12 5)))
(let (($x25 (= observed_lane 1)))
(let (($x19 (>= ?x12 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x33 (ite (and $x17 $x19 (< ?x12 4) $x22 $x23) 0 (ite (and $x25 $x27 (< ?x12 8) $x22 $x23) 1 (- 1)))))
(and (distinct ?x33 (- 1)) true)))))))))))
; EXPECT: unsat
(check-sat)
; Query 9: candidate_acceptance_frame_1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 1))
; EXPECT: sat
(check-sat)
; Query 10: candidate_acceptance_frame_1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (= current 4))
(assert
 (= current_lane 1))
(assert
 current_done)
(assert
 (= observed_lane 1))
(assert
 (= pending_head 5))
(assert
 (= frame 1))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let ((?x15 (ite (and (= current_lane observed_lane) (not current_done)) current pending_head)))
(let (($x25 (= observed_lane 1)))
(let ((?x40 (ite (and $x25 (>= ?x15 5) (< ?x15 8) $x22 $x23) 1 (- 1))))
(let (($x17 (= observed_lane 0)))
(let ((?x41 (ite (and $x17 (>= ?x15 1) (< ?x15 4) $x22 $x23) 0 ?x40)))
(and (distinct ?x41 1) true)))))))))
; EXPECT: unsat
(check-sat)
; Query 11: completed_cursor_uses_pending_head
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
; EXPECT: sat
(check-sat)
; Query 12: completed_cursor_uses_pending_head
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
(assert
 (let ((?x15 (ite (and (= current_lane observed_lane) (not current_done)) current pending_head)))
(and (distinct ?x15 pending_head) true)))
; EXPECT: unsat
(check-sat)
; Query 13: live_cursor_behavior_unchanged
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (not current_done))
; EXPECT: sat
(check-sat)
; Query 14: live_cursor_behavior_unchanged
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (not current_done))
(assert
 (let (($x11 (= current_lane observed_lane)))
(let ((?x12 (ite $x11 current pending_head)))
(let ((?x15 (ite (and $x11 (not current_done)) current pending_head)))
(and (distinct ?x15 ?x12) true)))))
; EXPECT: unsat
(check-sat)
; Query 15: foreign_cursor_behavior_unchanged
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (and (distinct current_lane observed_lane) true))
; EXPECT: sat
(check-sat)
; Query 16: foreign_cursor_behavior_unchanged
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 (and (distinct current_lane observed_lane) true))
(assert
 (let (($x11 (= current_lane observed_lane)))
(let ((?x12 (ite $x11 current pending_head)))
(let ((?x15 (ite (and $x11 (not current_done)) current pending_head)))
(and (distinct ?x15 ?x12) true)))))
; EXPECT: unsat
(check-sat)
; Query 17: valid_pending_scope_not_hidden
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x55 (>= pending_head 5)))
(let (($x25 (= observed_lane 1)))
(let (($x52 (>= pending_head 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
(>= ?x59 0)))))))))
; EXPECT: sat
(check-sat)
; Query 18: valid_pending_scope_not_hidden
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
(assert
 (let (($x23 (<= frame 1)))
 (let (($x22 (>= frame 0)))
 (let (($x55 (>= pending_head 5)))
 (let (($x25 (= observed_lane 1)))
 (let (($x52 (>= pending_head 1)))
 (let (($x17 (= observed_lane 0)))
 (let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
 (>= ?x59 0)))))))))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x55 (>= pending_head 5)))
(let (($x25 (= observed_lane 1)))
(let (($x52 (>= pending_head 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
(let ((?x15 (ite (and (= current_lane observed_lane) (not current_done)) current pending_head)))
(let ((?x40 (ite (and $x25 (>= ?x15 5) (< ?x15 8) $x22 $x23) 1 (- 1))))
(let ((?x41 (ite (and $x17 (>= ?x15 1) (< ?x15 4) $x22 $x23) 0 ?x40)))
(and (distinct ?x41 ?x59) true))))))))))))
; EXPECT: unsat
(check-sat)
; Query 19: no_pending_scope_is_not_invented
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let (($x55 (>= pending_head 5)))
(let (($x25 (= observed_lane 1)))
(let (($x52 (>= pending_head 1)))
(let (($x17 (= observed_lane 0)))
(let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
(= ?x59 (- 1))))))))))
; EXPECT: sat
(check-sat)
; Query 20: no_pending_scope_is_not_invented
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun current () Int)
(declare-fun current_lane () Int)
(declare-fun observed_lane () Int)
(declare-fun pending_head () Int)
(declare-fun frame () Int)
(declare-fun current_done () Bool)
(assert
 (>= current 0))
(assert
 (>= current_lane (- 1)))
(assert
 (>= observed_lane 0))
(assert
 (>= pending_head (- 1)))
(assert
 (>= frame 0))
(assert
 current_done)
(assert
 (let (($x23 (<= frame 1)))
 (let (($x22 (>= frame 0)))
 (let (($x55 (>= pending_head 5)))
 (let (($x25 (= observed_lane 1)))
 (let (($x52 (>= pending_head 1)))
 (let (($x17 (= observed_lane 0)))
 (let ((?x59 (ite (and $x17 $x52 (< pending_head 4) $x22 $x23) 0 (ite (and $x25 $x55 (< pending_head 8) $x22 $x23) 1 (- 1)))))
 (= ?x59 (- 1))))))))))
(assert
 (let (($x23 (<= frame 1)))
(let (($x22 (>= frame 0)))
(let ((?x15 (ite (and (= current_lane observed_lane) (not current_done)) current pending_head)))
(let (($x25 (= observed_lane 1)))
(let ((?x40 (ite (and $x25 (>= ?x15 5) (< ?x15 8) $x22 $x23) 1 (- 1))))
(let (($x17 (= observed_lane 0)))
(let ((?x41 (ite (and $x17 (>= ?x15 1) (< ?x15 4) $x22 $x23) 0 ?x40)))
(and (distinct ?x41 (- 1)) true)))))))))
; EXPECT: unsat
(check-sat)
