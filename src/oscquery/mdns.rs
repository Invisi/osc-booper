use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
};

use mdns_proto::{
    error::{BufferType, ProtoError},
    proto::{Label, Message, MessageType, Question, ResourceRecord, ResourceType, Serialize},
    server::{Endpoint, QueryHandle, SlabEndpoint},
};
use smallvec::SmallVec;
use tokio::net::UdpSocket;
use tracing::{debug, error, info, trace};

// WireShark query
// (mdns) && (_ws.col.info matches "VRCFT" || _ws.col.info matches "osc-booper")

const IPV4_MDNS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

/// create cross-platform reusable UDP socket for mDNS listening
fn create_mdns_socket() -> UdpSocket {
    // create reusable UDP socket (please look away)
    let socket2_socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .map_err(|e| error!(err=%e, "failed to create unbound socket"))
    .unwrap();
    socket2_socket
        .set_reuse_address(true)
        .map_err(|e| error!(err=%e, "failed to set SO_REUSEADDR on socket"))
        .unwrap();

    // bind socket to any interface on port 5353
    let addr: SocketAddr = (Ipv4Addr::UNSPECIFIED, MDNS_PORT).into();
    socket2_socket
        .bind(&addr.into())
        .map_err(|e| error!(err=%e, "failed to set bind socket to interface"))
        .unwrap();

    // join multicast
    socket2_socket
        .join_multicast_v4(&IPV4_MDNS, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| error!(err=%e, "failed to join multicast group"))
        .unwrap();
    socket2_socket
        .set_multicast_loop_v4(true)
        .map_err(|e| {
            error!(err=%e, "failed to set IP_MULTICAST_LOOP on socket");
        })
        .unwrap();

    // convert to std socket, for tokio
    let std_socket = std::net::UdpSocket::from(socket2_socket);
    std_socket
        .set_nonblocking(true)
        .map_err(|e| error!(err=%e, "failed to mark socket as non-blocking"))
        .unwrap();

    UdpSocket::from_std(std_socket)
        .map_err(|e| error!(err=%e, "failed to convert std socket to tokio socket"))
        .unwrap()
}

pub(crate) struct MdnsServer<'a> {
    endpoint: SlabEndpoint,
    socket: UdpSocket,
    known_records: HashMap<&'a str, Vec<ResourceRecord<'a>>>,
}

impl<'a> MdnsServer<'a> {
    pub(crate) fn new(service_name: &str, http_port: u16) -> Self {
        let socket = create_mdns_socket();
        debug!("created mDNS socket");

        let mut this = MdnsServer {
            socket,
            endpoint: Endpoint::new(),
            known_records: HashMap::default(),
        };
        this.create_records(service_name, http_port);

        this
    }

    pub(crate) async fn run(&mut self) {
        info!("starting mDNS server");

        let mut buf = [0u8; 1500];

        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((n, from)) => {
                    let data = &buf[..n];
                    self.handle_query(from, data).await;
                }
                Err(e) => {
                    error!(err=%e, "error receiving from socket");
                    break;
                }
            }
        }
    }

    async fn handle_query(&mut self, peer: SocketAddr, data: &[u8]) {
        let conn_handle = match self.endpoint.accept() {
            Err(e) => {
                error!(from=%peer, err=%e, "failed to accept connection");
                return;
            }
            Ok(ch) => ch,
        };

        let mut questions: SmallVec<[Question; 4]> = SmallVec::new();
        questions.extend_from_slice(&[Question::default(); 4]);
        let mut answers: SmallVec<[ResourceRecord; 1]> = SmallVec::new();
        answers.extend_from_slice(&[ResourceRecord::default(); 1]);
        let mut authorities: SmallVec<[ResourceRecord; 0]> = SmallVec::new();
        let mut add_records: SmallVec<[ResourceRecord; 0]> = SmallVec::new();

        let message =
            {
                loop {
                    match Message::read(
                        data,
                        &mut questions,
                        &mut answers,
                        &mut authorities,
                        &mut add_records,
                    ) {
                        Ok(message) => break message,
                        Err(e) => match e {
                            ProtoError::NotEnoughWriteSpace {
                                tried_to_write,
                                buffer_type,
                                ..
                            } => match buffer_type {
                                BufferType::Question => {
                                    questions.resize(tried_to_write.into(), Question::default())
                                }
                                BufferType::Answer => {
                                    answers.resize(tried_to_write.into(), ResourceRecord::default())
                                }
                                BufferType::Authority => authorities
                                    .resize(tried_to_write.into(), ResourceRecord::default()),
                                BufferType::Additional => add_records
                                    .resize(tried_to_write.into(), ResourceRecord::default()),
                            },
                            _ => {
                                error!(from=%peer, err=%e, "failed to parse message");
                            }
                        },
                    }
                }
            };

        let query = match self.endpoint.recv(conn_handle, message) {
            Err(e) => {
                error!(addr=%peer, err=%e, "failed to retrieve query from SlabEndpoint");
                return;
            }
            Ok(q) => q,
        };

        for question in query.questions() {
            let msg = self
                .lookup_answer(query.query_handle(), peer, *question)
                .await;

            if let Some(msg) = msg {
                match self.socket.send_to(&msg, (IPV4_MDNS, MDNS_PORT)).await {
                    Ok(bytes_written) => {
                        debug!(addr=%peer, size=%bytes_written, "response sent off");
                    }
                    Err(e) => {
                        error!(addr=%peer, err=%e, "failed to send response");
                    }
                }
            }
        }

        if let Err(e) = self.endpoint.drain_query(query.query_handle()) {
            error!(from=%peer, err=%e, "failed to drain query");
        }

        if let Err(e) = self.endpoint.drain_connection(conn_handle) {
            error!(from=%peer, err=%e, "failed to drain connection");
        }
    }

    async fn lookup_answer(
        &mut self,
        qc: QueryHandle,
        peer: SocketAddr,
        question: Question<'_>,
    ) -> Option<Vec<u8>> {
        let service_name = question.name().to_string();
        let responses = self.known_records.get(service_name.as_str());
        if responses.is_none() {
            debug!(addr=%peer, service_name=%service_name, "skipping response");
            return None;
        }

        let responses = responses.unwrap();
        if responses.is_empty() {
            return None;
        }

        debug!(addr=%peer, service_name=%service_name, "answering query");

        match self.endpoint.response(qc, question) {
            Err(e) => {
                error!(addr=%peer, err=%e, "failed question");
                None
            }
            Ok(out) => {
                let mut flags = out.flags();
                // mark message as reply
                // for some reason mdns_proto does not do this by itself
                flags.set_qr(MessageType::Reply);

                let mut answers: SmallVec<[ResourceRecord; 1]> = SmallVec::new();
                let mut add_records: SmallVec<[ResourceRecord; 0]> = SmallVec::new();

                if responses.len() == 1 {
                    answers.extend_from_slice(&[responses[0]])
                } else {
                    answers.extend_from_slice(&[responses[0]]);
                    add_records.extend_from_slice(&responses[1..]);
                }

                let msg = Message::new(
                    out.id(),
                    flags,
                    &mut [],
                    &mut answers,
                    &mut [],
                    &mut add_records,
                );

                let msg_size = msg.space_needed();
                let mut buf = vec![0; msg_size];

                match msg.write(&mut buf) {
                    Ok(bytes_written) => {
                        trace!(addr=%peer, size=%bytes_written, "response serialized to buffer");
                    }
                    Err(e) => {
                        error!(addr=%peer, err=%e, "failed to serialize message");
                    }
                }

                Some(buf[..msg_size].to_owned())
            }
        }
    }

    /// create DNS records for OSCJSON service
    fn create_records(&mut self, service_name: &str, http_port: u16) {
        // we only announce an _oscjson._tcp service here as only that seems
        // necessary the oscjson server's response contains the OSC_IP and the
        // OSC_PORT anyway

        let ttl = 120;

        // I'll consider IPv6 a myth for now
        let localhost_rdata: &'a [u8] = [127u8, 0, 0, 1].as_slice();

        let oscjson_ptr_name: &'a str = format!("{service_name}.oscjson.tcp.local.").leak();
        let oscjson_service_name: &'a str = format!("{service_name}._oscjson._tcp.local.").leak();

        let ptr_rdata: &'a mut [u8] = make_dns_label(oscjson_service_name).unwrap().leak();
        let srv_rdata = make_srv_rdata(0, 0, http_port, oscjson_ptr_name)
            .unwrap()
            .leak();

        self.known_records.insert(
            "_oscjson._tcp.local",
            vec![
                ResourceRecord::new("_oscjson._tcp.local", ResourceType::Ptr, 1, ttl, ptr_rdata),
                ResourceRecord::new(
                    oscjson_service_name,
                    ResourceType::Txt,
                    1,
                    ttl,
                    // I know, this is cursed, but it works.
                    "\x09txtvers=1".as_bytes(),
                ),
                ResourceRecord::new(oscjson_service_name, ResourceType::Srv, 1, ttl, srv_rdata),
                ResourceRecord::new(oscjson_ptr_name, ResourceType::A, 1, ttl, localhost_rdata),
            ],
        );

        self.known_records.insert(
            oscjson_ptr_name,
            vec![ResourceRecord::new(
                oscjson_ptr_name,
                ResourceType::A,
                1,
                ttl,
                localhost_rdata,
            )],
        );
    }
}

fn make_dns_label(name: &str) -> Result<Vec<u8>, ProtoError> {
    let label = Label::from(name);
    let len = label.serialized_len();
    let mut buf = vec![0; len];
    label.serialize(&mut buf).map(|size| {
        buf.truncate(size);
        buf
    })
}

fn make_srv_rdata(
    priority: u16,
    weight: u16,
    port: u16,
    target: &str,
) -> Result<Vec<u8>, ProtoError> {
    let label = Label::from(target);
    let len = label.serialized_len();

    let mut buf = vec![0; 6 + len];
    buf[0..2].copy_from_slice(priority.to_be_bytes().as_ref());
    buf[2..4].copy_from_slice(weight.to_be_bytes().as_ref());
    buf[4..6].copy_from_slice(port.to_be_bytes().as_ref());

    label.serialize(&mut buf[6..]).map(|size| {
        buf.truncate(6 + size);
        buf
    })
}
