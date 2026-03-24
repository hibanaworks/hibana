use hibana::g;
use hibana::g::advanced::steps::StepNil;

// Parallel lanes must contain at least one step.
const _: () = {
    let _ = g::par(StepNil::PROGRAM, StepNil::PROGRAM);
};

fn main() {}
