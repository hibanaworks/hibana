use hibana::{g, runtime};

fn main() {
    let left = g::seq(
        g::send::<2, 2, g::Msg<7, ()>>(),
        g::seq(
            g::send::<0, 2, g::Msg<1, ()>>(),
            g::seq(
                g::send::<2, 0, g::Msg<3, ()>>(),
                g::send::<2, 1, g::Msg<4, ()>>(),
            ),
        ),
    );
    let right = g::seq(
        g::send::<2, 2, g::Msg<8, ()>>(),
        g::seq(
            g::send::<1, 2, g::Msg<2, ()>>(),
            g::seq(
                g::send::<2, 0, g::Msg<5, ()>>(),
                g::send::<2, 1, g::Msg<6, ()>>(),
            ),
        ),
    );
    let protocol = g::route(left, right).resolve::<7>().roll();
    let role0: runtime::program::RoleProgram<0> = runtime::program::project(&protocol);
    let role1: runtime::program::RoleProgram<1> = runtime::program::project(&protocol);
    let role2: runtime::program::RoleProgram<2> = runtime::program::project(&protocol);
    core::hint::black_box((role0, role1, role2));
}
