use std::{
    net::{SocketAddrV4, UdpSocket},
    ops::Add,
    str::FromStr,
};

use jiff::{SignedDuration, Timestamp};
use log::{debug, error, info};
use rosc::{OscMessage, OscPacket, OscType};

use crate::storage::BoopStorage;

pub(crate) struct OscBooper {
    /// Our receiving socket
    socket: UdpSocket,

    /// VRChat/OSC receiver socket
    osc_receiver: SocketAddrV4,

    /// Boop counter storage
    storage: BoopStorage,

    /// Last sent timestamp, used for cooldown
    last_message: Timestamp,
}

impl OscBooper {
    pub fn new(listen_port: u16, send_port: u16) -> Self {
        let socket = match UdpSocket::bind(format!("127.0.0.1:{listen_port}")) {
            Ok(sock) => sock,
            Err(e) => {
                error!("failed to bind socket: {}", e);
                std::process::exit(1);
            }
        };
        let osc_receiver = match SocketAddrV4::from_str(format!("127.0.0.1:{send_port}").as_str()) {
            Ok(addr) => addr,
            Err(e) => {
                error!("port seems to be invalid: {}", e);
                std::process::exit(1);
            }
        };

        OscBooper {
            socket,
            osc_receiver,
            storage: BoopStorage::load(),
            last_message: Timestamp::now(),
        }
    }
}

impl OscBooper {
    /// Main program loop
    pub(crate) fn run(&mut self) {
        // todo: is that buffer big enough? otherwise 4096 should be plenty
        let mut buf = [0u8; rosc::decoder::MTU];

        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((size, addr)) => {
                    let packet = match rosc::decoder::decode_udp(&buf[..size]) {
                        Ok((_, packet)) => Some(packet),
                        Err(e) => {
                            error!("failed to parse packet on {}: {}", addr, e);
                            None
                        }
                    };

                    if let Some(packet) = packet {
                        self.handle_packet(packet);
                    }
                }
                Err(e) => {
                    println!("Error receiving from socket: {}", e);
                    break;
                }
            }
        }
    }

    /// Handle received OSC packet
    fn handle_packet(&mut self, packet: OscPacket) {
        match packet {
            OscPacket::Message(msg) => {
                debug!(
                    "OSC message address: {}, arguments: {:?}",
                    msg.addr, msg.args
                );
                self.handle_message(&msg);
            }
            OscPacket::Bundle(bundle) => {
                info!("OSC Bundle: {:?}", bundle);
            }
        }
    }

    /// Handle received OSC message
    fn handle_message(&mut self, message: &OscMessage) {
        if message.addr.ends_with("/OSCBoop") && !message.args.is_empty(){
            // skip when contact sender leaves receiver bubble
            // let's assume that only bools will be sent
            if let OscType::Bool(false) = message.args[0] {
                return;
            }
            self.storage.inc_boops();

            let (message, contains_funny) = self.storage.generate_message();

            // skip unless funny or off cooldown
            if !contains_funny && !self.should_send_message() {
                return;
            }

            self.publish_chatbox(message);
        } else if message.addr.ends_with("/BoopSave") {
            self.storage.save();
        } else if message.addr == "/avatar/change" {
            if message.args.is_empty() {
                error!("expected avatar id in message, got {:?}", message.args);
                return;
            }

            let avatar_id = message.args[0].to_string();
            info!("avatar switched to {avatar_id}");

            // todo: listen to avatar change event (/avatar/change) and then query
            //      if avatar is compatible via OSC capability query
            // https://github.com/vrchat-community/osc/wiki/OSCQuery
            // https://github.com/Vidvox/OSCQueryProposal
            // todo: advertise configured port and receive data
        }

        // save storage if it's been a while
        // due to the amount of messages spammed every second, this should be fine™
        if self.storage.time_to_save() {
            self.storage.save();
        }
    }

    /// send string to VRChat chatbox
    /// https://docs.vrchat.com/docs/osc-as-input-controller
    fn publish_chatbox(&mut self, message: String) {
        let packet = OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            // message | send immediately, bypass keyboard | don't trigger SFX
            args: vec![
                OscType::String(message),
                OscType::Bool(true),
                OscType::Bool(false),
            ],
        });

        let msg_buf = match rosc::encoder::encode(&packet) {
            Ok(buf) => Some(buf),
            Err(e) => {
                error!("failed to encode chatbox packet: {}", e);
                None
            }
        };

        if let Some(msg_buf) = msg_buf {
            if let Err(e) = self.socket.send_to(&msg_buf, self.osc_receiver) {
                error!("failed to send message to chatbox: {}", e);
            }
        }

        self.last_message = Timestamp::now();
    }

    /// Whether we should send a chat message again
    fn should_send_message(&self) -> bool {
        Timestamp::now() > self.last_message.add(SignedDuration::from_secs(2))
    }
}
