use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type X = Endpoint<'static, 10>;
    type Y = Endpoint<'static, 11>;
    type LoanX = g::Msg<141, X>;
    type LoanY = g::Msg<142, Y>;
    type ReturnX = g::Msg<143, X>;
    type ReturnY = g::Msg<144, Y>;
    type TailLoanY = g::Msg<151, Y>;
    type Ack = g::Msg<152, ()>;
    type TailLoanX = g::Msg<153, X>;
    type TailReturnY = g::Msg<154, Y>;
    type TailReturnX = g::Msg<155, X>;
    let body = g::seq(
        g::send::<0, 1, LoanX>(),
        g::seq(g::send::<1, 0, LoanY>(),
            g::seq(g::send::<1, 0, ReturnX>(), g::send::<0, 1, ReturnY>())),
    );
    let tail = g::seq(
        g::send::<1, 2, TailLoanY>(),
        g::seq(g::send::<2, 0, Ack>(),
            g::seq(g::send::<0, 2, TailLoanX>(),
                g::seq(g::send::<2, 1, TailReturnY>(), g::send::<2, 0, TailReturnX>()))),
    );
    let _ = project::<0>(&g::seq(body.roll(), tail));
}
