use hibana::{RouteBranch, g};

fn route_branch_decode_is_not_public(branch: RouteBranch<'_, '_, 0>) {
    let _ = branch.decode::<g::Msg<7, ()>>();
}

fn main() {}
