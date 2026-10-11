use hibana::{
    g,
    runtime::program::{RoleProgram, project},
};

fn main() {
    let program = g::seq(
        g::send::<16, 17, g::Msg<1, u8>>(),
        g::send::<254, 255, g::Msg<2, u8>>(),
    );
    let _: RoleProgram<16> = project(&program);
    let _: RoleProgram<255> = project(&program);
}
