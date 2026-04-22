use hibana::substrate::cap::advanced::LoopContinueKind;

fn main() {
    let _ = core::mem::size_of::<hibana::substrate::cap::CanonicalControl<LoopContinueKind>>();
}
