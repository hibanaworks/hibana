use hibana::g::{self};

fn main() {
    let arm_one = g::send::<g::Role<0>, g::Role<0>, g::Msg<5, ()>, 0>();
    let arm_two = g::send::<g::Role<0>, g::Role<0>, g::Msg<5, ()>, 0>();
    let route = g::route(arm_one, arm_two);
    let _projected: g::advanced::RoleProgram<0> = g::advanced::project(&route);
}
