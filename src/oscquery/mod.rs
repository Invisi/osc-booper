use std::{collections::HashMap, net::SocketAddr, time::Duration};

use oscquery::{
    node::{AccessMode, HostInfo, OSCTransport, OscNode, OscType, OscTypeTag},
    server::OscQueryServer,
};
use rand::distr::{Alphanumeric, SampleString};
use tokio::net::TcpListener;
use tracing::info;

pub mod mdns;

pub async fn announce(osc_port: u16) {
    // listener is dropped after this context to allow oscquery to bind again
    // this is kinda stupid, but it'll do for now
    let http_addr = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    };

    let random_suffix: String = Alphanumeric.sample_string(&mut rand::rng(), 8);
    let service_name = format!("osc-booper-{random_suffix}");

    info!("announcing ourselves as {service_name}");

    // todo: cancellation token & signal listeners
    start_oscjson_server(service_name.clone(), http_addr, osc_port).await;
    tokio::task::spawn(async move {
        let mut server = mdns::MdnsServer::new(&service_name, http_addr.port());
        server.run().await;
    });
}

async fn start_oscjson_server(service_name: String, socket_addr: SocketAddr, osc_port: u16) {
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

    let mut avatar_contents = HashMap::new();
    avatar_contents.insert(
        "change".to_string(),
        OscNode::new("/avatar/change")
            .with_access(AccessMode::None)
            .with_type(OscTypeTag::new(vec![OscType::OscString])),
    );

    server
        .add_node(
            "/avatar",
            OscNode::new("/avatar")
                .with_access(AccessMode::None)
                .with_contents(avatar_contents),
        )
        .await
        .unwrap();

    server
        .add_node(
            "/OSCBoop",
            OscNode::new("/OSCBoop")
                .with_access(AccessMode::ReadOnly)
                .with_type(OscTypeTag::new(vec![OscType::True, OscType::False])),
        )
        .await
        .unwrap();

    info!("oscjson server listening on {}", socket_addr);

    tokio::task::spawn(async move {
        let join = server.serve().await;

        // todo: keep server alive until we close
        tokio::time::sleep(Duration::from_secs(600)).await;

        info!("oscjson server stopping");
        server.shutdown();
        join.await.unwrap().unwrap();
    });
}
