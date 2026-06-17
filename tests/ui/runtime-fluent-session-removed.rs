use hibana::runtime::ids::SessionId;
use hibana::runtime::transport::Transport;
use hibana::runtime::RendezvousKit;

fn needs_session<T: Transport>(rv: &RendezvousKit<'_, '_, T>, sid: SessionId) {
    let _ = rv.session(sid);
}

fn main() {}
