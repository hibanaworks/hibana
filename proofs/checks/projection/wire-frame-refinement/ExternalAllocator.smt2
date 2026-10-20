; Local mathematical obligations; runtime coverage remains an explicit premise.
; Query 1: unique_exit_after_enter_premises_sat
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun marker_count () Int)
(declare-fun enter () Int)
(declare-fun unique_exit () Int)
(declare-fun matches_closing_scope_and_tag () (Array Int Bool))
(declare-fun full_first () Int)
(declare-fun suffix_first () Int)
(assert
 (> marker_count 0))
(assert
 (>= enter 0))
(assert
 (< enter unique_exit))
(assert
 (< unique_exit marker_count))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x180 (= $x143 (= j unique_exit))))
 (=> (and (>= j 0) (< j marker_count)) $x180))))
 )
(assert
 (>= full_first 0))
(assert
 (< full_first marker_count))
(assert
 (select matches_closing_scope_and_tag full_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (>= j 0) (< j full_first)) $x147))))
 )
(assert
 (> suffix_first enter))
(assert
 (< suffix_first marker_count))
(assert
 (select matches_closing_scope_and_tag suffix_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
(let (($x147 (not $x143)))
(=> (and (> j enter) (< j suffix_first)) $x147))))
)
; EXPECT: sat
(check-sat)
; Query 2: full_list_and_suffix_find_same_exit_under_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun marker_count () Int)
(declare-fun enter () Int)
(declare-fun unique_exit () Int)
(declare-fun matches_closing_scope_and_tag () (Array Int Bool))
(declare-fun full_first () Int)
(declare-fun suffix_first () Int)
(assert
 (> marker_count 0))
(assert
 (>= enter 0))
(assert
 (< enter unique_exit))
(assert
 (< unique_exit marker_count))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x180 (= $x143 (= j unique_exit))))
 (=> (and (>= j 0) (< j marker_count)) $x180))))
 )
(assert
 (>= full_first 0))
(assert
 (< full_first marker_count))
(assert
 (select matches_closing_scope_and_tag full_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (>= j 0) (< j full_first)) $x147))))
 )
(assert
 (> suffix_first enter))
(assert
 (< suffix_first marker_count))
(assert
 (select matches_closing_scope_and_tag suffix_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (> j enter) (< j suffix_first)) $x147))))
 )
(assert
 (and (distinct full_first suffix_first) true))
; EXPECT: unsat
(check-sat)
; Query 3: matching_segment_end_offsets_equal_under_premise
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun marker_count () Int)
(declare-fun enter () Int)
(declare-fun unique_exit () Int)
(declare-fun matches_closing_scope_and_tag () (Array Int Bool))
(declare-fun full_first () Int)
(declare-fun suffix_first () Int)
(declare-fun marker_offsets () (Array Int Int))
(assert
 (> marker_count 0))
(assert
 (>= enter 0))
(assert
 (< enter unique_exit))
(assert
 (< unique_exit marker_count))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x180 (= $x143 (= j unique_exit))))
 (=> (and (>= j 0) (< j marker_count)) $x180))))
 )
(assert
 (>= full_first 0))
(assert
 (< full_first marker_count))
(assert
 (select matches_closing_scope_and_tag full_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (>= j 0) (< j full_first)) $x147))))
 )
(assert
 (> suffix_first enter))
(assert
 (< suffix_first marker_count))
(assert
 (select matches_closing_scope_and_tag suffix_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (> j enter) (< j suffix_first)) $x147))))
 )
(assert
 (let ((?x1658 (select marker_offsets suffix_first)))
(let ((?x7792 (select marker_offsets full_first)))
(and (distinct ?x7792 ?x1658) true))))
; EXPECT: unsat
(check-sat)
; Query 4: unique_exit_is_in_full_and_suffix_search_domains
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun marker_count () Int)
(declare-fun enter () Int)
(declare-fun unique_exit () Int)
(declare-fun matches_closing_scope_and_tag () (Array Int Bool))
(assert
 (> marker_count 0))
(assert
 (>= enter 0))
(assert
 (< enter unique_exit))
(assert
 (< unique_exit marker_count))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x180 (= $x143 (= j unique_exit))))
 (=> (and (>= j 0) (< j marker_count)) $x180))))
 )
(assert
 (let (($x626 (>= unique_exit marker_count)))
(let (($x728 (<= unique_exit enter)))
(let (($x730 (not (select matches_closing_scope_and_tag unique_exit))))
(or $x730 $x728 $x626)))))
; EXPECT: unsat
(check-sat)
; Query 5: earlier_duplicate_exit_breaks_bridge_negative_control
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun marker_count () Int)
(declare-fun enter () Int)
(declare-fun full_first () Int)
(declare-fun suffix_first () Int)
(declare-fun matches_closing_scope_and_tag () (Array Int Bool))
(assert
 (= marker_count 3))
(assert
 (= enter 1))
(assert
 (= full_first 0))
(assert
 (= suffix_first 2))
(assert
 (select matches_closing_scope_and_tag 0))
(assert
 (not (select matches_closing_scope_and_tag 1)))
(assert
 (select matches_closing_scope_and_tag 2))
(assert
 (>= full_first 0))
(assert
 (< full_first marker_count))
(assert
 (select matches_closing_scope_and_tag full_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (>= j 0) (< j full_first)) $x147))))
 )
(assert
 (> suffix_first enter))
(assert
 (< suffix_first marker_count))
(assert
 (select matches_closing_scope_and_tag suffix_first))
(assert
 (forall ((j Int) )(let (($x143 (select matches_closing_scope_and_tag j)))
 (let (($x147 (not $x143)))
 (=> (and (> j enter) (< j suffix_first)) $x147))))
 )
(assert
 (and (distinct full_first suffix_first) true))
; EXPECT: sat
(check-sat)
; Query 6: missing_exit_external_zero_former_fallback_owner_negative_control
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun atom_count () Int)
(declare-fun start () Int)
(declare-fun event () Int)
(declare-fun ordinal () Int)
(assert
 (> atom_count 0))
(assert
 (>= start 0))
(assert
 (<= start event))
(assert
 (< event atom_count))
(assert
 (>= ordinal 0))
(assert
 (< ordinal 8192))
(assert
 (and (distinct (+ ordinal 1) 0) true))
; EXPECT: sat
(check-sat)
; Query 7: prior_color_membership_induction_premises_sat
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun tail_reverse () Bool)
(declare-fun tail_forward () Bool)
(declare-fun entry_conflicts () Bool)
(declare-fun candidate_color () Int)
(declare-fun entry_color () Int)
(assert
 (= tail_forward tail_reverse))
(assert
 entry_conflicts)
(assert
 (= entry_color candidate_color))
(assert
 (not tail_forward))
; EXPECT: sat
(check-sat)
; Query 8: append_filterMap_reverse_filter_map_membership_step
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun tail_reverse () Bool)
(declare-fun tail_forward () Bool)
(declare-fun candidate_color () Int)
(declare-fun entry_color () Int)
(declare-fun entry_conflicts () Bool)
(assert
 (= tail_forward tail_reverse))
(assert
 (let (($x179 (and entry_conflicts (= entry_color candidate_color))))
(let (($x138 (or tail_reverse $x179)))
(let (($x81 (or $x179 tail_forward)))
(and (distinct $x81 $x138) true)))))
; EXPECT: unsat
(check-sat)
; Query 9: empty_prior_membership_base
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(assert
 (and (distinct false false) true))
; EXPECT: unsat
(check-sat)
; Query 10: drop_matching_assignment_negative_control
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun entry_conflicts () Bool)
(declare-fun candidate_color () Int)
(declare-fun entry_color () Int)
(declare-fun tail_forward () Bool)
(assert
 entry_conflicts)
(assert
 (= entry_color candidate_color))
(assert
 (not tail_forward))
(assert
 (let (($x172 (= entry_color candidate_color)))
(let (($x179 (and entry_conflicts $x172)))
(let (($x81 (or $x179 tail_forward)))
(and (distinct $x81 tail_forward) true)))))
; EXPECT: sat
(check-sat)
; Query 11: symbolic_prior_set_equivalence_length_0
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(assert
 (and (distinct or or) true))
; EXPECT: unsat
(check-sat)
; Query 12: symbolic_prior_set_equivalence_length_1
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_1_color_0 () Int)
(declare-fun prefix_1_filter_0 () Bool)
(assert
 (let (($x198 (and prefix_1_filter_0 (= prefix_1_color_0 candidate_color))))
(let (($x3455 (or $x198)))
(and (distinct $x3455 $x3455) true))))
; EXPECT: unsat
(check-sat)
; Query 13: symbolic_prior_set_equivalence_length_2
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_2_color_0 () Int)
(declare-fun prefix_2_filter_0 () Bool)
(declare-fun prefix_2_color_1 () Int)
(declare-fun prefix_2_filter_1 () Bool)
(assert
 (let (($x4800 (= prefix_2_color_0 candidate_color)))
(let (($x4089 (and prefix_2_filter_0 $x4800)))
(let (($x193 (= prefix_2_color_1 candidate_color)))
(let (($x263 (and prefix_2_filter_1 $x193)))
(let (($x305 (or $x263 $x4089)))
(let (($x264 (or $x4089 $x263)))
(and (distinct $x264 $x305) true))))))))
; EXPECT: unsat
(check-sat)
; Query 14: symbolic_prior_set_equivalence_length_3
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_3_color_0 () Int)
(declare-fun prefix_3_filter_0 () Bool)
(declare-fun prefix_3_color_1 () Int)
(declare-fun prefix_3_filter_1 () Bool)
(declare-fun prefix_3_color_2 () Int)
(declare-fun prefix_3_filter_2 () Bool)
(assert
 (let (($x284 (and prefix_3_filter_0 (= prefix_3_color_0 candidate_color))))
(let (($x268 (and prefix_3_filter_1 (= prefix_3_color_1 candidate_color))))
(let (($x128 (and prefix_3_filter_2 (= prefix_3_color_2 candidate_color))))
(let (($x144 (or $x128 $x268 $x284)))
(let (($x4990 (or $x284 $x268 $x128)))
(and (distinct $x4990 $x144) true)))))))
; EXPECT: unsat
(check-sat)
; Query 15: symbolic_prior_set_equivalence_length_4
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_4_color_0 () Int)
(declare-fun prefix_4_filter_0 () Bool)
(declare-fun prefix_4_color_1 () Int)
(declare-fun prefix_4_filter_1 () Bool)
(declare-fun prefix_4_color_2 () Int)
(declare-fun prefix_4_filter_2 () Bool)
(declare-fun prefix_4_color_3 () Int)
(declare-fun prefix_4_filter_3 () Bool)
(assert
 (let (($x730 (= prefix_4_color_0 candidate_color)))
(let (($x4432 (and prefix_4_filter_0 $x730)))
(let (($x620 (= prefix_4_color_1 candidate_color)))
(let (($x211 (and prefix_4_filter_1 $x620)))
(let (($x195 (= prefix_4_color_2 candidate_color)))
(let (($x264 (and prefix_4_filter_2 $x195)))
(let (($x193 (= prefix_4_color_3 candidate_color)))
(let (($x2917 (and prefix_4_filter_3 $x193)))
(let (($x4089 (or $x2917 $x264 $x211 $x4432)))
(let (($x185 (or $x4432 $x211 $x264 $x2917)))
(and (distinct $x185 $x4089) true))))))))))))
; EXPECT: unsat
(check-sat)
; Query 16: symbolic_prior_set_equivalence_length_5
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_5_color_0 () Int)
(declare-fun prefix_5_filter_0 () Bool)
(declare-fun prefix_5_color_1 () Int)
(declare-fun prefix_5_filter_1 () Bool)
(declare-fun prefix_5_color_2 () Int)
(declare-fun prefix_5_filter_2 () Bool)
(declare-fun prefix_5_color_3 () Int)
(declare-fun prefix_5_filter_3 () Bool)
(declare-fun prefix_5_color_4 () Int)
(declare-fun prefix_5_filter_4 () Bool)
(assert
 (let (($x119 (= prefix_5_color_0 candidate_color)))
(let (($x166 (and prefix_5_filter_0 $x119)))
(let (($x189 (= prefix_5_color_1 candidate_color)))
(let (($x266 (and prefix_5_filter_1 $x189)))
(let (($x53 (= prefix_5_color_2 candidate_color)))
(let (($x140 (and prefix_5_filter_2 $x53)))
(let (($x5438 (= prefix_5_color_3 candidate_color)))
(let (($x334 (and prefix_5_filter_3 $x5438)))
(let (($x311 (= prefix_5_color_4 candidate_color)))
(let (($x245 (and prefix_5_filter_4 $x311)))
(let (($x319 (or $x245 $x334 $x140 $x266 $x166)))
(let (($x248 (or $x166 $x266 $x140 $x334 $x245)))
(and (distinct $x248 $x319) true))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 17: symbolic_prior_set_equivalence_length_6
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_6_color_0 () Int)
(declare-fun prefix_6_filter_0 () Bool)
(declare-fun prefix_6_color_1 () Int)
(declare-fun prefix_6_filter_1 () Bool)
(declare-fun prefix_6_color_2 () Int)
(declare-fun prefix_6_filter_2 () Bool)
(declare-fun prefix_6_color_3 () Int)
(declare-fun prefix_6_filter_3 () Bool)
(declare-fun prefix_6_color_4 () Int)
(declare-fun prefix_6_filter_4 () Bool)
(declare-fun prefix_6_color_5 () Int)
(declare-fun prefix_6_filter_5 () Bool)
(assert
 (let (($x330 (= prefix_6_color_0 candidate_color)))
(let (($x5947 (and prefix_6_filter_0 $x330)))
(let (($x1668 (= prefix_6_color_1 candidate_color)))
(let (($x225 (and prefix_6_filter_1 $x1668)))
(let (($x223 (= prefix_6_color_2 candidate_color)))
(let (($x358 (and prefix_6_filter_2 $x223)))
(let (($x2037 (= prefix_6_color_3 candidate_color)))
(let (($x354 (and prefix_6_filter_3 $x2037)))
(let (($x378 (= prefix_6_color_4 candidate_color)))
(let (($x372 (and prefix_6_filter_4 $x378)))
(let (($x242 (= prefix_6_color_5 candidate_color)))
(let (($x230 (and prefix_6_filter_5 $x242)))
(let (($x368 (or $x230 $x372 $x354 $x358 $x225 $x5947)))
(let (($x130 (or $x5947 $x225 $x358 $x354 $x372 $x230)))
(and (distinct $x130 $x368) true))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 18: symbolic_prior_set_equivalence_length_7
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_7_color_0 () Int)
(declare-fun prefix_7_filter_0 () Bool)
(declare-fun prefix_7_color_1 () Int)
(declare-fun prefix_7_filter_1 () Bool)
(declare-fun prefix_7_color_2 () Int)
(declare-fun prefix_7_filter_2 () Bool)
(declare-fun prefix_7_color_3 () Int)
(declare-fun prefix_7_filter_3 () Bool)
(declare-fun prefix_7_color_4 () Int)
(declare-fun prefix_7_filter_4 () Bool)
(declare-fun prefix_7_color_5 () Int)
(declare-fun prefix_7_filter_5 () Bool)
(declare-fun prefix_7_color_6 () Int)
(declare-fun prefix_7_filter_6 () Bool)
(assert
 (let (($x728 (= prefix_7_color_0 candidate_color)))
(let (($x231 (and prefix_7_filter_0 $x728)))
(let (($x245 (= prefix_7_color_1 candidate_color)))
(let (($x319 (and prefix_7_filter_1 $x245)))
(let (($x333 (= prefix_7_color_2 candidate_color)))
(let (($x5177 (and prefix_7_filter_2 $x333)))
(let (($x2115 (= prefix_7_color_3 candidate_color)))
(let (($x255 (and prefix_7_filter_3 $x2115)))
(let (($x626 (= prefix_7_color_4 candidate_color)))
(let (($x337 (and prefix_7_filter_4 $x626)))
(let (($x271 (= prefix_7_color_5 candidate_color)))
(let (($x237 (and prefix_7_filter_5 $x271)))
(let (($x191 (= prefix_7_color_6 candidate_color)))
(let (($x190 (and prefix_7_filter_6 $x191)))
(let (($x40 (or $x190 $x237 $x337 $x255 $x5177 $x319 $x231)))
(let (($x183 (or $x231 $x319 $x5177 $x255 $x337 $x237 $x190)))
(and (distinct $x183 $x40) true))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 19: symbolic_prior_set_equivalence_length_8
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun candidate_color () Int)
(declare-fun prefix_8_color_0 () Int)
(declare-fun prefix_8_filter_0 () Bool)
(declare-fun prefix_8_color_1 () Int)
(declare-fun prefix_8_filter_1 () Bool)
(declare-fun prefix_8_color_2 () Int)
(declare-fun prefix_8_filter_2 () Bool)
(declare-fun prefix_8_color_3 () Int)
(declare-fun prefix_8_filter_3 () Bool)
(declare-fun prefix_8_color_4 () Int)
(declare-fun prefix_8_filter_4 () Bool)
(declare-fun prefix_8_color_5 () Int)
(declare-fun prefix_8_filter_5 () Bool)
(declare-fun prefix_8_color_6 () Int)
(declare-fun prefix_8_filter_6 () Bool)
(declare-fun prefix_8_color_7 () Int)
(declare-fun prefix_8_filter_7 () Bool)
(assert
 (let (($x70 (= prefix_8_color_0 candidate_color)))
(let (($x132 (and prefix_8_filter_0 $x70)))
(let (($x61 (= prefix_8_color_1 candidate_color)))
(let (($x339 (and prefix_8_filter_1 $x61)))
(let (($x195 (= prefix_8_color_2 candidate_color)))
(let (($x268 (and prefix_8_filter_2 $x195)))
(let (($x524 (= prefix_8_color_3 candidate_color)))
(let (($x3051 (and prefix_8_filter_3 $x524)))
(let (($x130 (= prefix_8_color_4 candidate_color)))
(let (($x368 (and prefix_8_filter_4 $x130)))
(let (($x2410 (= prefix_8_color_5 candidate_color)))
(let (($x182 (and prefix_8_filter_5 $x2410)))
(let (($x3893 (= prefix_8_color_6 candidate_color)))
(let (($x2512 (and prefix_8_filter_6 $x3893)))
(let (($x194 (= prefix_8_color_7 candidate_color)))
(let (($x181 (and prefix_8_filter_7 $x194)))
(let (($x51 (or $x181 $x2512 $x182 $x368 $x3051 $x268 $x339 $x132)))
(let (($x152 (or $x132 $x339 $x268 $x3051 $x368 $x182 $x2512 $x181)))
(and (distinct $x152 $x51) true))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 20: equal_used_color_sets_give_same_first_color
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun reverse_used_0 () Bool)
(declare-fun chronological_used_0 () Bool)
(declare-fun reverse_used_1 () Bool)
(declare-fun chronological_used_1 () Bool)
(declare-fun reverse_used_2 () Bool)
(declare-fun chronological_used_2 () Bool)
(declare-fun reverse_used_3 () Bool)
(declare-fun chronological_used_3 () Bool)
(declare-fun reverse_used_4 () Bool)
(declare-fun chronological_used_4 () Bool)
(declare-fun reverse_used_5 () Bool)
(declare-fun chronological_used_5 () Bool)
(declare-fun reverse_used_6 () Bool)
(declare-fun chronological_used_6 () Bool)
(declare-fun reverse_used_7 () Bool)
(declare-fun chronological_used_7 () Bool)
(declare-fun reverse_used_8 () Bool)
(declare-fun chronological_used_8 () Bool)
(declare-fun reverse_used_9 () Bool)
(declare-fun chronological_used_9 () Bool)
(declare-fun reverse_used_10 () Bool)
(declare-fun chronological_used_10 () Bool)
(declare-fun reverse_used_11 () Bool)
(declare-fun chronological_used_11 () Bool)
(declare-fun reverse_used_12 () Bool)
(declare-fun chronological_used_12 () Bool)
(declare-fun reverse_used_13 () Bool)
(declare-fun chronological_used_13 () Bool)
(declare-fun reverse_used_14 () Bool)
(declare-fun chronological_used_14 () Bool)
(declare-fun reverse_used_15 () Bool)
(declare-fun chronological_used_15 () Bool)
(declare-fun reverse_used_16 () Bool)
(declare-fun chronological_used_16 () Bool)
(declare-fun reverse_used_17 () Bool)
(declare-fun chronological_used_17 () Bool)
(declare-fun reverse_used_18 () Bool)
(declare-fun chronological_used_18 () Bool)
(declare-fun reverse_used_19 () Bool)
(declare-fun chronological_used_19 () Bool)
(declare-fun reverse_used_20 () Bool)
(declare-fun chronological_used_20 () Bool)
(declare-fun reverse_used_21 () Bool)
(declare-fun chronological_used_21 () Bool)
(declare-fun reverse_used_22 () Bool)
(declare-fun chronological_used_22 () Bool)
(declare-fun reverse_used_23 () Bool)
(declare-fun chronological_used_23 () Bool)
(declare-fun reverse_used_24 () Bool)
(declare-fun chronological_used_24 () Bool)
(declare-fun reverse_used_25 () Bool)
(declare-fun chronological_used_25 () Bool)
(declare-fun reverse_used_26 () Bool)
(declare-fun chronological_used_26 () Bool)
(declare-fun reverse_used_27 () Bool)
(declare-fun chronological_used_27 () Bool)
(declare-fun reverse_used_28 () Bool)
(declare-fun chronological_used_28 () Bool)
(declare-fun reverse_used_29 () Bool)
(declare-fun chronological_used_29 () Bool)
(declare-fun reverse_used_30 () Bool)
(declare-fun chronological_used_30 () Bool)
(declare-fun reverse_used_31 () Bool)
(declare-fun chronological_used_31 () Bool)
(declare-fun reverse_used_32 () Bool)
(declare-fun chronological_used_32 () Bool)
(declare-fun reverse_used_33 () Bool)
(declare-fun chronological_used_33 () Bool)
(declare-fun reverse_used_34 () Bool)
(declare-fun chronological_used_34 () Bool)
(declare-fun reverse_used_35 () Bool)
(declare-fun chronological_used_35 () Bool)
(declare-fun reverse_used_36 () Bool)
(declare-fun chronological_used_36 () Bool)
(declare-fun reverse_used_37 () Bool)
(declare-fun chronological_used_37 () Bool)
(declare-fun reverse_used_38 () Bool)
(declare-fun chronological_used_38 () Bool)
(declare-fun reverse_used_39 () Bool)
(declare-fun chronological_used_39 () Bool)
(declare-fun reverse_used_40 () Bool)
(declare-fun chronological_used_40 () Bool)
(declare-fun reverse_used_41 () Bool)
(declare-fun chronological_used_41 () Bool)
(declare-fun reverse_used_42 () Bool)
(declare-fun chronological_used_42 () Bool)
(declare-fun reverse_used_43 () Bool)
(declare-fun chronological_used_43 () Bool)
(declare-fun reverse_used_44 () Bool)
(declare-fun chronological_used_44 () Bool)
(declare-fun reverse_used_45 () Bool)
(declare-fun chronological_used_45 () Bool)
(declare-fun reverse_used_46 () Bool)
(declare-fun chronological_used_46 () Bool)
(declare-fun reverse_used_47 () Bool)
(declare-fun chronological_used_47 () Bool)
(declare-fun reverse_used_48 () Bool)
(declare-fun chronological_used_48 () Bool)
(declare-fun reverse_used_49 () Bool)
(declare-fun chronological_used_49 () Bool)
(declare-fun reverse_used_50 () Bool)
(declare-fun chronological_used_50 () Bool)
(declare-fun reverse_used_51 () Bool)
(declare-fun chronological_used_51 () Bool)
(declare-fun reverse_used_52 () Bool)
(declare-fun chronological_used_52 () Bool)
(declare-fun reverse_used_53 () Bool)
(declare-fun chronological_used_53 () Bool)
(declare-fun reverse_used_54 () Bool)
(declare-fun chronological_used_54 () Bool)
(declare-fun reverse_used_55 () Bool)
(declare-fun chronological_used_55 () Bool)
(declare-fun reverse_used_56 () Bool)
(declare-fun chronological_used_56 () Bool)
(declare-fun reverse_used_57 () Bool)
(declare-fun chronological_used_57 () Bool)
(declare-fun reverse_used_58 () Bool)
(declare-fun chronological_used_58 () Bool)
(declare-fun reverse_used_59 () Bool)
(declare-fun chronological_used_59 () Bool)
(declare-fun reverse_used_60 () Bool)
(declare-fun chronological_used_60 () Bool)
(declare-fun reverse_used_61 () Bool)
(declare-fun chronological_used_61 () Bool)
(declare-fun reverse_used_62 () Bool)
(declare-fun chronological_used_62 () Bool)
(declare-fun reverse_used_63 () Bool)
(declare-fun chronological_used_63 () Bool)
(declare-fun reverse_used_64 () Bool)
(declare-fun chronological_used_64 () Bool)
(declare-fun reverse_used_65 () Bool)
(declare-fun chronological_used_65 () Bool)
(declare-fun reverse_used_66 () Bool)
(declare-fun chronological_used_66 () Bool)
(declare-fun reverse_used_67 () Bool)
(declare-fun chronological_used_67 () Bool)
(declare-fun reverse_used_68 () Bool)
(declare-fun chronological_used_68 () Bool)
(declare-fun reverse_used_69 () Bool)
(declare-fun chronological_used_69 () Bool)
(declare-fun reverse_used_70 () Bool)
(declare-fun chronological_used_70 () Bool)
(declare-fun reverse_used_71 () Bool)
(declare-fun chronological_used_71 () Bool)
(declare-fun reverse_used_72 () Bool)
(declare-fun chronological_used_72 () Bool)
(declare-fun reverse_used_73 () Bool)
(declare-fun chronological_used_73 () Bool)
(declare-fun reverse_used_74 () Bool)
(declare-fun chronological_used_74 () Bool)
(declare-fun reverse_used_75 () Bool)
(declare-fun chronological_used_75 () Bool)
(declare-fun reverse_used_76 () Bool)
(declare-fun chronological_used_76 () Bool)
(declare-fun reverse_used_77 () Bool)
(declare-fun chronological_used_77 () Bool)
(declare-fun reverse_used_78 () Bool)
(declare-fun chronological_used_78 () Bool)
(declare-fun reverse_used_79 () Bool)
(declare-fun chronological_used_79 () Bool)
(declare-fun reverse_used_80 () Bool)
(declare-fun chronological_used_80 () Bool)
(declare-fun reverse_used_81 () Bool)
(declare-fun chronological_used_81 () Bool)
(declare-fun reverse_used_82 () Bool)
(declare-fun chronological_used_82 () Bool)
(declare-fun reverse_used_83 () Bool)
(declare-fun chronological_used_83 () Bool)
(declare-fun reverse_used_84 () Bool)
(declare-fun chronological_used_84 () Bool)
(declare-fun reverse_used_85 () Bool)
(declare-fun chronological_used_85 () Bool)
(declare-fun reverse_used_86 () Bool)
(declare-fun chronological_used_86 () Bool)
(declare-fun reverse_used_87 () Bool)
(declare-fun chronological_used_87 () Bool)
(declare-fun reverse_used_88 () Bool)
(declare-fun chronological_used_88 () Bool)
(declare-fun reverse_used_89 () Bool)
(declare-fun chronological_used_89 () Bool)
(declare-fun reverse_used_90 () Bool)
(declare-fun chronological_used_90 () Bool)
(declare-fun reverse_used_91 () Bool)
(declare-fun chronological_used_91 () Bool)
(declare-fun reverse_used_92 () Bool)
(declare-fun chronological_used_92 () Bool)
(declare-fun reverse_used_93 () Bool)
(declare-fun chronological_used_93 () Bool)
(declare-fun reverse_used_94 () Bool)
(declare-fun chronological_used_94 () Bool)
(declare-fun reverse_used_95 () Bool)
(declare-fun chronological_used_95 () Bool)
(declare-fun reverse_used_96 () Bool)
(declare-fun chronological_used_96 () Bool)
(declare-fun reverse_used_97 () Bool)
(declare-fun chronological_used_97 () Bool)
(declare-fun reverse_used_98 () Bool)
(declare-fun chronological_used_98 () Bool)
(declare-fun reverse_used_99 () Bool)
(declare-fun chronological_used_99 () Bool)
(declare-fun reverse_used_100 () Bool)
(declare-fun chronological_used_100 () Bool)
(declare-fun reverse_used_101 () Bool)
(declare-fun chronological_used_101 () Bool)
(declare-fun reverse_used_102 () Bool)
(declare-fun chronological_used_102 () Bool)
(declare-fun reverse_used_103 () Bool)
(declare-fun chronological_used_103 () Bool)
(declare-fun reverse_used_104 () Bool)
(declare-fun chronological_used_104 () Bool)
(declare-fun reverse_used_105 () Bool)
(declare-fun chronological_used_105 () Bool)
(declare-fun reverse_used_106 () Bool)
(declare-fun chronological_used_106 () Bool)
(declare-fun reverse_used_107 () Bool)
(declare-fun chronological_used_107 () Bool)
(declare-fun reverse_used_108 () Bool)
(declare-fun chronological_used_108 () Bool)
(declare-fun reverse_used_109 () Bool)
(declare-fun chronological_used_109 () Bool)
(declare-fun reverse_used_110 () Bool)
(declare-fun chronological_used_110 () Bool)
(declare-fun reverse_used_111 () Bool)
(declare-fun chronological_used_111 () Bool)
(declare-fun reverse_used_112 () Bool)
(declare-fun chronological_used_112 () Bool)
(declare-fun reverse_used_113 () Bool)
(declare-fun chronological_used_113 () Bool)
(declare-fun reverse_used_114 () Bool)
(declare-fun chronological_used_114 () Bool)
(declare-fun reverse_used_115 () Bool)
(declare-fun chronological_used_115 () Bool)
(declare-fun reverse_used_116 () Bool)
(declare-fun chronological_used_116 () Bool)
(declare-fun reverse_used_117 () Bool)
(declare-fun chronological_used_117 () Bool)
(declare-fun reverse_used_118 () Bool)
(declare-fun chronological_used_118 () Bool)
(declare-fun reverse_used_119 () Bool)
(declare-fun chronological_used_119 () Bool)
(declare-fun reverse_used_120 () Bool)
(declare-fun chronological_used_120 () Bool)
(declare-fun reverse_used_121 () Bool)
(declare-fun chronological_used_121 () Bool)
(declare-fun reverse_used_122 () Bool)
(declare-fun chronological_used_122 () Bool)
(declare-fun reverse_used_123 () Bool)
(declare-fun chronological_used_123 () Bool)
(declare-fun reverse_used_124 () Bool)
(declare-fun chronological_used_124 () Bool)
(declare-fun reverse_used_125 () Bool)
(declare-fun chronological_used_125 () Bool)
(declare-fun reverse_used_126 () Bool)
(declare-fun chronological_used_126 () Bool)
(declare-fun reverse_used_127 () Bool)
(declare-fun chronological_used_127 () Bool)
(declare-fun reverse_used_128 () Bool)
(declare-fun chronological_used_128 () Bool)
(declare-fun reverse_used_129 () Bool)
(declare-fun chronological_used_129 () Bool)
(declare-fun reverse_used_130 () Bool)
(declare-fun chronological_used_130 () Bool)
(declare-fun reverse_used_131 () Bool)
(declare-fun chronological_used_131 () Bool)
(declare-fun reverse_used_132 () Bool)
(declare-fun chronological_used_132 () Bool)
(declare-fun reverse_used_133 () Bool)
(declare-fun chronological_used_133 () Bool)
(declare-fun reverse_used_134 () Bool)
(declare-fun chronological_used_134 () Bool)
(declare-fun reverse_used_135 () Bool)
(declare-fun chronological_used_135 () Bool)
(declare-fun reverse_used_136 () Bool)
(declare-fun chronological_used_136 () Bool)
(declare-fun reverse_used_137 () Bool)
(declare-fun chronological_used_137 () Bool)
(declare-fun reverse_used_138 () Bool)
(declare-fun chronological_used_138 () Bool)
(declare-fun reverse_used_139 () Bool)
(declare-fun chronological_used_139 () Bool)
(declare-fun reverse_used_140 () Bool)
(declare-fun chronological_used_140 () Bool)
(declare-fun reverse_used_141 () Bool)
(declare-fun chronological_used_141 () Bool)
(declare-fun reverse_used_142 () Bool)
(declare-fun chronological_used_142 () Bool)
(declare-fun reverse_used_143 () Bool)
(declare-fun chronological_used_143 () Bool)
(declare-fun reverse_used_144 () Bool)
(declare-fun chronological_used_144 () Bool)
(declare-fun reverse_used_145 () Bool)
(declare-fun chronological_used_145 () Bool)
(declare-fun reverse_used_146 () Bool)
(declare-fun chronological_used_146 () Bool)
(declare-fun reverse_used_147 () Bool)
(declare-fun chronological_used_147 () Bool)
(declare-fun reverse_used_148 () Bool)
(declare-fun chronological_used_148 () Bool)
(declare-fun reverse_used_149 () Bool)
(declare-fun chronological_used_149 () Bool)
(declare-fun reverse_used_150 () Bool)
(declare-fun chronological_used_150 () Bool)
(declare-fun reverse_used_151 () Bool)
(declare-fun chronological_used_151 () Bool)
(declare-fun reverse_used_152 () Bool)
(declare-fun chronological_used_152 () Bool)
(declare-fun reverse_used_153 () Bool)
(declare-fun chronological_used_153 () Bool)
(declare-fun reverse_used_154 () Bool)
(declare-fun chronological_used_154 () Bool)
(declare-fun reverse_used_155 () Bool)
(declare-fun chronological_used_155 () Bool)
(declare-fun reverse_used_156 () Bool)
(declare-fun chronological_used_156 () Bool)
(declare-fun reverse_used_157 () Bool)
(declare-fun chronological_used_157 () Bool)
(declare-fun reverse_used_158 () Bool)
(declare-fun chronological_used_158 () Bool)
(declare-fun reverse_used_159 () Bool)
(declare-fun chronological_used_159 () Bool)
(declare-fun reverse_used_160 () Bool)
(declare-fun chronological_used_160 () Bool)
(declare-fun reverse_used_161 () Bool)
(declare-fun chronological_used_161 () Bool)
(declare-fun reverse_used_162 () Bool)
(declare-fun chronological_used_162 () Bool)
(declare-fun reverse_used_163 () Bool)
(declare-fun chronological_used_163 () Bool)
(declare-fun reverse_used_164 () Bool)
(declare-fun chronological_used_164 () Bool)
(declare-fun reverse_used_165 () Bool)
(declare-fun chronological_used_165 () Bool)
(declare-fun reverse_used_166 () Bool)
(declare-fun chronological_used_166 () Bool)
(declare-fun reverse_used_167 () Bool)
(declare-fun chronological_used_167 () Bool)
(declare-fun reverse_used_168 () Bool)
(declare-fun chronological_used_168 () Bool)
(declare-fun reverse_used_169 () Bool)
(declare-fun chronological_used_169 () Bool)
(declare-fun reverse_used_170 () Bool)
(declare-fun chronological_used_170 () Bool)
(declare-fun reverse_used_171 () Bool)
(declare-fun chronological_used_171 () Bool)
(declare-fun reverse_used_172 () Bool)
(declare-fun chronological_used_172 () Bool)
(declare-fun reverse_used_173 () Bool)
(declare-fun chronological_used_173 () Bool)
(declare-fun reverse_used_174 () Bool)
(declare-fun chronological_used_174 () Bool)
(declare-fun reverse_used_175 () Bool)
(declare-fun chronological_used_175 () Bool)
(declare-fun reverse_used_176 () Bool)
(declare-fun chronological_used_176 () Bool)
(declare-fun reverse_used_177 () Bool)
(declare-fun chronological_used_177 () Bool)
(declare-fun reverse_used_178 () Bool)
(declare-fun chronological_used_178 () Bool)
(declare-fun reverse_used_179 () Bool)
(declare-fun chronological_used_179 () Bool)
(declare-fun reverse_used_180 () Bool)
(declare-fun chronological_used_180 () Bool)
(declare-fun reverse_used_181 () Bool)
(declare-fun chronological_used_181 () Bool)
(declare-fun reverse_used_182 () Bool)
(declare-fun chronological_used_182 () Bool)
(declare-fun reverse_used_183 () Bool)
(declare-fun chronological_used_183 () Bool)
(declare-fun reverse_used_184 () Bool)
(declare-fun chronological_used_184 () Bool)
(declare-fun reverse_used_185 () Bool)
(declare-fun chronological_used_185 () Bool)
(declare-fun reverse_used_186 () Bool)
(declare-fun chronological_used_186 () Bool)
(declare-fun reverse_used_187 () Bool)
(declare-fun chronological_used_187 () Bool)
(declare-fun reverse_used_188 () Bool)
(declare-fun chronological_used_188 () Bool)
(declare-fun reverse_used_189 () Bool)
(declare-fun chronological_used_189 () Bool)
(declare-fun reverse_used_190 () Bool)
(declare-fun chronological_used_190 () Bool)
(declare-fun reverse_used_191 () Bool)
(declare-fun chronological_used_191 () Bool)
(declare-fun reverse_used_192 () Bool)
(declare-fun chronological_used_192 () Bool)
(declare-fun reverse_used_193 () Bool)
(declare-fun chronological_used_193 () Bool)
(declare-fun reverse_used_194 () Bool)
(declare-fun chronological_used_194 () Bool)
(declare-fun reverse_used_195 () Bool)
(declare-fun chronological_used_195 () Bool)
(declare-fun reverse_used_196 () Bool)
(declare-fun chronological_used_196 () Bool)
(declare-fun reverse_used_197 () Bool)
(declare-fun chronological_used_197 () Bool)
(declare-fun reverse_used_198 () Bool)
(declare-fun chronological_used_198 () Bool)
(declare-fun reverse_used_199 () Bool)
(declare-fun chronological_used_199 () Bool)
(declare-fun reverse_used_200 () Bool)
(declare-fun chronological_used_200 () Bool)
(declare-fun reverse_used_201 () Bool)
(declare-fun chronological_used_201 () Bool)
(declare-fun reverse_used_202 () Bool)
(declare-fun chronological_used_202 () Bool)
(declare-fun reverse_used_203 () Bool)
(declare-fun chronological_used_203 () Bool)
(declare-fun reverse_used_204 () Bool)
(declare-fun chronological_used_204 () Bool)
(declare-fun reverse_used_205 () Bool)
(declare-fun chronological_used_205 () Bool)
(declare-fun reverse_used_206 () Bool)
(declare-fun chronological_used_206 () Bool)
(declare-fun reverse_used_207 () Bool)
(declare-fun chronological_used_207 () Bool)
(declare-fun reverse_used_208 () Bool)
(declare-fun chronological_used_208 () Bool)
(declare-fun reverse_used_209 () Bool)
(declare-fun chronological_used_209 () Bool)
(declare-fun reverse_used_210 () Bool)
(declare-fun chronological_used_210 () Bool)
(declare-fun reverse_used_211 () Bool)
(declare-fun chronological_used_211 () Bool)
(declare-fun reverse_used_212 () Bool)
(declare-fun chronological_used_212 () Bool)
(declare-fun reverse_used_213 () Bool)
(declare-fun chronological_used_213 () Bool)
(declare-fun reverse_used_214 () Bool)
(declare-fun chronological_used_214 () Bool)
(declare-fun reverse_used_215 () Bool)
(declare-fun chronological_used_215 () Bool)
(declare-fun reverse_used_216 () Bool)
(declare-fun chronological_used_216 () Bool)
(declare-fun reverse_used_217 () Bool)
(declare-fun chronological_used_217 () Bool)
(declare-fun reverse_used_218 () Bool)
(declare-fun chronological_used_218 () Bool)
(declare-fun reverse_used_219 () Bool)
(declare-fun chronological_used_219 () Bool)
(declare-fun reverse_used_220 () Bool)
(declare-fun chronological_used_220 () Bool)
(declare-fun reverse_used_221 () Bool)
(declare-fun chronological_used_221 () Bool)
(declare-fun reverse_used_222 () Bool)
(declare-fun chronological_used_222 () Bool)
(declare-fun reverse_used_223 () Bool)
(declare-fun chronological_used_223 () Bool)
(declare-fun reverse_used_224 () Bool)
(declare-fun chronological_used_224 () Bool)
(declare-fun reverse_used_225 () Bool)
(declare-fun chronological_used_225 () Bool)
(declare-fun reverse_used_226 () Bool)
(declare-fun chronological_used_226 () Bool)
(declare-fun reverse_used_227 () Bool)
(declare-fun chronological_used_227 () Bool)
(declare-fun reverse_used_228 () Bool)
(declare-fun chronological_used_228 () Bool)
(declare-fun reverse_used_229 () Bool)
(declare-fun chronological_used_229 () Bool)
(declare-fun reverse_used_230 () Bool)
(declare-fun chronological_used_230 () Bool)
(declare-fun reverse_used_231 () Bool)
(declare-fun chronological_used_231 () Bool)
(declare-fun reverse_used_232 () Bool)
(declare-fun chronological_used_232 () Bool)
(declare-fun reverse_used_233 () Bool)
(declare-fun chronological_used_233 () Bool)
(declare-fun reverse_used_234 () Bool)
(declare-fun chronological_used_234 () Bool)
(declare-fun reverse_used_235 () Bool)
(declare-fun chronological_used_235 () Bool)
(declare-fun reverse_used_236 () Bool)
(declare-fun chronological_used_236 () Bool)
(declare-fun reverse_used_237 () Bool)
(declare-fun chronological_used_237 () Bool)
(declare-fun reverse_used_238 () Bool)
(declare-fun chronological_used_238 () Bool)
(declare-fun reverse_used_239 () Bool)
(declare-fun chronological_used_239 () Bool)
(declare-fun reverse_used_240 () Bool)
(declare-fun chronological_used_240 () Bool)
(declare-fun reverse_used_241 () Bool)
(declare-fun chronological_used_241 () Bool)
(declare-fun reverse_used_242 () Bool)
(declare-fun chronological_used_242 () Bool)
(declare-fun reverse_used_243 () Bool)
(declare-fun chronological_used_243 () Bool)
(declare-fun reverse_used_244 () Bool)
(declare-fun chronological_used_244 () Bool)
(declare-fun reverse_used_245 () Bool)
(declare-fun chronological_used_245 () Bool)
(declare-fun reverse_used_246 () Bool)
(declare-fun chronological_used_246 () Bool)
(declare-fun reverse_used_247 () Bool)
(declare-fun chronological_used_247 () Bool)
(declare-fun reverse_used_248 () Bool)
(declare-fun chronological_used_248 () Bool)
(declare-fun reverse_used_249 () Bool)
(declare-fun chronological_used_249 () Bool)
(declare-fun reverse_used_250 () Bool)
(declare-fun chronological_used_250 () Bool)
(declare-fun reverse_used_251 () Bool)
(declare-fun chronological_used_251 () Bool)
(declare-fun reverse_used_252 () Bool)
(declare-fun chronological_used_252 () Bool)
(declare-fun reverse_used_253 () Bool)
(declare-fun chronological_used_253 () Bool)
(declare-fun reverse_used_254 () Bool)
(declare-fun chronological_used_254 () Bool)
(declare-fun reverse_used_255 () Bool)
(declare-fun chronological_used_255 () Bool)
(assert
 (= chronological_used_0 reverse_used_0))
(assert
 (= chronological_used_1 reverse_used_1))
(assert
 (= chronological_used_2 reverse_used_2))
(assert
 (= chronological_used_3 reverse_used_3))
(assert
 (= chronological_used_4 reverse_used_4))
(assert
 (= chronological_used_5 reverse_used_5))
(assert
 (= chronological_used_6 reverse_used_6))
(assert
 (= chronological_used_7 reverse_used_7))
(assert
 (= chronological_used_8 reverse_used_8))
(assert
 (= chronological_used_9 reverse_used_9))
(assert
 (= chronological_used_10 reverse_used_10))
(assert
 (= chronological_used_11 reverse_used_11))
(assert
 (= chronological_used_12 reverse_used_12))
(assert
 (= chronological_used_13 reverse_used_13))
(assert
 (= chronological_used_14 reverse_used_14))
(assert
 (= chronological_used_15 reverse_used_15))
(assert
 (= chronological_used_16 reverse_used_16))
(assert
 (= chronological_used_17 reverse_used_17))
(assert
 (= chronological_used_18 reverse_used_18))
(assert
 (= chronological_used_19 reverse_used_19))
(assert
 (= chronological_used_20 reverse_used_20))
(assert
 (= chronological_used_21 reverse_used_21))
(assert
 (= chronological_used_22 reverse_used_22))
(assert
 (= chronological_used_23 reverse_used_23))
(assert
 (= chronological_used_24 reverse_used_24))
(assert
 (= chronological_used_25 reverse_used_25))
(assert
 (= chronological_used_26 reverse_used_26))
(assert
 (= chronological_used_27 reverse_used_27))
(assert
 (= chronological_used_28 reverse_used_28))
(assert
 (= chronological_used_29 reverse_used_29))
(assert
 (= chronological_used_30 reverse_used_30))
(assert
 (= chronological_used_31 reverse_used_31))
(assert
 (= chronological_used_32 reverse_used_32))
(assert
 (= chronological_used_33 reverse_used_33))
(assert
 (= chronological_used_34 reverse_used_34))
(assert
 (= chronological_used_35 reverse_used_35))
(assert
 (= chronological_used_36 reverse_used_36))
(assert
 (= chronological_used_37 reverse_used_37))
(assert
 (= chronological_used_38 reverse_used_38))
(assert
 (= chronological_used_39 reverse_used_39))
(assert
 (= chronological_used_40 reverse_used_40))
(assert
 (= chronological_used_41 reverse_used_41))
(assert
 (= chronological_used_42 reverse_used_42))
(assert
 (= chronological_used_43 reverse_used_43))
(assert
 (= chronological_used_44 reverse_used_44))
(assert
 (= chronological_used_45 reverse_used_45))
(assert
 (= chronological_used_46 reverse_used_46))
(assert
 (= chronological_used_47 reverse_used_47))
(assert
 (= chronological_used_48 reverse_used_48))
(assert
 (= chronological_used_49 reverse_used_49))
(assert
 (= chronological_used_50 reverse_used_50))
(assert
 (= chronological_used_51 reverse_used_51))
(assert
 (= chronological_used_52 reverse_used_52))
(assert
 (= chronological_used_53 reverse_used_53))
(assert
 (= chronological_used_54 reverse_used_54))
(assert
 (= chronological_used_55 reverse_used_55))
(assert
 (= chronological_used_56 reverse_used_56))
(assert
 (= chronological_used_57 reverse_used_57))
(assert
 (= chronological_used_58 reverse_used_58))
(assert
 (= chronological_used_59 reverse_used_59))
(assert
 (= chronological_used_60 reverse_used_60))
(assert
 (= chronological_used_61 reverse_used_61))
(assert
 (= chronological_used_62 reverse_used_62))
(assert
 (= chronological_used_63 reverse_used_63))
(assert
 (= chronological_used_64 reverse_used_64))
(assert
 (= chronological_used_65 reverse_used_65))
(assert
 (= chronological_used_66 reverse_used_66))
(assert
 (= chronological_used_67 reverse_used_67))
(assert
 (= chronological_used_68 reverse_used_68))
(assert
 (= chronological_used_69 reverse_used_69))
(assert
 (= chronological_used_70 reverse_used_70))
(assert
 (= chronological_used_71 reverse_used_71))
(assert
 (= chronological_used_72 reverse_used_72))
(assert
 (= chronological_used_73 reverse_used_73))
(assert
 (= chronological_used_74 reverse_used_74))
(assert
 (= chronological_used_75 reverse_used_75))
(assert
 (= chronological_used_76 reverse_used_76))
(assert
 (= chronological_used_77 reverse_used_77))
(assert
 (= chronological_used_78 reverse_used_78))
(assert
 (= chronological_used_79 reverse_used_79))
(assert
 (= chronological_used_80 reverse_used_80))
(assert
 (= chronological_used_81 reverse_used_81))
(assert
 (= chronological_used_82 reverse_used_82))
(assert
 (= chronological_used_83 reverse_used_83))
(assert
 (= chronological_used_84 reverse_used_84))
(assert
 (= chronological_used_85 reverse_used_85))
(assert
 (= chronological_used_86 reverse_used_86))
(assert
 (= chronological_used_87 reverse_used_87))
(assert
 (= chronological_used_88 reverse_used_88))
(assert
 (= chronological_used_89 reverse_used_89))
(assert
 (= chronological_used_90 reverse_used_90))
(assert
 (= chronological_used_91 reverse_used_91))
(assert
 (= chronological_used_92 reverse_used_92))
(assert
 (= chronological_used_93 reverse_used_93))
(assert
 (= chronological_used_94 reverse_used_94))
(assert
 (= chronological_used_95 reverse_used_95))
(assert
 (= chronological_used_96 reverse_used_96))
(assert
 (= chronological_used_97 reverse_used_97))
(assert
 (= chronological_used_98 reverse_used_98))
(assert
 (= chronological_used_99 reverse_used_99))
(assert
 (= chronological_used_100 reverse_used_100))
(assert
 (= chronological_used_101 reverse_used_101))
(assert
 (= chronological_used_102 reverse_used_102))
(assert
 (= chronological_used_103 reverse_used_103))
(assert
 (= chronological_used_104 reverse_used_104))
(assert
 (= chronological_used_105 reverse_used_105))
(assert
 (= chronological_used_106 reverse_used_106))
(assert
 (= chronological_used_107 reverse_used_107))
(assert
 (= chronological_used_108 reverse_used_108))
(assert
 (= chronological_used_109 reverse_used_109))
(assert
 (= chronological_used_110 reverse_used_110))
(assert
 (= chronological_used_111 reverse_used_111))
(assert
 (= chronological_used_112 reverse_used_112))
(assert
 (= chronological_used_113 reverse_used_113))
(assert
 (= chronological_used_114 reverse_used_114))
(assert
 (= chronological_used_115 reverse_used_115))
(assert
 (= chronological_used_116 reverse_used_116))
(assert
 (= chronological_used_117 reverse_used_117))
(assert
 (= chronological_used_118 reverse_used_118))
(assert
 (= chronological_used_119 reverse_used_119))
(assert
 (= chronological_used_120 reverse_used_120))
(assert
 (= chronological_used_121 reverse_used_121))
(assert
 (= chronological_used_122 reverse_used_122))
(assert
 (= chronological_used_123 reverse_used_123))
(assert
 (= chronological_used_124 reverse_used_124))
(assert
 (= chronological_used_125 reverse_used_125))
(assert
 (= chronological_used_126 reverse_used_126))
(assert
 (= chronological_used_127 reverse_used_127))
(assert
 (= chronological_used_128 reverse_used_128))
(assert
 (= chronological_used_129 reverse_used_129))
(assert
 (= chronological_used_130 reverse_used_130))
(assert
 (= chronological_used_131 reverse_used_131))
(assert
 (= chronological_used_132 reverse_used_132))
(assert
 (= chronological_used_133 reverse_used_133))
(assert
 (= chronological_used_134 reverse_used_134))
(assert
 (= chronological_used_135 reverse_used_135))
(assert
 (= chronological_used_136 reverse_used_136))
(assert
 (= chronological_used_137 reverse_used_137))
(assert
 (= chronological_used_138 reverse_used_138))
(assert
 (= chronological_used_139 reverse_used_139))
(assert
 (= chronological_used_140 reverse_used_140))
(assert
 (= chronological_used_141 reverse_used_141))
(assert
 (= chronological_used_142 reverse_used_142))
(assert
 (= chronological_used_143 reverse_used_143))
(assert
 (= chronological_used_144 reverse_used_144))
(assert
 (= chronological_used_145 reverse_used_145))
(assert
 (= chronological_used_146 reverse_used_146))
(assert
 (= chronological_used_147 reverse_used_147))
(assert
 (= chronological_used_148 reverse_used_148))
(assert
 (= chronological_used_149 reverse_used_149))
(assert
 (= chronological_used_150 reverse_used_150))
(assert
 (= chronological_used_151 reverse_used_151))
(assert
 (= chronological_used_152 reverse_used_152))
(assert
 (= chronological_used_153 reverse_used_153))
(assert
 (= chronological_used_154 reverse_used_154))
(assert
 (= chronological_used_155 reverse_used_155))
(assert
 (= chronological_used_156 reverse_used_156))
(assert
 (= chronological_used_157 reverse_used_157))
(assert
 (= chronological_used_158 reverse_used_158))
(assert
 (= chronological_used_159 reverse_used_159))
(assert
 (= chronological_used_160 reverse_used_160))
(assert
 (= chronological_used_161 reverse_used_161))
(assert
 (= chronological_used_162 reverse_used_162))
(assert
 (= chronological_used_163 reverse_used_163))
(assert
 (= chronological_used_164 reverse_used_164))
(assert
 (= chronological_used_165 reverse_used_165))
(assert
 (= chronological_used_166 reverse_used_166))
(assert
 (= chronological_used_167 reverse_used_167))
(assert
 (= chronological_used_168 reverse_used_168))
(assert
 (= chronological_used_169 reverse_used_169))
(assert
 (= chronological_used_170 reverse_used_170))
(assert
 (= chronological_used_171 reverse_used_171))
(assert
 (= chronological_used_172 reverse_used_172))
(assert
 (= chronological_used_173 reverse_used_173))
(assert
 (= chronological_used_174 reverse_used_174))
(assert
 (= chronological_used_175 reverse_used_175))
(assert
 (= chronological_used_176 reverse_used_176))
(assert
 (= chronological_used_177 reverse_used_177))
(assert
 (= chronological_used_178 reverse_used_178))
(assert
 (= chronological_used_179 reverse_used_179))
(assert
 (= chronological_used_180 reverse_used_180))
(assert
 (= chronological_used_181 reverse_used_181))
(assert
 (= chronological_used_182 reverse_used_182))
(assert
 (= chronological_used_183 reverse_used_183))
(assert
 (= chronological_used_184 reverse_used_184))
(assert
 (= chronological_used_185 reverse_used_185))
(assert
 (= chronological_used_186 reverse_used_186))
(assert
 (= chronological_used_187 reverse_used_187))
(assert
 (= chronological_used_188 reverse_used_188))
(assert
 (= chronological_used_189 reverse_used_189))
(assert
 (= chronological_used_190 reverse_used_190))
(assert
 (= chronological_used_191 reverse_used_191))
(assert
 (= chronological_used_192 reverse_used_192))
(assert
 (= chronological_used_193 reverse_used_193))
(assert
 (= chronological_used_194 reverse_used_194))
(assert
 (= chronological_used_195 reverse_used_195))
(assert
 (= chronological_used_196 reverse_used_196))
(assert
 (= chronological_used_197 reverse_used_197))
(assert
 (= chronological_used_198 reverse_used_198))
(assert
 (= chronological_used_199 reverse_used_199))
(assert
 (= chronological_used_200 reverse_used_200))
(assert
 (= chronological_used_201 reverse_used_201))
(assert
 (= chronological_used_202 reverse_used_202))
(assert
 (= chronological_used_203 reverse_used_203))
(assert
 (= chronological_used_204 reverse_used_204))
(assert
 (= chronological_used_205 reverse_used_205))
(assert
 (= chronological_used_206 reverse_used_206))
(assert
 (= chronological_used_207 reverse_used_207))
(assert
 (= chronological_used_208 reverse_used_208))
(assert
 (= chronological_used_209 reverse_used_209))
(assert
 (= chronological_used_210 reverse_used_210))
(assert
 (= chronological_used_211 reverse_used_211))
(assert
 (= chronological_used_212 reverse_used_212))
(assert
 (= chronological_used_213 reverse_used_213))
(assert
 (= chronological_used_214 reverse_used_214))
(assert
 (= chronological_used_215 reverse_used_215))
(assert
 (= chronological_used_216 reverse_used_216))
(assert
 (= chronological_used_217 reverse_used_217))
(assert
 (= chronological_used_218 reverse_used_218))
(assert
 (= chronological_used_219 reverse_used_219))
(assert
 (= chronological_used_220 reverse_used_220))
(assert
 (= chronological_used_221 reverse_used_221))
(assert
 (= chronological_used_222 reverse_used_222))
(assert
 (= chronological_used_223 reverse_used_223))
(assert
 (= chronological_used_224 reverse_used_224))
(assert
 (= chronological_used_225 reverse_used_225))
(assert
 (= chronological_used_226 reverse_used_226))
(assert
 (= chronological_used_227 reverse_used_227))
(assert
 (= chronological_used_228 reverse_used_228))
(assert
 (= chronological_used_229 reverse_used_229))
(assert
 (= chronological_used_230 reverse_used_230))
(assert
 (= chronological_used_231 reverse_used_231))
(assert
 (= chronological_used_232 reverse_used_232))
(assert
 (= chronological_used_233 reverse_used_233))
(assert
 (= chronological_used_234 reverse_used_234))
(assert
 (= chronological_used_235 reverse_used_235))
(assert
 (= chronological_used_236 reverse_used_236))
(assert
 (= chronological_used_237 reverse_used_237))
(assert
 (= chronological_used_238 reverse_used_238))
(assert
 (= chronological_used_239 reverse_used_239))
(assert
 (= chronological_used_240 reverse_used_240))
(assert
 (= chronological_used_241 reverse_used_241))
(assert
 (= chronological_used_242 reverse_used_242))
(assert
 (= chronological_used_243 reverse_used_243))
(assert
 (= chronological_used_244 reverse_used_244))
(assert
 (= chronological_used_245 reverse_used_245))
(assert
 (= chronological_used_246 reverse_used_246))
(assert
 (= chronological_used_247 reverse_used_247))
(assert
 (= chronological_used_248 reverse_used_248))
(assert
 (= chronological_used_249 reverse_used_249))
(assert
 (= chronological_used_250 reverse_used_250))
(assert
 (= chronological_used_251 reverse_used_251))
(assert
 (= chronological_used_252 reverse_used_252))
(assert
 (= chronological_used_253 reverse_used_253))
(assert
 (= chronological_used_254 reverse_used_254))
(assert
 (= chronological_used_255 reverse_used_255))
(assert
 (let ((?x36 (ite reverse_used_253 (ite reverse_used_254 (ite reverse_used_255 256 255) 254) 253)))
(let ((?x39 (ite reverse_used_250 (ite reverse_used_251 (ite reverse_used_252 ?x36 252) 251) 250)))
(let ((?x2978 (ite reverse_used_247 (ite reverse_used_248 (ite reverse_used_249 ?x39 249) 248) 247)))
(let ((?x2057 (ite reverse_used_244 (ite reverse_used_245 (ite reverse_used_246 ?x2978 246) 245) 244)))
(let ((?x3671 (ite reverse_used_241 (ite reverse_used_242 (ite reverse_used_243 ?x2057 243) 242) 241)))
(let ((?x1472 (ite reverse_used_238 (ite reverse_used_239 (ite reverse_used_240 ?x3671 240) 239) 238)))
(let ((?x8549 (ite reverse_used_235 (ite reverse_used_236 (ite reverse_used_237 ?x1472 237) 236) 235)))
(let ((?x7248 (ite reverse_used_232 (ite reverse_used_233 (ite reverse_used_234 ?x8549 234) 233) 232)))
(let ((?x8891 (ite reverse_used_229 (ite reverse_used_230 (ite reverse_used_231 ?x7248 231) 230) 229)))
(let ((?x5365 (ite reverse_used_226 (ite reverse_used_227 (ite reverse_used_228 ?x8891 228) 227) 226)))
(let ((?x2811 (ite reverse_used_223 (ite reverse_used_224 (ite reverse_used_225 ?x5365 225) 224) 223)))
(let ((?x4670 (ite reverse_used_220 (ite reverse_used_221 (ite reverse_used_222 ?x2811 222) 221) 220)))
(let ((?x3303 (ite reverse_used_217 (ite reverse_used_218 (ite reverse_used_219 ?x4670 219) 218) 217)))
(let ((?x1299 (ite reverse_used_214 (ite reverse_used_215 (ite reverse_used_216 ?x3303 216) 215) 214)))
(let ((?x6494 (ite reverse_used_211 (ite reverse_used_212 (ite reverse_used_213 ?x1299 213) 212) 211)))
(let ((?x1207 (ite reverse_used_208 (ite reverse_used_209 (ite reverse_used_210 ?x6494 210) 209) 208)))
(let ((?x4977 (ite reverse_used_205 (ite reverse_used_206 (ite reverse_used_207 ?x1207 207) 206) 205)))
(let ((?x4763 (ite reverse_used_202 (ite reverse_used_203 (ite reverse_used_204 ?x4977 204) 203) 202)))
(let ((?x919 (ite reverse_used_199 (ite reverse_used_200 (ite reverse_used_201 ?x4763 201) 200) 199)))
(let ((?x8623 (ite reverse_used_196 (ite reverse_used_197 (ite reverse_used_198 ?x919 198) 197) 196)))
(let ((?x1894 (ite reverse_used_193 (ite reverse_used_194 (ite reverse_used_195 ?x8623 195) 194) 193)))
(let ((?x3565 (ite reverse_used_190 (ite reverse_used_191 (ite reverse_used_192 ?x1894 192) 191) 190)))
(let ((?x2070 (ite reverse_used_187 (ite reverse_used_188 (ite reverse_used_189 ?x3565 189) 188) 187)))
(let ((?x5679 (ite reverse_used_184 (ite reverse_used_185 (ite reverse_used_186 ?x2070 186) 185) 184)))
(let ((?x2979 (ite reverse_used_181 (ite reverse_used_182 (ite reverse_used_183 ?x5679 183) 182) 181)))
(let ((?x5752 (ite reverse_used_178 (ite reverse_used_179 (ite reverse_used_180 ?x2979 180) 179) 178)))
(let ((?x3502 (ite reverse_used_175 (ite reverse_used_176 (ite reverse_used_177 ?x5752 177) 176) 175)))
(let ((?x5159 (ite reverse_used_172 (ite reverse_used_173 (ite reverse_used_174 ?x3502 174) 173) 172)))
(let ((?x7746 (ite reverse_used_169 (ite reverse_used_170 (ite reverse_used_171 ?x5159 171) 170) 169)))
(let ((?x2624 (ite reverse_used_166 (ite reverse_used_167 (ite reverse_used_168 ?x7746 168) 167) 166)))
(let ((?x6280 (ite reverse_used_163 (ite reverse_used_164 (ite reverse_used_165 ?x2624 165) 164) 163)))
(let ((?x2332 (ite reverse_used_160 (ite reverse_used_161 (ite reverse_used_162 ?x6280 162) 161) 160)))
(let ((?x2933 (ite reverse_used_157 (ite reverse_used_158 (ite reverse_used_159 ?x2332 159) 158) 157)))
(let ((?x3790 (ite reverse_used_154 (ite reverse_used_155 (ite reverse_used_156 ?x2933 156) 155) 154)))
(let ((?x1767 (ite reverse_used_151 (ite reverse_used_152 (ite reverse_used_153 ?x3790 153) 152) 151)))
(let ((?x6524 (ite reverse_used_148 (ite reverse_used_149 (ite reverse_used_150 ?x1767 150) 149) 148)))
(let ((?x4408 (ite reverse_used_145 (ite reverse_used_146 (ite reverse_used_147 ?x6524 147) 146) 145)))
(let ((?x4678 (ite reverse_used_142 (ite reverse_used_143 (ite reverse_used_144 ?x4408 144) 143) 142)))
(let ((?x3935 (ite reverse_used_139 (ite reverse_used_140 (ite reverse_used_141 ?x4678 141) 140) 139)))
(let ((?x3285 (ite reverse_used_136 (ite reverse_used_137 (ite reverse_used_138 ?x3935 138) 137) 136)))
(let ((?x9483 (ite reverse_used_133 (ite reverse_used_134 (ite reverse_used_135 ?x3285 135) 134) 133)))
(let ((?x5340 (ite reverse_used_130 (ite reverse_used_131 (ite reverse_used_132 ?x9483 132) 131) 130)))
(let ((?x4887 (ite reverse_used_127 (ite reverse_used_128 (ite reverse_used_129 ?x5340 129) 128) 127)))
(let ((?x8149 (ite reverse_used_124 (ite reverse_used_125 (ite reverse_used_126 ?x4887 126) 125) 124)))
(let ((?x8968 (ite reverse_used_121 (ite reverse_used_122 (ite reverse_used_123 ?x8149 123) 122) 121)))
(let ((?x1347 (ite reverse_used_118 (ite reverse_used_119 (ite reverse_used_120 ?x8968 120) 119) 118)))
(let ((?x6578 (ite reverse_used_115 (ite reverse_used_116 (ite reverse_used_117 ?x1347 117) 116) 115)))
(let ((?x9303 (ite reverse_used_112 (ite reverse_used_113 (ite reverse_used_114 ?x6578 114) 113) 112)))
(let ((?x2374 (ite reverse_used_109 (ite reverse_used_110 (ite reverse_used_111 ?x9303 111) 110) 109)))
(let ((?x2873 (ite reverse_used_106 (ite reverse_used_107 (ite reverse_used_108 ?x2374 108) 107) 106)))
(let ((?x1496 (ite reverse_used_103 (ite reverse_used_104 (ite reverse_used_105 ?x2873 105) 104) 103)))
(let ((?x6691 (ite reverse_used_100 (ite reverse_used_101 (ite reverse_used_102 ?x1496 102) 101) 100)))
(let ((?x987 (ite reverse_used_97 (ite reverse_used_98 (ite reverse_used_99 ?x6691 99) 98) 97)))
(let ((?x2481 (ite reverse_used_94 (ite reverse_used_95 (ite reverse_used_96 ?x987 96) 95) 94)))
(let ((?x8873 (ite reverse_used_91 (ite reverse_used_92 (ite reverse_used_93 ?x2481 93) 92) 91)))
(let ((?x2635 (ite reverse_used_88 (ite reverse_used_89 (ite reverse_used_90 ?x8873 90) 89) 88)))
(let ((?x7225 (ite reverse_used_85 (ite reverse_used_86 (ite reverse_used_87 ?x2635 87) 86) 85)))
(let ((?x9440 (ite reverse_used_82 (ite reverse_used_83 (ite reverse_used_84 ?x7225 84) 83) 82)))
(let ((?x4303 (ite reverse_used_79 (ite reverse_used_80 (ite reverse_used_81 ?x9440 81) 80) 79)))
(let ((?x7611 (ite reverse_used_76 (ite reverse_used_77 (ite reverse_used_78 ?x4303 78) 77) 76)))
(let ((?x6343 (ite reverse_used_73 (ite reverse_used_74 (ite reverse_used_75 ?x7611 75) 74) 73)))
(let ((?x4041 (ite reverse_used_70 (ite reverse_used_71 (ite reverse_used_72 ?x6343 72) 71) 70)))
(let ((?x5376 (ite reverse_used_67 (ite reverse_used_68 (ite reverse_used_69 ?x4041 69) 68) 67)))
(let ((?x7558 (ite reverse_used_64 (ite reverse_used_65 (ite reverse_used_66 ?x5376 66) 65) 64)))
(let ((?x3321 (ite reverse_used_61 (ite reverse_used_62 (ite reverse_used_63 ?x7558 63) 62) 61)))
(let ((?x3365 (ite reverse_used_58 (ite reverse_used_59 (ite reverse_used_60 ?x3321 60) 59) 58)))
(let ((?x3923 (ite reverse_used_55 (ite reverse_used_56 (ite reverse_used_57 ?x3365 57) 56) 55)))
(let ((?x9675 (ite reverse_used_52 (ite reverse_used_53 (ite reverse_used_54 ?x3923 54) 53) 52)))
(let ((?x3731 (ite reverse_used_49 (ite reverse_used_50 (ite reverse_used_51 ?x9675 51) 50) 49)))
(let ((?x3287 (ite reverse_used_46 (ite reverse_used_47 (ite reverse_used_48 ?x3731 48) 47) 46)))
(let ((?x3255 (ite reverse_used_43 (ite reverse_used_44 (ite reverse_used_45 ?x3287 45) 44) 43)))
(let ((?x6428 (ite reverse_used_40 (ite reverse_used_41 (ite reverse_used_42 ?x3255 42) 41) 40)))
(let ((?x4260 (ite reverse_used_37 (ite reverse_used_38 (ite reverse_used_39 ?x6428 39) 38) 37)))
(let ((?x1855 (ite reverse_used_34 (ite reverse_used_35 (ite reverse_used_36 ?x4260 36) 35) 34)))
(let ((?x867 (ite reverse_used_31 (ite reverse_used_32 (ite reverse_used_33 ?x1855 33) 32) 31)))
(let ((?x5029 (ite reverse_used_28 (ite reverse_used_29 (ite reverse_used_30 ?x867 30) 29) 28)))
(let ((?x1197 (ite reverse_used_25 (ite reverse_used_26 (ite reverse_used_27 ?x5029 27) 26) 25)))
(let ((?x3278 (ite reverse_used_22 (ite reverse_used_23 (ite reverse_used_24 ?x1197 24) 23) 22)))
(let ((?x3877 (ite reverse_used_19 (ite reverse_used_20 (ite reverse_used_21 ?x3278 21) 20) 19)))
(let ((?x3475 (ite reverse_used_16 (ite reverse_used_17 (ite reverse_used_18 ?x3877 18) 17) 16)))
(let ((?x1048 (ite reverse_used_13 (ite reverse_used_14 (ite reverse_used_15 ?x3475 15) 14) 13)))
(let ((?x3851 (ite reverse_used_10 (ite reverse_used_11 (ite reverse_used_12 ?x1048 12) 11) 10)))
(let ((?x3309 (ite reverse_used_7 (ite reverse_used_8 (ite reverse_used_9 ?x3851 9) 8) 7)))
(let ((?x2563 (ite reverse_used_4 (ite reverse_used_5 (ite reverse_used_6 ?x3309 6) 5) 4)))
(let ((?x8877 (ite reverse_used_1 (ite reverse_used_2 (ite reverse_used_3 ?x2563 3) 2) 1)))
(let ((?x7073 (ite reverse_used_0 ?x8877 0)))
(let ((?x4435 (ite chronological_used_254 (ite chronological_used_255 256 255) 254)))
(let ((?x2680 (ite chronological_used_252 (ite chronological_used_253 ?x4435 253) 252)))
(let ((?x660 (ite chronological_used_250 (ite chronological_used_251 ?x2680 251) 250)))
(let ((?x748 (ite chronological_used_248 (ite chronological_used_249 ?x660 249) 248)))
(let ((?x4843 (ite chronological_used_246 (ite chronological_used_247 ?x748 247) 246)))
(let ((?x712 (ite chronological_used_244 (ite chronological_used_245 ?x4843 245) 244)))
(let ((?x678 (ite chronological_used_242 (ite chronological_used_243 ?x712 243) 242)))
(let ((?x650 (ite chronological_used_240 (ite chronological_used_241 ?x678 241) 240)))
(let ((?x5971 (ite chronological_used_238 (ite chronological_used_239 ?x650 239) 238)))
(let ((?x652 (ite chronological_used_236 (ite chronological_used_237 ?x5971 237) 236)))
(let ((?x684 (ite chronological_used_234 (ite chronological_used_235 ?x652 235) 234)))
(let ((?x632 (ite chronological_used_232 (ite chronological_used_233 ?x684 233) 232)))
(let ((?x5992 (ite chronological_used_230 (ite chronological_used_231 ?x632 231) 230)))
(let ((?x4818 (ite chronological_used_228 (ite chronological_used_229 ?x5992 229) 228)))
(let ((?x192 (ite chronological_used_226 (ite chronological_used_227 ?x4818 227) 226)))
(let ((?x2967 (ite chronological_used_224 (ite chronological_used_225 ?x192 225) 224)))
(let ((?x744 (ite chronological_used_222 (ite chronological_used_223 ?x2967 223) 222)))
(let ((?x5139 (ite chronological_used_220 (ite chronological_used_221 ?x744 221) 220)))
(let ((?x750 (ite chronological_used_218 (ite chronological_used_219 ?x5139 219) 218)))
(let ((?x756 (ite chronological_used_216 (ite chronological_used_217 ?x750 217) 216)))
(let ((?x760 (ite chronological_used_214 (ite chronological_used_215 ?x756 215) 214)))
(let ((?x764 (ite chronological_used_212 (ite chronological_used_213 ?x760 213) 212)))
(let ((?x768 (ite chronological_used_210 (ite chronological_used_211 ?x764 211) 210)))
(let ((?x772 (ite chronological_used_208 (ite chronological_used_209 ?x768 209) 208)))
(let ((?x776 (ite chronological_used_206 (ite chronological_used_207 ?x772 207) 206)))
(let ((?x780 (ite chronological_used_204 (ite chronological_used_205 ?x776 205) 204)))
(let ((?x784 (ite chronological_used_202 (ite chronological_used_203 ?x780 203) 202)))
(let ((?x788 (ite chronological_used_200 (ite chronological_used_201 ?x784 201) 200)))
(let ((?x792 (ite chronological_used_198 (ite chronological_used_199 ?x788 199) 198)))
(let ((?x796 (ite chronological_used_196 (ite chronological_used_197 ?x792 197) 196)))
(let ((?x800 (ite chronological_used_194 (ite chronological_used_195 ?x796 195) 194)))
(let ((?x804 (ite chronological_used_192 (ite chronological_used_193 ?x800 193) 192)))
(let ((?x808 (ite chronological_used_190 (ite chronological_used_191 ?x804 191) 190)))
(let ((?x812 (ite chronological_used_188 (ite chronological_used_189 ?x808 189) 188)))
(let ((?x816 (ite chronological_used_186 (ite chronological_used_187 ?x812 187) 186)))
(let ((?x820 (ite chronological_used_184 (ite chronological_used_185 ?x816 185) 184)))
(let ((?x824 (ite chronological_used_182 (ite chronological_used_183 ?x820 183) 182)))
(let ((?x2774 (ite chronological_used_180 (ite chronological_used_181 ?x824 181) 180)))
(let ((?x2796 (ite chronological_used_178 (ite chronological_used_179 ?x2774 179) 178)))
(let ((?x5572 (ite chronological_used_176 (ite chronological_used_177 ?x2796 177) 176)))
(let ((?x4526 (ite chronological_used_174 (ite chronological_used_175 ?x5572 175) 174)))
(let ((?x347 (ite chronological_used_172 (ite chronological_used_173 ?x4526 173) 172)))
(let ((?x351 (ite chronological_used_170 (ite chronological_used_171 ?x347 171) 170)))
(let ((?x359 (ite chronological_used_168 (ite chronological_used_169 ?x351 169) 168)))
(let ((?x367 (ite chronological_used_166 (ite chronological_used_167 ?x359 167) 166)))
(let ((?x375 (ite chronological_used_164 (ite chronological_used_165 ?x367 165) 164)))
(let ((?x383 (ite chronological_used_162 (ite chronological_used_163 ?x375 163) 162)))
(let ((?x391 (ite chronological_used_160 (ite chronological_used_161 ?x383 161) 160)))
(let ((?x399 (ite chronological_used_158 (ite chronological_used_159 ?x391 159) 158)))
(let ((?x407 (ite chronological_used_156 (ite chronological_used_157 ?x399 157) 156)))
(let ((?x415 (ite chronological_used_154 (ite chronological_used_155 ?x407 155) 154)))
(let ((?x423 (ite chronological_used_152 (ite chronological_used_153 ?x415 153) 152)))
(let ((?x431 (ite chronological_used_150 (ite chronological_used_151 ?x423 151) 150)))
(let ((?x439 (ite chronological_used_148 (ite chronological_used_149 ?x431 149) 148)))
(let ((?x447 (ite chronological_used_146 (ite chronological_used_147 ?x439 147) 146)))
(let ((?x455 (ite chronological_used_144 (ite chronological_used_145 ?x447 145) 144)))
(let ((?x463 (ite chronological_used_142 (ite chronological_used_143 ?x455 143) 142)))
(let ((?x471 (ite chronological_used_140 (ite chronological_used_141 ?x463 141) 140)))
(let ((?x479 (ite chronological_used_138 (ite chronological_used_139 ?x471 139) 138)))
(let ((?x487 (ite chronological_used_136 (ite chronological_used_137 ?x479 137) 136)))
(let ((?x495 (ite chronological_used_134 (ite chronological_used_135 ?x487 135) 134)))
(let ((?x503 (ite chronological_used_132 (ite chronological_used_133 ?x495 133) 132)))
(let ((?x511 (ite chronological_used_130 (ite chronological_used_131 ?x503 131) 130)))
(let ((?x519 (ite chronological_used_128 (ite chronological_used_129 ?x511 129) 128)))
(let ((?x527 (ite chronological_used_126 (ite chronological_used_127 ?x519 127) 126)))
(let ((?x535 (ite chronological_used_124 (ite chronological_used_125 ?x527 125) 124)))
(let ((?x543 (ite chronological_used_122 (ite chronological_used_123 ?x535 123) 122)))
(let ((?x551 (ite chronological_used_120 (ite chronological_used_121 ?x543 121) 120)))
(let ((?x559 (ite chronological_used_118 (ite chronological_used_119 ?x551 119) 118)))
(let ((?x567 (ite chronological_used_116 (ite chronological_used_117 ?x559 117) 116)))
(let ((?x575 (ite chronological_used_114 (ite chronological_used_115 ?x567 115) 114)))
(let ((?x583 (ite chronological_used_112 (ite chronological_used_113 ?x575 113) 112)))
(let ((?x591 (ite chronological_used_110 (ite chronological_used_111 ?x583 111) 110)))
(let ((?x599 (ite chronological_used_108 (ite chronological_used_109 ?x591 109) 108)))
(let ((?x607 (ite chronological_used_106 (ite chronological_used_107 ?x599 107) 106)))
(let ((?x615 (ite chronological_used_104 (ite chronological_used_105 ?x607 105) 104)))
(let ((?x623 (ite chronological_used_102 (ite chronological_used_103 ?x615 103) 102)))
(let ((?x631 (ite chronological_used_100 (ite chronological_used_101 ?x623 101) 100)))
(let ((?x639 (ite chronological_used_98 (ite chronological_used_99 ?x631 99) 98)))
(let ((?x647 (ite chronological_used_96 (ite chronological_used_97 ?x639 97) 96)))
(let ((?x655 (ite chronological_used_94 (ite chronological_used_95 ?x647 95) 94)))
(let ((?x663 (ite chronological_used_92 (ite chronological_used_93 ?x655 93) 92)))
(let ((?x671 (ite chronological_used_90 (ite chronological_used_91 ?x663 91) 90)))
(let ((?x679 (ite chronological_used_88 (ite chronological_used_89 ?x671 89) 88)))
(let ((?x687 (ite chronological_used_86 (ite chronological_used_87 ?x679 87) 86)))
(let ((?x695 (ite chronological_used_84 (ite chronological_used_85 ?x687 85) 84)))
(let ((?x703 (ite chronological_used_82 (ite chronological_used_83 ?x695 83) 82)))
(let ((?x711 (ite chronological_used_80 (ite chronological_used_81 ?x703 81) 80)))
(let ((?x719 (ite chronological_used_78 (ite chronological_used_79 ?x711 79) 78)))
(let ((?x727 (ite chronological_used_76 (ite chronological_used_77 ?x719 77) 76)))
(let ((?x735 (ite chronological_used_74 (ite chronological_used_75 ?x727 75) 74)))
(let ((?x743 (ite chronological_used_72 (ite chronological_used_73 ?x735 73) 72)))
(let ((?x751 (ite chronological_used_70 (ite chronological_used_71 ?x743 71) 70)))
(let ((?x759 (ite chronological_used_68 (ite chronological_used_69 ?x751 69) 68)))
(let ((?x767 (ite chronological_used_66 (ite chronological_used_67 ?x759 67) 66)))
(let ((?x775 (ite chronological_used_64 (ite chronological_used_65 ?x767 65) 64)))
(let ((?x783 (ite chronological_used_62 (ite chronological_used_63 ?x775 63) 62)))
(let ((?x791 (ite chronological_used_60 (ite chronological_used_61 ?x783 61) 60)))
(let ((?x799 (ite chronological_used_58 (ite chronological_used_59 ?x791 59) 58)))
(let ((?x807 (ite chronological_used_56 (ite chronological_used_57 ?x799 57) 56)))
(let ((?x815 (ite chronological_used_54 (ite chronological_used_55 ?x807 55) 54)))
(let ((?x823 (ite chronological_used_52 (ite chronological_used_53 ?x815 53) 52)))
(let ((?x831 (ite chronological_used_50 (ite chronological_used_51 ?x823 51) 50)))
(let ((?x839 (ite chronological_used_48 (ite chronological_used_49 ?x831 49) 48)))
(let ((?x846 (ite chronological_used_46 (ite chronological_used_47 ?x839 47) 46)))
(let ((?x850 (ite chronological_used_44 (ite chronological_used_45 ?x846 45) 44)))
(let ((?x8 (ite chronological_used_42 (ite chronological_used_43 ?x850 43) 42)))
(let ((?x121 (ite chronological_used_40 (ite chronological_used_41 ?x8 41) 40)))
(let ((?x98 (ite chronological_used_38 (ite chronological_used_39 ?x121 39) 38)))
(let ((?x96 (ite chronological_used_36 (ite chronological_used_37 ?x98 37) 36)))
(let ((?x78 (ite chronological_used_34 (ite chronological_used_35 ?x96 35) 34)))
(let ((?x103 (ite chronological_used_32 (ite chronological_used_33 ?x78 33) 32)))
(let ((?x135 (ite chronological_used_30 (ite chronological_used_31 ?x103 31) 30)))
(let ((?x100 (ite chronological_used_28 (ite chronological_used_29 ?x135 29) 28)))
(let ((?x58 (ite chronological_used_26 (ite chronological_used_27 ?x100 27) 26)))
(let ((?x32 (ite chronological_used_24 (ite chronological_used_25 ?x58 25) 24)))
(let ((?x19 (ite chronological_used_22 (ite chronological_used_23 ?x32 23) 22)))
(let ((?x21 (ite chronological_used_20 (ite chronological_used_21 ?x19 21) 20)))
(let ((?x23 (ite chronological_used_18 (ite chronological_used_19 ?x21 19) 18)))
(let ((?x13 (ite chronological_used_16 (ite chronological_used_17 ?x23 17) 16)))
(let ((?x52 (ite chronological_used_14 (ite chronological_used_15 ?x13 15) 14)))
(let ((?x17 (ite chronological_used_12 (ite chronological_used_13 ?x52 13) 12)))
(let ((?x95 (ite chronological_used_10 (ite chronological_used_11 ?x17 11) 10)))
(let ((?x26 (ite chronological_used_7 (ite chronological_used_8 (ite chronological_used_9 ?x95 9) 8) 7)))
(let ((?x74 (ite chronological_used_4 (ite chronological_used_5 (ite chronological_used_6 ?x26 6) 5) 4)))
(let ((?x97 (ite chronological_used_1 (ite chronological_used_2 (ite chronological_used_3 ?x74 3) 2) 1)))
(let ((?x50 (ite chronological_used_0 ?x97 0)))
(and (distinct ?x50 ?x7073) true)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 21: equal_full_sets_retain_256_sentinel
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun reverse_used_0 () Bool)
(declare-fun chronological_used_0 () Bool)
(declare-fun reverse_used_1 () Bool)
(declare-fun chronological_used_1 () Bool)
(declare-fun reverse_used_2 () Bool)
(declare-fun chronological_used_2 () Bool)
(declare-fun reverse_used_3 () Bool)
(declare-fun chronological_used_3 () Bool)
(declare-fun reverse_used_4 () Bool)
(declare-fun chronological_used_4 () Bool)
(declare-fun reverse_used_5 () Bool)
(declare-fun chronological_used_5 () Bool)
(declare-fun reverse_used_6 () Bool)
(declare-fun chronological_used_6 () Bool)
(declare-fun reverse_used_7 () Bool)
(declare-fun chronological_used_7 () Bool)
(declare-fun reverse_used_8 () Bool)
(declare-fun chronological_used_8 () Bool)
(declare-fun reverse_used_9 () Bool)
(declare-fun chronological_used_9 () Bool)
(declare-fun reverse_used_10 () Bool)
(declare-fun chronological_used_10 () Bool)
(declare-fun reverse_used_11 () Bool)
(declare-fun chronological_used_11 () Bool)
(declare-fun reverse_used_12 () Bool)
(declare-fun chronological_used_12 () Bool)
(declare-fun reverse_used_13 () Bool)
(declare-fun chronological_used_13 () Bool)
(declare-fun reverse_used_14 () Bool)
(declare-fun chronological_used_14 () Bool)
(declare-fun reverse_used_15 () Bool)
(declare-fun chronological_used_15 () Bool)
(declare-fun reverse_used_16 () Bool)
(declare-fun chronological_used_16 () Bool)
(declare-fun reverse_used_17 () Bool)
(declare-fun chronological_used_17 () Bool)
(declare-fun reverse_used_18 () Bool)
(declare-fun chronological_used_18 () Bool)
(declare-fun reverse_used_19 () Bool)
(declare-fun chronological_used_19 () Bool)
(declare-fun reverse_used_20 () Bool)
(declare-fun chronological_used_20 () Bool)
(declare-fun reverse_used_21 () Bool)
(declare-fun chronological_used_21 () Bool)
(declare-fun reverse_used_22 () Bool)
(declare-fun chronological_used_22 () Bool)
(declare-fun reverse_used_23 () Bool)
(declare-fun chronological_used_23 () Bool)
(declare-fun reverse_used_24 () Bool)
(declare-fun chronological_used_24 () Bool)
(declare-fun reverse_used_25 () Bool)
(declare-fun chronological_used_25 () Bool)
(declare-fun reverse_used_26 () Bool)
(declare-fun chronological_used_26 () Bool)
(declare-fun reverse_used_27 () Bool)
(declare-fun chronological_used_27 () Bool)
(declare-fun reverse_used_28 () Bool)
(declare-fun chronological_used_28 () Bool)
(declare-fun reverse_used_29 () Bool)
(declare-fun chronological_used_29 () Bool)
(declare-fun reverse_used_30 () Bool)
(declare-fun chronological_used_30 () Bool)
(declare-fun reverse_used_31 () Bool)
(declare-fun chronological_used_31 () Bool)
(declare-fun reverse_used_32 () Bool)
(declare-fun chronological_used_32 () Bool)
(declare-fun reverse_used_33 () Bool)
(declare-fun chronological_used_33 () Bool)
(declare-fun reverse_used_34 () Bool)
(declare-fun chronological_used_34 () Bool)
(declare-fun reverse_used_35 () Bool)
(declare-fun chronological_used_35 () Bool)
(declare-fun reverse_used_36 () Bool)
(declare-fun chronological_used_36 () Bool)
(declare-fun reverse_used_37 () Bool)
(declare-fun chronological_used_37 () Bool)
(declare-fun reverse_used_38 () Bool)
(declare-fun chronological_used_38 () Bool)
(declare-fun reverse_used_39 () Bool)
(declare-fun chronological_used_39 () Bool)
(declare-fun reverse_used_40 () Bool)
(declare-fun chronological_used_40 () Bool)
(declare-fun reverse_used_41 () Bool)
(declare-fun chronological_used_41 () Bool)
(declare-fun reverse_used_42 () Bool)
(declare-fun chronological_used_42 () Bool)
(declare-fun reverse_used_43 () Bool)
(declare-fun chronological_used_43 () Bool)
(declare-fun reverse_used_44 () Bool)
(declare-fun chronological_used_44 () Bool)
(declare-fun reverse_used_45 () Bool)
(declare-fun chronological_used_45 () Bool)
(declare-fun reverse_used_46 () Bool)
(declare-fun chronological_used_46 () Bool)
(declare-fun reverse_used_47 () Bool)
(declare-fun chronological_used_47 () Bool)
(declare-fun reverse_used_48 () Bool)
(declare-fun chronological_used_48 () Bool)
(declare-fun reverse_used_49 () Bool)
(declare-fun chronological_used_49 () Bool)
(declare-fun reverse_used_50 () Bool)
(declare-fun chronological_used_50 () Bool)
(declare-fun reverse_used_51 () Bool)
(declare-fun chronological_used_51 () Bool)
(declare-fun reverse_used_52 () Bool)
(declare-fun chronological_used_52 () Bool)
(declare-fun reverse_used_53 () Bool)
(declare-fun chronological_used_53 () Bool)
(declare-fun reverse_used_54 () Bool)
(declare-fun chronological_used_54 () Bool)
(declare-fun reverse_used_55 () Bool)
(declare-fun chronological_used_55 () Bool)
(declare-fun reverse_used_56 () Bool)
(declare-fun chronological_used_56 () Bool)
(declare-fun reverse_used_57 () Bool)
(declare-fun chronological_used_57 () Bool)
(declare-fun reverse_used_58 () Bool)
(declare-fun chronological_used_58 () Bool)
(declare-fun reverse_used_59 () Bool)
(declare-fun chronological_used_59 () Bool)
(declare-fun reverse_used_60 () Bool)
(declare-fun chronological_used_60 () Bool)
(declare-fun reverse_used_61 () Bool)
(declare-fun chronological_used_61 () Bool)
(declare-fun reverse_used_62 () Bool)
(declare-fun chronological_used_62 () Bool)
(declare-fun reverse_used_63 () Bool)
(declare-fun chronological_used_63 () Bool)
(declare-fun reverse_used_64 () Bool)
(declare-fun chronological_used_64 () Bool)
(declare-fun reverse_used_65 () Bool)
(declare-fun chronological_used_65 () Bool)
(declare-fun reverse_used_66 () Bool)
(declare-fun chronological_used_66 () Bool)
(declare-fun reverse_used_67 () Bool)
(declare-fun chronological_used_67 () Bool)
(declare-fun reverse_used_68 () Bool)
(declare-fun chronological_used_68 () Bool)
(declare-fun reverse_used_69 () Bool)
(declare-fun chronological_used_69 () Bool)
(declare-fun reverse_used_70 () Bool)
(declare-fun chronological_used_70 () Bool)
(declare-fun reverse_used_71 () Bool)
(declare-fun chronological_used_71 () Bool)
(declare-fun reverse_used_72 () Bool)
(declare-fun chronological_used_72 () Bool)
(declare-fun reverse_used_73 () Bool)
(declare-fun chronological_used_73 () Bool)
(declare-fun reverse_used_74 () Bool)
(declare-fun chronological_used_74 () Bool)
(declare-fun reverse_used_75 () Bool)
(declare-fun chronological_used_75 () Bool)
(declare-fun reverse_used_76 () Bool)
(declare-fun chronological_used_76 () Bool)
(declare-fun reverse_used_77 () Bool)
(declare-fun chronological_used_77 () Bool)
(declare-fun reverse_used_78 () Bool)
(declare-fun chronological_used_78 () Bool)
(declare-fun reverse_used_79 () Bool)
(declare-fun chronological_used_79 () Bool)
(declare-fun reverse_used_80 () Bool)
(declare-fun chronological_used_80 () Bool)
(declare-fun reverse_used_81 () Bool)
(declare-fun chronological_used_81 () Bool)
(declare-fun reverse_used_82 () Bool)
(declare-fun chronological_used_82 () Bool)
(declare-fun reverse_used_83 () Bool)
(declare-fun chronological_used_83 () Bool)
(declare-fun reverse_used_84 () Bool)
(declare-fun chronological_used_84 () Bool)
(declare-fun reverse_used_85 () Bool)
(declare-fun chronological_used_85 () Bool)
(declare-fun reverse_used_86 () Bool)
(declare-fun chronological_used_86 () Bool)
(declare-fun reverse_used_87 () Bool)
(declare-fun chronological_used_87 () Bool)
(declare-fun reverse_used_88 () Bool)
(declare-fun chronological_used_88 () Bool)
(declare-fun reverse_used_89 () Bool)
(declare-fun chronological_used_89 () Bool)
(declare-fun reverse_used_90 () Bool)
(declare-fun chronological_used_90 () Bool)
(declare-fun reverse_used_91 () Bool)
(declare-fun chronological_used_91 () Bool)
(declare-fun reverse_used_92 () Bool)
(declare-fun chronological_used_92 () Bool)
(declare-fun reverse_used_93 () Bool)
(declare-fun chronological_used_93 () Bool)
(declare-fun reverse_used_94 () Bool)
(declare-fun chronological_used_94 () Bool)
(declare-fun reverse_used_95 () Bool)
(declare-fun chronological_used_95 () Bool)
(declare-fun reverse_used_96 () Bool)
(declare-fun chronological_used_96 () Bool)
(declare-fun reverse_used_97 () Bool)
(declare-fun chronological_used_97 () Bool)
(declare-fun reverse_used_98 () Bool)
(declare-fun chronological_used_98 () Bool)
(declare-fun reverse_used_99 () Bool)
(declare-fun chronological_used_99 () Bool)
(declare-fun reverse_used_100 () Bool)
(declare-fun chronological_used_100 () Bool)
(declare-fun reverse_used_101 () Bool)
(declare-fun chronological_used_101 () Bool)
(declare-fun reverse_used_102 () Bool)
(declare-fun chronological_used_102 () Bool)
(declare-fun reverse_used_103 () Bool)
(declare-fun chronological_used_103 () Bool)
(declare-fun reverse_used_104 () Bool)
(declare-fun chronological_used_104 () Bool)
(declare-fun reverse_used_105 () Bool)
(declare-fun chronological_used_105 () Bool)
(declare-fun reverse_used_106 () Bool)
(declare-fun chronological_used_106 () Bool)
(declare-fun reverse_used_107 () Bool)
(declare-fun chronological_used_107 () Bool)
(declare-fun reverse_used_108 () Bool)
(declare-fun chronological_used_108 () Bool)
(declare-fun reverse_used_109 () Bool)
(declare-fun chronological_used_109 () Bool)
(declare-fun reverse_used_110 () Bool)
(declare-fun chronological_used_110 () Bool)
(declare-fun reverse_used_111 () Bool)
(declare-fun chronological_used_111 () Bool)
(declare-fun reverse_used_112 () Bool)
(declare-fun chronological_used_112 () Bool)
(declare-fun reverse_used_113 () Bool)
(declare-fun chronological_used_113 () Bool)
(declare-fun reverse_used_114 () Bool)
(declare-fun chronological_used_114 () Bool)
(declare-fun reverse_used_115 () Bool)
(declare-fun chronological_used_115 () Bool)
(declare-fun reverse_used_116 () Bool)
(declare-fun chronological_used_116 () Bool)
(declare-fun reverse_used_117 () Bool)
(declare-fun chronological_used_117 () Bool)
(declare-fun reverse_used_118 () Bool)
(declare-fun chronological_used_118 () Bool)
(declare-fun reverse_used_119 () Bool)
(declare-fun chronological_used_119 () Bool)
(declare-fun reverse_used_120 () Bool)
(declare-fun chronological_used_120 () Bool)
(declare-fun reverse_used_121 () Bool)
(declare-fun chronological_used_121 () Bool)
(declare-fun reverse_used_122 () Bool)
(declare-fun chronological_used_122 () Bool)
(declare-fun reverse_used_123 () Bool)
(declare-fun chronological_used_123 () Bool)
(declare-fun reverse_used_124 () Bool)
(declare-fun chronological_used_124 () Bool)
(declare-fun reverse_used_125 () Bool)
(declare-fun chronological_used_125 () Bool)
(declare-fun reverse_used_126 () Bool)
(declare-fun chronological_used_126 () Bool)
(declare-fun reverse_used_127 () Bool)
(declare-fun chronological_used_127 () Bool)
(declare-fun reverse_used_128 () Bool)
(declare-fun chronological_used_128 () Bool)
(declare-fun reverse_used_129 () Bool)
(declare-fun chronological_used_129 () Bool)
(declare-fun reverse_used_130 () Bool)
(declare-fun chronological_used_130 () Bool)
(declare-fun reverse_used_131 () Bool)
(declare-fun chronological_used_131 () Bool)
(declare-fun reverse_used_132 () Bool)
(declare-fun chronological_used_132 () Bool)
(declare-fun reverse_used_133 () Bool)
(declare-fun chronological_used_133 () Bool)
(declare-fun reverse_used_134 () Bool)
(declare-fun chronological_used_134 () Bool)
(declare-fun reverse_used_135 () Bool)
(declare-fun chronological_used_135 () Bool)
(declare-fun reverse_used_136 () Bool)
(declare-fun chronological_used_136 () Bool)
(declare-fun reverse_used_137 () Bool)
(declare-fun chronological_used_137 () Bool)
(declare-fun reverse_used_138 () Bool)
(declare-fun chronological_used_138 () Bool)
(declare-fun reverse_used_139 () Bool)
(declare-fun chronological_used_139 () Bool)
(declare-fun reverse_used_140 () Bool)
(declare-fun chronological_used_140 () Bool)
(declare-fun reverse_used_141 () Bool)
(declare-fun chronological_used_141 () Bool)
(declare-fun reverse_used_142 () Bool)
(declare-fun chronological_used_142 () Bool)
(declare-fun reverse_used_143 () Bool)
(declare-fun chronological_used_143 () Bool)
(declare-fun reverse_used_144 () Bool)
(declare-fun chronological_used_144 () Bool)
(declare-fun reverse_used_145 () Bool)
(declare-fun chronological_used_145 () Bool)
(declare-fun reverse_used_146 () Bool)
(declare-fun chronological_used_146 () Bool)
(declare-fun reverse_used_147 () Bool)
(declare-fun chronological_used_147 () Bool)
(declare-fun reverse_used_148 () Bool)
(declare-fun chronological_used_148 () Bool)
(declare-fun reverse_used_149 () Bool)
(declare-fun chronological_used_149 () Bool)
(declare-fun reverse_used_150 () Bool)
(declare-fun chronological_used_150 () Bool)
(declare-fun reverse_used_151 () Bool)
(declare-fun chronological_used_151 () Bool)
(declare-fun reverse_used_152 () Bool)
(declare-fun chronological_used_152 () Bool)
(declare-fun reverse_used_153 () Bool)
(declare-fun chronological_used_153 () Bool)
(declare-fun reverse_used_154 () Bool)
(declare-fun chronological_used_154 () Bool)
(declare-fun reverse_used_155 () Bool)
(declare-fun chronological_used_155 () Bool)
(declare-fun reverse_used_156 () Bool)
(declare-fun chronological_used_156 () Bool)
(declare-fun reverse_used_157 () Bool)
(declare-fun chronological_used_157 () Bool)
(declare-fun reverse_used_158 () Bool)
(declare-fun chronological_used_158 () Bool)
(declare-fun reverse_used_159 () Bool)
(declare-fun chronological_used_159 () Bool)
(declare-fun reverse_used_160 () Bool)
(declare-fun chronological_used_160 () Bool)
(declare-fun reverse_used_161 () Bool)
(declare-fun chronological_used_161 () Bool)
(declare-fun reverse_used_162 () Bool)
(declare-fun chronological_used_162 () Bool)
(declare-fun reverse_used_163 () Bool)
(declare-fun chronological_used_163 () Bool)
(declare-fun reverse_used_164 () Bool)
(declare-fun chronological_used_164 () Bool)
(declare-fun reverse_used_165 () Bool)
(declare-fun chronological_used_165 () Bool)
(declare-fun reverse_used_166 () Bool)
(declare-fun chronological_used_166 () Bool)
(declare-fun reverse_used_167 () Bool)
(declare-fun chronological_used_167 () Bool)
(declare-fun reverse_used_168 () Bool)
(declare-fun chronological_used_168 () Bool)
(declare-fun reverse_used_169 () Bool)
(declare-fun chronological_used_169 () Bool)
(declare-fun reverse_used_170 () Bool)
(declare-fun chronological_used_170 () Bool)
(declare-fun reverse_used_171 () Bool)
(declare-fun chronological_used_171 () Bool)
(declare-fun reverse_used_172 () Bool)
(declare-fun chronological_used_172 () Bool)
(declare-fun reverse_used_173 () Bool)
(declare-fun chronological_used_173 () Bool)
(declare-fun reverse_used_174 () Bool)
(declare-fun chronological_used_174 () Bool)
(declare-fun reverse_used_175 () Bool)
(declare-fun chronological_used_175 () Bool)
(declare-fun reverse_used_176 () Bool)
(declare-fun chronological_used_176 () Bool)
(declare-fun reverse_used_177 () Bool)
(declare-fun chronological_used_177 () Bool)
(declare-fun reverse_used_178 () Bool)
(declare-fun chronological_used_178 () Bool)
(declare-fun reverse_used_179 () Bool)
(declare-fun chronological_used_179 () Bool)
(declare-fun reverse_used_180 () Bool)
(declare-fun chronological_used_180 () Bool)
(declare-fun reverse_used_181 () Bool)
(declare-fun chronological_used_181 () Bool)
(declare-fun reverse_used_182 () Bool)
(declare-fun chronological_used_182 () Bool)
(declare-fun reverse_used_183 () Bool)
(declare-fun chronological_used_183 () Bool)
(declare-fun reverse_used_184 () Bool)
(declare-fun chronological_used_184 () Bool)
(declare-fun reverse_used_185 () Bool)
(declare-fun chronological_used_185 () Bool)
(declare-fun reverse_used_186 () Bool)
(declare-fun chronological_used_186 () Bool)
(declare-fun reverse_used_187 () Bool)
(declare-fun chronological_used_187 () Bool)
(declare-fun reverse_used_188 () Bool)
(declare-fun chronological_used_188 () Bool)
(declare-fun reverse_used_189 () Bool)
(declare-fun chronological_used_189 () Bool)
(declare-fun reverse_used_190 () Bool)
(declare-fun chronological_used_190 () Bool)
(declare-fun reverse_used_191 () Bool)
(declare-fun chronological_used_191 () Bool)
(declare-fun reverse_used_192 () Bool)
(declare-fun chronological_used_192 () Bool)
(declare-fun reverse_used_193 () Bool)
(declare-fun chronological_used_193 () Bool)
(declare-fun reverse_used_194 () Bool)
(declare-fun chronological_used_194 () Bool)
(declare-fun reverse_used_195 () Bool)
(declare-fun chronological_used_195 () Bool)
(declare-fun reverse_used_196 () Bool)
(declare-fun chronological_used_196 () Bool)
(declare-fun reverse_used_197 () Bool)
(declare-fun chronological_used_197 () Bool)
(declare-fun reverse_used_198 () Bool)
(declare-fun chronological_used_198 () Bool)
(declare-fun reverse_used_199 () Bool)
(declare-fun chronological_used_199 () Bool)
(declare-fun reverse_used_200 () Bool)
(declare-fun chronological_used_200 () Bool)
(declare-fun reverse_used_201 () Bool)
(declare-fun chronological_used_201 () Bool)
(declare-fun reverse_used_202 () Bool)
(declare-fun chronological_used_202 () Bool)
(declare-fun reverse_used_203 () Bool)
(declare-fun chronological_used_203 () Bool)
(declare-fun reverse_used_204 () Bool)
(declare-fun chronological_used_204 () Bool)
(declare-fun reverse_used_205 () Bool)
(declare-fun chronological_used_205 () Bool)
(declare-fun reverse_used_206 () Bool)
(declare-fun chronological_used_206 () Bool)
(declare-fun reverse_used_207 () Bool)
(declare-fun chronological_used_207 () Bool)
(declare-fun reverse_used_208 () Bool)
(declare-fun chronological_used_208 () Bool)
(declare-fun reverse_used_209 () Bool)
(declare-fun chronological_used_209 () Bool)
(declare-fun reverse_used_210 () Bool)
(declare-fun chronological_used_210 () Bool)
(declare-fun reverse_used_211 () Bool)
(declare-fun chronological_used_211 () Bool)
(declare-fun reverse_used_212 () Bool)
(declare-fun chronological_used_212 () Bool)
(declare-fun reverse_used_213 () Bool)
(declare-fun chronological_used_213 () Bool)
(declare-fun reverse_used_214 () Bool)
(declare-fun chronological_used_214 () Bool)
(declare-fun reverse_used_215 () Bool)
(declare-fun chronological_used_215 () Bool)
(declare-fun reverse_used_216 () Bool)
(declare-fun chronological_used_216 () Bool)
(declare-fun reverse_used_217 () Bool)
(declare-fun chronological_used_217 () Bool)
(declare-fun reverse_used_218 () Bool)
(declare-fun chronological_used_218 () Bool)
(declare-fun reverse_used_219 () Bool)
(declare-fun chronological_used_219 () Bool)
(declare-fun reverse_used_220 () Bool)
(declare-fun chronological_used_220 () Bool)
(declare-fun reverse_used_221 () Bool)
(declare-fun chronological_used_221 () Bool)
(declare-fun reverse_used_222 () Bool)
(declare-fun chronological_used_222 () Bool)
(declare-fun reverse_used_223 () Bool)
(declare-fun chronological_used_223 () Bool)
(declare-fun reverse_used_224 () Bool)
(declare-fun chronological_used_224 () Bool)
(declare-fun reverse_used_225 () Bool)
(declare-fun chronological_used_225 () Bool)
(declare-fun reverse_used_226 () Bool)
(declare-fun chronological_used_226 () Bool)
(declare-fun reverse_used_227 () Bool)
(declare-fun chronological_used_227 () Bool)
(declare-fun reverse_used_228 () Bool)
(declare-fun chronological_used_228 () Bool)
(declare-fun reverse_used_229 () Bool)
(declare-fun chronological_used_229 () Bool)
(declare-fun reverse_used_230 () Bool)
(declare-fun chronological_used_230 () Bool)
(declare-fun reverse_used_231 () Bool)
(declare-fun chronological_used_231 () Bool)
(declare-fun reverse_used_232 () Bool)
(declare-fun chronological_used_232 () Bool)
(declare-fun reverse_used_233 () Bool)
(declare-fun chronological_used_233 () Bool)
(declare-fun reverse_used_234 () Bool)
(declare-fun chronological_used_234 () Bool)
(declare-fun reverse_used_235 () Bool)
(declare-fun chronological_used_235 () Bool)
(declare-fun reverse_used_236 () Bool)
(declare-fun chronological_used_236 () Bool)
(declare-fun reverse_used_237 () Bool)
(declare-fun chronological_used_237 () Bool)
(declare-fun reverse_used_238 () Bool)
(declare-fun chronological_used_238 () Bool)
(declare-fun reverse_used_239 () Bool)
(declare-fun chronological_used_239 () Bool)
(declare-fun reverse_used_240 () Bool)
(declare-fun chronological_used_240 () Bool)
(declare-fun reverse_used_241 () Bool)
(declare-fun chronological_used_241 () Bool)
(declare-fun reverse_used_242 () Bool)
(declare-fun chronological_used_242 () Bool)
(declare-fun reverse_used_243 () Bool)
(declare-fun chronological_used_243 () Bool)
(declare-fun reverse_used_244 () Bool)
(declare-fun chronological_used_244 () Bool)
(declare-fun reverse_used_245 () Bool)
(declare-fun chronological_used_245 () Bool)
(declare-fun reverse_used_246 () Bool)
(declare-fun chronological_used_246 () Bool)
(declare-fun reverse_used_247 () Bool)
(declare-fun chronological_used_247 () Bool)
(declare-fun reverse_used_248 () Bool)
(declare-fun chronological_used_248 () Bool)
(declare-fun reverse_used_249 () Bool)
(declare-fun chronological_used_249 () Bool)
(declare-fun reverse_used_250 () Bool)
(declare-fun chronological_used_250 () Bool)
(declare-fun reverse_used_251 () Bool)
(declare-fun chronological_used_251 () Bool)
(declare-fun reverse_used_252 () Bool)
(declare-fun chronological_used_252 () Bool)
(declare-fun reverse_used_253 () Bool)
(declare-fun chronological_used_253 () Bool)
(declare-fun reverse_used_254 () Bool)
(declare-fun chronological_used_254 () Bool)
(declare-fun reverse_used_255 () Bool)
(declare-fun chronological_used_255 () Bool)
(assert
 (= chronological_used_0 reverse_used_0))
(assert
 (= chronological_used_1 reverse_used_1))
(assert
 (= chronological_used_2 reverse_used_2))
(assert
 (= chronological_used_3 reverse_used_3))
(assert
 (= chronological_used_4 reverse_used_4))
(assert
 (= chronological_used_5 reverse_used_5))
(assert
 (= chronological_used_6 reverse_used_6))
(assert
 (= chronological_used_7 reverse_used_7))
(assert
 (= chronological_used_8 reverse_used_8))
(assert
 (= chronological_used_9 reverse_used_9))
(assert
 (= chronological_used_10 reverse_used_10))
(assert
 (= chronological_used_11 reverse_used_11))
(assert
 (= chronological_used_12 reverse_used_12))
(assert
 (= chronological_used_13 reverse_used_13))
(assert
 (= chronological_used_14 reverse_used_14))
(assert
 (= chronological_used_15 reverse_used_15))
(assert
 (= chronological_used_16 reverse_used_16))
(assert
 (= chronological_used_17 reverse_used_17))
(assert
 (= chronological_used_18 reverse_used_18))
(assert
 (= chronological_used_19 reverse_used_19))
(assert
 (= chronological_used_20 reverse_used_20))
(assert
 (= chronological_used_21 reverse_used_21))
(assert
 (= chronological_used_22 reverse_used_22))
(assert
 (= chronological_used_23 reverse_used_23))
(assert
 (= chronological_used_24 reverse_used_24))
(assert
 (= chronological_used_25 reverse_used_25))
(assert
 (= chronological_used_26 reverse_used_26))
(assert
 (= chronological_used_27 reverse_used_27))
(assert
 (= chronological_used_28 reverse_used_28))
(assert
 (= chronological_used_29 reverse_used_29))
(assert
 (= chronological_used_30 reverse_used_30))
(assert
 (= chronological_used_31 reverse_used_31))
(assert
 (= chronological_used_32 reverse_used_32))
(assert
 (= chronological_used_33 reverse_used_33))
(assert
 (= chronological_used_34 reverse_used_34))
(assert
 (= chronological_used_35 reverse_used_35))
(assert
 (= chronological_used_36 reverse_used_36))
(assert
 (= chronological_used_37 reverse_used_37))
(assert
 (= chronological_used_38 reverse_used_38))
(assert
 (= chronological_used_39 reverse_used_39))
(assert
 (= chronological_used_40 reverse_used_40))
(assert
 (= chronological_used_41 reverse_used_41))
(assert
 (= chronological_used_42 reverse_used_42))
(assert
 (= chronological_used_43 reverse_used_43))
(assert
 (= chronological_used_44 reverse_used_44))
(assert
 (= chronological_used_45 reverse_used_45))
(assert
 (= chronological_used_46 reverse_used_46))
(assert
 (= chronological_used_47 reverse_used_47))
(assert
 (= chronological_used_48 reverse_used_48))
(assert
 (= chronological_used_49 reverse_used_49))
(assert
 (= chronological_used_50 reverse_used_50))
(assert
 (= chronological_used_51 reverse_used_51))
(assert
 (= chronological_used_52 reverse_used_52))
(assert
 (= chronological_used_53 reverse_used_53))
(assert
 (= chronological_used_54 reverse_used_54))
(assert
 (= chronological_used_55 reverse_used_55))
(assert
 (= chronological_used_56 reverse_used_56))
(assert
 (= chronological_used_57 reverse_used_57))
(assert
 (= chronological_used_58 reverse_used_58))
(assert
 (= chronological_used_59 reverse_used_59))
(assert
 (= chronological_used_60 reverse_used_60))
(assert
 (= chronological_used_61 reverse_used_61))
(assert
 (= chronological_used_62 reverse_used_62))
(assert
 (= chronological_used_63 reverse_used_63))
(assert
 (= chronological_used_64 reverse_used_64))
(assert
 (= chronological_used_65 reverse_used_65))
(assert
 (= chronological_used_66 reverse_used_66))
(assert
 (= chronological_used_67 reverse_used_67))
(assert
 (= chronological_used_68 reverse_used_68))
(assert
 (= chronological_used_69 reverse_used_69))
(assert
 (= chronological_used_70 reverse_used_70))
(assert
 (= chronological_used_71 reverse_used_71))
(assert
 (= chronological_used_72 reverse_used_72))
(assert
 (= chronological_used_73 reverse_used_73))
(assert
 (= chronological_used_74 reverse_used_74))
(assert
 (= chronological_used_75 reverse_used_75))
(assert
 (= chronological_used_76 reverse_used_76))
(assert
 (= chronological_used_77 reverse_used_77))
(assert
 (= chronological_used_78 reverse_used_78))
(assert
 (= chronological_used_79 reverse_used_79))
(assert
 (= chronological_used_80 reverse_used_80))
(assert
 (= chronological_used_81 reverse_used_81))
(assert
 (= chronological_used_82 reverse_used_82))
(assert
 (= chronological_used_83 reverse_used_83))
(assert
 (= chronological_used_84 reverse_used_84))
(assert
 (= chronological_used_85 reverse_used_85))
(assert
 (= chronological_used_86 reverse_used_86))
(assert
 (= chronological_used_87 reverse_used_87))
(assert
 (= chronological_used_88 reverse_used_88))
(assert
 (= chronological_used_89 reverse_used_89))
(assert
 (= chronological_used_90 reverse_used_90))
(assert
 (= chronological_used_91 reverse_used_91))
(assert
 (= chronological_used_92 reverse_used_92))
(assert
 (= chronological_used_93 reverse_used_93))
(assert
 (= chronological_used_94 reverse_used_94))
(assert
 (= chronological_used_95 reverse_used_95))
(assert
 (= chronological_used_96 reverse_used_96))
(assert
 (= chronological_used_97 reverse_used_97))
(assert
 (= chronological_used_98 reverse_used_98))
(assert
 (= chronological_used_99 reverse_used_99))
(assert
 (= chronological_used_100 reverse_used_100))
(assert
 (= chronological_used_101 reverse_used_101))
(assert
 (= chronological_used_102 reverse_used_102))
(assert
 (= chronological_used_103 reverse_used_103))
(assert
 (= chronological_used_104 reverse_used_104))
(assert
 (= chronological_used_105 reverse_used_105))
(assert
 (= chronological_used_106 reverse_used_106))
(assert
 (= chronological_used_107 reverse_used_107))
(assert
 (= chronological_used_108 reverse_used_108))
(assert
 (= chronological_used_109 reverse_used_109))
(assert
 (= chronological_used_110 reverse_used_110))
(assert
 (= chronological_used_111 reverse_used_111))
(assert
 (= chronological_used_112 reverse_used_112))
(assert
 (= chronological_used_113 reverse_used_113))
(assert
 (= chronological_used_114 reverse_used_114))
(assert
 (= chronological_used_115 reverse_used_115))
(assert
 (= chronological_used_116 reverse_used_116))
(assert
 (= chronological_used_117 reverse_used_117))
(assert
 (= chronological_used_118 reverse_used_118))
(assert
 (= chronological_used_119 reverse_used_119))
(assert
 (= chronological_used_120 reverse_used_120))
(assert
 (= chronological_used_121 reverse_used_121))
(assert
 (= chronological_used_122 reverse_used_122))
(assert
 (= chronological_used_123 reverse_used_123))
(assert
 (= chronological_used_124 reverse_used_124))
(assert
 (= chronological_used_125 reverse_used_125))
(assert
 (= chronological_used_126 reverse_used_126))
(assert
 (= chronological_used_127 reverse_used_127))
(assert
 (= chronological_used_128 reverse_used_128))
(assert
 (= chronological_used_129 reverse_used_129))
(assert
 (= chronological_used_130 reverse_used_130))
(assert
 (= chronological_used_131 reverse_used_131))
(assert
 (= chronological_used_132 reverse_used_132))
(assert
 (= chronological_used_133 reverse_used_133))
(assert
 (= chronological_used_134 reverse_used_134))
(assert
 (= chronological_used_135 reverse_used_135))
(assert
 (= chronological_used_136 reverse_used_136))
(assert
 (= chronological_used_137 reverse_used_137))
(assert
 (= chronological_used_138 reverse_used_138))
(assert
 (= chronological_used_139 reverse_used_139))
(assert
 (= chronological_used_140 reverse_used_140))
(assert
 (= chronological_used_141 reverse_used_141))
(assert
 (= chronological_used_142 reverse_used_142))
(assert
 (= chronological_used_143 reverse_used_143))
(assert
 (= chronological_used_144 reverse_used_144))
(assert
 (= chronological_used_145 reverse_used_145))
(assert
 (= chronological_used_146 reverse_used_146))
(assert
 (= chronological_used_147 reverse_used_147))
(assert
 (= chronological_used_148 reverse_used_148))
(assert
 (= chronological_used_149 reverse_used_149))
(assert
 (= chronological_used_150 reverse_used_150))
(assert
 (= chronological_used_151 reverse_used_151))
(assert
 (= chronological_used_152 reverse_used_152))
(assert
 (= chronological_used_153 reverse_used_153))
(assert
 (= chronological_used_154 reverse_used_154))
(assert
 (= chronological_used_155 reverse_used_155))
(assert
 (= chronological_used_156 reverse_used_156))
(assert
 (= chronological_used_157 reverse_used_157))
(assert
 (= chronological_used_158 reverse_used_158))
(assert
 (= chronological_used_159 reverse_used_159))
(assert
 (= chronological_used_160 reverse_used_160))
(assert
 (= chronological_used_161 reverse_used_161))
(assert
 (= chronological_used_162 reverse_used_162))
(assert
 (= chronological_used_163 reverse_used_163))
(assert
 (= chronological_used_164 reverse_used_164))
(assert
 (= chronological_used_165 reverse_used_165))
(assert
 (= chronological_used_166 reverse_used_166))
(assert
 (= chronological_used_167 reverse_used_167))
(assert
 (= chronological_used_168 reverse_used_168))
(assert
 (= chronological_used_169 reverse_used_169))
(assert
 (= chronological_used_170 reverse_used_170))
(assert
 (= chronological_used_171 reverse_used_171))
(assert
 (= chronological_used_172 reverse_used_172))
(assert
 (= chronological_used_173 reverse_used_173))
(assert
 (= chronological_used_174 reverse_used_174))
(assert
 (= chronological_used_175 reverse_used_175))
(assert
 (= chronological_used_176 reverse_used_176))
(assert
 (= chronological_used_177 reverse_used_177))
(assert
 (= chronological_used_178 reverse_used_178))
(assert
 (= chronological_used_179 reverse_used_179))
(assert
 (= chronological_used_180 reverse_used_180))
(assert
 (= chronological_used_181 reverse_used_181))
(assert
 (= chronological_used_182 reverse_used_182))
(assert
 (= chronological_used_183 reverse_used_183))
(assert
 (= chronological_used_184 reverse_used_184))
(assert
 (= chronological_used_185 reverse_used_185))
(assert
 (= chronological_used_186 reverse_used_186))
(assert
 (= chronological_used_187 reverse_used_187))
(assert
 (= chronological_used_188 reverse_used_188))
(assert
 (= chronological_used_189 reverse_used_189))
(assert
 (= chronological_used_190 reverse_used_190))
(assert
 (= chronological_used_191 reverse_used_191))
(assert
 (= chronological_used_192 reverse_used_192))
(assert
 (= chronological_used_193 reverse_used_193))
(assert
 (= chronological_used_194 reverse_used_194))
(assert
 (= chronological_used_195 reverse_used_195))
(assert
 (= chronological_used_196 reverse_used_196))
(assert
 (= chronological_used_197 reverse_used_197))
(assert
 (= chronological_used_198 reverse_used_198))
(assert
 (= chronological_used_199 reverse_used_199))
(assert
 (= chronological_used_200 reverse_used_200))
(assert
 (= chronological_used_201 reverse_used_201))
(assert
 (= chronological_used_202 reverse_used_202))
(assert
 (= chronological_used_203 reverse_used_203))
(assert
 (= chronological_used_204 reverse_used_204))
(assert
 (= chronological_used_205 reverse_used_205))
(assert
 (= chronological_used_206 reverse_used_206))
(assert
 (= chronological_used_207 reverse_used_207))
(assert
 (= chronological_used_208 reverse_used_208))
(assert
 (= chronological_used_209 reverse_used_209))
(assert
 (= chronological_used_210 reverse_used_210))
(assert
 (= chronological_used_211 reverse_used_211))
(assert
 (= chronological_used_212 reverse_used_212))
(assert
 (= chronological_used_213 reverse_used_213))
(assert
 (= chronological_used_214 reverse_used_214))
(assert
 (= chronological_used_215 reverse_used_215))
(assert
 (= chronological_used_216 reverse_used_216))
(assert
 (= chronological_used_217 reverse_used_217))
(assert
 (= chronological_used_218 reverse_used_218))
(assert
 (= chronological_used_219 reverse_used_219))
(assert
 (= chronological_used_220 reverse_used_220))
(assert
 (= chronological_used_221 reverse_used_221))
(assert
 (= chronological_used_222 reverse_used_222))
(assert
 (= chronological_used_223 reverse_used_223))
(assert
 (= chronological_used_224 reverse_used_224))
(assert
 (= chronological_used_225 reverse_used_225))
(assert
 (= chronological_used_226 reverse_used_226))
(assert
 (= chronological_used_227 reverse_used_227))
(assert
 (= chronological_used_228 reverse_used_228))
(assert
 (= chronological_used_229 reverse_used_229))
(assert
 (= chronological_used_230 reverse_used_230))
(assert
 (= chronological_used_231 reverse_used_231))
(assert
 (= chronological_used_232 reverse_used_232))
(assert
 (= chronological_used_233 reverse_used_233))
(assert
 (= chronological_used_234 reverse_used_234))
(assert
 (= chronological_used_235 reverse_used_235))
(assert
 (= chronological_used_236 reverse_used_236))
(assert
 (= chronological_used_237 reverse_used_237))
(assert
 (= chronological_used_238 reverse_used_238))
(assert
 (= chronological_used_239 reverse_used_239))
(assert
 (= chronological_used_240 reverse_used_240))
(assert
 (= chronological_used_241 reverse_used_241))
(assert
 (= chronological_used_242 reverse_used_242))
(assert
 (= chronological_used_243 reverse_used_243))
(assert
 (= chronological_used_244 reverse_used_244))
(assert
 (= chronological_used_245 reverse_used_245))
(assert
 (= chronological_used_246 reverse_used_246))
(assert
 (= chronological_used_247 reverse_used_247))
(assert
 (= chronological_used_248 reverse_used_248))
(assert
 (= chronological_used_249 reverse_used_249))
(assert
 (= chronological_used_250 reverse_used_250))
(assert
 (= chronological_used_251 reverse_used_251))
(assert
 (= chronological_used_252 reverse_used_252))
(assert
 (= chronological_used_253 reverse_used_253))
(assert
 (= chronological_used_254 reverse_used_254))
(assert
 (= chronological_used_255 reverse_used_255))
(assert
 chronological_used_0)
(assert
 chronological_used_1)
(assert
 chronological_used_2)
(assert
 chronological_used_3)
(assert
 chronological_used_4)
(assert
 chronological_used_5)
(assert
 chronological_used_6)
(assert
 chronological_used_7)
(assert
 chronological_used_8)
(assert
 chronological_used_9)
(assert
 chronological_used_10)
(assert
 chronological_used_11)
(assert
 chronological_used_12)
(assert
 chronological_used_13)
(assert
 chronological_used_14)
(assert
 chronological_used_15)
(assert
 chronological_used_16)
(assert
 chronological_used_17)
(assert
 chronological_used_18)
(assert
 chronological_used_19)
(assert
 chronological_used_20)
(assert
 chronological_used_21)
(assert
 chronological_used_22)
(assert
 chronological_used_23)
(assert
 chronological_used_24)
(assert
 chronological_used_25)
(assert
 chronological_used_26)
(assert
 chronological_used_27)
(assert
 chronological_used_28)
(assert
 chronological_used_29)
(assert
 chronological_used_30)
(assert
 chronological_used_31)
(assert
 chronological_used_32)
(assert
 chronological_used_33)
(assert
 chronological_used_34)
(assert
 chronological_used_35)
(assert
 chronological_used_36)
(assert
 chronological_used_37)
(assert
 chronological_used_38)
(assert
 chronological_used_39)
(assert
 chronological_used_40)
(assert
 chronological_used_41)
(assert
 chronological_used_42)
(assert
 chronological_used_43)
(assert
 chronological_used_44)
(assert
 chronological_used_45)
(assert
 chronological_used_46)
(assert
 chronological_used_47)
(assert
 chronological_used_48)
(assert
 chronological_used_49)
(assert
 chronological_used_50)
(assert
 chronological_used_51)
(assert
 chronological_used_52)
(assert
 chronological_used_53)
(assert
 chronological_used_54)
(assert
 chronological_used_55)
(assert
 chronological_used_56)
(assert
 chronological_used_57)
(assert
 chronological_used_58)
(assert
 chronological_used_59)
(assert
 chronological_used_60)
(assert
 chronological_used_61)
(assert
 chronological_used_62)
(assert
 chronological_used_63)
(assert
 chronological_used_64)
(assert
 chronological_used_65)
(assert
 chronological_used_66)
(assert
 chronological_used_67)
(assert
 chronological_used_68)
(assert
 chronological_used_69)
(assert
 chronological_used_70)
(assert
 chronological_used_71)
(assert
 chronological_used_72)
(assert
 chronological_used_73)
(assert
 chronological_used_74)
(assert
 chronological_used_75)
(assert
 chronological_used_76)
(assert
 chronological_used_77)
(assert
 chronological_used_78)
(assert
 chronological_used_79)
(assert
 chronological_used_80)
(assert
 chronological_used_81)
(assert
 chronological_used_82)
(assert
 chronological_used_83)
(assert
 chronological_used_84)
(assert
 chronological_used_85)
(assert
 chronological_used_86)
(assert
 chronological_used_87)
(assert
 chronological_used_88)
(assert
 chronological_used_89)
(assert
 chronological_used_90)
(assert
 chronological_used_91)
(assert
 chronological_used_92)
(assert
 chronological_used_93)
(assert
 chronological_used_94)
(assert
 chronological_used_95)
(assert
 chronological_used_96)
(assert
 chronological_used_97)
(assert
 chronological_used_98)
(assert
 chronological_used_99)
(assert
 chronological_used_100)
(assert
 chronological_used_101)
(assert
 chronological_used_102)
(assert
 chronological_used_103)
(assert
 chronological_used_104)
(assert
 chronological_used_105)
(assert
 chronological_used_106)
(assert
 chronological_used_107)
(assert
 chronological_used_108)
(assert
 chronological_used_109)
(assert
 chronological_used_110)
(assert
 chronological_used_111)
(assert
 chronological_used_112)
(assert
 chronological_used_113)
(assert
 chronological_used_114)
(assert
 chronological_used_115)
(assert
 chronological_used_116)
(assert
 chronological_used_117)
(assert
 chronological_used_118)
(assert
 chronological_used_119)
(assert
 chronological_used_120)
(assert
 chronological_used_121)
(assert
 chronological_used_122)
(assert
 chronological_used_123)
(assert
 chronological_used_124)
(assert
 chronological_used_125)
(assert
 chronological_used_126)
(assert
 chronological_used_127)
(assert
 chronological_used_128)
(assert
 chronological_used_129)
(assert
 chronological_used_130)
(assert
 chronological_used_131)
(assert
 chronological_used_132)
(assert
 chronological_used_133)
(assert
 chronological_used_134)
(assert
 chronological_used_135)
(assert
 chronological_used_136)
(assert
 chronological_used_137)
(assert
 chronological_used_138)
(assert
 chronological_used_139)
(assert
 chronological_used_140)
(assert
 chronological_used_141)
(assert
 chronological_used_142)
(assert
 chronological_used_143)
(assert
 chronological_used_144)
(assert
 chronological_used_145)
(assert
 chronological_used_146)
(assert
 chronological_used_147)
(assert
 chronological_used_148)
(assert
 chronological_used_149)
(assert
 chronological_used_150)
(assert
 chronological_used_151)
(assert
 chronological_used_152)
(assert
 chronological_used_153)
(assert
 chronological_used_154)
(assert
 chronological_used_155)
(assert
 chronological_used_156)
(assert
 chronological_used_157)
(assert
 chronological_used_158)
(assert
 chronological_used_159)
(assert
 chronological_used_160)
(assert
 chronological_used_161)
(assert
 chronological_used_162)
(assert
 chronological_used_163)
(assert
 chronological_used_164)
(assert
 chronological_used_165)
(assert
 chronological_used_166)
(assert
 chronological_used_167)
(assert
 chronological_used_168)
(assert
 chronological_used_169)
(assert
 chronological_used_170)
(assert
 chronological_used_171)
(assert
 chronological_used_172)
(assert
 chronological_used_173)
(assert
 chronological_used_174)
(assert
 chronological_used_175)
(assert
 chronological_used_176)
(assert
 chronological_used_177)
(assert
 chronological_used_178)
(assert
 chronological_used_179)
(assert
 chronological_used_180)
(assert
 chronological_used_181)
(assert
 chronological_used_182)
(assert
 chronological_used_183)
(assert
 chronological_used_184)
(assert
 chronological_used_185)
(assert
 chronological_used_186)
(assert
 chronological_used_187)
(assert
 chronological_used_188)
(assert
 chronological_used_189)
(assert
 chronological_used_190)
(assert
 chronological_used_191)
(assert
 chronological_used_192)
(assert
 chronological_used_193)
(assert
 chronological_used_194)
(assert
 chronological_used_195)
(assert
 chronological_used_196)
(assert
 chronological_used_197)
(assert
 chronological_used_198)
(assert
 chronological_used_199)
(assert
 chronological_used_200)
(assert
 chronological_used_201)
(assert
 chronological_used_202)
(assert
 chronological_used_203)
(assert
 chronological_used_204)
(assert
 chronological_used_205)
(assert
 chronological_used_206)
(assert
 chronological_used_207)
(assert
 chronological_used_208)
(assert
 chronological_used_209)
(assert
 chronological_used_210)
(assert
 chronological_used_211)
(assert
 chronological_used_212)
(assert
 chronological_used_213)
(assert
 chronological_used_214)
(assert
 chronological_used_215)
(assert
 chronological_used_216)
(assert
 chronological_used_217)
(assert
 chronological_used_218)
(assert
 chronological_used_219)
(assert
 chronological_used_220)
(assert
 chronological_used_221)
(assert
 chronological_used_222)
(assert
 chronological_used_223)
(assert
 chronological_used_224)
(assert
 chronological_used_225)
(assert
 chronological_used_226)
(assert
 chronological_used_227)
(assert
 chronological_used_228)
(assert
 chronological_used_229)
(assert
 chronological_used_230)
(assert
 chronological_used_231)
(assert
 chronological_used_232)
(assert
 chronological_used_233)
(assert
 chronological_used_234)
(assert
 chronological_used_235)
(assert
 chronological_used_236)
(assert
 chronological_used_237)
(assert
 chronological_used_238)
(assert
 chronological_used_239)
(assert
 chronological_used_240)
(assert
 chronological_used_241)
(assert
 chronological_used_242)
(assert
 chronological_used_243)
(assert
 chronological_used_244)
(assert
 chronological_used_245)
(assert
 chronological_used_246)
(assert
 chronological_used_247)
(assert
 chronological_used_248)
(assert
 chronological_used_249)
(assert
 chronological_used_250)
(assert
 chronological_used_251)
(assert
 chronological_used_252)
(assert
 chronological_used_253)
(assert
 chronological_used_254)
(assert
 chronological_used_255)
(assert
 (let ((?x4435 (ite chronological_used_254 (ite chronological_used_255 256 255) 254)))
(let ((?x2680 (ite chronological_used_252 (ite chronological_used_253 ?x4435 253) 252)))
(let ((?x660 (ite chronological_used_250 (ite chronological_used_251 ?x2680 251) 250)))
(let ((?x748 (ite chronological_used_248 (ite chronological_used_249 ?x660 249) 248)))
(let ((?x4843 (ite chronological_used_246 (ite chronological_used_247 ?x748 247) 246)))
(let ((?x712 (ite chronological_used_244 (ite chronological_used_245 ?x4843 245) 244)))
(let ((?x678 (ite chronological_used_242 (ite chronological_used_243 ?x712 243) 242)))
(let ((?x650 (ite chronological_used_240 (ite chronological_used_241 ?x678 241) 240)))
(let ((?x5971 (ite chronological_used_238 (ite chronological_used_239 ?x650 239) 238)))
(let ((?x652 (ite chronological_used_236 (ite chronological_used_237 ?x5971 237) 236)))
(let ((?x684 (ite chronological_used_234 (ite chronological_used_235 ?x652 235) 234)))
(let ((?x632 (ite chronological_used_232 (ite chronological_used_233 ?x684 233) 232)))
(let ((?x5992 (ite chronological_used_230 (ite chronological_used_231 ?x632 231) 230)))
(let ((?x4818 (ite chronological_used_228 (ite chronological_used_229 ?x5992 229) 228)))
(let ((?x192 (ite chronological_used_226 (ite chronological_used_227 ?x4818 227) 226)))
(let ((?x2967 (ite chronological_used_224 (ite chronological_used_225 ?x192 225) 224)))
(let ((?x744 (ite chronological_used_222 (ite chronological_used_223 ?x2967 223) 222)))
(let ((?x5139 (ite chronological_used_220 (ite chronological_used_221 ?x744 221) 220)))
(let ((?x750 (ite chronological_used_218 (ite chronological_used_219 ?x5139 219) 218)))
(let ((?x756 (ite chronological_used_216 (ite chronological_used_217 ?x750 217) 216)))
(let ((?x760 (ite chronological_used_214 (ite chronological_used_215 ?x756 215) 214)))
(let ((?x764 (ite chronological_used_212 (ite chronological_used_213 ?x760 213) 212)))
(let ((?x768 (ite chronological_used_210 (ite chronological_used_211 ?x764 211) 210)))
(let ((?x772 (ite chronological_used_208 (ite chronological_used_209 ?x768 209) 208)))
(let ((?x776 (ite chronological_used_206 (ite chronological_used_207 ?x772 207) 206)))
(let ((?x780 (ite chronological_used_204 (ite chronological_used_205 ?x776 205) 204)))
(let ((?x784 (ite chronological_used_202 (ite chronological_used_203 ?x780 203) 202)))
(let ((?x788 (ite chronological_used_200 (ite chronological_used_201 ?x784 201) 200)))
(let ((?x792 (ite chronological_used_198 (ite chronological_used_199 ?x788 199) 198)))
(let ((?x796 (ite chronological_used_196 (ite chronological_used_197 ?x792 197) 196)))
(let ((?x800 (ite chronological_used_194 (ite chronological_used_195 ?x796 195) 194)))
(let ((?x804 (ite chronological_used_192 (ite chronological_used_193 ?x800 193) 192)))
(let ((?x808 (ite chronological_used_190 (ite chronological_used_191 ?x804 191) 190)))
(let ((?x812 (ite chronological_used_188 (ite chronological_used_189 ?x808 189) 188)))
(let ((?x816 (ite chronological_used_186 (ite chronological_used_187 ?x812 187) 186)))
(let ((?x820 (ite chronological_used_184 (ite chronological_used_185 ?x816 185) 184)))
(let ((?x824 (ite chronological_used_182 (ite chronological_used_183 ?x820 183) 182)))
(let ((?x2774 (ite chronological_used_180 (ite chronological_used_181 ?x824 181) 180)))
(let ((?x2796 (ite chronological_used_178 (ite chronological_used_179 ?x2774 179) 178)))
(let ((?x5572 (ite chronological_used_176 (ite chronological_used_177 ?x2796 177) 176)))
(let ((?x4526 (ite chronological_used_174 (ite chronological_used_175 ?x5572 175) 174)))
(let ((?x347 (ite chronological_used_172 (ite chronological_used_173 ?x4526 173) 172)))
(let ((?x351 (ite chronological_used_170 (ite chronological_used_171 ?x347 171) 170)))
(let ((?x359 (ite chronological_used_168 (ite chronological_used_169 ?x351 169) 168)))
(let ((?x367 (ite chronological_used_166 (ite chronological_used_167 ?x359 167) 166)))
(let ((?x375 (ite chronological_used_164 (ite chronological_used_165 ?x367 165) 164)))
(let ((?x383 (ite chronological_used_162 (ite chronological_used_163 ?x375 163) 162)))
(let ((?x391 (ite chronological_used_160 (ite chronological_used_161 ?x383 161) 160)))
(let ((?x399 (ite chronological_used_158 (ite chronological_used_159 ?x391 159) 158)))
(let ((?x407 (ite chronological_used_156 (ite chronological_used_157 ?x399 157) 156)))
(let ((?x415 (ite chronological_used_154 (ite chronological_used_155 ?x407 155) 154)))
(let ((?x423 (ite chronological_used_152 (ite chronological_used_153 ?x415 153) 152)))
(let ((?x431 (ite chronological_used_150 (ite chronological_used_151 ?x423 151) 150)))
(let ((?x439 (ite chronological_used_148 (ite chronological_used_149 ?x431 149) 148)))
(let ((?x447 (ite chronological_used_146 (ite chronological_used_147 ?x439 147) 146)))
(let ((?x455 (ite chronological_used_144 (ite chronological_used_145 ?x447 145) 144)))
(let ((?x463 (ite chronological_used_142 (ite chronological_used_143 ?x455 143) 142)))
(let ((?x471 (ite chronological_used_140 (ite chronological_used_141 ?x463 141) 140)))
(let ((?x479 (ite chronological_used_138 (ite chronological_used_139 ?x471 139) 138)))
(let ((?x487 (ite chronological_used_136 (ite chronological_used_137 ?x479 137) 136)))
(let ((?x495 (ite chronological_used_134 (ite chronological_used_135 ?x487 135) 134)))
(let ((?x503 (ite chronological_used_132 (ite chronological_used_133 ?x495 133) 132)))
(let ((?x511 (ite chronological_used_130 (ite chronological_used_131 ?x503 131) 130)))
(let ((?x519 (ite chronological_used_128 (ite chronological_used_129 ?x511 129) 128)))
(let ((?x527 (ite chronological_used_126 (ite chronological_used_127 ?x519 127) 126)))
(let ((?x535 (ite chronological_used_124 (ite chronological_used_125 ?x527 125) 124)))
(let ((?x543 (ite chronological_used_122 (ite chronological_used_123 ?x535 123) 122)))
(let ((?x551 (ite chronological_used_120 (ite chronological_used_121 ?x543 121) 120)))
(let ((?x559 (ite chronological_used_118 (ite chronological_used_119 ?x551 119) 118)))
(let ((?x567 (ite chronological_used_116 (ite chronological_used_117 ?x559 117) 116)))
(let ((?x575 (ite chronological_used_114 (ite chronological_used_115 ?x567 115) 114)))
(let ((?x583 (ite chronological_used_112 (ite chronological_used_113 ?x575 113) 112)))
(let ((?x591 (ite chronological_used_110 (ite chronological_used_111 ?x583 111) 110)))
(let ((?x599 (ite chronological_used_108 (ite chronological_used_109 ?x591 109) 108)))
(let ((?x607 (ite chronological_used_106 (ite chronological_used_107 ?x599 107) 106)))
(let ((?x615 (ite chronological_used_104 (ite chronological_used_105 ?x607 105) 104)))
(let ((?x623 (ite chronological_used_102 (ite chronological_used_103 ?x615 103) 102)))
(let ((?x631 (ite chronological_used_100 (ite chronological_used_101 ?x623 101) 100)))
(let ((?x639 (ite chronological_used_98 (ite chronological_used_99 ?x631 99) 98)))
(let ((?x647 (ite chronological_used_96 (ite chronological_used_97 ?x639 97) 96)))
(let ((?x655 (ite chronological_used_94 (ite chronological_used_95 ?x647 95) 94)))
(let ((?x663 (ite chronological_used_92 (ite chronological_used_93 ?x655 93) 92)))
(let ((?x671 (ite chronological_used_90 (ite chronological_used_91 ?x663 91) 90)))
(let ((?x679 (ite chronological_used_88 (ite chronological_used_89 ?x671 89) 88)))
(let ((?x687 (ite chronological_used_86 (ite chronological_used_87 ?x679 87) 86)))
(let ((?x695 (ite chronological_used_84 (ite chronological_used_85 ?x687 85) 84)))
(let ((?x703 (ite chronological_used_82 (ite chronological_used_83 ?x695 83) 82)))
(let ((?x711 (ite chronological_used_80 (ite chronological_used_81 ?x703 81) 80)))
(let ((?x719 (ite chronological_used_78 (ite chronological_used_79 ?x711 79) 78)))
(let ((?x727 (ite chronological_used_76 (ite chronological_used_77 ?x719 77) 76)))
(let ((?x735 (ite chronological_used_74 (ite chronological_used_75 ?x727 75) 74)))
(let ((?x743 (ite chronological_used_72 (ite chronological_used_73 ?x735 73) 72)))
(let ((?x751 (ite chronological_used_70 (ite chronological_used_71 ?x743 71) 70)))
(let ((?x759 (ite chronological_used_68 (ite chronological_used_69 ?x751 69) 68)))
(let ((?x767 (ite chronological_used_66 (ite chronological_used_67 ?x759 67) 66)))
(let ((?x775 (ite chronological_used_64 (ite chronological_used_65 ?x767 65) 64)))
(let ((?x783 (ite chronological_used_62 (ite chronological_used_63 ?x775 63) 62)))
(let ((?x791 (ite chronological_used_60 (ite chronological_used_61 ?x783 61) 60)))
(let ((?x799 (ite chronological_used_58 (ite chronological_used_59 ?x791 59) 58)))
(let ((?x807 (ite chronological_used_56 (ite chronological_used_57 ?x799 57) 56)))
(let ((?x815 (ite chronological_used_54 (ite chronological_used_55 ?x807 55) 54)))
(let ((?x823 (ite chronological_used_52 (ite chronological_used_53 ?x815 53) 52)))
(let ((?x831 (ite chronological_used_50 (ite chronological_used_51 ?x823 51) 50)))
(let ((?x839 (ite chronological_used_48 (ite chronological_used_49 ?x831 49) 48)))
(let ((?x846 (ite chronological_used_46 (ite chronological_used_47 ?x839 47) 46)))
(let ((?x850 (ite chronological_used_44 (ite chronological_used_45 ?x846 45) 44)))
(let ((?x8 (ite chronological_used_42 (ite chronological_used_43 ?x850 43) 42)))
(let ((?x121 (ite chronological_used_40 (ite chronological_used_41 ?x8 41) 40)))
(let ((?x98 (ite chronological_used_38 (ite chronological_used_39 ?x121 39) 38)))
(let ((?x96 (ite chronological_used_36 (ite chronological_used_37 ?x98 37) 36)))
(let ((?x78 (ite chronological_used_34 (ite chronological_used_35 ?x96 35) 34)))
(let ((?x103 (ite chronological_used_32 (ite chronological_used_33 ?x78 33) 32)))
(let ((?x135 (ite chronological_used_30 (ite chronological_used_31 ?x103 31) 30)))
(let ((?x100 (ite chronological_used_28 (ite chronological_used_29 ?x135 29) 28)))
(let ((?x58 (ite chronological_used_26 (ite chronological_used_27 ?x100 27) 26)))
(let ((?x32 (ite chronological_used_24 (ite chronological_used_25 ?x58 25) 24)))
(let ((?x19 (ite chronological_used_22 (ite chronological_used_23 ?x32 23) 22)))
(let ((?x21 (ite chronological_used_20 (ite chronological_used_21 ?x19 21) 20)))
(let ((?x23 (ite chronological_used_18 (ite chronological_used_19 ?x21 19) 18)))
(let ((?x13 (ite chronological_used_16 (ite chronological_used_17 ?x23 17) 16)))
(let ((?x52 (ite chronological_used_14 (ite chronological_used_15 ?x13 15) 14)))
(let ((?x17 (ite chronological_used_12 (ite chronological_used_13 ?x52 13) 12)))
(let ((?x95 (ite chronological_used_10 (ite chronological_used_11 ?x17 11) 10)))
(let ((?x26 (ite chronological_used_7 (ite chronological_used_8 (ite chronological_used_9 ?x95 9) 8) 7)))
(let ((?x74 (ite chronological_used_4 (ite chronological_used_5 (ite chronological_used_6 ?x26 6) 5) 4)))
(let ((?x97 (ite chronological_used_1 (ite chronological_used_2 (ite chronological_used_3 ?x74 3) 2) 1)))
(let ((?x50 (ite chronological_used_0 ?x97 0)))
(and (distinct ?x50 256) true)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
; EXPECT: unsat
(check-sat)
; Query 22: equal_sets_last_available_byte_nonvacuity
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun reverse_used_0 () Bool)
(declare-fun chronological_used_0 () Bool)
(declare-fun reverse_used_1 () Bool)
(declare-fun chronological_used_1 () Bool)
(declare-fun reverse_used_2 () Bool)
(declare-fun chronological_used_2 () Bool)
(declare-fun reverse_used_3 () Bool)
(declare-fun chronological_used_3 () Bool)
(declare-fun reverse_used_4 () Bool)
(declare-fun chronological_used_4 () Bool)
(declare-fun reverse_used_5 () Bool)
(declare-fun chronological_used_5 () Bool)
(declare-fun reverse_used_6 () Bool)
(declare-fun chronological_used_6 () Bool)
(declare-fun reverse_used_7 () Bool)
(declare-fun chronological_used_7 () Bool)
(declare-fun reverse_used_8 () Bool)
(declare-fun chronological_used_8 () Bool)
(declare-fun reverse_used_9 () Bool)
(declare-fun chronological_used_9 () Bool)
(declare-fun reverse_used_10 () Bool)
(declare-fun chronological_used_10 () Bool)
(declare-fun reverse_used_11 () Bool)
(declare-fun chronological_used_11 () Bool)
(declare-fun reverse_used_12 () Bool)
(declare-fun chronological_used_12 () Bool)
(declare-fun reverse_used_13 () Bool)
(declare-fun chronological_used_13 () Bool)
(declare-fun reverse_used_14 () Bool)
(declare-fun chronological_used_14 () Bool)
(declare-fun reverse_used_15 () Bool)
(declare-fun chronological_used_15 () Bool)
(declare-fun reverse_used_16 () Bool)
(declare-fun chronological_used_16 () Bool)
(declare-fun reverse_used_17 () Bool)
(declare-fun chronological_used_17 () Bool)
(declare-fun reverse_used_18 () Bool)
(declare-fun chronological_used_18 () Bool)
(declare-fun reverse_used_19 () Bool)
(declare-fun chronological_used_19 () Bool)
(declare-fun reverse_used_20 () Bool)
(declare-fun chronological_used_20 () Bool)
(declare-fun reverse_used_21 () Bool)
(declare-fun chronological_used_21 () Bool)
(declare-fun reverse_used_22 () Bool)
(declare-fun chronological_used_22 () Bool)
(declare-fun reverse_used_23 () Bool)
(declare-fun chronological_used_23 () Bool)
(declare-fun reverse_used_24 () Bool)
(declare-fun chronological_used_24 () Bool)
(declare-fun reverse_used_25 () Bool)
(declare-fun chronological_used_25 () Bool)
(declare-fun reverse_used_26 () Bool)
(declare-fun chronological_used_26 () Bool)
(declare-fun reverse_used_27 () Bool)
(declare-fun chronological_used_27 () Bool)
(declare-fun reverse_used_28 () Bool)
(declare-fun chronological_used_28 () Bool)
(declare-fun reverse_used_29 () Bool)
(declare-fun chronological_used_29 () Bool)
(declare-fun reverse_used_30 () Bool)
(declare-fun chronological_used_30 () Bool)
(declare-fun reverse_used_31 () Bool)
(declare-fun chronological_used_31 () Bool)
(declare-fun reverse_used_32 () Bool)
(declare-fun chronological_used_32 () Bool)
(declare-fun reverse_used_33 () Bool)
(declare-fun chronological_used_33 () Bool)
(declare-fun reverse_used_34 () Bool)
(declare-fun chronological_used_34 () Bool)
(declare-fun reverse_used_35 () Bool)
(declare-fun chronological_used_35 () Bool)
(declare-fun reverse_used_36 () Bool)
(declare-fun chronological_used_36 () Bool)
(declare-fun reverse_used_37 () Bool)
(declare-fun chronological_used_37 () Bool)
(declare-fun reverse_used_38 () Bool)
(declare-fun chronological_used_38 () Bool)
(declare-fun reverse_used_39 () Bool)
(declare-fun chronological_used_39 () Bool)
(declare-fun reverse_used_40 () Bool)
(declare-fun chronological_used_40 () Bool)
(declare-fun reverse_used_41 () Bool)
(declare-fun chronological_used_41 () Bool)
(declare-fun reverse_used_42 () Bool)
(declare-fun chronological_used_42 () Bool)
(declare-fun reverse_used_43 () Bool)
(declare-fun chronological_used_43 () Bool)
(declare-fun reverse_used_44 () Bool)
(declare-fun chronological_used_44 () Bool)
(declare-fun reverse_used_45 () Bool)
(declare-fun chronological_used_45 () Bool)
(declare-fun reverse_used_46 () Bool)
(declare-fun chronological_used_46 () Bool)
(declare-fun reverse_used_47 () Bool)
(declare-fun chronological_used_47 () Bool)
(declare-fun reverse_used_48 () Bool)
(declare-fun chronological_used_48 () Bool)
(declare-fun reverse_used_49 () Bool)
(declare-fun chronological_used_49 () Bool)
(declare-fun reverse_used_50 () Bool)
(declare-fun chronological_used_50 () Bool)
(declare-fun reverse_used_51 () Bool)
(declare-fun chronological_used_51 () Bool)
(declare-fun reverse_used_52 () Bool)
(declare-fun chronological_used_52 () Bool)
(declare-fun reverse_used_53 () Bool)
(declare-fun chronological_used_53 () Bool)
(declare-fun reverse_used_54 () Bool)
(declare-fun chronological_used_54 () Bool)
(declare-fun reverse_used_55 () Bool)
(declare-fun chronological_used_55 () Bool)
(declare-fun reverse_used_56 () Bool)
(declare-fun chronological_used_56 () Bool)
(declare-fun reverse_used_57 () Bool)
(declare-fun chronological_used_57 () Bool)
(declare-fun reverse_used_58 () Bool)
(declare-fun chronological_used_58 () Bool)
(declare-fun reverse_used_59 () Bool)
(declare-fun chronological_used_59 () Bool)
(declare-fun reverse_used_60 () Bool)
(declare-fun chronological_used_60 () Bool)
(declare-fun reverse_used_61 () Bool)
(declare-fun chronological_used_61 () Bool)
(declare-fun reverse_used_62 () Bool)
(declare-fun chronological_used_62 () Bool)
(declare-fun reverse_used_63 () Bool)
(declare-fun chronological_used_63 () Bool)
(declare-fun reverse_used_64 () Bool)
(declare-fun chronological_used_64 () Bool)
(declare-fun reverse_used_65 () Bool)
(declare-fun chronological_used_65 () Bool)
(declare-fun reverse_used_66 () Bool)
(declare-fun chronological_used_66 () Bool)
(declare-fun reverse_used_67 () Bool)
(declare-fun chronological_used_67 () Bool)
(declare-fun reverse_used_68 () Bool)
(declare-fun chronological_used_68 () Bool)
(declare-fun reverse_used_69 () Bool)
(declare-fun chronological_used_69 () Bool)
(declare-fun reverse_used_70 () Bool)
(declare-fun chronological_used_70 () Bool)
(declare-fun reverse_used_71 () Bool)
(declare-fun chronological_used_71 () Bool)
(declare-fun reverse_used_72 () Bool)
(declare-fun chronological_used_72 () Bool)
(declare-fun reverse_used_73 () Bool)
(declare-fun chronological_used_73 () Bool)
(declare-fun reverse_used_74 () Bool)
(declare-fun chronological_used_74 () Bool)
(declare-fun reverse_used_75 () Bool)
(declare-fun chronological_used_75 () Bool)
(declare-fun reverse_used_76 () Bool)
(declare-fun chronological_used_76 () Bool)
(declare-fun reverse_used_77 () Bool)
(declare-fun chronological_used_77 () Bool)
(declare-fun reverse_used_78 () Bool)
(declare-fun chronological_used_78 () Bool)
(declare-fun reverse_used_79 () Bool)
(declare-fun chronological_used_79 () Bool)
(declare-fun reverse_used_80 () Bool)
(declare-fun chronological_used_80 () Bool)
(declare-fun reverse_used_81 () Bool)
(declare-fun chronological_used_81 () Bool)
(declare-fun reverse_used_82 () Bool)
(declare-fun chronological_used_82 () Bool)
(declare-fun reverse_used_83 () Bool)
(declare-fun chronological_used_83 () Bool)
(declare-fun reverse_used_84 () Bool)
(declare-fun chronological_used_84 () Bool)
(declare-fun reverse_used_85 () Bool)
(declare-fun chronological_used_85 () Bool)
(declare-fun reverse_used_86 () Bool)
(declare-fun chronological_used_86 () Bool)
(declare-fun reverse_used_87 () Bool)
(declare-fun chronological_used_87 () Bool)
(declare-fun reverse_used_88 () Bool)
(declare-fun chronological_used_88 () Bool)
(declare-fun reverse_used_89 () Bool)
(declare-fun chronological_used_89 () Bool)
(declare-fun reverse_used_90 () Bool)
(declare-fun chronological_used_90 () Bool)
(declare-fun reverse_used_91 () Bool)
(declare-fun chronological_used_91 () Bool)
(declare-fun reverse_used_92 () Bool)
(declare-fun chronological_used_92 () Bool)
(declare-fun reverse_used_93 () Bool)
(declare-fun chronological_used_93 () Bool)
(declare-fun reverse_used_94 () Bool)
(declare-fun chronological_used_94 () Bool)
(declare-fun reverse_used_95 () Bool)
(declare-fun chronological_used_95 () Bool)
(declare-fun reverse_used_96 () Bool)
(declare-fun chronological_used_96 () Bool)
(declare-fun reverse_used_97 () Bool)
(declare-fun chronological_used_97 () Bool)
(declare-fun reverse_used_98 () Bool)
(declare-fun chronological_used_98 () Bool)
(declare-fun reverse_used_99 () Bool)
(declare-fun chronological_used_99 () Bool)
(declare-fun reverse_used_100 () Bool)
(declare-fun chronological_used_100 () Bool)
(declare-fun reverse_used_101 () Bool)
(declare-fun chronological_used_101 () Bool)
(declare-fun reverse_used_102 () Bool)
(declare-fun chronological_used_102 () Bool)
(declare-fun reverse_used_103 () Bool)
(declare-fun chronological_used_103 () Bool)
(declare-fun reverse_used_104 () Bool)
(declare-fun chronological_used_104 () Bool)
(declare-fun reverse_used_105 () Bool)
(declare-fun chronological_used_105 () Bool)
(declare-fun reverse_used_106 () Bool)
(declare-fun chronological_used_106 () Bool)
(declare-fun reverse_used_107 () Bool)
(declare-fun chronological_used_107 () Bool)
(declare-fun reverse_used_108 () Bool)
(declare-fun chronological_used_108 () Bool)
(declare-fun reverse_used_109 () Bool)
(declare-fun chronological_used_109 () Bool)
(declare-fun reverse_used_110 () Bool)
(declare-fun chronological_used_110 () Bool)
(declare-fun reverse_used_111 () Bool)
(declare-fun chronological_used_111 () Bool)
(declare-fun reverse_used_112 () Bool)
(declare-fun chronological_used_112 () Bool)
(declare-fun reverse_used_113 () Bool)
(declare-fun chronological_used_113 () Bool)
(declare-fun reverse_used_114 () Bool)
(declare-fun chronological_used_114 () Bool)
(declare-fun reverse_used_115 () Bool)
(declare-fun chronological_used_115 () Bool)
(declare-fun reverse_used_116 () Bool)
(declare-fun chronological_used_116 () Bool)
(declare-fun reverse_used_117 () Bool)
(declare-fun chronological_used_117 () Bool)
(declare-fun reverse_used_118 () Bool)
(declare-fun chronological_used_118 () Bool)
(declare-fun reverse_used_119 () Bool)
(declare-fun chronological_used_119 () Bool)
(declare-fun reverse_used_120 () Bool)
(declare-fun chronological_used_120 () Bool)
(declare-fun reverse_used_121 () Bool)
(declare-fun chronological_used_121 () Bool)
(declare-fun reverse_used_122 () Bool)
(declare-fun chronological_used_122 () Bool)
(declare-fun reverse_used_123 () Bool)
(declare-fun chronological_used_123 () Bool)
(declare-fun reverse_used_124 () Bool)
(declare-fun chronological_used_124 () Bool)
(declare-fun reverse_used_125 () Bool)
(declare-fun chronological_used_125 () Bool)
(declare-fun reverse_used_126 () Bool)
(declare-fun chronological_used_126 () Bool)
(declare-fun reverse_used_127 () Bool)
(declare-fun chronological_used_127 () Bool)
(declare-fun reverse_used_128 () Bool)
(declare-fun chronological_used_128 () Bool)
(declare-fun reverse_used_129 () Bool)
(declare-fun chronological_used_129 () Bool)
(declare-fun reverse_used_130 () Bool)
(declare-fun chronological_used_130 () Bool)
(declare-fun reverse_used_131 () Bool)
(declare-fun chronological_used_131 () Bool)
(declare-fun reverse_used_132 () Bool)
(declare-fun chronological_used_132 () Bool)
(declare-fun reverse_used_133 () Bool)
(declare-fun chronological_used_133 () Bool)
(declare-fun reverse_used_134 () Bool)
(declare-fun chronological_used_134 () Bool)
(declare-fun reverse_used_135 () Bool)
(declare-fun chronological_used_135 () Bool)
(declare-fun reverse_used_136 () Bool)
(declare-fun chronological_used_136 () Bool)
(declare-fun reverse_used_137 () Bool)
(declare-fun chronological_used_137 () Bool)
(declare-fun reverse_used_138 () Bool)
(declare-fun chronological_used_138 () Bool)
(declare-fun reverse_used_139 () Bool)
(declare-fun chronological_used_139 () Bool)
(declare-fun reverse_used_140 () Bool)
(declare-fun chronological_used_140 () Bool)
(declare-fun reverse_used_141 () Bool)
(declare-fun chronological_used_141 () Bool)
(declare-fun reverse_used_142 () Bool)
(declare-fun chronological_used_142 () Bool)
(declare-fun reverse_used_143 () Bool)
(declare-fun chronological_used_143 () Bool)
(declare-fun reverse_used_144 () Bool)
(declare-fun chronological_used_144 () Bool)
(declare-fun reverse_used_145 () Bool)
(declare-fun chronological_used_145 () Bool)
(declare-fun reverse_used_146 () Bool)
(declare-fun chronological_used_146 () Bool)
(declare-fun reverse_used_147 () Bool)
(declare-fun chronological_used_147 () Bool)
(declare-fun reverse_used_148 () Bool)
(declare-fun chronological_used_148 () Bool)
(declare-fun reverse_used_149 () Bool)
(declare-fun chronological_used_149 () Bool)
(declare-fun reverse_used_150 () Bool)
(declare-fun chronological_used_150 () Bool)
(declare-fun reverse_used_151 () Bool)
(declare-fun chronological_used_151 () Bool)
(declare-fun reverse_used_152 () Bool)
(declare-fun chronological_used_152 () Bool)
(declare-fun reverse_used_153 () Bool)
(declare-fun chronological_used_153 () Bool)
(declare-fun reverse_used_154 () Bool)
(declare-fun chronological_used_154 () Bool)
(declare-fun reverse_used_155 () Bool)
(declare-fun chronological_used_155 () Bool)
(declare-fun reverse_used_156 () Bool)
(declare-fun chronological_used_156 () Bool)
(declare-fun reverse_used_157 () Bool)
(declare-fun chronological_used_157 () Bool)
(declare-fun reverse_used_158 () Bool)
(declare-fun chronological_used_158 () Bool)
(declare-fun reverse_used_159 () Bool)
(declare-fun chronological_used_159 () Bool)
(declare-fun reverse_used_160 () Bool)
(declare-fun chronological_used_160 () Bool)
(declare-fun reverse_used_161 () Bool)
(declare-fun chronological_used_161 () Bool)
(declare-fun reverse_used_162 () Bool)
(declare-fun chronological_used_162 () Bool)
(declare-fun reverse_used_163 () Bool)
(declare-fun chronological_used_163 () Bool)
(declare-fun reverse_used_164 () Bool)
(declare-fun chronological_used_164 () Bool)
(declare-fun reverse_used_165 () Bool)
(declare-fun chronological_used_165 () Bool)
(declare-fun reverse_used_166 () Bool)
(declare-fun chronological_used_166 () Bool)
(declare-fun reverse_used_167 () Bool)
(declare-fun chronological_used_167 () Bool)
(declare-fun reverse_used_168 () Bool)
(declare-fun chronological_used_168 () Bool)
(declare-fun reverse_used_169 () Bool)
(declare-fun chronological_used_169 () Bool)
(declare-fun reverse_used_170 () Bool)
(declare-fun chronological_used_170 () Bool)
(declare-fun reverse_used_171 () Bool)
(declare-fun chronological_used_171 () Bool)
(declare-fun reverse_used_172 () Bool)
(declare-fun chronological_used_172 () Bool)
(declare-fun reverse_used_173 () Bool)
(declare-fun chronological_used_173 () Bool)
(declare-fun reverse_used_174 () Bool)
(declare-fun chronological_used_174 () Bool)
(declare-fun reverse_used_175 () Bool)
(declare-fun chronological_used_175 () Bool)
(declare-fun reverse_used_176 () Bool)
(declare-fun chronological_used_176 () Bool)
(declare-fun reverse_used_177 () Bool)
(declare-fun chronological_used_177 () Bool)
(declare-fun reverse_used_178 () Bool)
(declare-fun chronological_used_178 () Bool)
(declare-fun reverse_used_179 () Bool)
(declare-fun chronological_used_179 () Bool)
(declare-fun reverse_used_180 () Bool)
(declare-fun chronological_used_180 () Bool)
(declare-fun reverse_used_181 () Bool)
(declare-fun chronological_used_181 () Bool)
(declare-fun reverse_used_182 () Bool)
(declare-fun chronological_used_182 () Bool)
(declare-fun reverse_used_183 () Bool)
(declare-fun chronological_used_183 () Bool)
(declare-fun reverse_used_184 () Bool)
(declare-fun chronological_used_184 () Bool)
(declare-fun reverse_used_185 () Bool)
(declare-fun chronological_used_185 () Bool)
(declare-fun reverse_used_186 () Bool)
(declare-fun chronological_used_186 () Bool)
(declare-fun reverse_used_187 () Bool)
(declare-fun chronological_used_187 () Bool)
(declare-fun reverse_used_188 () Bool)
(declare-fun chronological_used_188 () Bool)
(declare-fun reverse_used_189 () Bool)
(declare-fun chronological_used_189 () Bool)
(declare-fun reverse_used_190 () Bool)
(declare-fun chronological_used_190 () Bool)
(declare-fun reverse_used_191 () Bool)
(declare-fun chronological_used_191 () Bool)
(declare-fun reverse_used_192 () Bool)
(declare-fun chronological_used_192 () Bool)
(declare-fun reverse_used_193 () Bool)
(declare-fun chronological_used_193 () Bool)
(declare-fun reverse_used_194 () Bool)
(declare-fun chronological_used_194 () Bool)
(declare-fun reverse_used_195 () Bool)
(declare-fun chronological_used_195 () Bool)
(declare-fun reverse_used_196 () Bool)
(declare-fun chronological_used_196 () Bool)
(declare-fun reverse_used_197 () Bool)
(declare-fun chronological_used_197 () Bool)
(declare-fun reverse_used_198 () Bool)
(declare-fun chronological_used_198 () Bool)
(declare-fun reverse_used_199 () Bool)
(declare-fun chronological_used_199 () Bool)
(declare-fun reverse_used_200 () Bool)
(declare-fun chronological_used_200 () Bool)
(declare-fun reverse_used_201 () Bool)
(declare-fun chronological_used_201 () Bool)
(declare-fun reverse_used_202 () Bool)
(declare-fun chronological_used_202 () Bool)
(declare-fun reverse_used_203 () Bool)
(declare-fun chronological_used_203 () Bool)
(declare-fun reverse_used_204 () Bool)
(declare-fun chronological_used_204 () Bool)
(declare-fun reverse_used_205 () Bool)
(declare-fun chronological_used_205 () Bool)
(declare-fun reverse_used_206 () Bool)
(declare-fun chronological_used_206 () Bool)
(declare-fun reverse_used_207 () Bool)
(declare-fun chronological_used_207 () Bool)
(declare-fun reverse_used_208 () Bool)
(declare-fun chronological_used_208 () Bool)
(declare-fun reverse_used_209 () Bool)
(declare-fun chronological_used_209 () Bool)
(declare-fun reverse_used_210 () Bool)
(declare-fun chronological_used_210 () Bool)
(declare-fun reverse_used_211 () Bool)
(declare-fun chronological_used_211 () Bool)
(declare-fun reverse_used_212 () Bool)
(declare-fun chronological_used_212 () Bool)
(declare-fun reverse_used_213 () Bool)
(declare-fun chronological_used_213 () Bool)
(declare-fun reverse_used_214 () Bool)
(declare-fun chronological_used_214 () Bool)
(declare-fun reverse_used_215 () Bool)
(declare-fun chronological_used_215 () Bool)
(declare-fun reverse_used_216 () Bool)
(declare-fun chronological_used_216 () Bool)
(declare-fun reverse_used_217 () Bool)
(declare-fun chronological_used_217 () Bool)
(declare-fun reverse_used_218 () Bool)
(declare-fun chronological_used_218 () Bool)
(declare-fun reverse_used_219 () Bool)
(declare-fun chronological_used_219 () Bool)
(declare-fun reverse_used_220 () Bool)
(declare-fun chronological_used_220 () Bool)
(declare-fun reverse_used_221 () Bool)
(declare-fun chronological_used_221 () Bool)
(declare-fun reverse_used_222 () Bool)
(declare-fun chronological_used_222 () Bool)
(declare-fun reverse_used_223 () Bool)
(declare-fun chronological_used_223 () Bool)
(declare-fun reverse_used_224 () Bool)
(declare-fun chronological_used_224 () Bool)
(declare-fun reverse_used_225 () Bool)
(declare-fun chronological_used_225 () Bool)
(declare-fun reverse_used_226 () Bool)
(declare-fun chronological_used_226 () Bool)
(declare-fun reverse_used_227 () Bool)
(declare-fun chronological_used_227 () Bool)
(declare-fun reverse_used_228 () Bool)
(declare-fun chronological_used_228 () Bool)
(declare-fun reverse_used_229 () Bool)
(declare-fun chronological_used_229 () Bool)
(declare-fun reverse_used_230 () Bool)
(declare-fun chronological_used_230 () Bool)
(declare-fun reverse_used_231 () Bool)
(declare-fun chronological_used_231 () Bool)
(declare-fun reverse_used_232 () Bool)
(declare-fun chronological_used_232 () Bool)
(declare-fun reverse_used_233 () Bool)
(declare-fun chronological_used_233 () Bool)
(declare-fun reverse_used_234 () Bool)
(declare-fun chronological_used_234 () Bool)
(declare-fun reverse_used_235 () Bool)
(declare-fun chronological_used_235 () Bool)
(declare-fun reverse_used_236 () Bool)
(declare-fun chronological_used_236 () Bool)
(declare-fun reverse_used_237 () Bool)
(declare-fun chronological_used_237 () Bool)
(declare-fun reverse_used_238 () Bool)
(declare-fun chronological_used_238 () Bool)
(declare-fun reverse_used_239 () Bool)
(declare-fun chronological_used_239 () Bool)
(declare-fun reverse_used_240 () Bool)
(declare-fun chronological_used_240 () Bool)
(declare-fun reverse_used_241 () Bool)
(declare-fun chronological_used_241 () Bool)
(declare-fun reverse_used_242 () Bool)
(declare-fun chronological_used_242 () Bool)
(declare-fun reverse_used_243 () Bool)
(declare-fun chronological_used_243 () Bool)
(declare-fun reverse_used_244 () Bool)
(declare-fun chronological_used_244 () Bool)
(declare-fun reverse_used_245 () Bool)
(declare-fun chronological_used_245 () Bool)
(declare-fun reverse_used_246 () Bool)
(declare-fun chronological_used_246 () Bool)
(declare-fun reverse_used_247 () Bool)
(declare-fun chronological_used_247 () Bool)
(declare-fun reverse_used_248 () Bool)
(declare-fun chronological_used_248 () Bool)
(declare-fun reverse_used_249 () Bool)
(declare-fun chronological_used_249 () Bool)
(declare-fun reverse_used_250 () Bool)
(declare-fun chronological_used_250 () Bool)
(declare-fun reverse_used_251 () Bool)
(declare-fun chronological_used_251 () Bool)
(declare-fun reverse_used_252 () Bool)
(declare-fun chronological_used_252 () Bool)
(declare-fun reverse_used_253 () Bool)
(declare-fun chronological_used_253 () Bool)
(declare-fun reverse_used_254 () Bool)
(declare-fun chronological_used_254 () Bool)
(declare-fun reverse_used_255 () Bool)
(declare-fun chronological_used_255 () Bool)
(assert
 (= chronological_used_0 reverse_used_0))
(assert
 (= chronological_used_1 reverse_used_1))
(assert
 (= chronological_used_2 reverse_used_2))
(assert
 (= chronological_used_3 reverse_used_3))
(assert
 (= chronological_used_4 reverse_used_4))
(assert
 (= chronological_used_5 reverse_used_5))
(assert
 (= chronological_used_6 reverse_used_6))
(assert
 (= chronological_used_7 reverse_used_7))
(assert
 (= chronological_used_8 reverse_used_8))
(assert
 (= chronological_used_9 reverse_used_9))
(assert
 (= chronological_used_10 reverse_used_10))
(assert
 (= chronological_used_11 reverse_used_11))
(assert
 (= chronological_used_12 reverse_used_12))
(assert
 (= chronological_used_13 reverse_used_13))
(assert
 (= chronological_used_14 reverse_used_14))
(assert
 (= chronological_used_15 reverse_used_15))
(assert
 (= chronological_used_16 reverse_used_16))
(assert
 (= chronological_used_17 reverse_used_17))
(assert
 (= chronological_used_18 reverse_used_18))
(assert
 (= chronological_used_19 reverse_used_19))
(assert
 (= chronological_used_20 reverse_used_20))
(assert
 (= chronological_used_21 reverse_used_21))
(assert
 (= chronological_used_22 reverse_used_22))
(assert
 (= chronological_used_23 reverse_used_23))
(assert
 (= chronological_used_24 reverse_used_24))
(assert
 (= chronological_used_25 reverse_used_25))
(assert
 (= chronological_used_26 reverse_used_26))
(assert
 (= chronological_used_27 reverse_used_27))
(assert
 (= chronological_used_28 reverse_used_28))
(assert
 (= chronological_used_29 reverse_used_29))
(assert
 (= chronological_used_30 reverse_used_30))
(assert
 (= chronological_used_31 reverse_used_31))
(assert
 (= chronological_used_32 reverse_used_32))
(assert
 (= chronological_used_33 reverse_used_33))
(assert
 (= chronological_used_34 reverse_used_34))
(assert
 (= chronological_used_35 reverse_used_35))
(assert
 (= chronological_used_36 reverse_used_36))
(assert
 (= chronological_used_37 reverse_used_37))
(assert
 (= chronological_used_38 reverse_used_38))
(assert
 (= chronological_used_39 reverse_used_39))
(assert
 (= chronological_used_40 reverse_used_40))
(assert
 (= chronological_used_41 reverse_used_41))
(assert
 (= chronological_used_42 reverse_used_42))
(assert
 (= chronological_used_43 reverse_used_43))
(assert
 (= chronological_used_44 reverse_used_44))
(assert
 (= chronological_used_45 reverse_used_45))
(assert
 (= chronological_used_46 reverse_used_46))
(assert
 (= chronological_used_47 reverse_used_47))
(assert
 (= chronological_used_48 reverse_used_48))
(assert
 (= chronological_used_49 reverse_used_49))
(assert
 (= chronological_used_50 reverse_used_50))
(assert
 (= chronological_used_51 reverse_used_51))
(assert
 (= chronological_used_52 reverse_used_52))
(assert
 (= chronological_used_53 reverse_used_53))
(assert
 (= chronological_used_54 reverse_used_54))
(assert
 (= chronological_used_55 reverse_used_55))
(assert
 (= chronological_used_56 reverse_used_56))
(assert
 (= chronological_used_57 reverse_used_57))
(assert
 (= chronological_used_58 reverse_used_58))
(assert
 (= chronological_used_59 reverse_used_59))
(assert
 (= chronological_used_60 reverse_used_60))
(assert
 (= chronological_used_61 reverse_used_61))
(assert
 (= chronological_used_62 reverse_used_62))
(assert
 (= chronological_used_63 reverse_used_63))
(assert
 (= chronological_used_64 reverse_used_64))
(assert
 (= chronological_used_65 reverse_used_65))
(assert
 (= chronological_used_66 reverse_used_66))
(assert
 (= chronological_used_67 reverse_used_67))
(assert
 (= chronological_used_68 reverse_used_68))
(assert
 (= chronological_used_69 reverse_used_69))
(assert
 (= chronological_used_70 reverse_used_70))
(assert
 (= chronological_used_71 reverse_used_71))
(assert
 (= chronological_used_72 reverse_used_72))
(assert
 (= chronological_used_73 reverse_used_73))
(assert
 (= chronological_used_74 reverse_used_74))
(assert
 (= chronological_used_75 reverse_used_75))
(assert
 (= chronological_used_76 reverse_used_76))
(assert
 (= chronological_used_77 reverse_used_77))
(assert
 (= chronological_used_78 reverse_used_78))
(assert
 (= chronological_used_79 reverse_used_79))
(assert
 (= chronological_used_80 reverse_used_80))
(assert
 (= chronological_used_81 reverse_used_81))
(assert
 (= chronological_used_82 reverse_used_82))
(assert
 (= chronological_used_83 reverse_used_83))
(assert
 (= chronological_used_84 reverse_used_84))
(assert
 (= chronological_used_85 reverse_used_85))
(assert
 (= chronological_used_86 reverse_used_86))
(assert
 (= chronological_used_87 reverse_used_87))
(assert
 (= chronological_used_88 reverse_used_88))
(assert
 (= chronological_used_89 reverse_used_89))
(assert
 (= chronological_used_90 reverse_used_90))
(assert
 (= chronological_used_91 reverse_used_91))
(assert
 (= chronological_used_92 reverse_used_92))
(assert
 (= chronological_used_93 reverse_used_93))
(assert
 (= chronological_used_94 reverse_used_94))
(assert
 (= chronological_used_95 reverse_used_95))
(assert
 (= chronological_used_96 reverse_used_96))
(assert
 (= chronological_used_97 reverse_used_97))
(assert
 (= chronological_used_98 reverse_used_98))
(assert
 (= chronological_used_99 reverse_used_99))
(assert
 (= chronological_used_100 reverse_used_100))
(assert
 (= chronological_used_101 reverse_used_101))
(assert
 (= chronological_used_102 reverse_used_102))
(assert
 (= chronological_used_103 reverse_used_103))
(assert
 (= chronological_used_104 reverse_used_104))
(assert
 (= chronological_used_105 reverse_used_105))
(assert
 (= chronological_used_106 reverse_used_106))
(assert
 (= chronological_used_107 reverse_used_107))
(assert
 (= chronological_used_108 reverse_used_108))
(assert
 (= chronological_used_109 reverse_used_109))
(assert
 (= chronological_used_110 reverse_used_110))
(assert
 (= chronological_used_111 reverse_used_111))
(assert
 (= chronological_used_112 reverse_used_112))
(assert
 (= chronological_used_113 reverse_used_113))
(assert
 (= chronological_used_114 reverse_used_114))
(assert
 (= chronological_used_115 reverse_used_115))
(assert
 (= chronological_used_116 reverse_used_116))
(assert
 (= chronological_used_117 reverse_used_117))
(assert
 (= chronological_used_118 reverse_used_118))
(assert
 (= chronological_used_119 reverse_used_119))
(assert
 (= chronological_used_120 reverse_used_120))
(assert
 (= chronological_used_121 reverse_used_121))
(assert
 (= chronological_used_122 reverse_used_122))
(assert
 (= chronological_used_123 reverse_used_123))
(assert
 (= chronological_used_124 reverse_used_124))
(assert
 (= chronological_used_125 reverse_used_125))
(assert
 (= chronological_used_126 reverse_used_126))
(assert
 (= chronological_used_127 reverse_used_127))
(assert
 (= chronological_used_128 reverse_used_128))
(assert
 (= chronological_used_129 reverse_used_129))
(assert
 (= chronological_used_130 reverse_used_130))
(assert
 (= chronological_used_131 reverse_used_131))
(assert
 (= chronological_used_132 reverse_used_132))
(assert
 (= chronological_used_133 reverse_used_133))
(assert
 (= chronological_used_134 reverse_used_134))
(assert
 (= chronological_used_135 reverse_used_135))
(assert
 (= chronological_used_136 reverse_used_136))
(assert
 (= chronological_used_137 reverse_used_137))
(assert
 (= chronological_used_138 reverse_used_138))
(assert
 (= chronological_used_139 reverse_used_139))
(assert
 (= chronological_used_140 reverse_used_140))
(assert
 (= chronological_used_141 reverse_used_141))
(assert
 (= chronological_used_142 reverse_used_142))
(assert
 (= chronological_used_143 reverse_used_143))
(assert
 (= chronological_used_144 reverse_used_144))
(assert
 (= chronological_used_145 reverse_used_145))
(assert
 (= chronological_used_146 reverse_used_146))
(assert
 (= chronological_used_147 reverse_used_147))
(assert
 (= chronological_used_148 reverse_used_148))
(assert
 (= chronological_used_149 reverse_used_149))
(assert
 (= chronological_used_150 reverse_used_150))
(assert
 (= chronological_used_151 reverse_used_151))
(assert
 (= chronological_used_152 reverse_used_152))
(assert
 (= chronological_used_153 reverse_used_153))
(assert
 (= chronological_used_154 reverse_used_154))
(assert
 (= chronological_used_155 reverse_used_155))
(assert
 (= chronological_used_156 reverse_used_156))
(assert
 (= chronological_used_157 reverse_used_157))
(assert
 (= chronological_used_158 reverse_used_158))
(assert
 (= chronological_used_159 reverse_used_159))
(assert
 (= chronological_used_160 reverse_used_160))
(assert
 (= chronological_used_161 reverse_used_161))
(assert
 (= chronological_used_162 reverse_used_162))
(assert
 (= chronological_used_163 reverse_used_163))
(assert
 (= chronological_used_164 reverse_used_164))
(assert
 (= chronological_used_165 reverse_used_165))
(assert
 (= chronological_used_166 reverse_used_166))
(assert
 (= chronological_used_167 reverse_used_167))
(assert
 (= chronological_used_168 reverse_used_168))
(assert
 (= chronological_used_169 reverse_used_169))
(assert
 (= chronological_used_170 reverse_used_170))
(assert
 (= chronological_used_171 reverse_used_171))
(assert
 (= chronological_used_172 reverse_used_172))
(assert
 (= chronological_used_173 reverse_used_173))
(assert
 (= chronological_used_174 reverse_used_174))
(assert
 (= chronological_used_175 reverse_used_175))
(assert
 (= chronological_used_176 reverse_used_176))
(assert
 (= chronological_used_177 reverse_used_177))
(assert
 (= chronological_used_178 reverse_used_178))
(assert
 (= chronological_used_179 reverse_used_179))
(assert
 (= chronological_used_180 reverse_used_180))
(assert
 (= chronological_used_181 reverse_used_181))
(assert
 (= chronological_used_182 reverse_used_182))
(assert
 (= chronological_used_183 reverse_used_183))
(assert
 (= chronological_used_184 reverse_used_184))
(assert
 (= chronological_used_185 reverse_used_185))
(assert
 (= chronological_used_186 reverse_used_186))
(assert
 (= chronological_used_187 reverse_used_187))
(assert
 (= chronological_used_188 reverse_used_188))
(assert
 (= chronological_used_189 reverse_used_189))
(assert
 (= chronological_used_190 reverse_used_190))
(assert
 (= chronological_used_191 reverse_used_191))
(assert
 (= chronological_used_192 reverse_used_192))
(assert
 (= chronological_used_193 reverse_used_193))
(assert
 (= chronological_used_194 reverse_used_194))
(assert
 (= chronological_used_195 reverse_used_195))
(assert
 (= chronological_used_196 reverse_used_196))
(assert
 (= chronological_used_197 reverse_used_197))
(assert
 (= chronological_used_198 reverse_used_198))
(assert
 (= chronological_used_199 reverse_used_199))
(assert
 (= chronological_used_200 reverse_used_200))
(assert
 (= chronological_used_201 reverse_used_201))
(assert
 (= chronological_used_202 reverse_used_202))
(assert
 (= chronological_used_203 reverse_used_203))
(assert
 (= chronological_used_204 reverse_used_204))
(assert
 (= chronological_used_205 reverse_used_205))
(assert
 (= chronological_used_206 reverse_used_206))
(assert
 (= chronological_used_207 reverse_used_207))
(assert
 (= chronological_used_208 reverse_used_208))
(assert
 (= chronological_used_209 reverse_used_209))
(assert
 (= chronological_used_210 reverse_used_210))
(assert
 (= chronological_used_211 reverse_used_211))
(assert
 (= chronological_used_212 reverse_used_212))
(assert
 (= chronological_used_213 reverse_used_213))
(assert
 (= chronological_used_214 reverse_used_214))
(assert
 (= chronological_used_215 reverse_used_215))
(assert
 (= chronological_used_216 reverse_used_216))
(assert
 (= chronological_used_217 reverse_used_217))
(assert
 (= chronological_used_218 reverse_used_218))
(assert
 (= chronological_used_219 reverse_used_219))
(assert
 (= chronological_used_220 reverse_used_220))
(assert
 (= chronological_used_221 reverse_used_221))
(assert
 (= chronological_used_222 reverse_used_222))
(assert
 (= chronological_used_223 reverse_used_223))
(assert
 (= chronological_used_224 reverse_used_224))
(assert
 (= chronological_used_225 reverse_used_225))
(assert
 (= chronological_used_226 reverse_used_226))
(assert
 (= chronological_used_227 reverse_used_227))
(assert
 (= chronological_used_228 reverse_used_228))
(assert
 (= chronological_used_229 reverse_used_229))
(assert
 (= chronological_used_230 reverse_used_230))
(assert
 (= chronological_used_231 reverse_used_231))
(assert
 (= chronological_used_232 reverse_used_232))
(assert
 (= chronological_used_233 reverse_used_233))
(assert
 (= chronological_used_234 reverse_used_234))
(assert
 (= chronological_used_235 reverse_used_235))
(assert
 (= chronological_used_236 reverse_used_236))
(assert
 (= chronological_used_237 reverse_used_237))
(assert
 (= chronological_used_238 reverse_used_238))
(assert
 (= chronological_used_239 reverse_used_239))
(assert
 (= chronological_used_240 reverse_used_240))
(assert
 (= chronological_used_241 reverse_used_241))
(assert
 (= chronological_used_242 reverse_used_242))
(assert
 (= chronological_used_243 reverse_used_243))
(assert
 (= chronological_used_244 reverse_used_244))
(assert
 (= chronological_used_245 reverse_used_245))
(assert
 (= chronological_used_246 reverse_used_246))
(assert
 (= chronological_used_247 reverse_used_247))
(assert
 (= chronological_used_248 reverse_used_248))
(assert
 (= chronological_used_249 reverse_used_249))
(assert
 (= chronological_used_250 reverse_used_250))
(assert
 (= chronological_used_251 reverse_used_251))
(assert
 (= chronological_used_252 reverse_used_252))
(assert
 (= chronological_used_253 reverse_used_253))
(assert
 (= chronological_used_254 reverse_used_254))
(assert
 (= chronological_used_255 reverse_used_255))
(assert
 chronological_used_0)
(assert
 chronological_used_1)
(assert
 chronological_used_2)
(assert
 chronological_used_3)
(assert
 chronological_used_4)
(assert
 chronological_used_5)
(assert
 chronological_used_6)
(assert
 chronological_used_7)
(assert
 chronological_used_8)
(assert
 chronological_used_9)
(assert
 chronological_used_10)
(assert
 chronological_used_11)
(assert
 chronological_used_12)
(assert
 chronological_used_13)
(assert
 chronological_used_14)
(assert
 chronological_used_15)
(assert
 chronological_used_16)
(assert
 chronological_used_17)
(assert
 chronological_used_18)
(assert
 chronological_used_19)
(assert
 chronological_used_20)
(assert
 chronological_used_21)
(assert
 chronological_used_22)
(assert
 chronological_used_23)
(assert
 chronological_used_24)
(assert
 chronological_used_25)
(assert
 chronological_used_26)
(assert
 chronological_used_27)
(assert
 chronological_used_28)
(assert
 chronological_used_29)
(assert
 chronological_used_30)
(assert
 chronological_used_31)
(assert
 chronological_used_32)
(assert
 chronological_used_33)
(assert
 chronological_used_34)
(assert
 chronological_used_35)
(assert
 chronological_used_36)
(assert
 chronological_used_37)
(assert
 chronological_used_38)
(assert
 chronological_used_39)
(assert
 chronological_used_40)
(assert
 chronological_used_41)
(assert
 chronological_used_42)
(assert
 chronological_used_43)
(assert
 chronological_used_44)
(assert
 chronological_used_45)
(assert
 chronological_used_46)
(assert
 chronological_used_47)
(assert
 chronological_used_48)
(assert
 chronological_used_49)
(assert
 chronological_used_50)
(assert
 chronological_used_51)
(assert
 chronological_used_52)
(assert
 chronological_used_53)
(assert
 chronological_used_54)
(assert
 chronological_used_55)
(assert
 chronological_used_56)
(assert
 chronological_used_57)
(assert
 chronological_used_58)
(assert
 chronological_used_59)
(assert
 chronological_used_60)
(assert
 chronological_used_61)
(assert
 chronological_used_62)
(assert
 chronological_used_63)
(assert
 chronological_used_64)
(assert
 chronological_used_65)
(assert
 chronological_used_66)
(assert
 chronological_used_67)
(assert
 chronological_used_68)
(assert
 chronological_used_69)
(assert
 chronological_used_70)
(assert
 chronological_used_71)
(assert
 chronological_used_72)
(assert
 chronological_used_73)
(assert
 chronological_used_74)
(assert
 chronological_used_75)
(assert
 chronological_used_76)
(assert
 chronological_used_77)
(assert
 chronological_used_78)
(assert
 chronological_used_79)
(assert
 chronological_used_80)
(assert
 chronological_used_81)
(assert
 chronological_used_82)
(assert
 chronological_used_83)
(assert
 chronological_used_84)
(assert
 chronological_used_85)
(assert
 chronological_used_86)
(assert
 chronological_used_87)
(assert
 chronological_used_88)
(assert
 chronological_used_89)
(assert
 chronological_used_90)
(assert
 chronological_used_91)
(assert
 chronological_used_92)
(assert
 chronological_used_93)
(assert
 chronological_used_94)
(assert
 chronological_used_95)
(assert
 chronological_used_96)
(assert
 chronological_used_97)
(assert
 chronological_used_98)
(assert
 chronological_used_99)
(assert
 chronological_used_100)
(assert
 chronological_used_101)
(assert
 chronological_used_102)
(assert
 chronological_used_103)
(assert
 chronological_used_104)
(assert
 chronological_used_105)
(assert
 chronological_used_106)
(assert
 chronological_used_107)
(assert
 chronological_used_108)
(assert
 chronological_used_109)
(assert
 chronological_used_110)
(assert
 chronological_used_111)
(assert
 chronological_used_112)
(assert
 chronological_used_113)
(assert
 chronological_used_114)
(assert
 chronological_used_115)
(assert
 chronological_used_116)
(assert
 chronological_used_117)
(assert
 chronological_used_118)
(assert
 chronological_used_119)
(assert
 chronological_used_120)
(assert
 chronological_used_121)
(assert
 chronological_used_122)
(assert
 chronological_used_123)
(assert
 chronological_used_124)
(assert
 chronological_used_125)
(assert
 chronological_used_126)
(assert
 chronological_used_127)
(assert
 chronological_used_128)
(assert
 chronological_used_129)
(assert
 chronological_used_130)
(assert
 chronological_used_131)
(assert
 chronological_used_132)
(assert
 chronological_used_133)
(assert
 chronological_used_134)
(assert
 chronological_used_135)
(assert
 chronological_used_136)
(assert
 chronological_used_137)
(assert
 chronological_used_138)
(assert
 chronological_used_139)
(assert
 chronological_used_140)
(assert
 chronological_used_141)
(assert
 chronological_used_142)
(assert
 chronological_used_143)
(assert
 chronological_used_144)
(assert
 chronological_used_145)
(assert
 chronological_used_146)
(assert
 chronological_used_147)
(assert
 chronological_used_148)
(assert
 chronological_used_149)
(assert
 chronological_used_150)
(assert
 chronological_used_151)
(assert
 chronological_used_152)
(assert
 chronological_used_153)
(assert
 chronological_used_154)
(assert
 chronological_used_155)
(assert
 chronological_used_156)
(assert
 chronological_used_157)
(assert
 chronological_used_158)
(assert
 chronological_used_159)
(assert
 chronological_used_160)
(assert
 chronological_used_161)
(assert
 chronological_used_162)
(assert
 chronological_used_163)
(assert
 chronological_used_164)
(assert
 chronological_used_165)
(assert
 chronological_used_166)
(assert
 chronological_used_167)
(assert
 chronological_used_168)
(assert
 chronological_used_169)
(assert
 chronological_used_170)
(assert
 chronological_used_171)
(assert
 chronological_used_172)
(assert
 chronological_used_173)
(assert
 chronological_used_174)
(assert
 chronological_used_175)
(assert
 chronological_used_176)
(assert
 chronological_used_177)
(assert
 chronological_used_178)
(assert
 chronological_used_179)
(assert
 chronological_used_180)
(assert
 chronological_used_181)
(assert
 chronological_used_182)
(assert
 chronological_used_183)
(assert
 chronological_used_184)
(assert
 chronological_used_185)
(assert
 chronological_used_186)
(assert
 chronological_used_187)
(assert
 chronological_used_188)
(assert
 chronological_used_189)
(assert
 chronological_used_190)
(assert
 chronological_used_191)
(assert
 chronological_used_192)
(assert
 chronological_used_193)
(assert
 chronological_used_194)
(assert
 chronological_used_195)
(assert
 chronological_used_196)
(assert
 chronological_used_197)
(assert
 chronological_used_198)
(assert
 chronological_used_199)
(assert
 chronological_used_200)
(assert
 chronological_used_201)
(assert
 chronological_used_202)
(assert
 chronological_used_203)
(assert
 chronological_used_204)
(assert
 chronological_used_205)
(assert
 chronological_used_206)
(assert
 chronological_used_207)
(assert
 chronological_used_208)
(assert
 chronological_used_209)
(assert
 chronological_used_210)
(assert
 chronological_used_211)
(assert
 chronological_used_212)
(assert
 chronological_used_213)
(assert
 chronological_used_214)
(assert
 chronological_used_215)
(assert
 chronological_used_216)
(assert
 chronological_used_217)
(assert
 chronological_used_218)
(assert
 chronological_used_219)
(assert
 chronological_used_220)
(assert
 chronological_used_221)
(assert
 chronological_used_222)
(assert
 chronological_used_223)
(assert
 chronological_used_224)
(assert
 chronological_used_225)
(assert
 chronological_used_226)
(assert
 chronological_used_227)
(assert
 chronological_used_228)
(assert
 chronological_used_229)
(assert
 chronological_used_230)
(assert
 chronological_used_231)
(assert
 chronological_used_232)
(assert
 chronological_used_233)
(assert
 chronological_used_234)
(assert
 chronological_used_235)
(assert
 chronological_used_236)
(assert
 chronological_used_237)
(assert
 chronological_used_238)
(assert
 chronological_used_239)
(assert
 chronological_used_240)
(assert
 chronological_used_241)
(assert
 chronological_used_242)
(assert
 chronological_used_243)
(assert
 chronological_used_244)
(assert
 chronological_used_245)
(assert
 chronological_used_246)
(assert
 chronological_used_247)
(assert
 chronological_used_248)
(assert
 chronological_used_249)
(assert
 chronological_used_250)
(assert
 chronological_used_251)
(assert
 chronological_used_252)
(assert
 chronological_used_253)
(assert
 chronological_used_254)
(assert
 (not chronological_used_255))
(assert
 (let ((?x648 (ite chronological_used_255 256 255)))
(let ((?x4435 (ite chronological_used_254 ?x648 254)))
(let ((?x710 (ite chronological_used_253 ?x4435 253)))
(let ((?x2680 (ite chronological_used_252 ?x710 252)))
(let ((?x187 (ite chronological_used_251 ?x2680 251)))
(let ((?x660 (ite chronological_used_250 ?x187 250)))
(let ((?x3898 (ite chronological_used_249 ?x660 249)))
(let ((?x748 (ite chronological_used_248 ?x3898 248)))
(let ((?x3368 (ite chronological_used_247 ?x748 247)))
(let ((?x4843 (ite chronological_used_246 ?x3368 246)))
(let ((?x2990 (ite chronological_used_245 ?x4843 245)))
(let ((?x712 (ite chronological_used_244 ?x2990 244)))
(let ((?x716 (ite chronological_used_243 ?x712 243)))
(let ((?x678 (ite chronological_used_242 ?x716 242)))
(let ((?x3243 (ite chronological_used_241 ?x678 241)))
(let ((?x650 (ite chronological_used_240 ?x3243 240)))
(let ((?x5096 (ite chronological_used_239 ?x650 239)))
(let ((?x5971 (ite chronological_used_238 ?x5096 238)))
(let ((?x682 (ite chronological_used_237 ?x5971 237)))
(let ((?x652 (ite chronological_used_236 ?x682 236)))
(let ((?x2364 (ite chronological_used_235 ?x652 235)))
(let ((?x684 (ite chronological_used_234 ?x2364 234)))
(let ((?x636 (ite chronological_used_233 ?x684 233)))
(let ((?x632 (ite chronological_used_232 ?x636 232)))
(let ((?x204 (ite chronological_used_231 ?x632 231)))
(let ((?x5992 (ite chronological_used_230 ?x204 230)))
(let ((?x656 (ite chronological_used_229 ?x5992 229)))
(let ((?x4818 (ite chronological_used_228 ?x656 228)))
(let ((?x1922 (ite chronological_used_227 ?x4818 227)))
(let ((?x192 (ite chronological_used_226 ?x1922 226)))
(let ((?x754 (ite chronological_used_225 ?x192 225)))
(let ((?x2967 (ite chronological_used_224 ?x754 224)))
(let ((?x2902 (ite chronological_used_223 ?x2967 223)))
(let ((?x744 (ite chronological_used_222 ?x2902 222)))
(let ((?x742 (ite chronological_used_221 ?x744 221)))
(let ((?x5139 (ite chronological_used_220 ?x742 220)))
(let ((?x5565 (ite chronological_used_219 ?x5139 219)))
(let ((?x750 (ite chronological_used_218 ?x5565 218)))
(let ((?x5040 (ite chronological_used_217 ?x750 217)))
(let ((?x756 (ite chronological_used_216 ?x5040 216)))
(let ((?x758 (ite chronological_used_215 ?x756 215)))
(let ((?x760 (ite chronological_used_214 ?x758 214)))
(let ((?x762 (ite chronological_used_213 ?x760 213)))
(let ((?x764 (ite chronological_used_212 ?x762 212)))
(let ((?x766 (ite chronological_used_211 ?x764 211)))
(let ((?x768 (ite chronological_used_210 ?x766 210)))
(let ((?x770 (ite chronological_used_209 ?x768 209)))
(let ((?x772 (ite chronological_used_208 ?x770 208)))
(let ((?x774 (ite chronological_used_207 ?x772 207)))
(let ((?x776 (ite chronological_used_206 ?x774 206)))
(let ((?x778 (ite chronological_used_205 ?x776 205)))
(let ((?x780 (ite chronological_used_204 ?x778 204)))
(let ((?x782 (ite chronological_used_203 ?x780 203)))
(let ((?x784 (ite chronological_used_202 ?x782 202)))
(let ((?x786 (ite chronological_used_201 ?x784 201)))
(let ((?x788 (ite chronological_used_200 ?x786 200)))
(let ((?x790 (ite chronological_used_199 ?x788 199)))
(let ((?x792 (ite chronological_used_198 ?x790 198)))
(let ((?x794 (ite chronological_used_197 ?x792 197)))
(let ((?x796 (ite chronological_used_196 ?x794 196)))
(let ((?x798 (ite chronological_used_195 ?x796 195)))
(let ((?x800 (ite chronological_used_194 ?x798 194)))
(let ((?x802 (ite chronological_used_193 ?x800 193)))
(let ((?x804 (ite chronological_used_192 ?x802 192)))
(let ((?x806 (ite chronological_used_191 ?x804 191)))
(let ((?x808 (ite chronological_used_190 ?x806 190)))
(let ((?x810 (ite chronological_used_189 ?x808 189)))
(let ((?x812 (ite chronological_used_188 ?x810 188)))
(let ((?x814 (ite chronological_used_187 ?x812 187)))
(let ((?x816 (ite chronological_used_186 ?x814 186)))
(let ((?x818 (ite chronological_used_185 ?x816 185)))
(let ((?x820 (ite chronological_used_184 ?x818 184)))
(let ((?x822 (ite chronological_used_183 ?x820 183)))
(let ((?x824 (ite chronological_used_182 ?x822 182)))
(let ((?x826 (ite chronological_used_181 ?x824 181)))
(let ((?x2774 (ite chronological_used_180 ?x826 180)))
(let ((?x5498 (ite chronological_used_179 ?x2774 179)))
(let ((?x2796 (ite chronological_used_178 ?x5498 178)))
(let ((?x2081 (ite chronological_used_177 ?x2796 177)))
(let ((?x5572 (ite chronological_used_176 ?x2081 176)))
(let ((?x3311 (ite chronological_used_175 ?x5572 175)))
(let ((?x4526 (ite chronological_used_174 ?x3311 174)))
(let ((?x3468 (ite chronological_used_173 ?x4526 173)))
(let ((?x347 (ite chronological_used_172 ?x3468 172)))
(let ((?x348 (ite chronological_used_171 ?x347 171)))
(let ((?x351 (ite chronological_used_170 ?x348 170)))
(let ((?x355 (ite chronological_used_169 ?x351 169)))
(let ((?x359 (ite chronological_used_168 ?x355 168)))
(let ((?x363 (ite chronological_used_167 ?x359 167)))
(let ((?x367 (ite chronological_used_166 ?x363 166)))
(let ((?x371 (ite chronological_used_165 ?x367 165)))
(let ((?x375 (ite chronological_used_164 ?x371 164)))
(let ((?x379 (ite chronological_used_163 ?x375 163)))
(let ((?x383 (ite chronological_used_162 ?x379 162)))
(let ((?x387 (ite chronological_used_161 ?x383 161)))
(let ((?x391 (ite chronological_used_160 ?x387 160)))
(let ((?x395 (ite chronological_used_159 ?x391 159)))
(let ((?x399 (ite chronological_used_158 ?x395 158)))
(let ((?x403 (ite chronological_used_157 ?x399 157)))
(let ((?x407 (ite chronological_used_156 ?x403 156)))
(let ((?x411 (ite chronological_used_155 ?x407 155)))
(let ((?x415 (ite chronological_used_154 ?x411 154)))
(let ((?x419 (ite chronological_used_153 ?x415 153)))
(let ((?x423 (ite chronological_used_152 ?x419 152)))
(let ((?x427 (ite chronological_used_151 ?x423 151)))
(let ((?x431 (ite chronological_used_150 ?x427 150)))
(let ((?x435 (ite chronological_used_149 ?x431 149)))
(let ((?x439 (ite chronological_used_148 ?x435 148)))
(let ((?x443 (ite chronological_used_147 ?x439 147)))
(let ((?x447 (ite chronological_used_146 ?x443 146)))
(let ((?x451 (ite chronological_used_145 ?x447 145)))
(let ((?x455 (ite chronological_used_144 ?x451 144)))
(let ((?x459 (ite chronological_used_143 ?x455 143)))
(let ((?x463 (ite chronological_used_142 ?x459 142)))
(let ((?x467 (ite chronological_used_141 ?x463 141)))
(let ((?x471 (ite chronological_used_140 ?x467 140)))
(let ((?x475 (ite chronological_used_139 ?x471 139)))
(let ((?x479 (ite chronological_used_138 ?x475 138)))
(let ((?x483 (ite chronological_used_137 ?x479 137)))
(let ((?x487 (ite chronological_used_136 ?x483 136)))
(let ((?x491 (ite chronological_used_135 ?x487 135)))
(let ((?x495 (ite chronological_used_134 ?x491 134)))
(let ((?x499 (ite chronological_used_133 ?x495 133)))
(let ((?x503 (ite chronological_used_132 ?x499 132)))
(let ((?x507 (ite chronological_used_131 ?x503 131)))
(let ((?x511 (ite chronological_used_130 ?x507 130)))
(let ((?x515 (ite chronological_used_129 ?x511 129)))
(let ((?x519 (ite chronological_used_128 ?x515 128)))
(let ((?x523 (ite chronological_used_127 ?x519 127)))
(let ((?x527 (ite chronological_used_126 ?x523 126)))
(let ((?x531 (ite chronological_used_125 ?x527 125)))
(let ((?x535 (ite chronological_used_124 ?x531 124)))
(let ((?x539 (ite chronological_used_123 ?x535 123)))
(let ((?x543 (ite chronological_used_122 ?x539 122)))
(let ((?x547 (ite chronological_used_121 ?x543 121)))
(let ((?x551 (ite chronological_used_120 ?x547 120)))
(let ((?x555 (ite chronological_used_119 ?x551 119)))
(let ((?x559 (ite chronological_used_118 ?x555 118)))
(let ((?x563 (ite chronological_used_117 ?x559 117)))
(let ((?x567 (ite chronological_used_116 ?x563 116)))
(let ((?x571 (ite chronological_used_115 ?x567 115)))
(let ((?x575 (ite chronological_used_114 ?x571 114)))
(let ((?x579 (ite chronological_used_113 ?x575 113)))
(let ((?x583 (ite chronological_used_112 ?x579 112)))
(let ((?x587 (ite chronological_used_111 ?x583 111)))
(let ((?x591 (ite chronological_used_110 ?x587 110)))
(let ((?x595 (ite chronological_used_109 ?x591 109)))
(let ((?x599 (ite chronological_used_108 ?x595 108)))
(let ((?x603 (ite chronological_used_107 ?x599 107)))
(let ((?x607 (ite chronological_used_106 ?x603 106)))
(let ((?x611 (ite chronological_used_105 ?x607 105)))
(let ((?x615 (ite chronological_used_104 ?x611 104)))
(let ((?x619 (ite chronological_used_103 ?x615 103)))
(let ((?x623 (ite chronological_used_102 ?x619 102)))
(let ((?x627 (ite chronological_used_101 ?x623 101)))
(let ((?x631 (ite chronological_used_100 ?x627 100)))
(let ((?x635 (ite chronological_used_99 ?x631 99)))
(let ((?x639 (ite chronological_used_98 ?x635 98)))
(let ((?x643 (ite chronological_used_97 ?x639 97)))
(let ((?x647 (ite chronological_used_96 ?x643 96)))
(let ((?x651 (ite chronological_used_95 ?x647 95)))
(let ((?x655 (ite chronological_used_94 ?x651 94)))
(let ((?x659 (ite chronological_used_93 ?x655 93)))
(let ((?x663 (ite chronological_used_92 ?x659 92)))
(let ((?x667 (ite chronological_used_91 ?x663 91)))
(let ((?x671 (ite chronological_used_90 ?x667 90)))
(let ((?x675 (ite chronological_used_89 ?x671 89)))
(let ((?x679 (ite chronological_used_88 ?x675 88)))
(let ((?x683 (ite chronological_used_87 ?x679 87)))
(let ((?x687 (ite chronological_used_86 ?x683 86)))
(let ((?x691 (ite chronological_used_85 ?x687 85)))
(let ((?x695 (ite chronological_used_84 ?x691 84)))
(let ((?x699 (ite chronological_used_83 ?x695 83)))
(let ((?x703 (ite chronological_used_82 ?x699 82)))
(let ((?x707 (ite chronological_used_81 ?x703 81)))
(let ((?x711 (ite chronological_used_80 ?x707 80)))
(let ((?x715 (ite chronological_used_79 ?x711 79)))
(let ((?x719 (ite chronological_used_78 ?x715 78)))
(let ((?x723 (ite chronological_used_77 ?x719 77)))
(let ((?x727 (ite chronological_used_76 ?x723 76)))
(let ((?x731 (ite chronological_used_75 ?x727 75)))
(let ((?x735 (ite chronological_used_74 ?x731 74)))
(let ((?x739 (ite chronological_used_73 ?x735 73)))
(let ((?x743 (ite chronological_used_72 ?x739 72)))
(let ((?x747 (ite chronological_used_71 ?x743 71)))
(let ((?x751 (ite chronological_used_70 ?x747 70)))
(let ((?x755 (ite chronological_used_69 ?x751 69)))
(let ((?x759 (ite chronological_used_68 ?x755 68)))
(let ((?x763 (ite chronological_used_67 ?x759 67)))
(let ((?x767 (ite chronological_used_66 ?x763 66)))
(let ((?x771 (ite chronological_used_65 ?x767 65)))
(let ((?x775 (ite chronological_used_64 ?x771 64)))
(let ((?x779 (ite chronological_used_63 ?x775 63)))
(let ((?x783 (ite chronological_used_62 ?x779 62)))
(let ((?x787 (ite chronological_used_61 ?x783 61)))
(let ((?x791 (ite chronological_used_60 ?x787 60)))
(let ((?x795 (ite chronological_used_59 ?x791 59)))
(let ((?x799 (ite chronological_used_58 ?x795 58)))
(let ((?x803 (ite chronological_used_57 ?x799 57)))
(let ((?x807 (ite chronological_used_56 ?x803 56)))
(let ((?x811 (ite chronological_used_55 ?x807 55)))
(let ((?x815 (ite chronological_used_54 ?x811 54)))
(let ((?x819 (ite chronological_used_53 ?x815 53)))
(let ((?x823 (ite chronological_used_52 ?x819 52)))
(let ((?x827 (ite chronological_used_51 ?x823 51)))
(let ((?x831 (ite chronological_used_50 ?x827 50)))
(let ((?x835 (ite chronological_used_49 ?x831 49)))
(let ((?x839 (ite chronological_used_48 ?x835 48)))
(let ((?x842 (ite chronological_used_47 ?x839 47)))
(let ((?x846 (ite chronological_used_46 ?x842 46)))
(let ((?x848 (ite chronological_used_45 ?x846 45)))
(let ((?x850 (ite chronological_used_44 ?x848 44)))
(let ((?x852 (ite chronological_used_43 ?x850 43)))
(let ((?x8 (ite chronological_used_42 ?x852 42)))
(let ((?x48 (ite chronological_used_41 ?x8 41)))
(let ((?x121 (ite chronological_used_40 ?x48 40)))
(let ((?x89 (ite chronological_used_39 ?x121 39)))
(let ((?x98 (ite chronological_used_38 ?x89 38)))
(let ((?x44 (ite chronological_used_37 ?x98 37)))
(let ((?x96 (ite chronological_used_36 ?x44 36)))
(let ((?x114 (ite chronological_used_35 ?x96 35)))
(let ((?x78 (ite chronological_used_34 ?x114 34)))
(let ((?x82 (ite chronological_used_33 ?x78 33)))
(let ((?x103 (ite chronological_used_32 ?x82 32)))
(let ((?x57 (ite chronological_used_31 ?x103 31)))
(let ((?x135 (ite chronological_used_30 ?x57 30)))
(let ((?x129 (ite chronological_used_29 ?x135 29)))
(let ((?x100 (ite chronological_used_28 ?x129 28)))
(let ((?x59 (ite chronological_used_27 ?x100 27)))
(let ((?x58 (ite chronological_used_26 ?x59 26)))
(let ((?x15 (ite chronological_used_25 ?x58 25)))
(let ((?x32 (ite chronological_used_24 ?x15 24)))
(let ((?x34 (ite chronological_used_23 ?x32 23)))
(let ((?x19 (ite chronological_used_22 ?x34 22)))
(let ((?x20 (ite chronological_used_21 ?x19 21)))
(let ((?x21 (ite chronological_used_20 ?x20 20)))
(let ((?x10 (ite chronological_used_19 ?x21 19)))
(let ((?x23 (ite chronological_used_18 ?x10 18)))
(let ((?x12 (ite chronological_used_17 ?x23 17)))
(let ((?x13 (ite chronological_used_16 ?x12 16)))
(let ((?x14 (ite chronological_used_15 ?x13 15)))
(let ((?x52 (ite chronological_used_14 ?x14 14)))
(let ((?x99 (ite chronological_used_13 ?x52 13)))
(let ((?x17 (ite chronological_used_12 ?x99 12)))
(let ((?x101 (ite chronological_used_11 ?x17 11)))
(let ((?x95 (ite chronological_used_10 ?x101 10)))
(let ((?x24 (ite chronological_used_9 ?x95 9)))
(let ((?x25 (ite chronological_used_8 ?x24 8)))
(let ((?x26 (ite chronological_used_7 ?x25 7)))
(let ((?x27 (ite chronological_used_6 ?x26 6)))
(let ((?x28 (ite chronological_used_5 ?x27 5)))
(let ((?x74 (ite chronological_used_4 ?x28 4)))
(let ((?x90 (ite chronological_used_3 ?x74 3)))
(let ((?x54 (ite chronological_used_2 ?x90 2)))
(let ((?x97 (ite chronological_used_1 ?x54 1)))
(let ((?x50 (ite chronological_used_0 ?x97 0)))
(= ?x50 255))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
; EXPECT: sat
(check-sat)
; Query 23: participating_nonself_role_retains_exhausted_row
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun sender () Int)
(declare-fun receiver () Int)
(declare-fun role () Int)
(declare-fun schema () Int)
(assert
 (>= sender 0))
(assert
 (>= receiver 0))
(assert
 (>= role 0))
(assert
 (>= schema 0))
(assert
 (and (distinct sender receiver) true))
(assert
 (let (($x972 (= role receiver)))
 (let (($x6026 (= role sender)))
 (or $x6026 $x972))))
(assert
 (let (($x4632 (= schema 0)))
(let (($x972 (= role receiver)))
(let (($x6026 (= role sender)))
(let (($x1060 (and $x6026 $x972 $x4632)))
(let (($x6174 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))
(not $x6174)))))))
; EXPECT: unsat
(check-sat)
; Query 24: retained_invalid_row_rejects_exact_byte_equality
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun sender () Int)
(declare-fun receiver () Int)
(declare-fun role () Int)
(declare-fun schema () Int)
(declare-fun decoded () Int)
(assert
 (>= sender 0))
(assert
 (>= receiver 0))
(assert
 (>= role 0))
(assert
 (>= schema 0))
(assert
 (>= decoded 0))
(assert
 (< decoded 256))
(assert
 (let (($x972 (= role receiver)))
 (let (($x6026 (= role sender)))
 (let (($x1060 (and $x6026 $x972 (= schema 0))))
 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))))
(assert
 (let (($x1751 (= decoded 256)))
(let (($x972 (= role receiver)))
(let (($x6026 (= role sender)))
(let (($x1060 (and $x6026 $x972 (= schema 0))))
(let (($x6174 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))
(=> $x6174 $x1751)))))))
; EXPECT: unsat
(check-sat)
; Query 25: unrelated_role_omits_invalid_row_negative_control
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun sender () Int)
(declare-fun receiver () Int)
(declare-fun role () Int)
(declare-fun schema () Int)
(declare-fun decoded () Int)
(assert
 (>= sender 0))
(assert
 (>= receiver 0))
(assert
 (>= role 0))
(assert
 (>= schema 0))
(assert
 (>= decoded 0))
(assert
 (< decoded 256))
(assert
 (and (distinct role sender) true))
(assert
 (and (distinct role receiver) true))
(assert
 (let (($x1751 (= decoded 256)))
(let (($x972 (= role receiver)))
(let (($x6026 (= role sender)))
(let (($x1060 (and $x6026 $x972 (= schema 0))))
(let (($x6174 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))
(=> $x6174 $x1751)))))))
; EXPECT: sat
(check-sat)
; Query 26: remove_decoder_byte_bound_negative_control
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun sender () Int)
(declare-fun receiver () Int)
(declare-fun role () Int)
(declare-fun schema () Int)
(declare-fun decoded () Int)
(assert
 (>= sender 0))
(assert
 (>= receiver 0))
(assert
 (>= role 0))
(assert
 (>= schema 0))
(assert
 (let (($x4632 (= schema 0)))
 (let (($x972 (= role receiver)))
 (let (($x6026 (= role sender)))
 (let (($x1060 (and $x6026 $x972 $x4632)))
 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060))))))
(assert
 (let (($x1751 (= decoded 256)))
(let (($x4632 (= schema 0)))
(let (($x972 (= role receiver)))
(let (($x6026 (= role sender)))
(let (($x1060 (and $x6026 $x972 $x4632)))
(let (($x6174 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))
(=> $x6174 $x1751))))))))
; EXPECT: sat
(check-sat)
; Query 27: successful_lowering_retained_byte_nonvacuity
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun sender () Int)
(declare-fun receiver () Int)
(declare-fun role () Int)
(declare-fun schema () Int)
(declare-fun decoded () Int)
(assert
 (>= sender 0))
(assert
 (>= receiver 0))
(assert
 (>= role 0))
(assert
 (>= schema 0))
(assert
 (>= decoded 0))
(assert
 (< decoded 256))
(assert
 (and (distinct sender receiver) true))
(assert
 (= role sender))
(assert
 (= decoded 255))
; EXPECT: sat
(check-sat)
; Query 28: successful_lowering_excludes_any_global_overflow
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun row_count () Int)
(declare-fun overflow_index () Int)
(declare-fun total_reference_colors () (Array Int Int))
(assert
 (> row_count 0))
(assert
 (>= overflow_index 0))
(assert
 (< overflow_index row_count))
(assert
 (forall ((k Int) )(let ((?x7872 (select total_reference_colors k)))
 (let (($x886 (>= ?x7872 0)))
 (=> (and (>= k 0) (< k row_count)) (and $x886 (< ?x7872 256))))))
 )
(assert
 (= (select total_reference_colors overflow_index) 256))
; EXPECT: unsat
(check-sat)
; Query 29: role_local_empty_projection_does_not_imply_global_success
(reset)
; Standalone SMT-LIB query.
(set-info :status unknown)
(declare-fun row_count () Int)
(declare-fun total_reference_colors () (Array Int Int))
(declare-fun sender () Int)
(declare-fun role () Int)
(declare-fun receiver () Int)
(declare-fun schema () Int)
(assert
 (= row_count 1))
(assert
 (= (select total_reference_colors 0) 256))
(assert
 (and (distinct role sender) true))
(assert
 (and (distinct role receiver) true))
(assert
 (let (($x972 (= role receiver)))
 (let (($x6026 (= role sender)))
 (let (($x1060 (and $x6026 $x972 (= schema 0))))
 (let (($x6174 (or (and $x6026 (and (distinct role receiver) true)) (and $x972 (and (distinct role sender) true)) $x1060)))
 (not $x6174))))))
(assert
 (let (($x9523 (forall ((k Int) )(let ((?x7872 (select total_reference_colors k)))
(let (($x886 (>= ?x7872 0)))
(=> (and (>= k 0) (< k row_count)) (and $x886 (< ?x7872 256))))))
))
(not $x9523)))
; EXPECT: sat
(check-sat)
