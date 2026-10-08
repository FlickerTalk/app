//! What the plan decides once the free year is over (Plan §40–§47). Everything is decided on the
//! phone: the server never learns who pays.
//!
//! Ioan, 2026-10-08: chat, voice notes, files, calls, new conversations, circles and games are free
//! forever. Without the subscription, after the 15 free days, only the premium part is closed: the
//! tools (plugins of kind `tool`, the ones the app carries included) neither open nor install, and
//! no hidden session opens with a PIN. The main list always works.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_billing::Access;
use ft_core::{Core, Entitlements, Event, Peer, Transport, NEEDS_SUBSCRIPTION};
use ft_plugins::{sign_package, CatalogueEntry, Permissions};
use ft_storage::Store;
use tokio::sync::mpsc;
use vodozemac::Ed25519SecretKey;

/// The phone's own clock, in ms, as the core counts it.
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

/// A direct link between the test phones, delivered in order like a DataChannel.
#[derive(Default)]
struct Net {
    direct: AtomicBool,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
}

struct Link {
    net: Arc<Net>,
}

#[async_trait]
impl Transport for Link {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<bool> {
        if !self.net.direct.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let Some(queue) = self.net.queues.lock().unwrap().get(&to.device_id).cloned() else { return Ok(false) };
        Ok(queue.send(bytes).is_ok())
    }

    async fn send_mailbox(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-billing-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

fn net() -> Arc<Net> {
    let net = Arc::new(Net::default());
    net.direct.store(true, Ordering::SeqCst);
    net
}

async fn device(net: &Arc<Net>, name: &str) -> Arc<Core> {
    let core = Core::open(Store::open_in_memory().await.expect("store"), [9; 32], Arc::new(Link { net: net.clone() }))
        .await
        .expect("opens");
    core.set_name(name).await.expect("names");
    core.set_files_dir(scratch(name));
    core.set_plugins_dir(scratch(&format!("{name}-plugins")));
    let id = core.device_id().as_str().to_owned();
    let (queue, mut incoming) = mpsc::unbounded_channel::<Vec<u8>>();
    net.queues.lock().unwrap().insert(id, queue);
    let receiver: Weak<Core> = Arc::downgrade(&core);
    tokio::spawn(async move {
        while let Some(bytes) = incoming.recv().await {
            let Some(core) = receiver.upgrade() else { break };
            let _ = core.receive(&bytes).await;
        }
    });
    core
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting until {what}");
}

fn id(core: &Core) -> String {
    core.device_id().as_str().to_owned()
}

/// Bob scans Alice's card and Alice says yes: neither has written a word yet.
async fn pair(alice: &Core, bob: &Core) {
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact(&link, None).await.expect("bob adds alice");
    let (alice_id, bob_id) = (id(alice), id(bob));
    until("alice knows bob", || async { alice.store().contact(&bob_id).await.unwrap().is_some() }).await;
    alice.accept_contact(&bob_id).await.expect("alice accepts bob");
    until("alice's card came back", || async {
        bob.store().contact(&alice_id).await.unwrap().is_some_and(|contact| contact.introduced)
    })
    .await;
}

const DAY: i64 = 24 * 60 * 60 * 1000;

/// Moves the phone's own clock: the free days started 16 days ago.
async fn the_free_days_are_over(core: &Core) {
    the_free_days_end_in(core, -DAY).await;
}

/// The free days end `ms` from now (in the past when negative).
async fn the_free_days_end_in(core: &Core, ms: i64) {
    let installed = now() - 15 * DAY + ms;
    core.store().set_setting("installed_at", &installed.to_string()).await.expect("sets");
}

/// A card link of a new phone nobody has met.
async fn someone() -> String {
    let net = net();
    let other = device(&net, "Someone").await;
    other.my_card().await.expect("card").to_link()
}

const TOOL: &str = "com.example.code";
const GAME: &str = "game.example.chess";

/// A package as the catalogue signs it: a tool, or a game when `kind` says so.
fn package(id: &str, kind: &str, catalogue: &Ed25519SecretKey) -> Vec<u8> {
    let manifest = format!(
        r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"kind":"{kind}","permissions":{{"live":true}}}}"#
    );
    sign_package(
        &[("module.json".to_owned(), manifest.into_bytes()), ("dist/index.js".to_owned(), b"".to_vec())],
        catalogue,
    )
}

/// The catalogue as the site serves it: the packages, and the entries its signed index lists.
struct Shop {
    files: HashMap<String, Vec<u8>>,
}

#[async_trait]
impl ft_core::Fetch for Shop {
    async fn get(&self, url: &str, _limit: u64) -> anyhow::Result<Vec<u8>> {
        self.files.get(url).cloned().ok_or_else(|| anyhow::anyhow!("{url} is not there"))
    }

    async fn call(&self, _request: &ft_core::WebRequest, _limit: u64) -> anyhow::Result<ft_core::WebAnswer> {
        anyhow::bail!("the catalogue is only ever read")
    }
}

fn listed(packages: &[(&str, &str, &[u8])]) -> (Shop, Vec<CatalogueEntry>) {
    let mut files = HashMap::new();
    let mut entries = Vec::new();
    for (id, kind, package) in packages {
        let url = format!("{}/{id}/1.0.0.ftplugin", ft_core::CATALOGUE_HOME);
        entries.push(CatalogueEntry {
            id: (*id).to_owned(),
            name: "X".to_owned(),
            version: "1.0.0".to_owned(),
            min_core_version: "0.1.0".to_owned(),
            size: package.len() as u64,
            hash: blake3::hash(package).to_hex().to_string(),
            url: url.clone(),
            summary: String::new(),
            kind: if *kind == "game" { ft_plugins::Kind::Game } else { ft_plugins::Kind::Tool },
            locales: Default::default(),
            icon: String::new(),
            image: String::new(),
        });
        files.insert(url, package.to_vec());
    }
    (Shop { files }, entries)
}

fn a_file() -> PathBuf {
    let path = scratch("outgoing").join("a.bin");
    std::fs::write(&path, vec![7u8; 64]).expect("writes");
    path
}

#[tokio::test(flavor = "multi_thread")]
async fn the_free_days_include_the_tools_and_the_sessions() {
    let net = net();
    let core = device(&net, "Alice").await;
    assert!(matches!(core.access().await.expect("reads"), Access::Trial { .. }));

    let catalogue = Ed25519SecretKey::new();
    let tool = package(TOOL, "tool", &catalogue);
    let (shop, entries) = listed(&[(TOOL, "tool", &tool)]);
    core.add_plugin(&entries[0], &shop, &catalogue.public_key()).await.expect("installs a tool");
    core.open_plugin(TOOL).await.expect("opens it");
    assert!(core.open_session("123456").await.expect("opens a session").is_some());
}

// Ioan, 2026-10-08: extra sessions with a PIN are premium. After the free days, without the
// subscription, no PIN opens one, new or not, and the PIN says nothing about which; the main list
// goes on. With the subscription they open again.
#[tokio::test(flavor = "multi_thread")]
async fn after_the_free_days_a_pin_opens_no_session() {
    let net = net();
    let core = device(&net, "Alice").await;
    let session = core.open_session("123456").await.expect("opens").expect("a session");
    core.add_contact_in(&someone().await, None, Some(&session)).await.expect("someone in it");

    // One already open when the free days end keeps working until the user closes it (§108).
    the_free_days_are_over(&core).await;
    assert_eq!(core.open_sessions(), std::slice::from_ref(&session));
    core.add_contact_in(&someone().await, None, Some(&session)).await.expect("still works");
    core.close_session(&session).await.expect("closes");

    for pin in ["123456", "654321"] {
        let refused = core.open_session(pin).await.expect_err("a session needs the subscription");
        assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);
    }
    assert!(core.open_sessions().is_empty());

    core.set_entitlement(now() + 365 * DAY).await.expect("the Store said so");
    assert!(core.open_session("123456").await.expect("opens").is_some());
}

// Ioan, 2026-10-08: without the subscription, everything a chat app does goes on, new things
// included: writing to someone for the first time, a circle, a call and a file.
#[tokio::test(flavor = "multi_thread")]
async fn after_the_year_chat_calls_files_and_circles_stay_free() {
    let net = net();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    the_free_days_are_over(&alice).await;
    assert_eq!(alice.access().await.expect("reads"), Access::Limited);

    // Nothing has been said in this conversation: this starts it.
    alice.send_text(&id(&bob), "hello bob").await.expect("starts a conversation");
    until("bob has it", || async { bob.store().messages(&id(&alice), 10).await.unwrap().len() == 1 }).await;

    alice.send_file(&id(&bob), &a_file(), "a.bin", "application/octet-stream").await.expect("sends a file");

    let call = alice.place_call(&id(&bob), true).await.expect("calls");
    alice.end_call(&call, false).await.expect("hangs up");

    let circle = alice.create_circle("Friends", &[id(&bob)], None).await.expect("makes a circle");
    alice.send_circle_text(&circle, "hi all").await.expect("writes in it");
}

// Games are free forever: one installs from the catalogue, opens, and talks to its twin.
#[tokio::test(flavor = "multi_thread")]
async fn after_the_year_games_stay_free() {
    let net = net();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    the_free_days_are_over(&alice).await;

    let catalogue = Ed25519SecretKey::new();
    let game = package(GAME, "game", &catalogue);
    let (shop, entries) = listed(&[(GAME, "game", &game)]);
    alice.add_plugin(&entries[0], &shop, &catalogue.public_key()).await.expect("installs a game");
    alice.open_plugin(GAME).await.expect("opens it");

    let live = Permissions { live: true, ..Permissions::default() };
    alice.grant_plugin(GAME, live.clone()).await.expect("grants");
    bob.install_plugin(&game, &catalogue.public_key(), live).await.expect("bob has it too");
    let mut at_bob = bob.events();
    assert!(alice.plugin_live_send(GAME, &id(&bob), b"e4".to_vec()).await.expect("plays"));
    let mut heard = false;
    for _ in 0..100 {
        while let Ok(event) = at_bob.try_recv() {
            heard |= matches!(event, Event::PluginEvent { ref plugin, .. } if plugin == GAME);
        }
        if heard {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(heard, "bob's game hears the move");
}

// The tools are what the subscription pays for: without it, after the year, one installed in the
// free year does not open, and a new one does not install, from the catalogue or from the app.
#[tokio::test(flavor = "multi_thread")]
async fn after_the_year_tools_need_the_subscription() {
    let net = net();
    let core = device(&net, "Alice").await;
    let catalogue = Ed25519SecretKey::new();
    let tool = package(TOOL, "tool", &catalogue);
    let other = package("com.example.pdf", "tool", &catalogue);
    let (shop, entries) = listed(&[(TOOL, "tool", &tool), ("com.example.pdf", "tool", &other)]);
    core.add_plugin(&entries[0], &shop, &catalogue.public_key()).await.expect("installs in the free year");

    the_free_days_are_over(&core).await;
    let refused = core.open_plugin(TOOL).await.expect_err("a tool does not open");
    assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);
    let refused = core.add_plugin(&entries[1], &shop, &catalogue.public_key()).await.expect_err("nor installs");
    assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);
    // One the app carries (a seed) is installed straight from its package: the same.
    let refused = core.install_plugin(&other, &catalogue.public_key(), Permissions::default()).await.expect_err("nor a seed");
    assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);
    assert_eq!(core.plugins().await.unwrap().len(), 1, "nothing was installed");

    // An update of one already here is not a new tool: it still comes in, and waits for the
    // subscription to open.
    core.install_plugin(&tool, &catalogue.public_key(), Permissions::default()).await.expect("updates");
}

#[tokio::test(flavor = "multi_thread")]
async fn paying_opens_the_tools_again() {
    let net = net();
    let core = device(&net, "Alice").await;
    let catalogue = Ed25519SecretKey::new();
    let tool = package(TOOL, "tool", &catalogue);
    let (shop, entries) = listed(&[(TOOL, "tool", &tool)]);
    the_free_days_are_over(&core).await;
    assert!(core.add_plugin(&entries[0], &shop, &catalogue.public_key()).await.is_err());

    let a_year_from_now = now() + (365 * 24 * 60 * 60 * 1000);
    core.set_entitlement(a_year_from_now).await.expect("the Store said so");
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: a_year_from_now });
    core.add_plugin(&entries[0], &shop, &catalogue.public_key()).await.expect("installs");
    core.open_plugin(TOOL).await.expect("opens");
}

// 2026-10-07: the Store changes its mind while the app is open (a renewal, an expiry, a parent
// approving an Ask to Buy, a refund): the core keeps it and tells the UI, which reads the plan
// again. The same answer again (every time the app comes back) says nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_word_from_the_store_tells_the_ui_the_plan_changed() {
    let net = net();
    let core = device(&net, "Alice").await;
    the_free_days_are_over(&core).await;
    let mut events = core.events();

    let a_year_from_now = now() + (365 * 24 * 60 * 60 * 1000);
    core.set_entitlement(a_year_from_now).await.expect("the Store said so");
    assert_eq!(events.try_recv(), Ok(Event::PlanChanged));
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: a_year_from_now });

    core.set_entitlement(a_year_from_now).await.expect("the same again");
    assert!(events.try_recv().is_err(), "nothing changed, nothing to say");

    // It ran out (or was refunded): the Store says nothing is paid, and the UI hears it.
    core.set_entitlement(0).await.expect("the Store said so");
    assert_eq!(events.try_recv(), Ok(Event::PlanChanged));
    assert_eq!(core.access().await.expect("reads"), Access::Limited);
}

/// The Store as the bridge asks it (StoreKit's current entitlements, Play's purchases): what it
/// answers now, changed by the test.
struct FakeStore(Mutex<Result<i64, String>>);

impl FakeStore {
    fn answering(answer: Result<i64, String>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(answer)))
    }

    fn now_says(&self, answer: Result<i64, String>) {
        *self.0.lock().unwrap() = answer;
    }
}

#[async_trait]
impl Entitlements for FakeStore {
    async fn until(&self) -> anyhow::Result<i64> {
        self.0.lock().unwrap().clone().map_err(anyhow::Error::msg)
    }
}

/// Waits for the core to say the plan changed.
async fn plan_changed(events: &mut tokio::sync::broadcast::Receiver<Event>) -> bool {
    for _ in 0..150 {
        while let Ok(event) = events.try_recv() {
            if event == Event::PlanChanged {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

// Seen on an iPhone (2026-10-08): the plan changed by itself with the app open and nothing told
// the screens. The core looks again exactly when the free days end, and says so.
#[tokio::test(flavor = "multi_thread")]
async fn the_free_days_ending_with_the_app_open_is_told() {
    let net = net();
    let core = device(&net, "Alice").await;
    the_free_days_end_in(&core, 300).await;
    let mut events = core.events();
    tokio::spawn(Core::watch_plan(Arc::downgrade(&core), FakeStore::answering(Ok(0))));

    assert!(plan_changed(&mut events).await, "the screens hear of it");
    assert_eq!(core.access().await.expect("reads"), Access::Limited);
}

// Seen on an iPhone (2026-10-08): the Store renewed at the end of the period but said nothing to
// the open app, and the core went limited. Now, when the paid date passes, the Store is asked first.
#[tokio::test(flavor = "multi_thread")]
async fn when_the_paid_date_passes_the_store_is_asked_and_a_renewal_counts() {
    let net = net();
    let core = device(&net, "Alice").await;
    the_free_days_are_over(&core).await;
    core.set_entitlement(now() + 300).await.expect("the Store said so");
    let renewed = now() + 365 * DAY;
    let mut events = core.events();
    tokio::spawn(Core::watch_plan(Arc::downgrade(&core), FakeStore::answering(Ok(renewed))));

    assert!(plan_changed(&mut events).await, "the new date is told");
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: renewed });
}

#[tokio::test(flavor = "multi_thread")]
async fn when_the_paid_date_passes_and_the_store_has_nothing_the_phone_is_limited() {
    let net = net();
    let core = device(&net, "Alice").await;
    the_free_days_are_over(&core).await;
    core.set_entitlement(now() + 300).await.expect("the Store said so");
    let mut events = core.events();
    tokio::spawn(Core::watch_plan(Arc::downgrade(&core), FakeStore::answering(Ok(0))));

    assert!(plan_changed(&mut events).await);
    assert_eq!(core.access().await.expect("reads"), Access::Limited);
    // A date the Store gives that is already past is "nothing": no grace for it.
    core.set_entitlement(now() + 365 * DAY).await.expect("sets");
    core.set_entitlement(now() - 1).await.expect("sets");
    assert_eq!(core.access().await.expect("reads"), Access::Limited);
}

// A Store that cannot be reached is not a "nothing": the paid date keeps counting for the grace,
// and the app asks again when it comes back to the screen.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_that_does_not_answer_leaves_the_grace_and_is_asked_again_on_return() {
    let net = net();
    let core = device(&net, "Alice").await;
    the_free_days_are_over(&core).await;
    let paid = now() + 300;
    core.set_entitlement(paid).await.expect("the Store said so");
    let store = FakeStore::answering(Err("store_unavailable".to_owned()));
    let mut events = core.events();
    tokio::spawn(Core::watch_plan(Arc::downgrade(&core), store.clone()));

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: paid }, "the grace");
    assert!(!plan_changed(&mut events).await, "nothing changed");

    store.now_says(Ok(0));
    core.look_at_plan_again();
    assert!(plan_changed(&mut events).await);
    assert_eq!(core.access().await.expect("reads"), Access::Limited);
}
