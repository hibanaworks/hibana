use hibana::g::{self, SendStep, StepCons, StepNil};

type Client = g::Role<0>;
type Server = g::Role<1>;

type GlobalSteps = StepCons<SendStep<Client, Server, g::Msg<7, u16>>, StepNil>;

const PROGRAM: g::Program<GlobalSteps> = g::send::<Client, Server, g::Msg<7, u16>>();

// Omitting the projected send step must fail: the client actually performs a send.
type MissingLocal = StepNil;

const CLIENT: g::RoleProgram<'static, 0, MissingLocal> = g::project::<0, GlobalSteps, _>(&PROGRAM);

fn main() {
    let _ = CLIENT;
}
