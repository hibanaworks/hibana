use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::LoopContinueKind;

fn main() {
    let mgmt_prefix = || {
        g::seq(
            g::send::<
                g::Role<0>,
                g::Role<0>,
                g::Msg<48, GenericCapToken<LoopContinueKind>, CanonicalControl<LoopContinueKind>>,
                0,
            >(),
            g::send::<g::Role<0>, g::Role<1>, g::Msg<44, ()>, 0>(),
        )
    };
    let app = || {
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<10, u32>, 0>(),
            g::send::<g::Role<1>, g::Role<0>, g::Msg<11, u32>, 0>(),
        )
    };
    let program = g::seq(mgmt_prefix(), app());
    let projected: RoleProgram<0> = project(&program);
    let _ = projected;
}
