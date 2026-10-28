//! A TURN server that only learns who asks (2026-10-28): it answers the first Allocate of each
//! gathering with a 401 and a nonce, as coturn does, and writes down the USERNAME of the signed
//! Allocate that follows. It never allocates, so its candidates never come: the gathering ends at
//! its limit. Nothing leaves the machine.

use std::sync::{Arc, Mutex};

use ft_push::TurnGrant;

/// A STUN Allocate request, and the error response to it.
const ALLOCATE: [u8; 2] = [0x00, 0x03];
const ALLOCATE_ERROR: [u8; 2] = [0x01, 0x13];
const USERNAME: u16 = 0x0006;
const ERROR_CODE: u16 = 0x0009;
const REALM: u16 = 0x0014;
const NONCE: u16 = 0x0015;

pub struct TurnSpy {
    port: u16,
    users: Arc<Mutex<Vec<String>>>,
}

impl TurnSpy {
    pub async fn start() -> Self {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let users = Arc::new(Mutex::new(Vec::new()));
        let seen = users.clone();
        tokio::spawn(async move {
            let mut buffer = [0u8; 1500];
            while let Ok((length, from)) = socket.recv_from(&mut buffer).await {
                let request = &buffer[..length];
                if length < 20 || request[..2] != ALLOCATE {
                    continue;
                }
                match attribute(request, USERNAME) {
                    Some(user) => seen.lock().unwrap().push(String::from_utf8_lossy(user).into_owned()),
                    None => {
                        let _ = socket.send_to(&unauthorized(request), from).await;
                    }
                }
            }
        });
        Self { port, users }
    }

    /// A TURN user on this server, as the router hands them out.
    pub fn grant(&self, username: &str) -> TurnGrant {
        TurnGrant { urls: vec![format!("turn:127.0.0.1:{}?transport=udp", self.port)], username: username.to_owned(), credential: "c".to_owned() }
    }

    /// The users that signed an Allocate here, in order.
    pub fn users(&self) -> Vec<String> {
        self.users.lock().unwrap().clone()
    }
}

/// The value of the first attribute `kind` of a STUN message.
fn attribute(message: &[u8], kind: u16) -> Option<&[u8]> {
    let mut at = 20;
    while at + 4 <= message.len() {
        let found = u16::from_be_bytes([message[at], message[at + 1]]);
        let length = usize::from(u16::from_be_bytes([message[at + 2], message[at + 3]]));
        let value = message.get(at + 4..at + 4 + length)?;
        if found == kind {
            return Some(value);
        }
        at += 4 + length.div_ceil(4) * 4;
    }
    None
}

/// coturn's answer to an Allocate without credentials: 401, with its realm and a nonce.
fn unauthorized(request: &[u8]) -> Vec<u8> {
    let mut attributes = Vec::new();
    for (kind, value) in [(ERROR_CODE, &[0, 0, 4, 1][..]), (REALM, b"ft"), (NONCE, b"nonce")] {
        attributes.extend_from_slice(&kind.to_be_bytes());
        attributes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        attributes.extend_from_slice(value);
        attributes.resize(attributes.len().div_ceil(4) * 4, 0);
    }
    let mut response = ALLOCATE_ERROR.to_vec();
    response.extend_from_slice(&(attributes.len() as u16).to_be_bytes());
    // The magic cookie and the request's transaction id.
    response.extend_from_slice(&request[4..20]);
    response.extend_from_slice(&attributes);
    response
}
