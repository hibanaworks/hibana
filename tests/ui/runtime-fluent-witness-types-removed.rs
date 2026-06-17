use hibana::runtime::{RoleKit, SessionRendezvousKit, SessionRoleKit};

fn main() {
    let _: Option<RoleKit<'_, '_, '_, 0, ()>> = None;
    let _: Option<SessionRendezvousKit<'_, '_, ()>> = None;
    let _: Option<SessionRoleKit<'_, '_, '_, 0, ()>> = None;
}
