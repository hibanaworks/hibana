use hibana::control::cap::{
    resource_kinds::LoopContinueKind,
    GenericCapToken,
};
use hibana::g;
use hibana::runtime::consts::LABEL_LOOP_CONTINUE;

type BadMsg = g::Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    g::ExternalControl<LoopContinueKind>,
>;

fn main() {
    let _ = g::send::<g::Role<0>, g::Role<1>, BadMsg>();
}
