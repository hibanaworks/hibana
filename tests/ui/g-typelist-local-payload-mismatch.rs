use hibana::g::{self, LocalSend, SendStep, StepCons, StepNil};

type Client = g::Role<0>;
type Server = g::Role<1>;

type GlobalSteps = StepCons<SendStep<Client, Server, g::Msg<7, u16>>, StepNil>;

const PROGRAM: g::Program<GlobalSteps> = g::send::<Client, Server, g::Msg<7, u16>>();

// Intentionally declare an incorrect local typelist for the client role. The payload type
// (`u8`) mismatches the actual projection (`u16`), so this must fail during compilation.
type WrongClientLocal = StepCons<LocalSend<Server, g::Msg<7, u8>>, StepNil>;

const CLIENT: g::RoleProgram<'static, 0, WrongClientLocal> = g::project::<0, GlobalSteps, _>(&PROGRAM);

fn main() {
    let _ = CLIENT;
}
