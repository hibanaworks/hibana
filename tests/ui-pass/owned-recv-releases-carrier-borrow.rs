use hibana::{Endpoint, EndpointError, g};

type Loan = g::Msg<1, Endpoint<'static, 10>>;
type Control = g::Msg<2, u32>;
type Read = g::Msg<3, u32>;

pub async fn work<'r>(mut worker: Endpoint<'r, 1>) -> Result<(), EndpointError> {
    let mut db = worker.recv::<Loan>().await?;
    worker.send::<Control>(&7).await?;
    db.send::<Read>(&42).await?;
    worker.send::<Loan>(db).await
}

fn main() {}
