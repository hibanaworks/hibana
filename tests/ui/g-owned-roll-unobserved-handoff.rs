use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Loan = g::Msg<1, Endpoint<'static, 10>>;
    type Ready = g::Msg<38, ()>;
    type Tick = g::Msg<39, ()>;
    type Read = g::Msg<3, u32>;
    let program = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<1, 2, Ready>(),
            g::seq(
                g::seq(g::send::<2, 1, Tick>(), g::send::<10, 4, Read>()).roll(),
                g::send::<1, 5, Loan>(),
            ),
        ),
    );
    let _ = project::<2>(&program);
}
