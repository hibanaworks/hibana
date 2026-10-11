use hibana::runtime::program::RoleProgram;
use hibana::runtime::transport::Transport;
use hibana::runtime::RendezvousKit;

fn needs_role<T: Transport, const ROLE: u8>(
    rv: &RendezvousKit<'_, '_, T>,
    program: &RoleProgram<ROLE>,
) {
    let _ = rv.role(program);
}

fn main() {}
