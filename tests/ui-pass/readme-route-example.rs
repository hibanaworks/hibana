use hibana::g;
use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};

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
        _ => unreachable!(),
    }
    Ok(())
}

fn main() {
    let accepted = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<30, (), RouteDecisionKind>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<31, u32>, 0>(),
    );
    let rejected = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<32, (), RouteDecisionKind>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<33, ()>, 0>(),
    );
    let routed = g::route(accepted, rejected);
    let passive_program: RoleProgram<1> = project(&routed);
    let _ = passive_program;
    let _ = endpoint_offer_decode_example;
}
