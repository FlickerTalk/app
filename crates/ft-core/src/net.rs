//! The real network (Plan §13–19, §106 M2): WebRTC data channels between contacts, signalled
//! through the router, with the recipient's mailbox as the fallback.
//!
//! - Sending: an open data channel is reused; otherwise an offer, encrypted with Olm for the
//!   contact, goes through the router. A first contact adds our Contact Card to the offer so the
//!   other side can read it without the mailbox. If the contact is not connected to the router, or
//!   the channel does not open within `CONNECT_WAIT`, the core falls back to the mailbox.
//! - Receiving: offers from contacts are answered; answers complete our pending offers; each open
//!   data channel feeds the core. The router's mail notice (and every reconnection) triggers a
//!   collection of the mailbox and a retry of the outbox.
//!
//! The signalling is authenticated end to end by Olm, so the DTLS fingerprint in each description
//! is bound to the contact's identity: the router cannot place itself in the middle.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use ft_protocol::{Body, Signal, SignalKind, PROTOCOL_VERSION};
use ft_push::{RouterClient, RouterEvent, TurnGrant};
use ft_webrtc::{Inbox, Role, Session, SessionConfig, Signal as Description, TurnServer};
use tokio::sync::{mpsc, Mutex};

use crate::{Core, Event, Peer, Transport};

/// A transfer quiet for this long when a connection opens is asked for again over it.
const RESUME_QUIET: Duration = Duration::from_secs(2);

/// How long to wait for a data channel before falling back to the mailbox.
pub const CONNECT_WAIT: Duration = Duration::from_secs(12);

/// The router, as the network uses it (ft-push's `RouterClient` in the app, a fake in tests).
#[async_trait]
pub trait Relay: Send + Sync {
    /// `true` if the recipient is connected to the router and got the signal.
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool>;
    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()>;
    /// This device's mail: (id, blob), oldest first.
    async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>>;
    async fn acknowledge(&self, id: &str) -> Result<()>;
}

#[async_trait]
impl Relay for RouterClient {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        RouterClient::signal(self, to, capability, bytes).await
    }

    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()> {
        RouterClient::deposit(self, to, capability, blob).await
    }

    async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>> {
        Ok(RouterClient::collect(self).await?.into_iter().map(|mail| (mail.id, mail.blob)).collect())
    }

    async fn acknowledge(&self, id: &str) -> Result<()> {
        RouterClient::acknowledge(self, id).await
    }
}

/// An open data channel with a contact.
struct Link {
    id: u64,
    session: Session,
}

pub struct Network {
    relay: Arc<dyn Relay>,
    /// Where to listen; STUN and TURN come from the router's welcome.
    base: SessionConfig,
    ice: std::sync::Mutex<(Vec<String>, Option<TurnGrant>)>,
    core: OnceLock<Weak<Core>>,
    this: OnceLock<Weak<Network>>,
    links: Mutex<HashMap<String, Link>>,
    /// Our offers waiting for their answer, by signalling session id.
    pending: Mutex<HashMap<String, Session>>,
    /// One connection attempt at a time per contact.
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// One mailbox collection at a time: a blob must not be processed twice.
    collecting: Mutex<()>,
    next_link: AtomicU64,
}

impl Network {
    pub fn new(relay: Arc<dyn Relay>, base: SessionConfig) -> Arc<Self> {
        let network = Arc::new(Self {
            relay,
            base,
            ice: std::sync::Mutex::default(),
            core: OnceLock::new(),
            this: OnceLock::new(),
            links: Mutex::default(),
            pending: Mutex::default(),
            gates: Mutex::default(),
            collecting: Mutex::default(),
            next_link: AtomicU64::new(0),
        });
        let _ = network.this.set(Arc::downgrade(&network));
        network
    }

    /// The core is created with the network as its transport, then attached here.
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// Feeds the router's events until the channel closes (each is handled in the background).
    pub fn listen(self: &Arc<Self>, mut events: mpsc::Receiver<RouterEvent>) {
        let network = self.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                network.handle(event).await;
            }
        });
    }

    /// Returns at once: the work runs in the background, so that an answer arriving meanwhile
    /// is never stuck behind the mail whose processing is waiting for it.
    pub async fn handle(&self, event: RouterEvent) {
        let Some(network) = self.this.get().and_then(Weak::upgrade) else { return };
        match event {
            RouterEvent::Connected { stun, turn } => {
                *self.ice.lock().expect("ice poisoned") = (stun, turn);
                tokio::spawn(async move {
                    network.collect_mail().await;
                    if let Ok(core) = network.core() {
                        let _ = core.retry_now().await;
                    }
                });
            }
            RouterEvent::Mail => {
                tokio::spawn(async move { network.collect_mail().await });
            }
            RouterEvent::Signal(bytes) => {
                tokio::spawn(async move {
                    let _ = network.on_signal(&bytes).await;
                });
            }
            RouterEvent::Disconnected => {}
        }
    }

    fn core(&self) -> Result<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade).ok_or_else(|| anyhow!("the core is gone"))
    }

    /// The router's STUN servers and TURN user, from its last welcome.
    pub fn ice(&self) -> (Vec<String>, Option<TurnGrant>) {
        self.ice.lock().expect("ice poisoned").clone()
    }

    fn session_config(&self) -> SessionConfig {
        let (stun, turn) = self.ice.lock().expect("ice poisoned").clone();
        ice_config(&self.base, stun, turn)
    }

    async fn collect_mail(&self) {
        let _one_at_a_time = self.collecting.lock().await;
        let (Ok(core), Ok(mail)) = (self.core(), self.relay.collect().await) else { return };
        for (id, blob) in mail {
            // A blob that cannot be read is dropped too: it would never become readable.
            let _ = core.receive(&blob).await;
            let _ = self.relay.acknowledge(&id).await;
        }
    }

    async fn open_link(&self, contact: &str) -> Option<Session> {
        let mut links = self.links.lock().await;
        match links.get(contact) {
            Some(link) if link.session.is_open() => Some(link.session.clone()),
            Some(_) => {
                links.remove(contact);
                None
            }
            None => None,
        }
    }

    /// Whether a direct connection with the contact is open now.
    pub async fn is_connected(&self, contact: &str) -> bool {
        self.links.lock().await.get(contact).is_some_and(|link| link.session.is_open())
    }

    /// The contacts with a direct connection open now.
    pub async fn connected(&self) -> Vec<String> {
        let links = self.links.lock().await;
        let mut contacts: Vec<String> =
            links.iter().filter(|(_, link)| link.session.is_open()).map(|(contact, _)| contact.clone()).collect();
        contacts.sort();
        contacts
    }

    /// Closes the direct connection with the contact, if any (the other side sees it close too).
    pub async fn disconnect(&self, contact: &str) {
        let link = self.links.lock().await.remove(contact);
        if let Some(link) = link {
            let _ = link.session.close().await;
            self.announce(contact);
        }
    }

    fn announce(&self, contact: &str) {
        if let Ok(core) = self.core() {
            let _ = core.events.send(Event::ConnectionChanged { contact: contact.to_owned() });
        }
    }

    async fn gate(&self, contact: &str) -> Arc<Mutex<()>> {
        self.gates.lock().await.entry(contact.to_owned()).or_default().clone()
    }

    /// Opens a data channel with the contact, or returns `None` if it cannot be reached now.
    async fn connect(&self, peer: &Peer) -> Result<Option<Session>> {
        let core = self.core()?;
        let (signals, mut descriptions) = mpsc::channel(8);
        let (session, inbox) = Session::start(self.session_config(), Role::Caller, signals).await?;
        session.invite().await?;
        let Some(Description::Sdp(sdp)) = descriptions.recv().await else {
            bail!("no offer was produced");
        };

        let introduced = core.store().contact(&peer.device_id).await?.is_some_and(|contact| contact.introduced);
        let card = if introduced { None } else { Some(core.my_card().await?.encode()) };
        let sealed = core.seal_signal(&peer.device_id, Body::Offer { sdp, card }).await?;
        let session_id = uuid_like();
        let signal = Signal {
            version: PROTOCOL_VERSION,
            kind: SignalKind::Offer,
            session: session_id.clone(),
            from: core.device_id().to_string(),
            to: peer.device_id.clone(),
            sealed,
        };

        self.pending.lock().await.insert(session_id.clone(), session.clone());
        let delivered = self.relay.signal(&peer.device_id, peer.capability.as_bytes(), signal.encode()).await;
        let opened = match delivered {
            Ok(true) => tokio::time::timeout(CONNECT_WAIT, session.wait_open()).await.is_ok_and(|open| open.is_ok()),
            _ => false,
        };
        self.pending.lock().await.remove(&session_id);

        if opened {
            self.adopt(&peer.device_id, session.clone(), inbox).await;
            Ok(Some(session))
        } else {
            let _ = session.close().await;
            Ok(None)
        }
    }

    async fn on_signal(&self, bytes: &[u8]) -> Result<()> {
        let signal = Signal::decode(bytes)?;
        let core = self.core()?;
        let Some((from, body)) = core.open_signal(&signal.sealed).await? else {
            return Ok(());
        };
        if from != signal.from {
            bail!("the signal is not from who it claims");
        }
        match (signal.kind, body) {
            (SignalKind::Offer, Body::Offer { sdp, .. }) => self.answer(&core, &from, signal.session, sdp).await,
            (SignalKind::Answer, Body::Answer { sdp }) => {
                let pending = self.pending.lock().await.get(&signal.session).cloned();
                match pending {
                    Some(session) => session.handle_signal(Description::Sdp(sdp)).await,
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    async fn answer(&self, core: &Core, from: &str, session_id: String, offer: String) -> Result<()> {
        let (signals, mut descriptions) = mpsc::channel(8);
        let (session, inbox) = Session::start(self.session_config(), Role::Callee, signals).await?;
        session.handle_signal(Description::Sdp(offer)).await?;
        let Some(Description::Sdp(sdp)) = descriptions.recv().await else {
            bail!("no answer was produced");
        };
        let sealed = core.seal_signal(from, Body::Answer { sdp }).await?;
        let peer = core.peer(from).await?;
        let signal = Signal {
            version: PROTOCOL_VERSION,
            kind: SignalKind::Answer,
            session: session_id,
            from: core.device_id().to_string(),
            to: from.to_owned(),
            sealed,
        };
        self.relay.signal(from, peer.capability.as_bytes(), signal.encode()).await?;

        let network = self.this.get().and_then(Weak::upgrade).ok_or_else(|| anyhow!("the network is gone"))?;
        let contact = from.to_owned();
        tokio::spawn(async move {
            if tokio::time::timeout(CONNECT_WAIT, session.wait_open()).await.is_ok_and(|open| open.is_ok()) {
                network.adopt(&contact, session, inbox).await;
            } else {
                let _ = session.close().await;
            }
        });
        Ok(())
    }

    /// Keeps the open channel for the contact and feeds what arrives on it to the core.
    async fn adopt(&self, contact: &str, session: Session, mut inbox: Inbox) {
        let id = self.next_link.fetch_add(1, Ordering::Relaxed);
        self.links.lock().await.insert(contact.to_owned(), Link { id, session });
        self.announce(contact);
        // File transfers stopped by the last connection go on over this one (§63).
        if let Ok(core) = self.core() {
            let from = contact.to_owned();
            tokio::spawn(async move {
                let _ = core.resume_files_from(&from, RESUME_QUIET).await;
            });
        }

        let (Some(core), Some(network)) = (self.core.get().cloned(), self.this.get().cloned()) else { return };
        let contact = contact.to_owned();
        tokio::spawn(async move {
            while let Some(bytes) = inbox.next().await {
                let Some(core) = core.upgrade() else { break };
                let _ = core.receive(&bytes).await;
            }
            if let Some(network) = network.upgrade() {
                let removed = {
                    let mut links = network.links.lock().await;
                    links.get(&contact).is_some_and(|link| link.id == id) && links.remove(&contact).is_some()
                };
                if removed {
                    network.announce(&contact);
                }
            }
        });
    }
}

#[async_trait]
impl Transport for Network {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        if let Some(session) = self.open_link(&to.device_id).await {
            if session.send_bytes(&bytes).await.is_ok() {
                return Ok(true);
            }
        }
        let gate = self.gate(&to.device_id).await;
        let _one_at_a_time = gate.lock().await;
        // Another send may have connected while this one waited.
        let session = match self.open_link(&to.device_id).await {
            Some(session) => session,
            None => match self.connect(to).await? {
                Some(session) => session,
                None => return Ok(false),
            },
        };
        Ok(session.send_bytes(&bytes).await.is_ok())
    }

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> Result<()> {
        self.relay.deposit(&to.device_id, to.capability.as_bytes(), bytes).await
    }

    async fn disconnect(&self, device_id: &str) {
        Network::disconnect(self, device_id).await;
    }
}

/// The listening addresses of `base` with the router's STUN and TURN.
pub fn ice_config(base: &SessionConfig, stun: Vec<String>, turn: Option<TurnGrant>) -> SessionConfig {
    let mut config = base.clone();
    if !stun.is_empty() {
        config.stun_servers = stun;
    }
    if let Some(grant) = turn {
        config.turn_servers = grant
            .urls
            .into_iter()
            .map(|url| TurnServer { url, username: grant.username.clone(), credential: grant.credential.clone() })
            .collect();
    }
    config
}

/// A random id for one signalling exchange.
fn uuid_like() -> String {
    ft_protocol::MessageId::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_router_supplies_the_stun_and_turn_servers() {
        let base = SessionConfig { relay_only: true, ..SessionConfig::offline() };
        let grant = TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: "u".to_owned(), credential: "c".to_owned() };
        let config = ice_config(&base, vec!["stun:t:3478".to_owned()], Some(grant));
        assert_eq!(config.stun_servers, ["stun:t:3478"]);
        assert_eq!(config.turn_servers[0].url, "turn:t:3478");
        assert_eq!(config.turn_servers[0].username, "u");
        assert!(config.relay_only, "the listening setup is kept");
        assert_eq!(config.bind, base.bind);
    }

    #[test]
    fn without_a_welcome_the_base_configuration_stays() {
        let base = SessionConfig::with_stun(["stun:fallback:19302".to_owned()]);
        assert_eq!(ice_config(&base, vec![], None).stun_servers, ["stun:fallback:19302"]);
    }
}
