#[path = "support/in_memory.rs"]
mod in_memory;
#[path = "support/in_process.rs"]
pub mod in_process;

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use in_memory::InMemoryTransport;

// This host example's measured runtime budget; deployments measure their own descriptor set.
const RUNTIME_SLAB_BYTES: usize = 3 * 1024;

#[hibana_runtime::main(in_process::InProcess::default())]
async fn main() -> Result<(), hibana::EndpointError> {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<1, 0, Msg<2, u32>>(),
    );
    let client_program: RoleProgram<0> = project(&choreography);
    let server_program: RoleProgram<1> = project(&choreography);

    let mut slab = [0_u8; RUNTIME_SLAB_BYTES];
    let mut storage = SessionKitStorage::<InMemoryTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, InMemoryTransport::new([]))
        .expect("create rendezvous");
    let session = SessionId::new(1);
    let mut client = rendezvous
        .enter(session, &client_program)
        .expect("attach client");
    let mut server = rendezvous
        .enter(session, &server_program)
        .expect("attach server");

    let (pong, ping) = async {
        async {
            client.send::<Msg<1, u32>>(&7).await?;
            client.recv::<Msg<2, u32>>().await
        };
        async {
            let ping = server.recv::<Msg<1, u32>>().await?;
            server.send::<Msg<2, u32>>(&(ping + 1)).await?;
            Ok(ping)
        };
    }
    .await?;

    assert_eq!((ping, pong), (7, 8));
    println!("ping={ping}, pong={pong}");
    Ok(())
}
