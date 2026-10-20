/- Pure common-subexpression reuse with arbitrary errors, dependency state and bytes.
   No assumption that the first scan, row construction, dependency stage, or
   final serialization succeeds. The source and event index do not change. -/
namespace ConflictReuse

def original {S I C A O E : Type}
    (scan : S → I → Except E C) (source : S) (index : I)
    (hasRoute : Bool) (none : C)
    (rowThenDependencies : C → Except E A)
    (serialize : A → C → Except E O) : Except E O :=
  Except.bind (scan source index) fun first =>
  Except.bind (rowThenDependencies first) fun state =>
  Except.bind (if hasRoute then scan source index else Except.ok none) fun second =>
  serialize state second

def reused {S I C A O E : Type}
    (scan : S → I → Except E C) (source : S) (index : I)
    (hasRoute : Bool) (none : C)
    (rowThenDependencies : C → Except E A)
    (serialize : A → C → Except E O) : Except E O :=
  Except.bind (scan source index) fun first =>
  Except.bind (rowThenDependencies first) fun state =>
  serialize state (if hasRoute then first else none)

theorem reuse_exact {S I C A O E : Type}
    (scan : S → I → Except E C) (source : S) (index : I)
    (hasRoute : Bool) (none : C)
    (rowThenDependencies : C → Except E A)
    (serialize : A → C → Except E O) :
    original scan source index hasRoute none rowThenDependencies serialize =
    reused scan source index hasRoute none rowThenDependencies serialize := by
  cases hscan : scan source index with
  | error error => simp [original, reused, hscan, Except.bind]
  | ok conflict =>
    cases hstage : rowThenDependencies conflict with
    | error error => simp [original, reused, hscan, hstage, Except.bind]
    | ok state =>
      cases hasRoute <;> simp [original, reused, hscan, hstage, Except.bind]

/- A failing before (layout/lane checks, scope_at) still wins before either
   implementation. The continuation contains all subsequent source rows. -/
theorem whole_pipeline_exact {S I C A O E P : Type}
    (before : Except E P) (scan : S → I → Except E C)
    (source : S) (index : I) (hasRoute : Bool) (none : C)
    (rowThenDependencies : P → C → Except E A)
    (serialize : A → C → Except E O) :
    (before >>= fun p => original scan source index hasRoute none
      (rowThenDependencies p) serialize) =
    (before >>= fun p => reused scan source index hasRoute none
      (rowThenDependencies p) serialize) := by
  cases before with
  | error error => rfl
  | ok p => exact reuse_exact scan source index hasRoute none (rowThenDependencies p) serialize

#print axioms reuse_exact
#print axioms whole_pipeline_exact
end ConflictReuse
