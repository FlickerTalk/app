//! End-to-end tests of circles (2026-09-27): three complete devices talk through an in-memory
//! network. Each test states one promise the app makes about a circle: who gets what, who may
//! change it, and what the router sees. Written before the code that makes them pass.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::{Core, Event, MailboxRejected, Peer, Transport};
use ft_storage::{MessageState, Store};
use tokio::sync::mpsc;

/// A direct link that can be cut, and one mailbox per device, as the router keeps it.
#[derive(Default)]
struct Net {
    direct: AtomicBool,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
    mailboxes: Mutex<HashMap<String, Vec<Vec<u8>>>>,
    /// Devices whose mailbox the router refuses (403).
    rejecting: Mutex<HashSet<String>>,
}

impl Net {
    fn new() -> Arc<Self> {
        let net = Arc::new(Self::default());
        net.direct.store(true, Ordering::SeqCst);
        net
    }

    fn set_direct(&self, up: bool) {
        self.direct.store(up, Ordering::SeqCst);
    }

    /// What the router would hold for a device right now.
    fn mailbox(&self, device: &str) -> Vec<Vec<u8>> {
        self.mailboxes.lock().unwrap().get(device).cloned().unwrap_or_default()
    }

    async fn collect(&self, core: &Core) {
        let blobs = self.mailboxes.lock().unwrap().remove(core.device_id().as_str()).unwrap_or_default();
        for blob in blobs {
            let _ = core.receive(&blob).await;
        }
    }
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

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<()> {
        if self.net.rejecting.lock().unwrap().contains(&to.device_id) {
            return Err(MailboxRejected.into());
        }
        self.net.mailboxes.lock().unwrap().entry(to.device_id.clone()).or_default().push(bytes);
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-hardening-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
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

/// Alice scans Bob; pairing is over when Bob's card is back.
async fn pair(alice: &Core, bob: &Core) {
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact(&link, None).await.expect("alice adds bob");
    let (alice_id, bob_id) = (id(alice), id(bob));
    until("bob knows alice", || async { bob.store().contact(&alice_id).await.unwrap().is_some() }).await;
    // Alice wrote first: she waits in Bob's requests until he says yes (A5). He does.
    bob.accept_contact(&alice_id).await.expect("bob accepts alice");
    until("bob's card came back", || async {
        alice.store().contact(&bob_id).await.unwrap().is_some_and(|contact| contact.introduced)
    })
    .await;
}


fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// A member's copy of the circle: how many are in it, and whether this phone left.
async fn circle_of(core: &Core, circle: &str) -> Option<(usize, bool)> {
    let record = core.store().circle(circle).await.expect("reads")?;
    let members = core.store().circle_members(circle).await.expect("members").len();
    Some((members, record.left))
}

/// What was said in a circle, on one phone, as texts only.
async fn said(core: &Core, circle: &str) -> Vec<String> {
    core.store()
        .circle_messages(circle, 100)
        .await
        .expect("lists")
        .into_iter()
        .filter(|message| message.kind == "text")
        .map(|message| message.body)
        .collect()
}

/// What happened in a circle, on one phone: `kind:body` lines.
async fn happened(core: &Core, circle: &str) -> Vec<String> {
    core.store()
        .circle_messages(circle, 100)
        .await
        .expect("lists")
        .into_iter()
        .filter(|message| message.kind != "text")
        .map(|message| format!("{}:{}", message.kind, message.body))
        .collect()
}

/// Alice knows Bob and Carol; Bob and Carol have never met.
async fn three(net: &Arc<Net>) -> (Arc<Core>, Arc<Core>, Arc<Core>) {
    let (alice, bob, carol) = (device(net, "Alice").await, device(net, "Bob").await, device(net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    (alice, bob, carol)
}

/// Alice makes the circle and everyone has it.
async fn friends(alice: &Core, bob: &Core, carol: &Core) -> String {
    let circle = alice.create_circle("Friends", &[id(bob), id(carol)], None).await.expect("creates");
    until("bob and carol have the circle", || async {
        circle_of(bob, &circle).await == Some((3, false)) && circle_of(carol, &circle).await == Some((3, false))
    })
    .await;
    circle
}

// ---------------------------------------------------------------------------------------------
// The card: whoever is put in a circle meets every member without scanning anyone.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_circle_reaches_everyone_and_members_meet_through_it() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;

    // Bob now knows Carol, but only as someone of the circle: in no list, in no requests.
    let carol_at_bob = bob.store().contact(&id(&carol)).await.expect("reads").expect("bob knows carol");
    assert!(carol_at_bob.via_circle && !carol_at_bob.accepted);
    assert!(bob.store().contacts().await.expect("lists").iter().all(|contact| contact.device_id != id(&carol)));
    assert!(bob.requests().await.expect("requests").is_empty());
    assert!(alice.is_circle_admin(&circle).await.expect("asks"));
    assert!(!bob.is_circle_admin(&circle).await.expect("asks"));
    until("bob sees the circle made", || async { happened(&bob, &circle).await == ["created:Friends"] }).await;
    let names: Vec<String> = bob.circle_members(&circle).await.expect("members").into_iter().map(|member| member.name).collect();
    assert_eq!(names, ["Alice", "Bob", "Carol"], "admins first, then by name, as their own cards say");

    // Carol writes to Bob on her own, outside the circle: then she waits in his requests (A5).
    carol.send_text(&id(&bob), "hi bob, it's carol").await.expect("carol writes");
    until("carol waits in bob's requests", || async {
        bob.requests().await.expect("requests").iter().any(|request| request.contact.device_id == id(&carol))
    })
    .await;
}

// ---------------------------------------------------------------------------------------------
// Texts: once per member, directly or through each mailbox, and the router learns nothing.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_text_reaches_every_member_and_comes_back_from_any() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;

    let sent = alice.send_circle_text(&circle, "dinner on friday?").await.expect("sends");
    until("bob and carol got it", || async {
        said(&bob, &circle).await == ["dinner on friday?"] && said(&carol, &circle).await == ["dinner on friday?"]
    })
    .await;
    until("alice sees it delivered to both", || async {
        alice.store().circle_message(&sent).await.expect("reads").is_some_and(|m| m.state == MessageState::Delivered)
    })
    .await;
    assert!(alice.store().circle_pending(&sent).await.expect("pending").is_empty());

    // Bob answers: Carol, whom he never scanned, gets it through the channel the card opened.
    bob.send_circle_text(&circle, "yes!").await.expect("bob sends");
    until("alice and carol got bob's answer", || async {
        said(&alice, &circle).await == ["dinner on friday?", "yes!"] && said(&carol, &circle).await == ["dinner on friday?", "yes!"]
    })
    .await;
    let from_bob = carol.store().circle_messages(&circle, 10).await.expect("lists").into_iter().find(|m| m.body == "yes!").expect("there");
    assert_eq!(from_bob.sender, id(&bob), "each message says who said it");
    assert!(!from_bob.outgoing);
    assert_eq!(carol.store().circle_unread(&circle).await.expect("unread").len(), 2);
    carol.mark_circle_read(&circle).await.expect("reads");
    assert!(carol.store().circle_unread(&circle).await.expect("unread").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn through_the_mailbox_the_router_sees_one_envelope_per_member_and_no_circle() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;
    net.set_direct(false);

    alice.send_circle_text(&circle, "secret plan").await.expect("sends");
    until("one envelope in each mailbox", || async { !net.mailbox(&id(&bob)).is_empty() && !net.mailbox(&id(&carol)).is_empty() }).await;
    assert_eq!(net.mailbox(&id(&bob)).len(), 1);
    assert_eq!(net.mailbox(&id(&carol)).len(), 1);
    let alice_id = id(&alice);
    for blob in net.mailbox(&id(&bob)).into_iter().chain(net.mailbox(&id(&carol))) {
        assert!(!contains(&blob, circle.as_bytes()), "the circle id is in the clear");
        assert!(!contains(&blob, alice_id.as_bytes()), "the sender is in the clear");
        assert!(!contains(&blob, b"secret"), "the text is in the clear");
        assert!(!contains(&blob, b"Friends"), "the name is in the clear");
    }
    net.collect(&bob).await;
    net.collect(&carol).await;
    until("both read it from their mailbox", || async {
        said(&bob, &circle).await == ["secret plan"] && said(&carol, &circle).await == ["secret plan"]
    })
    .await;
}

// A member the router refuses does not stop the others, and their entry waits like one that
// cannot be reached instead of being tried again on every pass of the queue.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_the_router_refuses_is_tried_again_later_not_on_every_pass() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;
    net.set_direct(false);
    net.rejecting.lock().unwrap().insert(id(&bob));

    let sent = alice.send_circle_text(&circle, "secret plan").await.expect("sends");
    until("carol's envelope is in her mailbox", || async { net.mailbox(&id(&carol)).len() == 1 }).await;
    alice.retry_due().await.expect("retries");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let pending = alice.store().circle_pending(&sent).await.expect("pending");
    let bobs = pending.iter().find(|entry| entry.contact == id(&bob)).expect("bob is still to be reached");
    assert!(bobs.attempts >= 1 && bobs.next_attempt > now, "bob waits for a later try");
    let state = alice.store().circle_message(&sent).await.expect("reads").expect("there").state;
    assert_ne!(state, MessageState::Delivered, "bob never got it");
}

// ---------------------------------------------------------------------------------------------
// Membership: only the admins change the card; leaving and being taken out.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn only_an_admin_changes_the_circle() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let dave = device(&net, "Dave").await;
    pair(&bob, &dave).await;
    let circle = friends(&alice, &bob, &carol).await;

    assert!(bob.invite_to_circle(&circle, &id(&dave)).await.is_err(), "bob is no admin");
    assert!(bob.rename_circle(&circle, "Bob's").await.is_err());
    assert!(bob.remove_from_circle(&circle, &id(&carol)).await.is_err());
    assert!(alice.invite_to_circle(&circle, &id(&dave)).await.is_err(), "alice does not know dave");

    alice.rename_circle(&circle, "Close friends").await.expect("renames");
    until("everyone sees the new name", || async {
        bob.store().circle(&circle).await.expect("reads").is_some_and(|c| c.name == "Close friends")
            && carol.store().circle(&circle).await.expect("reads").is_some_and(|c| c.name == "Close friends")
    })
    .await;
    until("carol sees the renaming", || async { happened(&carol, &circle).await.contains(&"renamed:Close friends".to_owned()) }).await;

    // Alice makes Bob an admin: now he can bring Dave, and Dave meets everyone.
    alice.set_circle_admin(&circle, &id(&bob), true).await.expect("promotes");
    until("bob is an admin", || async { bob.is_circle_admin(&circle).await.expect("asks") }).await;
    bob.invite_to_circle(&circle, &id(&dave)).await.expect("bob invites dave");
    until("four everywhere", || async {
        [&alice, &bob, &carol, &dave].iter().all(|core| futures_ready(circle_of(core, &circle)) == Some((4, false)))
    })
    .await;
    until("alice sees dave join", || async { happened(&alice, &circle).await.contains(&"joined:Dave".to_owned()) }).await;
    assert!(dave.store().contact(&id(&alice)).await.expect("reads").is_some_and(|c| c.via_circle));

    // The last admin cannot step down; an admin can hand over.
    assert!(alice.set_circle_admin(&circle, &id(&alice), false).await.is_ok(), "bob remains");
    until("alice is a plain member", || async { !alice.is_circle_admin(&circle).await.expect("asks") }).await;
    until("bob is the only admin everywhere", || async {
        bob.circle_members(&circle).await.expect("members").iter().filter(|member| member.admin).count() == 1
    })
    .await;
    assert!(bob.set_circle_admin(&circle, &id(&bob), false).await.is_err(), "the last admin stays");
}

fn futures_ready<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_who_leaves_is_out_everywhere_and_keeps_what_was_said() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;
    alice.send_circle_text(&circle, "before").await.expect("sends");
    until("carol has it", || async { said(&carol, &circle).await == ["before"] }).await;

    carol.leave_circle(&circle).await.expect("carol leaves");
    assert_eq!(circle_of(&carol, &circle).await, Some((3, true)), "her copy says she left");
    until("alice and bob see two", || async {
        circle_of(&alice, &circle).await == Some((2, false)) && circle_of(&bob, &circle).await == Some((2, false))
    })
    .await;
    until("bob sees carol leave", || async { happened(&bob, &circle).await.contains(&"left:Carol".to_owned()) }).await;
    // Bob knew Carol through the circle alone: nothing left to know her by.
    until("bob forgot carol", || async { bob.store().contact(&id(&carol)).await.expect("reads").is_none() }).await;
    assert!(alice.store().contact(&id(&carol)).await.expect("reads").is_some(), "alice scanned her: she stays");

    alice.send_circle_text(&circle, "after").await.expect("sends");
    until("bob has it", || async { said(&bob, &circle).await == ["before", "after"] }).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(said(&carol, &circle).await, ["before"], "nothing more reaches her");
    assert!(carol.send_circle_text(&circle, "hello?").await.is_err(), "she cannot write");
    assert!(carol.forget_circle(&circle).await.is_ok());
    assert!(carol.store().circle(&circle).await.expect("reads").is_none());

    // The next change by an admin consolidates her out of the signed card too.
    alice.rename_circle(&circle, "Two").await.expect("renames");
    let card = ft_circles::CircleCard::decode(&alice.store().circle(&circle).await.expect("reads").expect("there").card).expect("decodes");
    assert_eq!(card.members().len(), 2);
    assert!(!card.is_member(&id(&carol)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_taken_out_learns_it_and_is_ignored_from_then_on() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;

    assert!(alice.remove_from_circle(&circle, &id(&alice)).await.is_err(), "leaving is not removing");
    alice.remove_from_circle(&circle, &id(&carol)).await.expect("alice takes carol out");
    until("carol knows she is out", || async { circle_of(&carol, &circle).await == Some((2, true)) }).await;
    until("bob sees two", || async { circle_of(&bob, &circle).await == Some((2, false)) }).await;
    until("bob sees carol taken out", || async { happened(&bob, &circle).await.contains(&"removed:Carol".to_owned()) }).await;
    until("bob forgot carol", || async { bob.store().contact(&id(&carol)).await.expect("reads").is_none() }).await;

    alice.send_circle_text(&circle, "just us").await.expect("sends");
    until("bob has it", || async { said(&bob, &circle).await == ["just us"] }).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(said(&carol, &circle).await.is_empty());
    assert!(carol.send_circle_text(&circle, "still here?").await.is_err());
    assert!(carol.invite_to_circle(&circle, &id(&alice)).await.is_err(), "and cannot change it");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_admins_only_the_others_read() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;

    alice.set_circle_admins_only(&circle, true).await.expect("sets");
    until("bob knows", || async { bob.store().circle(&circle).await.expect("reads").is_some_and(|c| c.admins_only) }).await;
    assert!(bob.send_circle_text(&circle, "may I?").await.is_err());
    alice.send_circle_text(&circle, "news").await.expect("alice may");
    until("bob and carol read it", || async { said(&bob, &circle).await == ["news"] && said(&carol, &circle).await == ["news"] }).await;
}

// Tried on two phones (2026-09-27): with the circle open on screen, "Carol left" only showed
// after leaving the thread and coming back. The thread listens for its messages, so a leave
// has to say its messages changed, not only the members.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_leaving_refreshes_the_thread_of_the_others() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;
    let mut at_bob = bob.events();

    carol.leave_circle(&circle).await.expect("carol leaves");
    let expected = Event::CircleMessagesChanged { circle: circle.clone() };
    let mut told = false;
    for _ in 0..250 {
        while let Ok(event) = at_bob.try_recv() {
            told |= event == expected;
        }
        if told {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(told, "bob's open thread hears of it");
    // The event may come from the circle being set up, before the leave is stored: wait for it.
    until("bob sees carol leave", || async { happened(&bob, &circle).await.contains(&"left:Carol".to_owned()) }).await;
}

// The app sends in the background, so it must be able to ask first: an error from the send
// itself would be lost (tried on two phones, 2026-09-27).
#[tokio::test(flavor = "multi_thread")]
async fn whoever_may_not_write_is_told_before_sending() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = friends(&alice, &bob, &carol).await;
    assert!(bob.circle_writable(&circle).await.is_ok());

    alice.set_circle_admins_only(&circle, true).await.expect("sets");
    until("bob knows", || async { bob.store().circle(&circle).await.expect("reads").is_some_and(|c| c.admins_only) }).await;
    assert!(bob.circle_writable(&circle).await.is_err(), "only the admins write");
    assert!(alice.circle_writable(&circle).await.is_ok());

    carol.leave_circle(&circle).await.expect("carol leaves");
    assert!(carol.circle_writable(&circle).await.is_err(), "she is not in it");
    assert!(alice.circle_writable("no such circle").await.is_err());
}

// A member renews their link (A5): the next revision an admin signs carries their new card, so
// whoever joins later reaches them, instead of getting the card as it was when they joined.
#[tokio::test(flavor = "multi_thread")]
async fn a_newcomer_gets_the_members_renewed_cards() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    let circle = alice.create_circle("Friends", &[id(&bob)], None).await.expect("creates");
    until("bob has it", || async { circle_of(&bob, &circle).await == Some((2, false)) }).await;

    let old = bob.my_card().await.expect("card").route_capability();
    bob.renew_link(None).await.expect("renews");
    let renewed = bob.my_card().await.expect("card").route_capability();
    assert_ne!(old.as_bytes(), renewed.as_bytes());
    until("alice holds bob's new card", || async {
        let card = alice.store().contact(&id(&bob)).await.expect("reads").expect("there").card;
        ft_contacts::ContactCard::decode(&card).expect("decodes").route_capability().as_bytes() == renewed.as_bytes()
    })
    .await;

    alice.invite_to_circle(&circle, &id(&carol)).await.expect("invites carol");
    until("carol knows bob", || async { carol.store().contact(&id(&bob)).await.expect("reads").is_some() }).await;
    let at_carol = carol.store().contact(&id(&bob)).await.expect("reads").expect("there").card;
    assert_eq!(
        ft_contacts::ContactCard::decode(&at_carol).expect("decodes").route_capability().as_bytes(),
        renewed.as_bytes(),
        "carol got bob's card as it is now, not as it was"
    );
}

// The other half: Carol knows Bob through the circle alone. When Bob renews his link, she must
// get his new card too, or what she writes to him goes to a retired link until he writes first.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_known_through_the_circle_gets_the_renewed_card() {
    let net = Net::new();
    let (alice, bob, carol) = three(&net).await;
    friends(&alice, &bob, &carol).await;
    until("carol knows bob", || async { carol.store().contact(&id(&bob)).await.expect("reads").is_some() }).await;

    bob.renew_link(None).await.expect("renews");
    let renewed = bob.my_card().await.expect("card").route_capability();
    until("carol holds bob's new card", || async {
        let card = carol.store().contact(&id(&bob)).await.expect("reads").expect("there").card;
        ft_contacts::ContactCard::decode(&card).expect("decodes").route_capability().as_bytes() == renewed.as_bytes()
    })
    .await;
}

// The admin who signs is a member too, and not a contact of their own phone: the revision
// carries their own card as it is now.
#[tokio::test(flavor = "multi_thread")]
async fn a_revision_carries_the_signers_own_card_as_it_is_now() {
    let net = Net::new();
    let (alice, bob, _carol) = three(&net).await;
    let circle = alice.create_circle("Friends", &[id(&bob)], None).await.expect("creates");
    alice.renew_link(None).await.expect("renews");
    alice.rename_circle(&circle, "Renamed").await.expect("renames");

    let card = ft_circles::CircleCard::decode(&alice.store().circle(&circle).await.expect("reads").expect("there").card).expect("decodes");
    let mine = card.member(&id(&alice)).expect("alice is in it");
    assert_eq!(
        ft_contacts::ContactCard::decode(&mine.card).expect("decodes").route_capability().as_bytes(),
        alice.route_capability().as_bytes()
    );
}

// ---------------------------------------------------------------------------------------------
// Who may put this phone in a circle, and where the circle lives.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_strangers_circle_waits_until_they_are_accepted() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    // Bob scans Alice's link: for Alice he is a stranger in the requests (A5).
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact(&link, None).await.expect("bob adds alice");
    until("alice has bob's request", || async { alice.store().contact(&id(&bob)).await.expect("reads").is_some() }).await;

    let circle = bob.create_circle("Bob's", &[id(&alice)], None).await.expect("bob creates");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(alice.store().circle(&circle).await.expect("reads").is_none(), "a stranger's circle is not taken");

    alice.accept_contact(&id(&bob)).await.expect("alice accepts");
    bob.retry_now().await.expect("retries");
    until("now alice has it", || async { circle_of(&alice, &circle).await == Some((2, false)) }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_circle_made_in_a_hidden_session_belongs_to_it() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    let session = alice.open_session("135790").await.expect("opens").expect("has room");
    for other in [&bob, &carol] {
        let link = other.my_card().await.expect("card").to_link();
        alice.add_contact_in(&link, None, Some(&session)).await.expect("adds in the session");
        let alice_id = id(&alice);
        until("they know alice", || async { other.store().contact(&alice_id).await.expect("reads").is_some() }).await;
        other.accept_contact(&alice_id).await.expect("accepts");
    }
    assert!(alice.create_circle("Hidden", &[id(&bob)], None).await.is_err(), "bob is not in the main list");
    let circle = alice.create_circle("Hidden", &[id(&bob), id(&carol)], Some(&session)).await.expect("creates");
    assert_eq!(alice.circles(Some(&session)).await.expect("lists").len(), 1);
    assert!(alice.circles(None).await.expect("lists").is_empty());
    until("bob and carol have it in their main list", || async {
        bob.circles(None).await.expect("lists").len() == 1 && carol.circles(None).await.expect("lists").len() == 1
    })
    .await;
    // Bob met Carol through a circle of Alice's session: she is a contact of that session. The
    // circle is saved before its members are met, so on a slow machine (CI) this has to wait too.
    let carol_id = id(&carol);
    until("bob has met carol", || async { bob.store().contact(&carol_id).await.expect("reads").is_some() }).await;
    assert_eq!(bob.store().contact(&carol_id).await.expect("reads").expect("there").session, None);
    assert_eq!(alice.store().circle(&circle).await.expect("reads").expect("there").session.as_deref(), Some(session.as_str()));

    // The session goes: the circle goes with it, and Bob's copy is simply frozen without news.
    alice.remove_session(&session).await.expect("removes");
    assert!(alice.store().circle(&circle).await.expect("reads").is_none());
    assert!(alice.store().circle_outbox().await.expect("outbox").is_empty());
}
