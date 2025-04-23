use dotenvy::dotenv;
use tokio_util::sync::CancellationToken;
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

    let token = CancellationToken::new();
    setup_signal_handlers(token.clone()).await;

    // todo: get sending port from VRC mDNS response

    let opt = Options::new();

    // set up OSC listener/responder & main loop
    let mut osc = OscBooper::new(opt).await;

    // set up OSCQuery & mDNS announcements
    oscquery::announce(token.clone(), osc.osc_port).await;

    // run main loop
    osc.run(token.clone()).await;

    // todo: prometheus interface for metrics
    //      - can I include avatar ID in there as label?
    //      - can I include world ID in there as label?
    //      needs prometheus_enable (pe) and prometheus_port (pp)

    // todo: ovr overlay with buttons or interact with application
    //      (pause, post again) via OSC messages?
}

#[cfg(unix)]
async fn setup_signal_handlers(token: CancellationToken) {
    // https://docs.rs/tokio/latest/tokio/signal/unix/struct.Signal.html
    use tokio::signal::{
        unix,
        unix::{SignalKind, signal},
    };

    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    let mut sigint = signal(SignalKind::interrupt()).unwrap();

    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::debug!("received SIGTERM");
                token.cancel();
            },
            _ = sigint.recv() => {
                tracing::info!("received SIGINT");
                token.cancel();
            }
        }
    });

    tracing::debug!("waiting for unix signal");
}

#[cfg(windows)]
async fn setup_signal_handlers(token: CancellationToken) {
    // https://docs.rs/tokio/latest/tokio/signal/windows/index.html
    // https://learn.microsoft.com/en-us/windows/console/console-control-handlers
    // https://learn.microsoft.com/en-us/windows/console/handlerroutine
    use tokio::signal::windows;

    let mut ctrl_break = windows::ctrl_break().unwrap();
    let mut ctrl_c = windows::ctrl_c().unwrap();
    let mut ctrl_close = windows::ctrl_close().unwrap();
    let mut ctrl_shutdown = windows::ctrl_shutdown().unwrap();

    tokio::spawn(async move {
        tokio::select! {
            _ = ctrl_break.recv() => {
                tracing::debug!("received CTRL_BREAK");
                token.cancel();
            },
            _ = ctrl_c.recv() => {
                tracing::info!("received CTRL_C");
                token.cancel();
            },
            _ = ctrl_close.recv() => {
                tracing::debug!("received CTRL_CLOSE");
                token.cancel();
            },
            _ = ctrl_shutdown.recv() => {
                tracing::debug!("received CTRL_SHUTDOWN");
                token.cancel();
            },
        };
    });

    tracing::debug!("waiting for windows signal");
}
