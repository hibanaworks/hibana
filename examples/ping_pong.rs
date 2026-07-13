#[path = "support/in_memory.rs"]
mod in_memory;

use futures::executor::block_on;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use in_memory::InMemoryTransport;

fn main() {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<1, 0, Msg<2, u32>>(),
    );
    let client_program: RoleProgram<0> = project(&choreography);
    let server_program: RoleProgram<1> = project(&choreography);

    let mut slab = [0_u8; 16 * 1024];
    let mut storage = SessionKitStorage::<InMemoryTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, InMemoryTransport::new())
        .expect("create rendezvous");
    let session = SessionId::new(1);
    let mut client = rendezvous
        .enter(session, &client_program)
        .expect("attach client");
    let mut server = rendezvous
        .enter(session, &server_program)
        .expect("attach server");

    let (ping, pong) = block_on(async {
        client.send::<Msg<1, u32>>(&7).await.expect("send ping");
        let ping = server.recv::<Msg<1, u32>>().await.expect("receive ping");
        server
            .send::<Msg<2, u32>>(&(ping + 1))
            .await
            .expect("send pong");
        let pong = client.recv::<Msg<2, u32>>().await.expect("receive pong");
        (ping, pong)
    });

    assert_eq!((ping, pong), (7, 8));
    println!("ping={ping}, pong={pong}");
}
