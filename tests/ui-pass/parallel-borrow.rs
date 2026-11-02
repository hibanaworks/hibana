use hibana::{Endpoint, EndpointError, g};

type Request = g::Msg<1, u32>;
type Reply = g::Msg<2, u32>;
type Notice = g::Msg<3, u32>;
type Done = g::Msg<4, ()>;

#[hibana_runtime::scope]
async fn client(mut endpoint: Endpoint<'_, 0>) -> Result<(), EndpointError> {
    {
        let [mut requests, mut notices] = endpoint.par()?;
        async {
            async {
                requests.send::<Request>(&7).await?;
                requests.recv::<Reply>().await
            };

            async { (notices.recv::<Notice>()).await };
        }
        .await?;
    }
    endpoint.send::<Done>(&()).await
}

fn main() {
    let global = g::seq(
        g::par(
            g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
            g::send::<1, 0, Notice>(),
        ),
        g::send::<0, 1, Done>(),
    );
    let _program = hibana::runtime::program::project::<0>(&global);
}
