use std::{collections::HashMap, net::SocketAddr};

use oscquery::{
    node::{AccessMode, HostInfo, OSCTransport, OscNode},
    server::OscQueryServer,
};
use rand::distr::{Alphanumeric, SampleString};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub mod mdns;

pub async fn announce(token: CancellationToken, osc_port: u16) {
    // listener is dropped after this context to allow oscquery to bind again
    // this is kinda stupid, but it'll do for now
    let http_addr = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    };

    let random_suffix: String = Alphanumeric.sample_string(&mut rand::rng(), 8);
    let service_name = format!("osc-booper-{random_suffix}");

    info!("announcing ourselves as {service_name}");

    start_oscjson_server(token.clone(), service_name.clone(), http_addr, osc_port).await;

    let mdns_token = token.clone();
    tokio::task::spawn(async move {
        let mut server = mdns::MdnsServer::new(&service_name, http_addr.port());
        server.run(mdns_token).await;
    });
}

async fn start_oscjson_server(
    token: CancellationToken,
    service_name: String,
    socket_addr: SocketAddr,
    osc_port: u16,
) {
    let mut server = OscQueryServer::new(HostInfo {
        name: Some(service_name),
        osc_ip: Some("127.0.0.1".into()),
        osc_port: Some(osc_port),
        osc_transport: Some(OSCTransport::UDP),
        extensions: Some(HashMap::from([
            ("ACCESS".into(), true),
            ("CLIPMODE".into(), false),
            ("RANGE".into(), true),
            ("TYPE".into(), true),
            ("VALUE".into(), true),
        ])),
        ..Default::default()
    })
    .with_address(socket_addr);

    // listen for all avatar events, including change and parameters
    // boop (contact receivers) should be part of parameters
    server
        .add_node(
            "/avatar",
            OscNode::new("/avatar").with_access(AccessMode::None),
        )
        .await
        .unwrap();

    info!("oscjson server listening on {}", socket_addr);

    tokio::task::spawn(async move {
        tokio::select! {
            _ = server.serve().await => {
                warn!("oscjson server stopped unexpectedly");
            },
            _ = token.cancelled() => {
                server.shutdown();
                info!("stopping oscjson server");
            },
        }
    });
}
