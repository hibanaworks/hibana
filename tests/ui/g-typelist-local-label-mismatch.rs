use hibana::g::{self, LocalSend, SendStep, StepCons, StepNil};

type Client = g::Role<0>;
type Server = g::Role<1>;

type GlobalSteps = StepCons<SendStep<Client, Server, g::Msg<7, u16>>, StepNil>;

const PROGRAM: g::Program<GlobalSteps> = g::send::<Client, Server, g::Msg<7, u16>>();

// Expecting the wrong label (`8` instead of `7`) must fail during compilation.
type WrongLocal = StepCons<LocalSend<Server, g::Msg<8, u16>>, StepNil>;

const CLIENT: g::RoleProgram<'static, 0, WrongLocal> = g::project::<0, GlobalSteps, _>(&PROGRAM);

fn main() {
    let _ = CLIENT;
}
