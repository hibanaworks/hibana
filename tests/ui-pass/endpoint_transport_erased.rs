use hibana::{Endpoint, RouteBranch};

fn takes_endpoint<'r>(_: &mut Endpoint<'r, 0>) {}

fn takes_branch<'e, 'r>(_: RouteBranch<'e, 'r, 0>) {}

fn main() {
    let _ = takes_endpoint;
    let _ = takes_branch;
}
