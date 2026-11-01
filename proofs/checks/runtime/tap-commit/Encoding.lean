import Std

namespace Hibana.TapCommit

def context (view lane : Fin 256) (source : Fin 65536) : Nat :=
  view.val * 16777216 + lane.val * 65536 + source.val

theorem context_fits (view lane : Fin 256) (source : Fin 65536) :
    context view lane source < 4294967296 := by
  have := view.isLt
  have := lane.isLt
  have := source.isLt
  simp only [context]
  omega

theorem source_exact (view lane : Fin 256) (source : Fin 65536) :
    context view lane source % 65536 = source.val := by
  have := source.isLt
  simp only [context]
  omega

theorem lane_exact (view lane : Fin 256) (source : Fin 65536) :
    context view lane source / 65536 % 256 = lane.val := by
  have := lane.isLt
  have := source.isLt
  simp only [context]
  omega

theorem view_exact (view lane : Fin 256) (source : Fin 65536) :
    context view lane source / 16777216 = view.val := by
  have := lane.isLt
  have := source.isLt
  simp only [context]
  omega

theorem context_injective (v l v' l' : Fin 256) (s s' : Fin 65536)
    (same : context v l s = context v' l' s') :
    v = v' ∧ l = l' ∧ s = s' := by
  have hv := congrArg (fun n => n / 16777216) same
  have hl := congrArg (fun n => n / 65536 % 256) same
  have hs := congrArg (fun n => n % 65536) same
  dsimp only at hv hl hs
  rw [view_exact, view_exact] at hv
  rw [lane_exact, lane_exact] at hl
  rw [source_exact, source_exact] at hs
  exact ⟨Fin.ext hv, Fin.ext hl, Fin.ext hs⟩

def causal (actor direction : Fin 256) : Nat :=
  actor.val * 256 + direction.val

theorem causal_fits (actor direction : Fin 256) :
    causal actor direction < 65536 := by
  have := actor.isLt
  have := direction.isLt
  simp only [causal]
  omega

theorem actor_exact (actor direction : Fin 256) :
    causal actor direction / 256 = actor.val := by
  have := direction.isLt
  simp only [causal]
  omega

theorem direction_exact (actor direction : Fin 256) :
    causal actor direction % 256 = direction.val := by
  have := direction.isLt
  simp only [causal]
  omega

theorem causal_injective (a d a' d' : Fin 256)
    (same : causal a d = causal a' d') : a = a' ∧ d = d' := by
  have ha := congrArg (fun n => n / 256) same
  have hd := congrArg (fun n => n % 256) same
  dsimp only at ha hd
  rw [actor_exact, actor_exact] at ha
  rw [direction_exact, direction_exact] at hd
  exact ⟨Fin.ext ha, Fin.ext hd⟩

#print axioms context_fits
#print axioms source_exact
#print axioms lane_exact
#print axioms view_exact
#print axioms context_injective
#print axioms causal_fits
#print axioms actor_exact
#print axioms direction_exact
#print axioms causal_injective

def reentry (scope detail : Fin 65536) : Nat :=
  scope.val * 65536 + detail.val

theorem reentry_fits (scope detail : Fin 65536) :
    reentry scope detail < 4294967296 := by
  have := scope.isLt
  have := detail.isLt
  simp only [reentry]
  omega

theorem reentry_scope_exact (scope detail : Fin 65536) :
    reentry scope detail / 65536 = scope.val := by
  have := detail.isLt
  simp only [reentry]
  omega

theorem reentry_detail_exact (scope detail : Fin 65536) :
    reentry scope detail % 65536 = detail.val := by
  have := detail.isLt
  simp only [reentry]
  omega

theorem reentry_injective (s d s' d' : Fin 65536)
    (same : reentry s d = reentry s' d') : s = s' ∧ d = d' := by
  have hs := congrArg (fun n => n / 65536) same
  have hd := congrArg (fun n => n % 65536) same
  dsimp only at hs hd
  rw [reentry_scope_exact, reentry_scope_exact] at hs
  rw [reentry_detail_exact, reentry_detail_exact] at hd
  exact ⟨Fin.ext hs, Fin.ext hd⟩

#print axioms reentry_fits
#print axioms reentry_scope_exact
#print axioms reentry_detail_exact
#print axioms reentry_injective

end Hibana.TapCommit
