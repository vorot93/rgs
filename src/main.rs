use futures::StreamExt;
use rgs::{
    Client,
    model::{Host, Query},
    protocol::make_default_protocols,
};
use tracing::*;
use tracing_subscriber::{EnvFilter, prelude::*};

#[tokio::main]
async fn main() {
    let filter = EnvFilter::from_default_env();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3m"].clone(),
        host: Host::Named {
            host: "master.ioquake3.org".into(),
            port: 27950,
        },
    }];

    let client = Client::builder().build();
    let mut stream = std::pin::pin!(client.query(queries));

    let mut total_queried = 0u64;
    while let Some(res) = stream.next().await {
        match res {
            Ok(entry) => {
                total_queried += 1;
                info!("{entry:?}");
            }
            Err(e) => warn!("query returned an error: {e:?}"),
        }
    }

    info!("Queried {total_queried} servers");
}
