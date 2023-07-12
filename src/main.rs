use rgs::models::*;
use tokio_stream::StreamExt;
use tracing::*;
use tracing_subscriber::{prelude::*, EnvFilter};

#[tokio::main]
async fn main() {
    let filter = EnvFilter::from_default_env();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let pconfig = rgs::protocols::make_default_protocols();

    let requests = vec![UserQuery {
        protocol: pconfig["q3m"].clone(),
        host: Host::S(StringAddr {
            host: "master.ioquake3.org".into(),
            port: 27950,
        }),
    }];

    let timeout = std::time::Duration::from_secs(5);

    let mut total_queried = 0;

    let mut query = rgs::UdpQuery::simple_query(requests).await;
    while let Some(res) = tokio::time::timeout(timeout, query.try_next())
        .await
        .map(|res| res.transpose())
        .ok()
        .flatten()
    {
        match res {
            Ok(entry) => {
                total_queried += 1;
                info!("{:?}", entry);
            }
            Err(e) => {
                info!("UdpQuery returned an error: {:?}", e);
            }
        }
    }

    info!("Queried {total_queried} servers");
}
