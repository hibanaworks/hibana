use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

async fn endpoint_offer_decode_example(
    endpoint: &mut hibana::Endpoint<'_, 1>,
) -> hibana::EndpointResult<()> {
    let branch = endpoint.offer().await?;
    match branch.label() {
        31 => {
            let value = branch.decode::<g::Msg<31, u32>>().await?;
            let _ = value;
        }
        33 => {
            let unit = branch.decode::<g::Msg<33, ()>>().await?;
            let _ = unit;
        }
        label => panic!("unexpected route label {label}"),
    }
    Ok(())
}

fn main() {
    let accepted = g::send::<0, 1, g::Msg<31, u32>>();
    let rejected = g::send::<0, 1, g::Msg<33, ()>>();
    let routed = g::route(accepted, rejected);
    let passive_program: RoleProgram<1> = project(&routed);
    let _ = passive_program;
    let _ = endpoint_offer_decode_example;
}
