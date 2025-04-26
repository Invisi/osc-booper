use std::{net::SocketAddr, ops::Add, sync::Arc, time::Duration};

use jiff::{SignedDuration, Timestamp};
use rosc::{OscMessage, OscPacket, OscType};
use tokio::{net::UdpSocket, sync::Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::{
    config::{Options, TextSuffixResult},
    storage::BoopStorage,
};

pub(crate) struct OscBooper<'a> {
    /// Our receiving socket
    socket: Arc<UdpSocket>,

    /// Our OSC port
    /// Stored separated for ease of access
    pub(crate) osc_port: u16,

    /// VRChat/OSC receiver address
    osc_receiver: SocketAddr,

    /// Boop counter storage
    storage: BoopStorage,

    /// Last sent timestamp, used for cooldown
    last_message: Timestamp,

    /// Suffix for boops
    boop_address: &'a str,

    /// Our settings/options
    options: Options,

    /// channel to notify chatbox clearing thread
    clear_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl<'a> OscBooper<'a> {
    pub async fn new(options: Options) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .map_err(|e| {
                error!(err=%e, "failed to bind osc socket");
            })
            .unwrap();
        let listen_addr = socket.local_addr().unwrap();

        let osc_receiver: SocketAddr = ([127u8, 0, 0, 1], options.osc_send_port).into();

        info!("receiving osc packets on {}", listen_addr);
        info!("sending osc packets to {}", osc_receiver);

        // we'll need the string for the entire runtime, just leak it
        let boop_address = options.boop_address.clone().leak();

        OscBooper {
            socket: Arc::new(socket),
            boop_address,
            options,
            osc_port: listen_addr.port(),
            osc_receiver,
            storage: BoopStorage::load(),
            last_message: Timestamp::now(),
            clear_tx: None,
        }
    }

    /// Main program loop
    pub(crate) async fn run(&mut self, token: CancellationToken) {
        let mut buf = [0u8; rosc::decoder::MTU];

        let main_socket = self.socket.clone();
        let clearing_socket = self.socket.clone();
        let osc_clone = self.osc_receiver;

        let (clear_tx, clear_rx) = tokio::sync::mpsc::channel(32);
        self.clear_tx = Some(clear_tx);

        let mut listener_loop = async || {
            loop {
                match main_socket.recv_from(&mut buf).await {
                    Ok((size, addr)) => {
                        let packet = match rosc::decoder::decode_udp(&buf[..size]) {
                            Ok((_, packet)) => Some(packet),
                            Err(e) => {
                                error!(err=%e, addr=%addr, "failed to parse packet");
                                None
                            }
                        };

                        if let Some(packet) = packet {
                            self.handle_packet(packet).await;
                        }
                    }
                    Err(e) => {
                        error!(err=%e, "error receiving from socket");
                    }
                }
            }
        };

        tokio::select! {
            _ = token.cancelled() => {
                info!("stopping osc listener");
            }
            _ = listener_loop() => {
                warn!("osc listener stopped unexpectedly");
            }
            _ = clear_chatbox_loop(clear_rx, clearing_socket, osc_clone) => {
                warn!("chatbox clearing loop stopped unexpectedly");
            }
        }

        info!("saving boop storage one last time");
        self.storage.save();
        info!("see ya!");
    }

    /// Handle received OSC packet
    async fn handle_packet(&mut self, packet: OscPacket) {
        match packet {
            OscPacket::Message(msg) => {
                if !msg.addr.ends_with("FluffSquishUpper") {
                    debug!(
                        "OSC message address: {}, arguments: {:?}",
                        msg.addr, msg.args
                    );
                }
                self.handle_message(&msg).await;
            }
            OscPacket::Bundle(bundle) => {
                info!("OSC Bundle: {:?}", bundle);
            }
        }
    }

    /// Handle received OSC message
    async fn handle_message(&mut self, message: &OscMessage) {
        if message.addr.ends_with(self.boop_address) && !message.args.is_empty() {
            // skip when contact sender leaves receiver bubble
            // let's assume that only bools will be sent
            if let OscType::Bool(false) = message.args[0] {
                return;
            }
            self.storage.inc_boops();

            let (message, has_suffix) = self.generate_message();

            // skip if on cooldown or message is without suffix
            if !has_suffix && !self.should_send_message() {
                return;
            }

            self.send_message(message).await;
        } else if message.addr == "/avatar/change" {
            // this event fires on map changes (usually) and on avatar change

            if message.args.is_empty() {
                error!("expected avatar id in message, got {:?}", message.args);
                return;
            }

            let avatar_id = message.args[0].to_string();
            info!("avatar switched to {avatar_id}");

            self.storage.save();
        }

        // save storage if it's been a while
        // due to the amount of messages spammed every second, this should be fine™
        // (/avatar/parameters/* gets spammed multiple times per second)
        if self.storage.time_to_save() {
            self.storage.save();
        }
    }

    async fn send_message(&mut self, message: String) {
        publish_chatbox(&self.socket, self.osc_receiver, message).await;
        self.last_message = Timestamp::now();

        // notify clear thread
        if let Some(tx) = &self.clear_tx {
            tx.send(()).await.ok();
        }
    }

    /// Whether we should send a chat message again
    fn should_send_message(&self) -> bool {
        Timestamp::now() > self.last_message.add(SignedDuration::from_secs(2))
    }

    /// Generate chatbox message
    fn generate_message(&mut self) -> (String, bool) {
        let (today_boops, total_boops) = self.storage.boop_numbers();

        let today_suffix = self
            .generate_text_suffix(today_boops as u64)
            .map_or("".into(), |suffix| format!(" {suffix}"));
        let total_suffix = self
            .generate_text_suffix(total_boops)
            .map_or("".into(), |suffix| format!(" {suffix}"));
        let is_suffixed = !today_suffix.is_empty() || !total_suffix.is_empty();

        // todo: configurable message template, with some boring validation
        (
            format!(
                "Today: {}{}\nTotal: {}{}",
                today_boops, today_suffix, total_boops, total_suffix
            )
            .to_string(),
            is_suffixed,
        )
    }

    /// Loop over registered [`crate::config::TextSuffix`]es and return first
    /// match, or None
    fn generate_text_suffix(&self, number: u64) -> Option<String> {
        for f_n in &self.options.text_suffixes {
            match f_n.check_value(number) {
                TextSuffixResult::Break => return None,
                TextSuffixResult::Skip => continue,
                TextSuffixResult::Message(suffix) => return Some(suffix),
            }
        }

        None
    }
}

/// send empty message to chatbox after main message has been sent
async fn clear_chatbox_loop(
    mut rx: tokio::sync::mpsc::Receiver<()>,
    socket: Arc<UdpSocket>,
    addr: SocketAddr,
) {
    let debounce_mutex: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(None));

    while rx.recv().await.is_some() {
        let mut task = debounce_mutex.lock().await;

        // cancel running thread
        if let Some(handle) = task.take() {
            trace!("cancelling existing clear task");
            handle.abort();
        }

        // wait a bit and then send clear
        let socket_clone = socket.clone();
        *task = Some(tokio::spawn(async move {
            trace!("waiting for clear timeout");
            tokio::time::sleep(Duration::from_secs(4)).await;
            publish_chatbox(&socket_clone, addr, "".into()).await;
            trace!("sent chatbox clear");
        }));
    }
}

/// create buffer for OSC chatbox message
/// https://docs.vrchat.com/docs/osc-as-input-controller
fn make_msg_buffer(message: String) -> Option<Vec<u8>> {
    let packet = OscPacket::Message(OscMessage {
        addr: "/chatbox/input".into(),
        args: vec![
            // message
            OscType::String(message),
            // send immediately, bypass keyboard input
            OscType::Bool(true),
            // don't trigger SFX
            OscType::Bool(false),
        ],
    });

    match rosc::encoder::encode(&packet) {
        Ok(buf) => Some(buf),
        Err(e) => {
            error!(err=%e, "failed to encode chatbox packet");
            None
        }
    }
}

/// send string to VRChat chatbox
async fn publish_chatbox(socket: &UdpSocket, addr: SocketAddr, message: String) {
    if let Some(msg_buf) = make_msg_buffer(message) {
        if let Err(e) = socket.send_to(&msg_buf, addr).await {
            error!(err=%e, "failed to send message to chatbox");
        }
    }
}
