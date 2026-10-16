; Relational ownership/order obligations for the projection reference model.
; These are model checks, not a proof of an owned-message runtime or wire ABI.
(set-logic QF_AUFLIA)
(declare-const owners (Array Int Int))
(declare-const resource Int)
(declare-const other Int)
(declare-const provider Int)
(declare-const alice Int)
(declare-const bob Int)

; Inferred issuance cannot satisfy two distinct concrete roots.
(push)
(assert (distinct provider alice))
(assert (= (select owners resource) provider))
(assert (= (select owners resource) alice))
; EXPECT: unsat
(check-sat)
(pop)

; Distinct resources may move independently: both legal linearizations agree.
(push)
(assert (distinct resource other))
(assert (distinct
  (store (store owners resource alice) other bob)
  (store (store owners other bob) resource alice)))
; EXPECT: unsat
(check-sat)
(pop)

; A handoff changes exactly one ownership binding.
(push)
(assert (distinct resource other))
(assert (distinct (select (store owners resource alice) other) (select owners other)))
; EXPECT: unsat
(check-sat)
(pop)

; After a successful P -> A move, P cannot move the same value again.
(push)
(assert (distinct provider alice))
(assert (= (select owners resource) provider))
(assert (= (select (store owners resource alice) resource) provider))
; EXPECT: unsat
(check-sat)
(pop)

; Historical incorrect parallel rule: both grants read the same old owner.
; A non-Clone Rust value alone does not repair that global ownership rule.
(push)
(assert (distinct provider alice bob))
(assert (= (select owners resource) provider))
(assert (= (select owners resource) provider))
(assert (distinct
  (select (store owners resource alice) resource)
  (select (store owners resource bob) resource)))
; EXPECT: sat
(check-sat)
(pop)

; A route may leave an unused endpoint in different hands. For any later
; carrier other than that endpoint, the branch context gives the same actor.
(push)
(assert (distinct resource other))
(assert (distinct
  (select (store owners resource alice) other)
  (select (store owners resource bob) other)))
; EXPECT: unsat
(check-sat)
(pop)

; A rolled next visit that uses the resource makes its join live even when
; there are no ordinary messages after the route in the current visit.
(declare-const ordinary_suffix_uses Bool)
(declare-const next_visit_uses Bool)
(define-fun live_join () Bool (or ordinary_suffix_uses next_visit_uses))
(push)
(assert (not ordinary_suffix_uses))
(assert next_visit_uses)
(assert (distinct alice bob))
(assert (=> live_join (= alice bob)))
; EXPECT: unsat
(check-sat)
(pop)

; Missing cross-facet actor order lets the holder wait on its control
; endpoint before doing the resource work needed to produce that response.
(declare-const resource_work_done Bool)
(declare-const control_wait_entered Bool)
(push)
(assert control_wait_entered)
(assert (not resource_work_done))
; EXPECT: sat
(check-sat)
(pop)

; Owner-aware sequential projection gates that wait on the preceding work.
(push)
(assert (=> control_wait_entered resource_work_done))
(assert control_wait_entered)
(assert (not resource_work_done))
; EXPECT: unsat
(check-sat)
(pop)
