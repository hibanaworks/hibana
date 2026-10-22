use hibana::Endpoint;

fn empty(mut endpoint: Endpoint<'_, 0>) {
    let [] = endpoint.par::<0>().unwrap();
}

fn main() {
    let function: fn(Endpoint<'_, 0>) = empty;
    std::hint::black_box(function);
}
