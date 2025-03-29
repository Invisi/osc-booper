#![feature(duration_constructors)]

use dotenvy::dotenv;
use log::info;

use crate::{config::Options, osc::OscBooper};

mod config;
mod osc;
mod storage;

fn main() {
    dotenv().ok();
    // default to info level unless set otherwise
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opt = Options::new();

    info!("Starting osc booper");
    info!("Listening on 127.0.0.1:{}", opt.listen);
    info!("Sending to 127.0.0.1:{}", opt.send);

    let mut osc = OscBooper::new(opt.listen, opt.send);
    osc.run();

    // todo: save on exit
    // https://rust-cli.github.io/book/in-depth/signals.html

    // todo: prometheus interface for metrics
    //      - can I include avatar ID in there as label?
    //      - can I include world ID in there as label?
    //      needs prometheus_enable (pe) and prometheus_port (pp)

    // todo: ovr overlay with buttons or interact with application
    //      (pause, post again) via OSC messages?
}
