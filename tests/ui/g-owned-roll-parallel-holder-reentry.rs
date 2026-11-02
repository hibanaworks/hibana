use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Loan = g::Msg<0, Endpoint<'static, 3>>;
    type Work = g::Msg<1, ()>;
    type Control = g::Msg<2, ()>;
    type Return = g::Msg<3, Endpoint<'static, 3>>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::par(g::send::<3, 2, Work>(), g::send::<0, 1, Control>()).roll(),
            g::send::<1, 0, Return>(),
        ),
    );
    let _ = project::<0>(&global);
}
