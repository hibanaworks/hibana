use hibana::integration::cap::control::LoopContinueKind;

fn main() {
    let _ = core::mem::size_of::<hibana::integration::cap::ExternalControl<LoopContinueKind>>();
}
