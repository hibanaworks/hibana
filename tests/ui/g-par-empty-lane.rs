use hibana::g;

// Parallel lanes must contain at least one step. Attempting to lift an empty
// program into `par_chain` should fail at compile time.
const _: () = {
    let _ = g::par(g::par_chain(g::Program::empty()));
};

fn main() {}
