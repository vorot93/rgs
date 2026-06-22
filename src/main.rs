use futures::StreamExt;
use qgs::{Client, model::Host};
use tracing::*;
use tracing_subscriber::{EnvFilter, prelude::*};

#[tokio::main]
async fn main() {
    let filter = EnvFilter::from_default_env();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let client = Client::builder().build();
    let masters = vec![Host::from(("master.ioquake3.org", 27950))];
    // More masters can be added to the list; an unreachable one surfaces as an
    // Err item and the run continues.
    let mut stream = std::pin::pin!(client.query_masters(None, masters, true));

    let mut total_queried = 0u64;
    while let Some(res) = stream.next().await {
        match res {
            Ok(server) => {
                total_queried += 1;
                info!("{server:?}");
            }
            Err(e) => warn!("query returned an error: {e:?}"),
        }
    }

    info!("Queried {total_queried} servers");
}
