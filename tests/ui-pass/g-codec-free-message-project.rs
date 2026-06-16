use hibana::{
    g,
    runtime::program::{RoleProgram, project},
};

struct NoCodec;

fn main() {
    type Descriptor = g::Msg<31, NoCodec>;
    let program = g::seq(
        g::send::<0, 1, Descriptor>(),
        g::send::<1, 0, g::Msg<32, NoCodec>>(),
    );
    let client: RoleProgram<0> = project(&program);
    let server: RoleProgram<1> = project(&program);
    let _ = (client, server);
}
