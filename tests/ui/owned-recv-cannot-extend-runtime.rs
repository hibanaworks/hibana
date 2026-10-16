use hibana::{Endpoint, g};

type Loan = g::Msg<1, Endpoint<'static, 10>>;

async fn escape_runtime<'short, 'long>(worker: &mut Endpoint<'short, 1>) -> Endpoint<'long, 10> {
    worker.recv::<Loan>().await.unwrap()
}

fn main() {}
