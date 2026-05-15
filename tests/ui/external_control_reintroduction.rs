use hibana::integration::cap::advanced::LoopContinueKind;

fn main() {
    let _ = core::mem::size_of::<hibana::integration::cap::ExternalControl<LoopContinueKind>>();
}
