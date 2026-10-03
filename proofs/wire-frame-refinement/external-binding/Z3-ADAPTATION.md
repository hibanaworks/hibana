# External allocator Z3/source-transcription qualification

The passing evidence is `external-specific-result-v3.json` and
`external-specific-run-v3.log`. The unchanged historical 40-query generator was
also replayed with an explicit `--repo` pointing to the external checkout;
`prior-40-query-replay.json` and its log preserve that result separately.

The target is external HEAD `731f910a2c8e30844598cd056ab077522333d83c` at
`/workspace/scratch/0915e8fbff81/hibana-external-send-followup`. No tracked file
in that checkout changed. All new work is isolated in this artifact directory.
`z3-source-manifest.json` binds the full external Rust allocator, Rust lowering,
Lean descriptor and refinement sources, relevant individual Lean declarations,
both checker inputs, and executed evidence by SHA-256.

## Executed results

| Evidence | Symbolic obligations and controls | Finite coverage |
| --- | --- | --- |
| Exact historical generator against external Rust sources | 40: 20 SAT, 20 UNSAT | 38,840 bounded cases; 9 fixtures; 11 historical models |
| External Lean transcription adaptation | 29: 10 SAT, 19 UNSAT | 38,840 bounded cases; 10 fixtures; 11 historical models; 5 raw-marker controls |

Every solver result matched its expected result using Z3 5.1.0. SAT records
include executed witnesses/nonvacuity cases. Both fresh runs pin the unchanged
Rust allocator hashes. The external run additionally pins the exact external
`DescriptorImage.lean` and `DescriptorRefinement.lean`; it does not reuse the
former package's different production Lean allocator as external evidence.

The 38,840 external cases compare four independently expressed paths: the
external full-list marker/chronological-prior transcription, the former
suffix-marker/reversed-prior transcription, and the historical Rust-mask and
list/argmin transcriptions. Their input bounds and layouts are recorded in the
JSON. These remain bounded tests, not unbounded program coverage. The fixtures
cover ordinary/no-Roll, separate/sibling Rolls, nested, reverse-nested,
coextensive, partitioned sender/receiver/lane domains, old route-label
inequalities, self sends, half-open/empty ranges, non-Roll markers, the entire
256-color palette, first overflow, and total-reference continuation after it.

## Exact semantic bridge and its limits

`elasticFrameOwner` calls `markers.find?` on the complete marker list. The
former `rollFrameOwner` used `scopeSegmentEnd`, which searched only after the
enter and fell back to the total atom count when no closing marker existed.
The external function instead ignores a Roll enter whose matching exit is
absent. Its no-Roll guard also preserves ordinary input if only orphan exits
are present.

The bridge premise is explicit: for every Roll enter, there is exactly one
matching exit in the full list, it occurs strictly after that enter, and its
offset equals the Rust marker's stored `segment_end`. The unbounded Z3
matching-index abstraction proves both searches choose the same exit and
offset under that premise, with a satisfiable premise witness. Interval
validity, laminarity, unique preorder ordinals and later ordinal for an inner
coextensive wrapper remain source-lowering assumptions. This gate does not
derive those premises for every compiler execution.

Executed raw-marker controls retain the important counterexamples:

- Missing exit: external owners/colors `[0, 0, 0]`; former owners/colors
  `[0, 1, 1]`
- An earlier duplicate exit: external owners `[0, 0, 0]`; former owners
  `[1, 1, 1]`. Colors happen to agree for this fixture; owner divergence is the
  counterexample
- Only an exit before the enter: the same owner divergence occurs
- An orphan exit without any enter: both no-Roll guards preserve `[7, 7]`
- Two later exits: both pick the first; changing their list order changes the
  external result. This malformed case is excluded by the unique-exit premise

There is no claim of equivalence for arbitrary malformed marker lists.

`separateElasticFrameDomainsFrom` appends assignments in source order and uses
`filterMap`. The former helper prepended assignments and used `filter` followed
by `map`. A local Z3 membership induction step, its empty base, symbolic
prefixes of lengths 0–8, and finite full-allocator comparisons establish the
reviewed set-membership correspondence. The first available palette color
depends only on membership. A concrete duplicate-color list demonstrates
that the used lists can differ in order while their color sets agree. This
does not present the Z3 local induction step as a mechanically checked
universal theorem about arbitrary Lean lists.

## Phase placement and capacity scope

The exact pinned declarations place the external phase once inside
`canonicalProgramSource`, consuming the complete `compiledOccurrences` and
`canonicalControlSource.markers`. The per-event and per-role accessors observe
that final source directly. There is no `canonicalWireAtoms` wrapper or second
phase. The pinned Rust lowering calls its phase once after complete structural
emission and count checks. Two executed source-text mutants are rejected: an
extra phase in the per-event accessor and two phases in the full source. This
is an exact-source placement check, not a general call-graph theorem.

Rust aborts globally at the first exhausted row. The total external Lean model
retains 256 in that row and may continue. Positive correspondence with emitted
Rust descriptors therefore assumes successful Rust lowering, equivalently no
exhausted row in the compared total result. This assumption is explicit.

The decoder's byte check and exact role-label equality reject an invalid row
only when that role's filter retains it. The executed 257-row overflow fixture
retains 256 for roles 0 and 1; unrelated role 2 has `[]`. Z3 proves rejection
with the retained-row/byte premises and supplies SAT controls when either the
retained-row premise or byte bound is absent. Role-local acceptance alone does
not imply global capacity success for arbitrary synthetic certificates.

Runtime `Covers`/`SameClassUnique` remains an external premise. The capacity
claim concerns the fixed greedy prefix, not optimal graph coloring. These are
manually reviewed source transcriptions and mathematical/finite evidence, not
extracted Rust or Lean execution or a universal Rust-to-Lean refinement proof.

## Replay

Use a fresh output filename; both checkers refuse to replace evidence:

```sh
cd /workspace/scratch/0915e8fbff81
/tmp/hibana-quic-proof-venv/bin/python \
  hibana-roll-color-fix/proofs/wire-frame-refinement/check_correspondence.py \
  --repo /workspace/scratch/0915e8fbff81/hibana-external-send-followup \
  --output artifacts/external-wire-refinement-gate/prior-replay-new.json

/tmp/hibana-quic-proof-venv/bin/python \
  artifacts/external-wire-refinement-gate/check_external_allocator.py \
  --repo /workspace/scratch/0915e8fbff81/hibana-external-send-followup \
  --prior /workspace/scratch/0915e8fbff81/hibana-roll-color-fix/proofs/wire-frame-refinement/check_correspondence.py \
  --prior-replay artifacts/external-wire-refinement-gate/prior-replay-new.json \
  --output artifacts/external-wire-refinement-gate/external-specific-new.json
```

The initial historical replay's output was moved byte-for-byte from its
temporary directory under the external checkout to this outside artifact root.
Its recorded command is the original execution, and that temporary directory
was removed. The historical package and all its previous evidence are untouched.

`development-attempts/` preserves two checker-development failures and their
exact checker snapshots: the first stopped on the declaration-name regex for
`readByte?`; the second passed Z3 and bounded cases but did not recognize the
historical model's `Par` spelling. Only the proof checker was corrected. The
v3 result is the completed passing run, and its checker hash matches the
current `check_external_allocator.py`.
