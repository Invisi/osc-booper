use dotenvy::dotenv;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Options, osc::OscBooper};

mod config;
mod osc;
mod oscquery;
mod storage;

#[tokio::main]
async fn main() {
    dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let opt = Options::new();
    let mut osc = OscBooper::new(opt.send).await;
    oscquery::announce(osc.osc_port).await;

    // todo: get sending port from VRC mDNS response
    osc.run().await;

    // todo: prometheus interface for metrics
    //      - can I include avatar ID in there as label?
    //      - can I include world ID in there as label?
    //      needs prometheus_enable (pe) and prometheus_port (pp)

    // todo: ovr overlay with buttons or interact with application
    //      (pause, post again) via OSC messages?
}
