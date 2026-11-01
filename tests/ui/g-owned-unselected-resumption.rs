use hibana::{Endpoint, g, runtime::program::project};

fn main() {
    type Loan = g::Msg<1, Endpoint<'static, 3>>;
    type Use = g::Msg<4, ()>;
    type Keep = g::Msg<2, ()>;
    type Leave = g::Msg<3, Endpoint<'static, 3>>;
    let body = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 1, Use>(), g::send::<1, 0, Loan>()),
    );
    let continuation =
        g::route(g::send::<0, 2, Keep>(), g::send::<0, 2, Leave>()).resolve::<17>();
    // Unlike a selected finite execution of roll, this global requires a
    // second loan from actor 0 after either arm, including the transfer to 2.
    let global = g::seq(g::seq(body, continuation), body);
    let _ = project::<0>(&global);
}
