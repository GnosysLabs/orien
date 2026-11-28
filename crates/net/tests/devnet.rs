//! Real validators talking over real sockets on this machine.

use orien_crypto::keytree::KeyTree;
use orien_crypto::{Hash32, Kdf, ots, prf};
use orien_ledger::{
    AccountId, Allocation, Genesis, Kind, Order, Reject, SPARE_HEIGHT, Signature, SignedOrder, Signer, Validator,
    ValidatorInfo, address, sign_stake_proof,
};
use orien_net::client::{Client, Done, Final, Head, TransferError, rpc};
use orien_net::keys::{ValidatorKey, Wallet};
use orien_net::node::{Admission, Limits, Node, NodeConfig};
use orien_net::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

const START: u64 = 1000;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

struct Options {
    validators: usize,
    window: Duration,
    sync: Duration,
    /// Extra funded wallets, named w0, w1, ...
    crowd: usize,
    /// Extra funded accounts that have no wallet file.
    extra: Vec<Allocation>,
    height: u8,
    durable: bool,
    /// Seconds from one checkpoint to the next.
    epoch: u64,
    /// The unit of the waits used while closing an epoch.
    pace: Duration,
    key_reserve: u32,
    min_stake: u64,
    /// Each launch seat's weight.
    seat: u64,
    /// The most coins released for work: none unless a test asks.
    cap: u64,
    first_target: u64,
    /// When mining moves to RandomX, if it does.
    randomx: Option<orien_ledger::RandomXFrom>,
    /// From this epoch on, the launch seats are reached by the name
    /// "localhost" rather than at 127.0.0.1.
    seat_names_from: Option<u64>,
    limits: Limits,
    /// How many work shares an epoch aims for.
    shares: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            validators: 4,
            window: Duration::from_millis(2),
            sync: Duration::from_millis(100),
            crowd: 0,
            extra: Vec::new(),
            height: 9,
            durable: false,
            epoch: 3600,
            pace: Duration::from_millis(60),
            key_reserve: 128,
            min_stake: 10_000,
            seat: 1,
            cap: 0,
            first_target: u64::MAX,
            randomx: None,
            seat_names_from: None,
            limits: Limits::default(),
            shares: 1_000,
        }
    }
}

/// A network with alice and bob funded and carol empty.
struct TestNet {
    dir: PathBuf,
    genesis: Genesis,
    network: Hash32,
    keys: Vec<ValidatorKey>,
    nodes: Vec<Option<Node>>,
    options: Options,
    /// Each validator's own listening address, whatever the genesis file
    /// names.
    direct: Vec<String>,
    /// With `seat_names_from`, the genesis addresses are these stand-ins,
    /// which pass connections on to the validators, so that a test can take
    /// the old addresses away as if their server had gone.
    proxies: Vec<Proxy>,
}

/// Passes every connection it receives on to `to`, until stopped.
struct Proxy {
    address: String,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    open: Arc<std::sync::Mutex<Vec<std::net::TcpStream>>>,
}

impl Proxy {
    fn start(to: String) -> Proxy {
        use std::sync::atomic::Ordering;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let open = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (stop, kept) = (stopping.clone(), open.clone());
        thread::spawn(move || {
            for incoming in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(incoming) = incoming else { continue };
                let Ok(outgoing) = std::net::TcpStream::connect(&to) else { continue };
                let _ = (incoming.set_nodelay(true), outgoing.set_nodelay(true));
                {
                    let mut kept = kept.lock().unwrap();
                    kept.push(incoming.try_clone().unwrap());
                    kept.push(outgoing.try_clone().unwrap());
                }
                let pipe = |mut from: std::net::TcpStream, mut to: std::net::TcpStream| {
                    thread::spawn(move || {
                        let _ = std::io::copy(&mut from, &mut to);
                        let _ = to.shutdown(std::net::Shutdown::Both);
                        let _ = from.shutdown(std::net::Shutdown::Both);
                    });
                };
                pipe(incoming.try_clone().unwrap(), outgoing.try_clone().unwrap());
                pipe(outgoing, incoming);
            }
        });
        Proxy { address, stopping, open }
    }

    /// Takes the address away: nothing answers there any more.
    fn stop(&self) {
        self.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
        // The accept loop notices the flag when a connection arrives.
        let _ = std::net::TcpStream::connect(&self.address);
        for stream in self.open.lock().unwrap().drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl TestNet {
    fn start(name: &str) -> TestNet {
        TestNet::start_with(name, Options::default())
    }

    fn start_with(name: &str, options: Options) -> TestNet {
        let dir = std::env::temp_dir().join(format!("orien-net-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut allocations = Vec::new();
        let mut funded = vec!["alice".to_string(), "bob".to_string()];
        funded.extend((0..options.crowd).map(|i| format!("w{i}")));
        for name in &funded {
            let wallet = Wallet::create(&dir.join(format!("{name}.wallet"))).unwrap();
            allocations.push(Allocation { account: wallet.address(), amount: START });
        }
        Wallet::create(&dir.join("carol.wallet")).unwrap();
        allocations.extend(options.extra.iter().cloned());

        let listeners: Vec<TcpListener> =
            (0..options.validators).map(|_| TcpListener::bind("127.0.0.1:0").unwrap()).collect();
        let keys: Vec<ValidatorKey> = (0..options.validators)
            .map(|i| ValidatorKey { seed: [i as u8 + 1; 32], height: options.height })
            .collect();
        let direct: Vec<String> = listeners.iter().map(|listener| listener.local_addr().unwrap().to_string()).collect();
        let proxies: Vec<Proxy> = match options.seat_names_from {
            Some(_) => direct.iter().map(|to| Proxy::start(to.clone())).collect(),
            None => Vec::new(),
        };
        let genesis = Genesis {
            network: name.into(),
            validators: keys
                .iter()
                .zip(&direct)
                .enumerate()
                .map(|(i, (key, direct))| ValidatorInfo {
                    name: format!("v{}", i + 1),
                    root: KeyTree::generate(key.seed, key.height).root(),
                    height: key.height,
                    stake: options.seat,
                    address: proxies.get(i).map_or_else(|| direct.clone(), |proxy| proxy.address.clone()),
                })
                .collect(),
            allocations,
            epoch_seconds: options.epoch,
            epoch_payments: 50_000,
            start_time: now(),
            min_stake: options.min_stake,
            max_validators: 33,
            unstake_epochs: 168,
            fee: orien_ledger::Fees::NONE,
            cap: options.cap,
            shares_per_epoch: options.shares,
            first_target: options.first_target,
            bitcoin: None,
            randomx: options.randomx,
            seat_addresses: options.seat_names_from.map(|from| orien_ledger::SeatAddresses {
                from,
                addresses: listeners
                    .iter()
                    .map(|listener| format!("localhost:{}", listener.local_addr().unwrap().port()))
                    .collect(),
            }),
        };
        let network = genesis.id();
        let mut net = TestNet { dir, genesis, network, keys, nodes: Vec::new(), options, direct, proxies };
        for (i, listener) in listeners.into_iter().enumerate() {
            let node = Node::start(net.config(i), Some(listener)).unwrap();
            net.nodes.push(Some(node));
        }
        net
    }

    fn config(&self, i: usize) -> NodeConfig {
        let data = self.dir.join(format!("data-{i}"));
        let mut config = NodeConfig::new(self.genesis.clone(), self.keys[i].clone(), data);
        config.batch_window = self.options.window;
        config.sync_interval = self.options.sync;
        config.durable = self.options.durable;
        config.close_pace = self.options.pace;
        config.key_reserve = self.options.key_reserve;
        config.limits = self.options.limits;
        config
    }

    fn stop(&mut self, i: usize) {
        self.nodes[i].take().unwrap().stop();
    }

    fn restart(&mut self, i: usize) {
        let listener = TcpListener::bind(&self.direct[i]).unwrap();
        self.nodes[i] = Some(Node::start(self.config(i), Some(listener)).unwrap());
    }

    fn client(&self) -> Client {
        Client::new(&self.genesis)
    }

    fn wallet(&self, name: &str) -> Wallet {
        Wallet::load(&self.dir.join(format!("{name}.wallet"))).unwrap()
    }

    fn address(&self, name: &str) -> AccountId {
        self.wallet(name).address()
    }

    fn ask(&self, validator: usize, request: &Request) -> Response {
        rpc(&self.direct[validator], request, Duration::from_secs(5)).unwrap()
    }

    fn view(&self, validator: usize, id: &AccountId) -> AccountView {
        match self.ask(validator, &Request::Account(*id)) {
            Response::Account(Some(view)) => view,
            Response::Account(None) => AccountView {
                balance: 0,
                seq: 0,
                signer: Signer::First,
                spare: Hash32::default(),
                lock: None,
            },
            other => panic!("unexpected answer: {other:?}"),
        }
    }

    fn info(&self, validator: usize) -> NodeInfo {
        match self.ask(validator, &Request::Info) {
            Response::Info(info) => info,
            other => panic!("unexpected answer: {other:?}"),
        }
    }

    /// Pays the way the wallet does: read the account, sign, transfer, confirm.
    fn try_pay(&self, from: &str, to: AccountId, amount: u64) -> Result<Final, TransferError> {
        let mut wallet = self.wallet(from);
        let client = self.client();
        let status = client.account(&wallet.address(), wallet.next_seq());
        let signed = wallet.sign_order(status.seq, to, amount, status.signer.as_ref(), &self.network).unwrap();
        let receipt = client.transfer(&signed)?;
        wallet.confirm(status.seq).unwrap();
        Ok(receipt)
    }

    fn pay(&self, from: &str, to: AccountId, amount: u64) -> Final {
        self.try_pay(from, to, amount).unwrap()
    }

    /// Pays the way the wallet does on a network whose epochs are closing:
    /// if a checkpoint gets in the way, waits for it and carries on.
    /// Stakes `amount` from wallet `from` behind validator `validator`, and
    /// waits until the stake is final.
    fn stake(&self, from: &str, validator: usize, amount: u64) {
        let address = self.genesis.validators[validator].address.clone();
        self.stake_for(from, &self.keys[validator], address, amount);
    }

    /// Stakes behind the validator with this key, reached at `address`.
    fn stake_for(&self, from: &str, key: &ValidatorKey, address: String, amount: u64) {
        let mut wallet = self.wallet(from);
        let tree = KeyTree::generate(key.seed, key.height);
        let proof = sign_stake_proof(&tree, &self.network, &wallet.address(), &address);
        let kind = Kind::Stake { validator: Validator { root: tree.root(), height: key.height, address }, proof };
        let client = self.client();
        client.follow();
        let status = client.account(&wallet.address(), wallet.next_seq());
        let seq = status.seq;
        let signed =
            wallet.sign(kind, seq, wallet.address(), amount, status.signer.as_ref(), &self.network).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while let Err(error) = client.pay(&signed, Duration::from_secs(10)) {
            if client.account(&wallet.address(), seq).seq > seq {
                break;
            }
            assert!(Instant::now() < deadline, "the stake never went through: {error}");
            thread::sleep(Duration::from_millis(100));
        }
        wallet.confirm(seq).unwrap();
    }

    fn pay_through(&self, from: &str, to: AccountId, amount: u64) -> Done {
        self.pay_through_with(&self.client(), from, to, amount)
    }

    /// Pays as `pay_through` does, through `client`.
    fn pay_through_with(&self, client: &Client, from: &str, to: AccountId, amount: u64) -> Done {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut wallet = self.wallet(from);
        let status = client.account(&wallet.address(), wallet.next_seq());
        let seq = status.seq;
        let signed = wallet.sign_order(seq, to, amount, status.signer.as_ref(), &self.network).unwrap();
        loop {
            match client.pay(&signed, Duration::from_secs(20)) {
                Ok(done) => {
                    wallet.confirm(seq).unwrap();
                    return done;
                }
                Err(error) => {
                    // It can have gone through all the same: enough
                    // validators recorded it, and too few said so in time.
                    // The wallet must not then pay a second time.
                    if client.account(&wallet.address(), seq).seq > seq {
                        wallet.confirm(seq).unwrap();
                        return Done::InCheckpoint(client.epoch());
                    }
                    assert!(Instant::now() < deadline, "payment never went through: {error}");
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn running(&self) -> Vec<usize> {
        (0..self.nodes.len()).filter(|v| self.nodes[*v].is_some()).collect()
    }

    /// Waits until every running validator has reached `epoch`.
    fn wait_for_epoch(&self, epoch: u64) {
        wait_until(&format!("every validator reaches epoch {epoch}"), || {
            self.running().iter().all(|v| self.info(*v).epoch >= epoch)
        });
    }

    /// Waits until every running validator is in the same epoch, between
    /// checkpoints, holding the same accounts. Returns that epoch.
    fn wait_until_level(&self) -> u64 {
        let mut epoch = 0;
        wait_until("every validator is in the same epoch with the same accounts", || {
            let infos: Vec<NodeInfo> = self.running().iter().map(|v| self.info(*v)).collect();
            let roots: Vec<Response> = self.running().iter().map(|v| self.ask(*v, &Request::Root)).collect();
            epoch = infos[0].epoch;
            let same_root = roots.iter().all(|root| match (root, &roots[0]) {
                (Response::Root(one), Response::Root(first)) => one == first,
                _ => false,
            });
            same_root && infos.iter().all(|info| info.epoch == epoch && !info.closing)
        });
        epoch
    }

    /// A network that takes a checkpoint every `seconds` seconds.
    fn with_epochs(name: &str, seconds: u64) -> TestNet {
        TestNet::start_with(name, Options { epoch: seconds, ..Options::default() })
    }

    /// Signs an order directly with a wallet's key, skipping the wallet's
    /// guard against using a key twice. This is what a cheater does.
    fn raw_order(&self, from: &str, seq: u64, to: AccountId, amount: u64) -> SignedOrder {
        let wallet = self.wallet(from);
        let order = wallet.order(seq, to, amount);
        let digest = order.digest(&self.network);
        SignedOrder { signature: Signature::Key(wallet.key(seq).sign(digest.as_bytes())), order }
    }
}

impl Drop for TestNet {
    fn drop(&mut self) {
        for node in self.nodes.iter_mut() {
            node.take();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn wait_until(what: &str, done: impl FnMut() -> bool) {
    wait_up_to(30, what, done)
}

fn wait_up_to(seconds: u64, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_payment_is_final_on_every_validator() {
    let net = TestNet::start("pay");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let receipt = net.pay("alice", bob, 250);

    assert_eq!(receipt.votes, 4);
    assert_eq!(receipt.confirmations, 4);
    assert_eq!(receipt.receipt.certificate.votes.len(), 3, "three of four votes are enough");
    assert_eq!(receipt.receipt.confirmations.len(), 3, "and three of four confirmations");
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 750);
        assert_eq!(net.view(v, &alice).seq, 1);
        assert_eq!(net.view(v, &bob).balance, 1250);
    }
    // The receipt stands on its own: genesis plus hashing verifies it.
    assert!(net.client().committee().verify_receipt(&receipt.receipt).is_ok());
    let status = net.client().account(&alice, 0);
    assert_eq!((status.seq, status.spendable), (1, Some(750)));
}

#[test]
fn keys_rotate_through_a_run_of_payments() {
    let net = TestNet::start("run");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    assert_eq!(net.view(0, &alice).signer, Signer::First, "an unused account is on its first key");
    let mut keys = vec![net.view(0, &alice).signer];
    for round in 0..5 {
        net.pay("alice", carol, 10);
        let key = net.view(0, &alice).signer;
        assert!(matches!(key, Signer::Key(_)));
        assert!(!keys.contains(&key), "payment {round} reused a key");
        keys.push(key);
    }
    // The first payment put the account's spare keys on record.
    assert_eq!(net.view(0, &alice).spare, net.wallet("alice").spare());
    // Carol's account came into being by being paid, and can spend at once.
    net.pay("carol", bob, 35);
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 950);
        assert_eq!(net.view(v, &carol).balance, 15);
        assert_eq!(net.view(v, &bob).balance, 1035);
        assert_eq!(net.view(v, &alice).seq, 5);
    }
}

#[test]
fn a_spent_payment_number_cannot_be_spent_again() {
    let net = TestNet::start("double");
    let (bob, carol) = (net.address("bob"), net.address("carol"));
    let to_bob = net.raw_order("alice", 0, bob, START);
    let to_carol = net.raw_order("alice", 0, carol, START);
    let client = net.client();

    client.transfer(&to_bob).unwrap();
    for v in 0..4 {
        match client.call(v, &Request::SubmitOrder(to_carol.clone())).unwrap() {
            Response::Rejected(Reject::StaleSeq { current: 1 }) => {}
            other => panic!("validator {v} answered {other:?}"),
        }
    }
    assert!(client.transfer(&to_carol).is_err());
    for v in 0..4 {
        assert_eq!(net.view(v, &bob).balance, 2 * START as i128);
        assert_eq!(net.view(v, &carol).balance, 0);
    }
}

#[test]
fn validators_hold_to_their_first_vote() {
    let net = TestNet::start("conflict");
    let (bob, carol) = (net.address("bob"), net.address("carol"));
    let to_bob = net.raw_order("alice", 0, bob, START);
    let to_carol = net.raw_order("alice", 0, carol, START);
    let client = net.client();

    // Every validator votes for the first order, but nobody finishes it.
    let (votes, refusals) = client.collect_votes(&to_bob);
    assert_eq!((votes.len(), refusals.len()), (4, 0));
    // The second order arrives before the first is final. Still refused.
    for v in 0..4 {
        assert!(matches!(
            client.call(v, &Request::SubmitOrder(to_carol.clone())).unwrap(),
            Response::Rejected(Reject::Conflict { .. })
        ));
    }
    // Asking again for the first order returns the same vote, not a new one.
    let (again, _) = client.collect_votes(&to_bob);
    assert_eq!(again, votes);
    // And the first can still be completed.
    assert_eq!(client.transfer(&to_bob).unwrap().confirmations, 4);
}

#[test]
fn splitting_the_vote_locks_only_the_cheater() {
    let net = TestNet::start("split");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let to_bob = net.raw_order("alice", 0, bob, START);
    let to_carol = net.raw_order("alice", 0, carol, START);
    let client = net.client();

    for v in [0, 1] {
        assert!(matches!(client.call(v, &Request::SubmitOrder(to_bob.clone())).unwrap(), Response::Vote(_)));
    }
    for v in [2, 3] {
        assert!(matches!(client.call(v, &Request::SubmitOrder(to_carol.clone())).unwrap(), Response::Vote(_)));
    }
    // Neither order can reach three votes, now or later.
    for order in [&to_bob, &to_carol] {
        match client.transfer(order) {
            Err(TransferError::NoQuorum { stake: 2, total: 4, .. }) => {}
            other => panic!("expected no quorum, got {:?}", other.map(|r| r.votes)),
        }
    }
    let status = client.account(&alice, 0);
    assert_eq!(status.locks.len(), 2);
    assert_eq!(net.view(0, &alice).balance, START as i128, "nothing moved");
    let unfinished = client.unfinished(&alice, &status);
    assert_eq!(unfinished.len(), 2);
    assert!(unfinished.iter().all(|u| u.stake == 2 && !client.can_still_finish(&unfinished, u)));

    // Everyone else is unaffected.
    assert_eq!(net.pay("bob", carol, 5).confirmations, 4);
}

#[test]
fn the_network_works_with_a_validator_down_and_it_catches_up() {
    let mut net = TestNet::start("down");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    net.stop(3);

    let receipt = net.pay("alice", bob, 100);
    assert_eq!((receipt.votes, receipt.confirmations), (3, 3));
    net.pay("bob", alice, 1);

    // The validator comes back with nothing but its old files and learns
    // what it missed from the others.
    net.restart(3);
    wait_until("validator 4 catches up", || {
        net.view(3, &bob).balance == 1099 && net.view(3, &alice).balance == 901
    });
    assert_eq!(net.view(3, &alice).seq, 1);
    assert_eq!(net.info(3).certificates, 2);
}

#[test]
fn a_wallet_brings_a_lagging_validator_up_to_date() {
    // Syncing between validators is off, so only the client can help.
    let options = Options { sync: Duration::ZERO, ..Options::default() };
    let mut net = TestNet::start_with("lag", options);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    net.stop(3);
    net.pay("alice", bob, 100);
    net.restart(3);
    assert_eq!(net.view(3, &alice).seq, 0, "validator 4 missed payment #0");

    let receipt = net.pay("alice", bob, 50);
    assert_eq!((receipt.votes, receipt.confirmations), (4, 4));
    assert_eq!(net.view(3, &alice).seq, 2);
    assert_eq!(net.view(3, &bob).balance, 1150);
}

#[test]
fn a_restart_keeps_balances_and_votes() {
    let mut net = TestNet::start("restart");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    net.pay("alice", bob, 100);

    // Validator 1 votes for alice's next payment, then crashes before it is final.
    let to_bob = net.raw_order("alice", 1, bob, 5);
    let to_carol = net.raw_order("alice", 1, carol, 5);
    let keys_before = net.info(0).keys_used;
    assert!(matches!(net.ask(0, &Request::SubmitOrder(to_bob.clone())), Response::Vote(_)));
    net.stop(0);
    net.restart(0);

    let view = net.view(0, &alice);
    assert_eq!((view.balance, view.seq), (900, 1));
    assert_eq!(view.lock, Some(to_bob.order.digest(&net.network)));
    // It still holds the signed order itself, for whoever comes to finish it.
    let holds_the_order = |net: &TestNet| match net.ask(0, &Request::Pending(alice)) {
        Response::Pending(held) => held == Some(to_bob.clone()),
        other => panic!("unexpected answer: {other:?}"),
    };
    assert!(holds_the_order(&net));
    // Payment #0 is final, so its order was dropped from the log on restart.
    let log = fs::metadata(net.dir.join("data-0").join("orders.log")).unwrap().len();
    assert_eq!(log, 4 + borsh::to_vec(&to_bob).unwrap().len() as u64, "one record: a length and the order");
    // It must not vote for a different order under the same number...
    assert!(matches!(
        net.ask(0, &Request::SubmitOrder(to_carol)),
        Response::Rejected(Reject::Conflict { .. })
    ));
    // ...must still back the one it voted for...
    match net.ask(0, &Request::SubmitOrder(to_bob.clone())) {
        Response::Vote(vote) => {
            let digest = to_bob.order.digest(&net.network);
            assert!(net.client().committee().verify_vote(&digest, &vote).is_ok());
            // ...and must never sign with a one-time key it used before.
            assert!(vote.signature.leaf > keys_before);
        }
        other => panic!("unexpected answer: {other:?}"),
    }
    // A second restart reads the log the first one rewrote.
    net.stop(0);
    net.restart(0);
    assert_eq!(net.view(0, &alice).lock, Some(to_bob.order.digest(&net.network)));
    assert!(holds_the_order(&net));

    assert_eq!(net.pay("alice", bob, 5).confirmations, 4);
    assert!(matches!(net.ask(0, &Request::Pending(alice)), Response::Pending(None)));
}

#[test]
fn forged_certificates_change_nothing() {
    let net = TestNet::start("forge");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    let order = net.raw_order("alice", 0, bob, 10);
    let (votes, _) = client.collect_votes(&order);
    let votes: Vec<_> = votes.into_values().collect();

    let too_few = orien_ledger::Certificate::new(0, &order, votes[..2].to_vec());
    let mut inflated = orien_ledger::Certificate::new(0, &order, votes[..3].to_vec());
    inflated.order.amount = 999;
    for forged in [too_few, inflated] {
        for v in 0..4 {
            assert!(matches!(
                net.ask(v, &Request::SubmitCertificate(forged.clone())),
                Response::Invalid(_)
            ));
        }
    }
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, START as i128);
        assert_eq!(net.view(v, &bob).balance, START as i128);
    }
}

#[test]
fn one_signature_covers_many_simultaneous_payments() {
    let crowd = 40;
    let options = Options { window: Duration::from_millis(100), crowd, ..Options::default() };
    let net = TestNet::start_with("batch", options);
    let carol = net.address("carol");
    let barrier = Barrier::new(crowd);
    thread::scope(|scope| {
        for i in 0..crowd {
            let (net, barrier) = (&net, &barrier);
            scope.spawn(move || {
                barrier.wait();
                net.pay(&format!("w{i}"), carol, 10);
            });
        }
    });
    for v in 0..4 {
        assert_eq!(net.view(v, &carol).balance, 10 * crowd as i128);
        let info = net.info(v);
        assert_eq!(info.certificates, crowd as u64);
        // Each payment needs a vote and a confirmation from this validator:
        // eighty things to sign. How they fall into batches depends on
        // timing, so the bound is loose, but it is far below eighty.
        assert!(info.keys_used <= 20, "validator {v} signed {} times for {crowd} payments", info.keys_used);
    }
}

#[test]
fn a_single_validator_is_already_a_network() {
    let net = TestNet::start_with("solo", Options { validators: 1, ..Options::default() });
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let receipt = net.pay("alice", bob, 1);
    assert_eq!((receipt.votes, receipt.confirmations), (1, 1));
    assert_eq!(net.view(0, &alice).balance, 999);
}

#[test]
fn a_wallet_never_signs_two_orders_with_one_key() {
    let net = TestNet::start_with("wallet", Options { validators: 1, ..Options::default() });
    let (bob, carol) = (net.address("bob"), net.address("carol"));
    let mut wallet = net.wallet("alice");

    let first = wallet.sign_order(0, bob, 5, None, &net.network).unwrap();
    assert!(wallet.sign_order(0, carol, 5, None, &net.network).is_err());
    assert!(wallet.sign_order(0, bob, 6, None, &net.network).is_err());
    assert_eq!(wallet.sign_order(0, bob, 5, None, &net.network).unwrap(), first);

    // The promise is on disk: a fresh load of the file remembers it.
    let mut reloaded = net.wallet("alice");
    assert!(reloaded.sign_order(0, carol, 5, None, &net.network).is_err());
    assert_eq!(reloaded.sign_order(0, bob, 5, None, &net.network).unwrap(), first);

    reloaded.confirm(0).unwrap();
    assert!(reloaded.sign_order(0, bob, 5, None, &net.network).is_err(), "payment #0 is done");
    assert!(reloaded.sign_order(1, carol, 5, None, &net.network).is_ok());
}

/// Not a pass/fail test: prints how many payments per second this machine
/// makes final, with every validator syncing its votes to disk. Run with
///
///   cargo test --release -p orien-net --test devnet throughput -- --ignored --nocapture
#[test]
fn another_copy_of_a_wallet_finishes_a_payment() {
    let net = TestNet::start("copy");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    // A second device holds a copy of alice's wallet, made before she paid anyone.
    fs::copy(net.dir.join("alice.wallet"), net.dir.join("alice-copy.wallet")).unwrap();

    // The first device signs a payment, reaches two validators, and is lost.
    let signed = net.wallet("alice").sign_order(0, bob, 100, None, &net.network).unwrap();
    for v in [0, 1] {
        assert!(matches!(client.call(v, &Request::SubmitOrder(signed.clone())).unwrap(), Response::Vote(_)));
    }

    // The copy knows nothing of that payment, but the validators kept it.
    let mut copy = net.wallet("alice-copy");
    assert!(copy.pending().is_none());
    let status = client.account(&alice, copy.next_seq());
    let unfinished = client.unfinished(&alice, &status);
    assert_eq!(unfinished.len(), 1);
    assert_eq!(unfinished[0].signed, signed);
    assert_eq!(unfinished[0].stake, 2);
    assert!(client.can_still_finish(&unfinished, &unfinished[0]));

    // It finishes that payment without signing anything.
    copy.adopt(&unfinished[0].signed, &net.network).unwrap();
    let receipt = client.transfer(copy.pending().unwrap()).unwrap();
    copy.confirm(0).unwrap();
    assert_eq!((receipt.votes, receipt.confirmations), (4, 4));
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 900);
        assert_eq!(net.view(v, &bob).balance, 1100);
        assert!(matches!(net.ask(v, &Request::Pending(alice)), Response::Pending(None)));
    }
    // And it carries on with the next key, which both copies derive alike.
    assert_eq!(net.pay("alice-copy", bob, 1).confirmations, 4);
    assert_eq!(net.view(0, &alice).seq, 2);
}

#[test]
fn one_of_two_conflicting_payments_can_still_be_finished() {
    let net = TestNet::start("rescue");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let client = net.client();
    // Two copies of alice's wallet each sign payment #0, and each reaches
    // one validator before stopping.
    let to_bob = net.raw_order("alice", 0, bob, 10);
    let to_carol = net.raw_order("alice", 0, carol, 10);
    assert!(matches!(client.call(0, &Request::SubmitOrder(to_bob.clone())).unwrap(), Response::Vote(_)));
    assert!(matches!(client.call(1, &Request::SubmitOrder(to_carol.clone())).unwrap(), Response::Vote(_)));

    // Two validators have not voted, so either payment can still reach three.
    let status = client.account(&alice, 0);
    let unfinished = client.unfinished(&alice, &status);
    assert_eq!(unfinished.len(), 2);
    assert!(unfinished.iter().all(|u| u.stake == 1 && client.can_still_finish(&unfinished, u)));

    // The owner picks one. The validator that voted for the other refuses,
    // then accepts the certificate like everyone else.
    let mut wallet = net.wallet("alice");
    wallet.adopt(&to_bob, &net.network).unwrap();
    let receipt = client.transfer(&to_bob).unwrap();
    assert_eq!((receipt.votes, receipt.confirmations), (3, 4));
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).seq, 1);
        assert_eq!(net.view(v, &alice).lock, None);
        assert_eq!(net.view(v, &bob).balance, START as i128 + 10);
        assert_eq!(net.view(v, &carol).balance, 0);
        assert!(matches!(net.ask(v, &Request::Pending(alice)), Response::Pending(None)));
    }
    assert!(matches!(
        client.call(1, &Request::SubmitOrder(to_carol)).unwrap(),
        Response::Rejected(Reject::StaleSeq { .. })
    ));
}

#[test]
fn validators_agree_on_one_fingerprint_of_all_balances() {
    let mut net = TestNet::start("root");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let client = net.client();
    let agreed = |client: &Client| {
        let roots = client.roots();
        let first = roots[0].expect("validator 1 answers");
        roots.iter().all(|root| *root == Some(first)).then_some(first)
    };
    let genesis = agreed(&client).expect("every validator starts from the same state");

    // Payments reach validator 4 late and by a different route: it is down
    // for the first two and learns of them from the others.
    net.stop(3);
    net.pay("alice", bob, 100);
    net.pay("bob", carol, 30);
    net.restart(3);
    net.pay("alice", carol, 1);
    wait_until("all four hold the same state", || agreed(&client).is_some());
    let after = agreed(&client).unwrap();
    assert_ne!(after, genesis);

    // The fingerprint is the one anybody computes from the genesis file and
    // the payments themselves.
    let mut expected = orien_ledger::State::from_genesis(&net.genesis);
    for (account, seq) in [(alice, 0), (bob, 0), (alice, 1)] {
        expected.apply(&client.certificate(&account, seq).unwrap().order).unwrap();
    }
    assert_eq!(after, expected.root());
}

#[test]
fn a_validator_keeps_proof_that_an_owner_signed_twice() {
    let mut net = TestNet::start("evidence");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let to_bob = net.raw_order("alice", 0, bob, 10);
    let to_carol = net.raw_order("alice", 0, carol, 10);
    let evidence = |net: &TestNet, validator: usize| match net.ask(validator, &Request::Evidence(alice)) {
        Response::Evidence(evidence) => evidence,
        other => panic!("unexpected answer: {other:?}"),
    };

    assert!(matches!(net.ask(0, &Request::SubmitOrder(to_bob.clone())), Response::Vote(_)));
    assert_eq!(evidence(&net, 0), None, "one signed order proves nothing");
    // The second order is refused, and kept.
    assert!(matches!(
        net.ask(0, &Request::SubmitOrder(to_carol.clone())),
        Response::Rejected(Reject::Conflict { .. })
    ));
    assert_eq!(evidence(&net, 0), Some((to_bob.clone(), to_carol.clone())));
    assert_eq!(evidence(&net, 1), None, "validator 2 has seen neither");

    // A forged second order is no evidence: its signature does not verify.
    let mut forged = net.raw_order("bob", 0, carol, 1);
    forged.order.from = bob;
    forged.order.amount = 2;
    assert!(matches!(net.ask(0, &Request::SubmitOrder(net.raw_order("bob", 0, carol, 1))), Response::Vote(_)));
    assert!(matches!(net.ask(0, &Request::SubmitOrder(forged)), Response::Rejected(Reject::BadSignature)));
    match net.ask(0, &Request::Evidence(bob)) {
        Response::Evidence(None) => {}
        other => panic!("unexpected answer: {other:?}"),
    }

    // The evidence survives a restart...
    net.stop(0);
    net.restart(0);
    assert_eq!(evidence(&net, 0), Some((to_bob.clone(), to_carol)));
    // ...and is dropped once that payment number is final and it no longer matters.
    net.client().transfer(&to_bob).unwrap();
    assert_eq!(evidence(&net, 0), None);
    net.stop(0);
    net.restart(0);
    assert_eq!(evidence(&net, 0), None);
}

#[test]
fn a_validator_holds_keys_back_for_closing_an_epoch() {
    // A tree of 16 one-time keys: the first is kept for stake proofs and a
    // quarter are held back. One payment at a time takes two: one for the
    // vote, one for the confirmation.
    let options = Options { validators: 1, height: 4, ..Options::default() };
    let net = TestNet::start_with("reserve", options);
    let bob = net.address("bob");
    for _ in 0..5 {
        net.pay("alice", bob, 1);
    }
    assert_eq!((net.info(0).keys_used, net.info(0).keys_total), (11, 16));

    // The sixth payment gets the last vote, but no confirmation: the keys
    // left are held back for closing the epoch.
    match net.try_pay("alice", bob, 1) {
        Err(TransferError::NotConfirmed { stake: 0, refusals, .. }) => {
            assert!(refusals[0].1.contains("no one-time keys left for payments"), "{refusals:?}");
        }
        other => panic!("expected a refusal, got {:?}", other.map(|r| r.votes)),
    }
    // Asking again changes nothing: no key below the reserve is left.
    assert!(net.try_pay("alice", bob, 1).is_err());
    assert_eq!(net.info(0).keys_used, 12);
}

/// Gathers votes for an order into a certificate, without going further.
fn certificate_for(client: &Client, signed: &SignedOrder) -> orien_ledger::Certificate {
    let (votes, _) = client.collect_votes(signed);
    orien_ledger::Certificate::new(client.epoch(), signed, votes.into_values().take(3).collect())
}

#[test]
fn an_address_binds_the_spare_keys_a_first_payment_reveals() {
    let net = TestNet::start("address");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let wallet = net.wallet("alice");
    let sign = |order: Order| {
        let digest = order.digest(&net.network);
        SignedOrder { signature: Signature::Key(wallet.key(0).sign(digest.as_bytes())), order }
    };

    // Signed with alice's real first key, but naming bob's spare keys as
    // hers. The two together do not make her address.
    let swapped = sign(Order { spare: net.wallet("bob").spare(), ..wallet.order(0, bob, 10) });
    assert!(matches!(
        net.ask(0, &Request::SubmitOrder(swapped)),
        Response::Rejected(Reject::BadSignature)
    ));

    // An order signed with a spare key is refused while the account is not
    // on its spares, whoever's spare keys they are.
    let order = wallet.order(0, bob, 10);
    let digest = order.digest(&net.network);
    let stray = KeyTree::generate([42; 32], SPARE_HEIGHT);
    let with_spare = SignedOrder {
        signature: Signature::Spare(stray.sign_with_leaf(0, digest.as_bytes())),
        order: order.clone(),
    };
    assert!(matches!(
        net.ask(0, &Request::SubmitOrder(with_spare)),
        Response::Rejected(Reject::BadSignature)
    ));
    assert_eq!(net.view(0, &alice).lock, None, "neither attempt earned a vote");

    // The order as the wallet makes it is voted for.
    assert!(matches!(net.ask(0, &Request::SubmitOrder(sign(order))), Response::Vote(_)));
    assert_eq!(alice, address(&wallet.key(0).commitment(), &wallet.spare()));
}

#[test]
fn a_validator_refuses_files_in_a_format_it_does_not_write() {
    let mut net = TestNet::start("format");
    let bob = net.address("bob");
    net.pay("alice", bob, 100);
    net.stop(0);
    let data = net.dir.join("data-0");

    // The same directory starts again without complaint.
    Node::start(net.config(0), None).map(Node::stop).unwrap();
    // A directory written by an older or a newer version does not.
    for other in [None, Some("1"), Some("5"), Some("8")] {
        match other {
            Some(format) => fs::write(data.join("format"), format).unwrap(),
            None => fs::remove_file(data.join("format")).unwrap(),
        }
        let refusal = Node::start(net.config(0), None).err().expect("must refuse").to_string();
        assert!(refusal.contains("data format"), "{refusal}");
    }
    // Format 6, which the live network was written in, is read as it is and
    // upgraded in place.
    fs::write(data.join("format"), "6").unwrap();
    net.restart(0);
    assert_eq!(fs::read_to_string(data.join("format")).unwrap(), "7");
    assert_eq!(net.view(0, &bob).balance, 1100);
}

#[test]
fn a_payment_is_final_only_once_enough_validators_confirm() {
    let mut net = TestNet::start("confirm");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    let signed = net.wallet("alice").sign_order(0, bob, 100, None, &net.network).unwrap();
    let certificate = certificate_for(&client, &signed);
    assert!(client.committee().verify_certificate(&certificate).is_ok());

    // Two validators go away before they hear of the certificate. The two
    // that remain record it and confirm, and two of four are not enough.
    net.stop(2);
    net.stop(3);
    match client.finish(certificate.clone(), 3) {
        Err(TransferError::NotConfirmed { stake: 2, total: 4, .. }) => {}
        other => panic!("expected too few confirmations, got {:?}", other.map(|done| done.confirmations)),
    }
    // A certificate with no confirmations proves nothing to anyone else.
    let bare = orien_ledger::Receipt { certificate: certificate.clone(), confirmations: Vec::new() };
    assert!(client.committee().verify_receipt(&bare).is_err());

    // When they return, the wallet simply tries the same payment again. The
    // validators that recorded it will not vote a second time, and need not:
    // the certificate is found and only the confirmations are gathered.
    net.restart(2);
    net.restart(3);
    let done = client.transfer(&signed).unwrap();
    assert_eq!(done.confirmations, 4);
    assert_eq!(done.receipt.certificate.order, signed.order);
    assert!(client.committee().verify_receipt(&done.receipt).is_ok());
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 900);
        assert_eq!(net.view(v, &bob).balance, 1100);
    }
}

#[test]
fn a_repeated_confirmation_costs_no_further_key() {
    let net = TestNet::start("reconfirm");
    let bob = net.address("bob");
    let done = net.pay("alice", bob, 100);
    let keys = net.info(0).keys_used;

    for _ in 0..3 {
        match net.ask(0, &Request::Confirm(done.receipt.certificate.clone())) {
            Response::Confirmed(again) => assert_eq!(again, done.receipt.confirmations[0]),
            other => panic!("unexpected answer: {other:?}"),
        }
    }
    assert_eq!(net.info(0).keys_used, keys, "the same confirmation is handed back, not signed anew");
}

#[test]
fn only_an_accounts_latest_payment_is_confirmed_late() {
    let mut net = TestNet::start("latest");
    let bob = net.address("bob");
    let first = net.pay("alice", bob, 10).receipt.certificate;
    let second = net.pay("alice", bob, 20).receipt.certificate;

    // A restart forgets the confirmations already signed, as a crash would.
    net.stop(0);
    net.restart(0);
    let keys = net.info(0).keys_used;

    // An earlier payment is past confirming: validators holding a quorum had
    // to record it before they could vote for the one after.
    assert!(matches!(net.ask(0, &Request::Confirm(first.clone())), Response::Applied { fresh: false }));
    assert_eq!(net.info(0).keys_used, keys);

    // The latest one is confirmed again, at the cost of one key, once.
    let digest = second.order.digest(&net.network);
    for _ in 0..2 {
        match net.ask(0, &Request::Confirm(second.clone())) {
            Response::Confirmed(confirmation) => {
                assert!(net.client().committee().verify_confirmation(&digest, &confirmation).is_ok())
            }
            other => panic!("unexpected answer: {other:?}"),
        }
    }
    assert_eq!(net.info(0).keys_used, keys + 1);

    // A different payment under a number that is taken gets no confirmation.
    let mut other = first.clone();
    other.order.amount = 11;
    assert!(matches!(net.ask(0, &Request::Confirm(other)), Response::Invalid(_)));
}

#[test]
fn a_certificate_must_carry_the_owners_real_signature() {
    let net = TestNet::start("owner-signature");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    let signed = net.wallet("alice").sign_order(0, bob, 100, None, &net.network).unwrap();
    let genuine = certificate_for(&client, &signed);

    // The votes are real, but the owner's signature has been swapped for
    // one made with a different key.
    let mut swapped = genuine.clone();
    swapped.signature = net.raw_order("bob", 0, alice, 1).signature;
    assert!(client.committee().verify_certificate(&swapped).is_ok(), "the votes alone check out");
    for request in [Request::Confirm(swapped.clone()), Request::SubmitCertificate(swapped)] {
        match net.ask(0, &request) {
            Response::Invalid(reason) => assert!(reason.contains("owner's signature"), "{reason}"),
            other => panic!("unexpected answer: {other:?}"),
        }
    }
    assert_eq!(net.view(0, &alice).balance, START as i128, "nothing was applied");

    // The genuine certificate goes through, and what the validator keeps is
    // the owner's own signed order.
    assert!(matches!(net.ask(0, &Request::Confirm(genuine)), Response::Confirmed(_)));
    match net.ask(0, &Request::GetCertificate { account: alice, seq: 0 }) {
        Response::Certificate(Some(kept)) => assert_eq!(kept.signed(), signed),
        other => panic!("unexpected answer: {other:?}"),
    }
}

#[test]
fn a_wallet_believes_a_payment_number_only_on_confirmations() {
    let mut net = TestNet::start("believe");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    let signed = net.wallet("alice").sign_order(0, bob, 100, None, &net.network).unwrap();
    let certificate = certificate_for(&client, &signed);

    // One validator has applied the certificate and now says alice is at
    // payment #1. The others are unreachable, so nobody else can confirm.
    assert!(matches!(net.ask(0, &Request::SubmitCertificate(certificate)), Response::Applied { fresh: true }));
    for v in 1..4 {
        net.stop(v);
    }
    assert_eq!(net.view(0, &alice).seq, 1);
    let status = client.account(&alice, 0);
    assert_eq!(status.seq, 0, "one validator's certificate is not proof the payment is final");

    // With the others back, the same question gathers confirmations from a
    // quorum, which is proof, and brings them up to date on the way.
    for v in 1..4 {
        net.restart(v);
    }
    let status = client.account(&alice, 0);
    assert_eq!((status.seq, status.spendable), (1, Some(900)));
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).seq, 1);
    }
}

#[test]
#[ignore = "measurement"]
fn throughput() {
    const PAYERS: usize = 1000;
    // Simultaneous clients. Override with the CLIENTS environment variable.
    let clients: usize = std::env::var("CLIENTS").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let key = |seed: &[u8; 32], index: u64| ots::SecretKey::from_seed(prf(seed, Kdf::WalletKey, index));
    let seeds: Vec<[u8; 32]> = (0..PAYERS as u64)
        .map(|i| {
            let mut seed = [0xabu8; 32];
            seed[..8].copy_from_slice(&i.to_le_bytes());
            seed
        })
        .collect();
    let spares: Vec<Hash32> = seeds
        .iter()
        .map(|seed| KeyTree::generate(prf(seed, Kdf::WalletSpare, 0), SPARE_HEIGHT).root())
        .collect();
    let accounts: Vec<AccountId> =
        seeds.iter().zip(&spares).map(|(seed, spare)| address(&key(seed, 0).commitment(), spare)).collect();
    let options = Options {
        extra: accounts.iter().map(|a| Allocation { account: *a, amount: START }).collect(),
        window: Duration::from_millis(5),
        height: 12,
        durable: true,
        ..Options::default()
    };
    let net = TestNet::start_with("throughput", options);
    let carol = net.address("carol");
    let orders: Vec<SignedOrder> = seeds
        .iter()
        .zip(accounts.iter().zip(&spares))
        .map(|(seed, (account, spare))| {
            let order = Order {
                kind: Kind::Payment,
                from: *account,
                seq: 0,
                to: carol,
                amount: 1,
                max_fee: 0,
                next_key: key(seed, 1).commitment(),
                spare: *spare,
            };
            let digest = order.digest(&net.network);
            SignedOrder { signature: Signature::Key(key(seed, 0).sign(digest.as_bytes())), order }
        })
        .collect();

    let client = net.client();
    let started = Instant::now();
    thread::scope(|scope| {
        for chunk in orders.chunks(PAYERS.div_ceil(clients)) {
            let client = &client;
            scope.spawn(move || {
                for order in chunk {
                    client.transfer(order).unwrap();
                }
            });
        }
    });
    let elapsed = started.elapsed().as_secs_f64();

    for v in 0..4 {
        assert_eq!(net.view(v, &carol).balance, PAYERS as i128);
    }
    let signatures = net.info(0).keys_used;
    println!(
        "{PAYERS} payments from {clients} clients final in {elapsed:.2} s = {:.0} payments/s; validator 1 made {signatures} \
         signatures ({:.1} payments per signature)",
        PAYERS as f64 / elapsed,
        PAYERS as f64 / signatures as f64
    );
}

#[test]
fn an_epoch_closes_and_every_validator_moves_on_together() {
    let net = TestNet::with_epochs("epoch", 1);
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let first = net.pay_through("alice", bob, 100);
    net.wait_for_epoch(1);
    // Payments carry on in the next epoch, and the one after.
    net.pay_through("bob", carol, 30);
    net.wait_for_epoch(2);
    net.pay_through("alice", carol, 5);
    net.wait_for_epoch(4);

    let epoch = net.wait_until_level();
    assert!(epoch >= 4);
    for v in 0..4 {
        assert_eq!((net.view(v, &alice).balance, net.view(v, &alice).seq), (895, 2), "validator {v}");
        assert_eq!((net.view(v, &bob).balance, net.view(v, &bob).seq), (1070, 1), "validator {v}");
        assert_eq!(net.view(v, &carol).balance, 35, "validator {v}");
    }

    // Someone who knows only the genesis file follows the checkpoints to
    // the present, and can then prove an account against the latest one.
    let client = net.client();
    assert_eq!(client.epoch(), 0);
    assert!(client.follow() >= 4);
    wait_until("the latest checkpoint proves alice's account", || {
        client.follow();
        client.proven(&alice).is_some_and(|account| (account.balance, account.seq) == (895, 2))
    });
    assert_eq!(client.proven(&Hash32([9; 32])), None, "an account that never held anything has no proof");

    // A receipt is checked against the validators of the epoch it was made in.
    if let Done::Receipt(done) = first {
        let made_in = done.receipt.certificate.epoch;
        assert!(client.committee_at(made_in).unwrap().verify_receipt(&done.receipt).is_ok());
        assert!(client.committee().verify_receipt(&done.receipt).is_err(), "votes count in their own epoch only");
        assert!(net.client().committee_at(made_in + 1).is_none(), "a client that has not followed cannot check it");
    }

    // What it has followed can be saved and picked up again.
    let saved = net.dir.join("head");
    client.head().save(&saved).unwrap();
    let resumed = Client::resume(&net.genesis, Head::load(&saved, &net.genesis).unwrap());
    assert_eq!(resumed.epoch(), client.epoch());
    let mut elsewhere = net.genesis.clone();
    elsewhere.network = "another network".into();
    assert!(Head::load(&saved, &elsewhere).is_none(), "a head belongs to one network");
}

#[test]
fn a_checkpoint_frees_an_account_whose_owner_signed_twice() {
    let net = TestNet::with_epochs("freed", 1);
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let client = net.client();
    let to_bob = net.raw_order("alice", 0, bob, 600);
    let to_carol = net.raw_order("alice", 0, carol, 600);

    // Half the validators are shown one payment and half the other. Then
    // each is shown the one it has not seen, which gives it the proof.
    wait_until("a checkpoint cancels the payments", || {
        for (first, second) in [(&to_bob, &to_carol), (&to_carol, &to_bob)] {
            for v in 0..4 {
                let order = if v < 2 { first } else { second };
                net.ask(v, &Request::SubmitOrder(order.clone()));
            }
        }
        client.account(&alice, 0).signer == Some(Signer::Spare(0))
    });
    net.wait_until_level();
    // Nothing moved, the number is free again, and no validator is still
    // promised to either payment.
    let status = client.account(&alice, 0);
    assert_eq!((status.seq, status.spendable), (0, Some(START as i128)));
    assert!(status.locks.is_empty());
    for v in 0..4 {
        let view = net.view(v, &alice);
        assert_eq!((view.balance, view.seq, view.lock), (START as i128, 0, None), "validator {v}");
        assert_eq!(view.signer, Signer::Spare(0), "validator {v}");
        // The cancelled payments stay dead: their key is retired.
        for dead in [&to_bob, &to_carol] {
            let answer = net.ask(v, &Request::SubmitOrder(dead.clone()));
            assert!(matches!(answer, Response::Rejected(Reject::BadSignature) | Response::Closing), "{answer:?}");
        }
    }
    assert!(client.unfinished(&alice, &status).is_empty());

    // The wallet's next payment is signed with its first spare key.
    let mut wallet = net.wallet("alice");
    let signed = wallet.sign_order(0, bob, 100, status.signer.as_ref(), &net.network).unwrap();
    assert!(matches!(signed.signature, Signature::Spare(_)));
    client.pay(&signed, Duration::from_secs(20)).unwrap();
    wallet.confirm(0).unwrap();
    // After that it is back to ordinary keys.
    net.pay_through("alice", carol, 50);
    net.wait_until_level();
    for v in 0..4 {
        let view = net.view(v, &alice);
        assert_eq!((view.balance, view.seq), (850, 2), "validator {v}");
        assert!(matches!(view.signer, Signer::Key(_)));
        assert_eq!(net.view(v, &bob).balance + net.view(v, &carol).balance + view.balance, 2 * START as i128);
    }
}

#[test]
fn a_validator_that_missed_checkpoints_catches_up() {
    let mut net = TestNet::with_epochs("missed", 1);
    let (bob, carol) = (net.address("bob"), net.address("carol"));
    net.stop(3);
    net.pay_through("alice", bob, 100);
    // Three of four are enough to close an epoch.
    wait_until("the other three close two epochs", || (0..3).all(|v| net.info(v).epoch >= 2));
    net.pay_through("bob", carol, 30);

    net.restart(3);
    net.wait_until_level();
    assert_eq!(net.view(3, &bob).balance, 1070);
    assert_eq!(net.view(3, &carol).balance, 30);
    // It takes full part again.
    wait_until("all four validators confirm a payment", || {
        matches!(net.pay_through("alice", carol, 1), Done::Receipt(done) if done.confirmations == 4)
    });
}

#[test]
fn a_close_waits_for_two_thirds_and_resumes_when_they_return() {
    let mut net = TestNet::with_epochs("stalled", 2);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    // A payment with its certificate made, and shown to nobody yet.
    let paid = net.raw_order("alice", 0, bob, 100);
    let certificate = certificate_for(&net.client(), &paid);
    assert_eq!(certificate.epoch, 0, "the first epoch closed before the test got going");
    net.stop(2);
    net.stop(3);
    // Two of four cannot close an epoch. They pause and wait.
    wait_until("the two remaining validators pause", || (0..2).all(|v| net.info(v).closing));
    thread::sleep(Duration::from_millis(1500));
    assert!((0..2).all(|v| net.info(v).epoch == 0 && net.info(v).closing));

    // While paused, a validator votes on nothing and confirms nothing. It
    // still records a certificate it is shown.
    let other = net.raw_order("bob", 0, alice, 5);
    for v in 0..2 {
        assert!(matches!(net.ask(v, &Request::SubmitOrder(other.clone())), Response::Closing), "validator {v}");
        assert_eq!(net.view(v, &bob).lock, None, "validator {v} promised its vote while paused");
        assert!(matches!(net.ask(v, &Request::Confirm(certificate.clone())), Response::Closing), "validator {v}");
        assert_eq!(net.view(v, &alice).seq, 1, "validator {v} did not record the certificate");
        assert!(matches!(net.ask(v, &Request::Confirm(certificate.clone())), Response::Closing), "validator {v}");
    }
    let stalled = net.client().pay(&other, Duration::from_millis(300)).err().expect("nothing can be paid");
    assert!(stalled.closing(), "{stalled}");
    // With a third validator back, the close goes through.
    net.restart(2);
    wait_until("three validators close the epoch", || (0..3).all(|v| net.info(v).epoch >= 1));
    // The certificate reached the two validators after they had paused, so
    // the checkpoint could not take the payment in. It is not lost: the
    // same signed order goes through in the new epoch.
    assert!(matches!(net.client().pay(&paid, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    wait_until("the payment is recorded everywhere", || (0..3).all(|v| net.view(v, &bob).balance == 1100));
}

#[test]
fn an_unfinished_payment_is_still_finishable_after_a_checkpoint() {
    let net = TestNet::with_epochs("carried", 1);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let signed = net.raw_order("alice", 0, bob, 100);
    let digest = signed.order.digest(&net.network);
    // Two of four validators vote, and then the payer is gone.
    wait_until("two validators have voted", || {
        (0..2).all(|v| matches!(net.ask(v, &Request::SubmitOrder(signed.clone())), Response::Vote(_)))
    });
    let voted_in = net.info(0).epoch;
    net.wait_for_epoch(voted_in + 2);
    net.wait_until_level();

    // The promise outlived the checkpoints, and the signed order is still there.
    for v in 0..2 {
        assert_eq!(net.view(v, &alice).lock, Some(digest), "validator {v}");
        assert!(matches!(net.ask(v, &Request::Pending(alice)), Response::Pending(Some(kept)) if kept == signed));
    }
    // A different payment under the same number is still refused.
    let other = net.raw_order("alice", 0, bob, 7);
    wait_until("the conflicting payment is refused", || {
        matches!(net.ask(0, &Request::SubmitOrder(other.clone())), Response::Rejected(Reject::Conflict { .. }))
    });

    // Anyone holding the signed order can finish it. The votes of the old
    // epoch count for nothing, so it gathers them again.
    let client = net.client();
    assert!(matches!(client.pay(&signed, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!((net.view(v, &bob).balance, net.view(v, &alice).seq), (1100, 1), "validator {v}");
    }
}

#[test]
fn a_payment_only_an_absent_validator_recorded_is_put_back_not_lost() {
    let mut net = TestNet::with_epochs("left-out", 2);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    // Start just after a checkpoint, so that the next one is two seconds off.
    net.wait_for_epoch(1);
    net.wait_until_level();
    let client = net.client();
    client.follow();
    let epoch = client.epoch();
    let signed = net.raw_order("alice", 0, bob, 100);
    let digest = signed.order.digest(&net.network);
    let certificate = certificate_for(&client, &signed);
    assert_eq!(certificate.epoch, epoch);

    // Only validator 4 is given the certificate, and it stops before the
    // epoch closes. The others close the epoch without it.
    assert!(matches!(net.ask(3, &Request::SubmitCertificate(certificate)), Response::Applied { fresh: true }));
    assert_eq!(net.view(3, &alice).seq, 1);
    net.stop(3);
    wait_until("the other three close the epoch", || (0..3).all(|v| net.info(v).epoch > epoch));
    assert_eq!(net.view(0, &alice).seq, 0, "the checkpoint could not know of the payment");

    // When it returns, it takes the checkpoint's state. The payment it had
    // recorded is undone there, and the order behind it is a promise again.
    net.restart(3);
    net.wait_until_level();
    let view = net.view(3, &alice);
    assert_eq!((view.balance, view.seq, view.lock), (START as i128, 0, Some(digest)));
    assert!(matches!(net.ask(3, &Request::Pending(alice)), Response::Pending(Some(kept)) if kept == signed));
    // The promise is on disk: it is still there after a restart.
    net.stop(3);
    net.restart(3);
    assert_eq!(net.view(3, &alice).lock, Some(digest));
    assert!(matches!(net.ask(3, &Request::Pending(alice)), Response::Pending(Some(kept)) if kept == signed));

    // The same signed order then goes through in the new epoch.
    let client = net.client();
    assert!(matches!(client.pay(&signed, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!((net.view(v, &bob).balance, net.view(v, &alice).balance), (1100, 900), "validator {v}");
    }
}

#[test]
fn a_payment_recorded_before_the_pause_is_settled_by_the_checkpoint() {
    let net = TestNet::with_epochs("settled", 2);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    net.wait_for_epoch(1);
    net.wait_until_level();
    let client = net.client();
    client.follow();
    let epoch = client.epoch();
    let signed = net.raw_order("alice", 0, bob, 100);
    let certificate = certificate_for(&client, &signed);
    // One validator records the certificate. Nobody confirms it.
    assert!(matches!(net.ask(1, &Request::SubmitCertificate(certificate)), Response::Applied { fresh: true }));
    assert_eq!(net.view(0, &alice).seq, 0);

    // That validator's log goes into the checkpoint, so the payment does.
    net.wait_for_epoch(epoch + 1);
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!((net.view(v, &alice).seq, net.view(v, &bob).balance), (1, 1100), "validator {v}");
    }
    wait_until("the checkpoint proves it", || {
        client.follow();
        client.proven(&alice).is_some_and(|account| account.seq == 1)
    });
}

#[test]
fn validators_restarted_while_epochs_close_end_up_agreeing() {
    let mut net = TestNet::with_epochs("restarts", 1);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    for round in 0..8 {
        net.pay_through("alice", bob, 1);
        let victim = round % 4;
        net.stop(victim);
        thread::sleep(Duration::from_millis(120 * (round as u64 % 4)));
        net.restart(victim);
    }
    let epoch = net.wait_until_level();
    assert!(epoch >= 1);
    for v in 0..4 {
        assert_eq!((net.view(v, &alice).balance, net.view(v, &bob).balance), (992, 1008), "validator {v}");
    }
}

#[test]
fn validators_replace_their_key_trees_before_they_run_out() {
    // A tree of 64 one-time keys, of which 8 are held back from payments.
    let options = Options { epoch: 1, height: 6, key_reserve: 8, ..Options::default() };
    let mut net = TestNet::start_with("rekey", options);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let first = net.pay_through("alice", bob, 1);
    // Sixty payments need more than a hundred signatures from each
    // validator: far more than one tree holds.
    for _ in 1..60 {
        net.pay_through("alice", bob, 1);
    }
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!((net.view(v, &alice).balance, net.view(v, &alice).seq), (940, 60), "validator {v}");
        assert_eq!(net.info(v).keys_total, 64);
    }
    // Someone following from the genesis file sees the validators' keys change.
    let client = net.client();
    client.follow();
    assert!(client.head().validators.len() >= 2, "no validator ever changed its key tree");
    if let Done::Receipt(done) = first {
        let made_in = done.receipt.certificate.epoch;
        assert!(client.committee_at(made_in).unwrap().verify_receipt(&done.receipt).is_ok());
        assert!(client.committee().at_epoch(made_in).verify_receipt(&done.receipt).is_err());
    }
    // A validator restarted with its original key file finds its newer one.
    net.stop(0);
    net.restart(0);
    wait_until("all four validators confirm a payment", || {
        matches!(net.pay_through("alice", bob, 1), Done::Receipt(done) if done.confirmations == 4)
    });
}

/// Stands in for the validator at `address` and answers every request with
/// whatever `answer` makes of it. This is how a test plays a validator that
/// lies. The validator it replaces must be stopped first.
fn impostor(address: &str, answer: impl Fn(Request) -> Response + Send + Sync + 'static) {
    let listener = TcpListener::bind(address).unwrap();
    let answer = Arc::new(answer);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let answer = answer.clone();
            thread::spawn(move || {
                let mut stream = stream;
                while let Ok(Some(frame)) = read_frame(&mut stream) {
                    let Ok(request) = borsh::from_slice::<Request>(&frame) else { break };
                    let response = borsh::to_vec(&answer(request)).unwrap();
                    if write_frame(&mut stream, &response).is_err() {
                        break;
                    }
                }
            });
        }
    });
}

#[test]
fn a_validator_is_not_fooled_by_a_wrong_state_for_a_real_checkpoint() {
    let mut net = TestNet::with_epochs("wrong-state", 2);
    let bob = net.address("bob");
    let epoch = net.wait_until_level();
    net.stop(2);
    net.pay_through("alice", bob, 100);
    wait_until("the other three close the epoch", || [0, 1, 3].iter().all(|v| net.info(*v).epoch > epoch));
    net.wait_until_level();
    // The genuine checkpoint that closed the epoch validator 3 missed, and
    // the genuine state it led to.
    let Response::Checkpoints(checkpoints) = net.ask(0, &Request::Checkpoints { from: epoch, max: 1 }) else {
        panic!("no checkpoint")
    };
    let decided = checkpoints[0].clone();
    for v in [0, 1, 3] {
        net.stop(v);
    }
    let genuine = fs::read(net.dir.join(format!("data-0/epochs/{}/state", epoch + 1))).unwrap();
    // A state in which bob is rich. Nobody agreed to it.
    let mut rich = net.genesis.clone();
    rich.allocations.iter_mut().find(|allocation| allocation.account == bob).unwrap().amount = 1_000_000_000;
    let forged = orien_ledger::State::from_genesis(&rich).snapshot();

    // The other three validators are replaced by ones that show the real
    // checkpoint and then hand over a state of their own making.
    let honest = Arc::new(std::sync::atomic::AtomicBool::new(false));
    for v in [0, 1, 3] {
        let (decided, genuine, forged, honest) = (decided.clone(), genuine.clone(), forged.clone(), honest.clone());
        impostor(&net.genesis.validators[v].address, move |request| match request {
            Request::Sync { epoch: asked, .. } if asked == epoch => Response::Sync {
                epoch: epoch + 1,
                certs: Vec::new(),
                certs_total: 0,
                messages: Vec::new(),
                messages_total: 0,
                decided: Some(decided.clone()),
            },
            Request::Snapshot { epoch: asked, offset, .. } if asked == epoch + 1 => {
                let state = if honest.load(std::sync::atomic::Ordering::SeqCst) { &genuine } else { &forged };
                Response::Snapshot(Some((state[(offset as usize).min(state.len())..].to_vec(), state.len() as u64)))
            }
            _ => Response::Unavailable("not this one".into()),
        });
    }

    net.restart(2);
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(net.info(2).epoch, epoch, "it moved on to a state nobody agreed to");
    assert_eq!(net.view(2, &bob).balance, START as i128);
    // Offered the state the checkpoint does name, it takes it.
    honest.store(true, std::sync::atomic::Ordering::SeqCst);
    wait_until("it takes the genuine state", || net.info(2).epoch == epoch + 1);
    assert_eq!(net.view(2, &bob).balance, 1100);
}

#[test]
fn a_wallet_is_not_fooled_by_a_validator_lying_about_an_account() {
    let mut net = TestNet::with_epochs("lying-proof", 1);
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    net.pay_through("alice", bob, 100);
    let paid_in = net.info(1).epoch;
    net.wait_for_epoch(paid_in + 1);
    net.wait_until_level();
    // With two validators gone no further epoch can close, so the state the
    // wallet checks against stays put. The first validator is replaced by
    // one that says alice never paid and holds a fortune.
    net.stop(0);
    net.stop(3);
    impostor(&net.genesis.validators[0].address, move |request| match request {
        Request::Proof(_) => Response::Proof {
            epoch: paid_in + 1,
            account: Some((
                orien_ledger::Account {
                    balance: 1_000_000_000,
                    seq: 0,
                    signer: Signer::First,
                    spare: Hash32::default(),
                    spares_used: 0,
                    staked: 0,
                    unstaking: 0,
                    releases: Vec::new(),
                    validator: None,
                },
                orien_crypto::merkle::Path { index: 0, siblings: Vec::new() },
            )),
        },
        _ => Response::Unavailable("not this one".into()),
    });

    let client = net.client();
    assert!(client.follow() > paid_in);
    let proven = client.proven(&alice).expect("two honest validators still answer");
    assert_eq!((proven.balance, proven.seq), (900, 1), "the wallet took a validator's word for it");
    // So the wallet will not reuse the key of a payment number already spent.
    assert_eq!(client.account(&alice, 0).seq, 1);
}

#[test]
fn validators_leave_the_list_and_come_back_with_their_stake() {
    // The four launch seats weigh 1 each. Once real stake passes 4 they
    // leave, and only validators someone staked for remain.
    let options = Options { crowd: 2, epoch: 1, min_stake: 10, ..Options::default() };
    let net = TestNet::start_with("comeback", options);
    let carol = net.address("carol");
    let places = || -> Vec<Option<u16>> { net.running().iter().map(|v| net.info(*v).validator).collect() };
    assert_eq!(places(), [Some(0), Some(1), Some(2), Some(3)]);

    // Stake backs v1 and v2 only. After the next checkpoint v3 and v4 are
    // out: they follow the network and vote on nothing.
    net.stake("alice", 0, 100);
    net.stake("bob", 1, 200);
    // The largest stake comes first. The two stakes may land in different
    // epochs, so this waits for the list with both.
    wait_until("v3 and v4 leave the list", || places() == [Some(1), Some(0), None, None]);
    net.pay_through("alice", carol, 5);
    net.wait_until_level();

    // Stake for v3 and v4, and they are back.
    net.stake("w0", 2, 300);
    net.stake("w1", 3, 50);
    wait_until("v3 and v4 return", || places() == [Some(2), Some(1), Some(0), Some(3)]);
    net.pay_through("bob", carol, 7);
    net.wait_until_level();
    assert_eq!(net.view(3, &carol).balance, 12);
}

#[test]
fn a_validator_outside_the_genesis_file_joins_once_staked() {
    // Seats heavy enough that a newcomer's stake does not push them out.
    let options = Options { epoch: 1, min_stake: 10, seat: 1_000, ..Options::default() };
    let net = TestNet::start_with("newcomer", options);
    let key = ValidatorKey { seed: [99; 32], height: net.options.height };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let config = NodeConfig {
        sync_interval: net.options.sync,
        close_pace: net.options.pace,
        batch_window: net.options.window,
        ..NodeConfig::new(net.genesis.clone(), key.clone(), net.dir.join("data-new"))
    };
    let newcomer = Node::start(config, Some(listener)).unwrap();
    assert_eq!(newcomer.index, None);
    let info = |address: &str| match rpc(address, &Request::Info, Duration::from_secs(5)).unwrap() {
        Response::Info(info) => info,
        other => panic!("unexpected answer: {other:?}"),
    };
    assert_eq!(info(&address).validator, None, "not chosen yet");

    // It follows the network while outside the list...
    let carol = net.address("carol");
    net.pay_through("alice", carol, 5);
    wait_until("the newcomer follows the checkpoints", || info(&address).epoch >= net.info(0).epoch);

    // ...and is one of the validators once a stake gets it chosen. It then
    // takes part in closing every epoch, and holds what everyone holds.
    net.stake_for("bob", &key, address.clone(), 100);
    wait_until("the newcomer is chosen", || info(&address).validator == Some(4));
    let joined = info(&address).epoch;
    wait_until("two more epochs close with it", || net.info(0).epoch >= joined + 2);

    // A wallet that follows the checkpoints asks it to vote too.
    let client = net.client();
    let mut wallet = net.wallet("alice");
    let status = client.account(&wallet.address(), wallet.next_seq());
    let signed = wallet.sign_order(status.seq, carol, 7, status.signer.as_ref(), &net.network).unwrap();
    wait_until("the newcomer votes for a payment", || {
        client.follow();
        client.endpoints().contains(&address) && client.collect_votes(&signed).0.contains_key(&4)
    });
    while client.pay(&signed, Duration::from_secs(10)).is_err() {
        client.follow();
    }
    wallet.confirm(status.seq).unwrap();
    let epoch = net.wait_until_level();
    wait_until("the newcomer holds the same accounts", || {
        let root = |response: Response| match response {
            Response::Root(root) => Some(root),
            _ => None,
        };
        let theirs = root(net.ask(0, &Request::Root));
        let its = root(rpc(&address, &Request::Root, Duration::from_secs(5)).unwrap());
        info(&address).epoch == epoch && its.is_some() && its == theirs
    });
    newcomer.stop();
}

#[test]
fn a_validator_that_signs_twice_with_one_key_loses_its_stake() {
    let options = Options { crowd: 0, epoch: 2, min_stake: 10, ..Options::default() };
    let net = TestNet::start_with("cheat", options);
    net.stake("alice", 0, 100);
    net.stake("bob", 1, 200);
    let places = || -> Vec<Option<u16>> { net.running().iter().map(|v| net.info(*v).validator).collect() };
    wait_until("only v1 and v2 are validators", || places() == [Some(1), Some(0), None, None]);
    let client = net.client();
    let alice = net.address("alice");
    let staked = |client: &Client| {
        client.follow();
        client.proven(&alice).map_or(0, |account| account.staked)
    };
    assert_eq!(staked(&client), 100);

    // v1 votes on a payment as usual. Then, playing a dishonest v1, the
    // test signs a different batch with the very key v1 used, and hands the
    // result to v2 as if it were a certificate.
    let tree = KeyTree::generate(net.keys[0].seed, net.keys[0].height);
    let carol = net.address("carol");
    wait_until("v1 is caught", || {
        let paid = net.pay_through("bob", carol, 1);
        let Done::Receipt(done) = paid else { return false };
        let real = done.receipt.certificate;
        let committee = { client.follow(); client.committee() };
        let Some(vote) = real.votes.iter().find(|vote| vote.validator == 1) else { return false };
        let digest = real.order.digest(&net.network);
        let batch = orien_crypto::merkle::Tree::build(&[digest, Hash32([7; 32])]);
        let message = committee.batch_message(&batch.root());
        let signature = tree.sign_with_leaf(vote.signature.leaf, message.as_bytes());
        let forged = orien_ledger::Vote { validator: 1, batch_path: batch.path(0), signature };
        let handed = orien_ledger::Certificate { votes: vec![forged], ..real };
        let _ = net.ask(1, &Request::SubmitCertificate(handed));
        // Caught once the next checkpoint drops it from the list.
        thread::sleep(Duration::from_millis(2500));
        net.info(0).validator.is_none()
    });

    // Its stake is gone, burned, and it is no longer a validator.
    wait_until("alice's stake is burned", || staked(&client) == 0);
    assert_eq!(client.proven(&orien_ledger::FORFEITED).map(|account| account.balance), Some(100));
    assert_eq!(places()[..2], [None, Some(0)], "v1 is out; v2 is now the only validator");
}

#[test]
fn work_earns_coins_at_the_next_checkpoint() {
    // Two-second epochs release about a coin each, and one try in 64 makes
    // a share.
    let options = Options { epoch: 2, cap: orien_ledger::release::CAP, first_target: u64::MAX >> 6, ..Options::default() };
    let net = TestNet::start_with("mining", options);
    let carol = net.address("carol");
    let client = net.client();
    let earned = |client: &Client| {
        client.follow();
        client.proven(&carol).map_or(0, |account| account.balance)
    };
    assert_eq!(earned(&client), 0, "carol starts with nothing");

    // Mine for carol: ask for the seed and target, find a few shares and
    // hand them in, until a checkpoint has paid her.
    let mut from = 0u64;
    wait_until("carol is paid for her work", || {
        client.follow();
        let (epoch, seed, target) = client.followed_work();
        let math = client.committee().work().math(epoch);
        let shares: Vec<orien_ledger::close::Share> = (from..)
            .map(|nonce| orien_ledger::close::Share { account: carol, nonce })
            .filter(|share| share.counts(math, &seed, target))
            .take(5)
            .collect();
        from = shares.last().map_or(from, |share| share.nonce + 1);
        // Validators closing an epoch take no shares for that second; the
        // ones that open the next take them again.
        for share in shares {
            client.submit_share(share);
        }
        thread::sleep(Duration::from_millis(500));
        earned(&client) > 0
    });
    let paid = earned(&client);
    println!("carol earned {} coins for her work", orien_ledger::coins(paid));
    // Each validator agrees on what she holds.
    net.wait_until_level();
}

#[test]
fn validators_keep_each_accounts_incoming_history() {
    use orien_ledger::Income;
    let options = Options { epoch: 2, cap: orien_ledger::release::CAP, first_target: u64::MAX >> 6, ..Options::default() };
    let net = TestNet::start_with("history", options);
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let client = net.client();

    // A payment shows at once, in the epoch under way, and keeps its place
    // once a checkpoint has closed that epoch.
    net.pay_through("alice", bob, 100);
    let page = client.history(&bob, 0, 100).expect("validators keep history");
    let paid = page.entries.iter().find(|e| e.income == Income::Payment).expect("the payment is listed");
    assert_eq!((paid.amount, paid.counterparty, paid.order), (100, Some(alice), Some((alice, 0))));
    let made_in = paid.epoch;
    // Alice's side: what she sent, for wallets that ask, and only for them.
    let sent = |page: &orien_net::history::HistoryPage| {
        page.entries.iter().any(|e| e.income == Income::Sent && e.amount == 100 && e.counterparty == Some(bob))
    };
    assert!(sent(&client.activity(&alice, 0, 100).unwrap()), "shown while its epoch is under way");
    assert!(!sent(&client.history(&alice, 0, 100).unwrap()), "older wallets see only what came in");
    net.wait_for_epoch(made_in + 1);
    wait_until("the payment is recorded with its checkpoint's time", || {
        let page = client.history(&bob, 0, 100).unwrap();
        page.entries.iter().any(|e| e.income == Income::Payment && e.epoch == made_in && e.time.is_some())
    });
    wait_until("what alice sent is recorded too", || {
        let page = client.activity(&alice, 0, 100).unwrap();
        page.entries.iter().any(|e| e.income == Income::Sent && e.epoch == made_in && e.time.is_some())
    });

    // Mining: one entry for each epoch's reward, adding up to the balance.
    let mut from = 0u64;
    wait_until("carol is paid for her work", || {
        client.follow();
        let (epoch, seed, target) = client.followed_work();
        let math = client.committee().work().math(epoch);
        let shares: Vec<orien_ledger::close::Share> = (from..)
            .map(|nonce| orien_ledger::close::Share { account: carol, nonce })
            .filter(|share| share.counts(math, &seed, target))
            .take(5)
            .collect();
        from = shares.last().map_or(from, |share| share.nonce + 1);
        for share in shares {
            client.submit_share(share);
        }
        thread::sleep(Duration::from_millis(500));
        client.proven(&carol).is_some_and(|account| account.balance > 0)
    });
    net.wait_until_level();
    let balance = net.view(0, &carol).balance as u64;
    let page = client.history(&carol, 0, 1000).unwrap();
    assert!(page.unknown.is_empty(), "every epoch is known: {:?}", page.unknown);
    assert!(page.entries.iter().all(|e| e.income == Income::Mining && e.time.is_some()));
    let epochs: std::collections::BTreeSet<u64> = page.entries.iter().map(|e| e.epoch).collect();
    assert_eq!(epochs.len(), page.entries.len(), "one reward per epoch");
    assert_eq!(page.entries.iter().map(|e| e.amount).sum::<u64>(), balance);

    // Pages end on whole epochs and say where to go on.
    if page.entries.len() >= 2 {
        let first = client.history(&carol, 0, 1).unwrap();
        assert_eq!(first.entries.len(), 1);
        let next = first.next.expect("there is more");
        let rest = client.history(&carol, next, 1000).unwrap();
        assert_eq!(rest.entries.len() + 1, page.entries.len());
    }
}

#[test]
fn mining_moves_to_randomx_at_the_epoch_set_and_keeps_paying() {
    use orien_ledger::Income;
    use orien_ledger::work::Math;
    // BLAKE3 for epochs 0 to 2, RandomX from epoch 3 at an easy target.
    let switch = 3;
    let randomx = orien_ledger::RandomXFrom { epoch: switch, target: u64::MAX >> 3 };
    let options =
        Options { epoch: 3, cap: orien_ledger::release::CAP, first_target: u64::MAX >> 6, randomx: Some(randomx), ..Options::default() };
    let net = TestNet::start_with("randomx", options);
    let carol = net.address("carol");
    let client = net.client();
    let mut from = 0u64;
    let mut maths = std::collections::BTreeSet::new();
    wait_until("carol is paid for RandomX work after the switch", || {
        client.follow();
        let (epoch, seed, target) = client.followed_work();
        let math = client.committee().work().math(epoch);
        assert_eq!(math == Math::RandomX, epoch >= switch);
        if epoch == switch {
            assert_eq!(target, randomx.target, "the switch starts at the target set for it");
        }
        // The miner's own hasher, as `orien mine` and the app use it, in
        // light mode: the full dataset takes longer to make than these
        // test epochs last.
        let ready = orien_ledger::work::Prepared::light_only(math, &seed);
        let hasher = ready.hasher();
        let shares: Vec<orien_ledger::close::Share> = (from..)
            .map(|nonce| orien_ledger::close::Share { account: carol, nonce })
            .filter(|share| hasher.number(share) < target)
            .take(3)
            .collect();
        from = shares.last().map_or(from, |share| share.nonce + 1);
        for share in &shares {
            assert!(share.counts(math, &seed, target), "validators judge it as the miner did");
            client.submit_share(*share);
        }
        maths.insert(format!("{math:?}"));
        thread::sleep(Duration::from_millis(300));
        let page = client.history(&carol, 0, 1000).unwrap();
        page.entries.iter().any(|e| e.income == Income::Mining && e.epoch >= switch)
            && page.entries.iter().any(|e| e.income == Income::Mining && e.epoch < switch)
    });
    assert_eq!(maths.len(), 2, "mined under both");
    // Every validator agrees on what she holds.
    net.wait_until_level();
}

#[test]
fn the_launch_seats_move_to_names_and_a_new_wallet_still_finds_them() {
    // The seats are reached at their genesis addresses (stand-ins here)
    // until epoch 2, and by name from then on.
    let from = 2;
    let options = Options { epoch: 3, seat_names_from: Some(from), ..Options::default() };
    let net = TestNet::start_with("seat-names", options);
    let carol = net.address("carol");
    net.pay_through("alice", carol, 50);
    net.wait_for_epoch(from);
    // The validator list the checkpoint names is reached by name.
    let client = net.client();
    client.follow();
    let names: Vec<String> = client.committee().members().iter().map(|m| m.address.clone()).collect();
    assert!(names.iter().all(|name| name.starts_with("localhost:")), "seats are reached by name: {names:?}");
    // The old addresses go, as if their server had stopped for good.
    for proxy in &net.proxies {
        proxy.stop();
    }
    // A brand-new wallet knows only the genesis file, whose addresses no
    // longer answer, and still finds the network by the seats' names.
    let fresh = net.client();
    assert!(fresh.follow() >= from, "a new wallet catches up through the names");
    assert!(fresh.committee().members().iter().all(|m| m.address.starts_with("localhost:")));
    // Through it, payments are final and epochs carry on closing.
    net.pay_through_with(&fresh, "alice", carol, 70);
    let reached = fresh.epoch();
    net.wait_for_epoch(reached + 1);
    net.pay_through_with(&fresh, "bob", carol, 30);
    net.wait_until_level();
    assert_eq!(net.view(0, &carol).balance, 150);
}

#[test]
fn the_live_networks_identity_is_unchanged() {
    let genesis = orien_net::keys::load_genesis(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/network/genesis.json")).unwrap();
    assert!(genesis.randomx.is_none());
    assert_eq!(genesis.id().to_string(), orien_ledger::work::LIVE_NETWORK);
    assert!(orien_net::node::is_live(&genesis.id()));
    // RandomX from epoch 21, starting at its own target.
    let work = orien_ledger::Committee::new(&genesis).work();
    use orien_ledger::work::Math;
    assert_eq!((work.math(20), work.math(21)), (Math::Blake3, Math::RandomX));
    assert_eq!(work.randomx_target, orien_ledger::work::LIVE_RANDOMX_TARGET);
    assert_eq!(work.target, genesis.first_target, "the live network started on BLAKE3's target");
    assert_eq!(orien_net::node::open_claims_from(&genesis.id()), 23);
    // The four launch seats are reached by name from the epoch set.
    assert!(genesis.seat_addresses.is_none());
    let committee = orien_ledger::Committee::new(&genesis);
    let from = orien_ledger::work::LIVE_SEAT_NAMES_FROM;
    assert_eq!(committee.seat_address(0, from - 1), None);
    assert_eq!(committee.seat_address(0, from), Some("v1.orien.network:7101"));
    assert_eq!(committee.seat_address(3, from), Some("v4.orien.network:7104"));
    assert!(committee.seat_names().iter().all(|name| name.len() <= orien_ledger::types::MAX_ADDRESS));

    // Ten thousand shares an hour from the epoch set: the target for that
    // epoch is ten times easier on top of the usual step, and moves as
    // before after it.
    let more = orien_ledger::work::LIVE_MORE_SHARES_FROM;
    assert_eq!((work.wanted(more - 1), work.wanted(more)), (1_000, 10_000));
    let target = 1_000_000_000u64;
    let before = committee.at_epoch(more - 1).with_target(target);
    assert_eq!(orien_ledger::close::next_target(&before, 1_000), target * 10);
    assert_eq!(orien_ledger::close::next_target(&before, 500), target * 20);
    assert_eq!(orien_ledger::close::next_target(&before, 0), target * 40);
    let after = committee.at_epoch(more).with_target(target);
    assert_eq!(orien_ledger::close::next_target(&after, 10_000), target);
    assert_eq!(orien_ledger::close::next_target(&after, 20_000), target / 2);
    let earlier = committee.at_epoch(more - 2).with_target(target);
    assert_eq!(orien_ledger::close::next_target(&earlier, 1_000), target);

    // Fifty thousand from the next epoch set, the same way: five times
    // easier at once on top of the usual step.
    let most = orien_ledger::work::LIVE_MOST_SHARES_FROM;
    assert_eq!((work.wanted(most - 1), work.wanted(most)), (10_000, 50_000));
    let before = committee.at_epoch(most - 1).with_target(target);
    assert_eq!(orien_ledger::close::next_target(&before, 10_000), target * 5);
    assert_eq!(orien_ledger::close::next_target(&before, 5_000), target * 10);
    let after = committee.at_epoch(most).with_target(target);
    assert_eq!(orien_ledger::close::next_target(&after, 50_000), target);
    assert_eq!(orien_ledger::close::next_target(&after, 100_000), target / 2);

    // A network on RandomX from the start starts at RandomX's target.
    let mut fresh = genesis.clone();
    fresh.network = "fresh".into();
    fresh.randomx = Some(orien_ledger::RandomXFrom { epoch: 0, target: 12345 });
    let work = orien_ledger::Committee::new(&fresh).work();
    assert_eq!((work.math(0), work.target), (Math::RandomX, 12345));
}

#[test]
fn a_validator_keeps_only_the_current_epochs_logs() {
    let net = TestNet::with_epochs("prune", 1);
    let bob = net.address("bob");
    // What a validator holds besides its state snapshots, in bytes.
    let held = |v: usize| -> (u64, Vec<String>) {
        let epochs = net.dir.join(format!("data-{v}")).join("epochs");
        let current = net.info(v).epoch;
        let (mut bytes, mut old) = (0, Vec::new());
        for folder in fs::read_dir(&epochs).unwrap() {
            let folder = folder.unwrap().path();
            let epoch: u64 = folder.file_name().unwrap().to_str().unwrap().parse().unwrap();
            for file in fs::read_dir(&folder).unwrap() {
                let file = file.unwrap();
                let name = file.file_name().into_string().unwrap();
                if name == "state" {
                    continue;
                }
                if epoch < current {
                    old.push(format!("{epoch}/{name}"));
                }
                bytes += file.metadata().unwrap().len();
            }
        }
        (bytes, old)
    };
    let mut sizes = Vec::new();
    for _ in 0..10 {
        net.pay_through("alice", bob, 1);
        // Everything held right after a payment: all logs, of all epochs.
        sizes.push(held(0).0);
        let epoch = net.info(0).epoch;
        net.wait_for_epoch(epoch + 1);
        // Once a switch is done, earlier epochs keep only their snapshots.
        wait_until("earlier epochs keep only their snapshots", || held(0).1.is_empty());
    }
    println!("logs held after each epoch, in bytes: {sizes:?}");
    let epochs = net.info(0).epoch;
    assert!(epochs >= 10, "{epochs} epochs closed");
    // Ten epochs in, a validator holds no more than in the first: the logs
    // do not pile up.
    assert!(sizes.iter().all(|size| *size > 0), "{sizes:?}");
    let most = *sizes.iter().max().unwrap();
    assert!(most < 3 * sizes[0], "{sizes:?}");
}

#[test]
fn a_validator_away_for_many_epochs_catches_up_by_the_checkpoints() {
    let mut net = TestNet::with_epochs("jump", 1);
    let bob = net.address("bob");
    net.pay_through("alice", bob, 5);
    let left = net.info(3).epoch;
    net.stop(3);

    // The others carry on for a good while, with payments, and drop the
    // snapshots of the epochs v4 is missing.
    for _ in 0..3 {
        net.pay_through("alice", bob, 1);
    }
    let now = net.info(0).epoch;
    net.wait_for_epoch(now.max(left + 8));
    let missed = net.dir.join("data-0").join("epochs").join((left + 1).to_string());
    wait_until("the others no longer hold the state v4 would need next", || !missed.exists());

    // Back on, v4 cannot take the next epoch's state from anyone. It follows
    // the checkpoints instead, takes the latest state, and is level with
    // the others: same epoch, same balances.
    net.restart(3);
    net.wait_until_level();
    let caught_up = net.info(3).epoch;
    println!("v4 left at epoch {left} and is back at epoch {caught_up}");
    assert!(caught_up >= left + 8);
    // It saved the checkpoints it skipped, so it can show them to others.
    let saved = |epoch: u64| net.dir.join("data-3").join("checkpoints").join(epoch.to_string()).exists();
    assert!((left..caught_up).all(saved));
    // And it works as a validator again.
    let done = net.pay_through("bob", net.address("carol"), 2);
    if let Done::Receipt(done) = done {
        assert!(done.receipt.certificate.votes.len() >= 3);
    }
    net.wait_until_level();
}

#[test]
fn connections_are_counted_per_address_and_in_total() {
    let mut admission = Admission::new(Limits { total: 5, per_address: 2, idle: Duration::from_secs(1) });
    let (one, two, three): (std::net::IpAddr, std::net::IpAddr, std::net::IpAddr) =
        ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap(), "10.0.0.3".parse().unwrap());
    assert!(admission.admit(one, false) && admission.admit(one, false));
    assert!(!admission.admit(one, false), "a third from one address is turned away");
    assert!(admission.admit(two, false) && admission.admit(two, false));
    assert!(admission.admit(three, false));
    assert!(!admission.admit(three, false), "five in all is the limit");
    // A validator is never turned away, even when everything is full.
    assert!(admission.admit(three, true));
    // Closing a connection makes room again.
    admission.release(one);
    admission.release(three);
    assert!(admission.admit(one, false));
}

#[test]
fn a_flood_of_idle_and_junk_connections_does_not_stop_payments() {
    let limits = Limits { total: 1024, per_address: 64, idle: Duration::from_secs(1) };
    let net = TestNet::start_with("flood", Options { limits, ..Options::default() });
    let target = &net.genesis.validators[0].address;
    // Hundreds of connections that send nothing, and many that send junk.
    let idle: Vec<std::net::TcpStream> = (0..300).filter_map(|_| std::net::TcpStream::connect(target).ok()).collect();
    let mut junk = Vec::new();
    for _ in 0..50 {
        if let Ok(mut stream) = std::net::TcpStream::connect(target) {
            let _ = write_frame(&mut stream, b"this is not a request");
            junk.push(stream);
        }
    }
    // Payments go through all the while.
    let bob = net.address("bob");
    for _ in 0..5 {
        net.pay("alice", bob, 1);
    }
    // Junk is answered once and the connection closed.
    for mut stream in junk {
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = std::io::BufReader::new(&mut stream);
        let _ = read_frame(&mut reader);
        assert!(matches!(read_frame(&mut reader), Ok(None) | Err(_)), "junk ends the connection");
    }
    // Silent connections are closed once they have been idle for a second.
    thread::sleep(Duration::from_millis(1500));
    for mut stream in idle {
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut byte = [0u8; 1];
        assert!(matches!(std::io::Read::read(&mut stream, &mut byte), Ok(0) | Err(_)), "an idle connection is closed");
    }
    // And the network still works.
    net.pay("bob", net.address("carol"), 1);
    net.wait_until_level();
}

#[test]
fn a_wallet_restored_from_its_phrase_carries_on_where_the_first_left_off() {
    let net = TestNet::start("restore");
    let bob = net.address("bob");
    net.pay("alice", bob, 7);
    // On another computer, the same phrase and the same account: it learns
    // from the network that payment #0 is done, and pays #1.
    let words = net.wallet("alice").phrase().unwrap().join(" ");
    let master = orien_net::phrase::from_words(&words).unwrap();
    let copy = net.dir.join("alice-copy.wallet");
    let restored = Wallet::from_master(&copy, master, 1).unwrap();
    assert_eq!(restored.address(), net.address("alice"));
    fs::copy(&copy, net.dir.join("alice2.wallet")).unwrap();
    net.pay("alice2", bob, 5);
    assert_eq!(net.view(0, &bob).balance, START as i128 + 12);
    assert_eq!(net.view(0, &net.address("alice")).seq, 2);
    // Account 2 of the same phrase is another account altogether.
    let other = Wallet::from_master(&net.dir.join("alice-desktop.wallet"), master, 2).unwrap();
    assert_ne!(other.address(), net.address("alice"));
}

#[test]
fn a_wallet_with_a_password_signs_only_with_it() {
    let net = TestNet::start("password");
    let path = net.dir.join("alice.wallet");
    let before = fs::read_to_string(&path).unwrap();
    let mut wallet = net.wallet("alice");
    let (address, phrase) = (wallet.address(), wallet.phrase().unwrap().join(" "));
    wallet.set_password(Some("correct horse battery")).unwrap();

    // On disk the secret is sealed: neither the seed nor the phrase's
    // secret is there to read.
    let after = fs::read_to_string(&path).unwrap();
    let secret = |text: &str| serde_json_seed(text);
    assert!(secret(&before).is_some() && secret(&after).is_none(), "{after}");
    assert!(!after.contains(&hex_of(&orien_net::phrase::from_words(&phrase).unwrap())));

    // Its address and balance need no password.
    assert_eq!(Wallet::peek(&path).unwrap().0, address);
    // Signing does: no password, or the wrong one, opens nothing.
    assert!(Wallet::load(&path).err().unwrap().to_string().contains("has a password"));
    assert!(Wallet::open(&path, Some("wrong")).err().unwrap().to_string().contains("not this wallet's password"));
    let mut opened = Wallet::open(&path, Some("correct horse battery")).unwrap();
    assert_eq!(opened.phrase().unwrap().join(" "), phrase);

    // It pays, and stays sealed after recording the payment.
    opened.accept_fee(net.genesis.fee);
    let client = net.client();
    let status = client.account(&address, opened.next_seq());
    let bob = net.address("bob");
    let signed = opened.sign_order(status.seq, bob, 3, status.signer.as_ref(), &net.network).unwrap();
    client.transfer(&signed).unwrap();
    opened.confirm(status.seq).unwrap();
    assert!(secret(&fs::read_to_string(&path).unwrap()).is_none());
    assert_eq!(net.view(0, &bob).balance, START as i128 + 3);

    // Removing the password brings the plain file back.
    let mut opened = Wallet::open(&path, Some("correct horse battery")).unwrap();
    opened.set_password(None).unwrap();
    assert_eq!(Wallet::load(&path).unwrap().address(), address);
}

/// The plain seed a wallet file holds, if it holds one.
fn serde_json_seed(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value.get("seed").and_then(|seed| seed.as_str()).filter(|seed| !seed.is_empty()).map(str::to_string)
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn an_unlocked_wallet_signs_within_its_limits_only() {
    use orien_net::unlock::{self, Limits as Unlock, Session, Signing};
    let net = TestNet::start("unlock");
    let path = net.dir.join("alice.wallet");
    net.wallet("alice").set_password(Some("correct horse battery")).unwrap();
    assert!(Signing::unlocked(&path).unwrap().is_none(), "nothing is unlocked yet");

    // Unlocked for an hour, for exactly a payment of 4 and one of 5, fees
    // included.
    let fees = net.genesis.fee;
    let cost = |amount: u64| amount + fees.on(&Kind::Payment, amount);
    let until = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 3_600;
    let limits = Unlock { until, spend: cost(4) + cost(5) };
    let wallet = Wallet::open(&path, Some("correct horse battery")).unwrap();
    let (ready, started) = std::sync::mpsc::channel();
    let helper = thread::spawn(move || unlock::serve(wallet, limits, move || ready.send(()).unwrap()));
    started.recv().unwrap();

    let mut signing = Signing::unlocked(&path).unwrap().expect("the session is found");
    signing.accept_fee(fees);
    let client = net.client();
    let bob = net.address("bob");
    let pay = |signing: &mut Signing, amount: u64| {
        let status = client.account(&signing.address(), signing.next_seq());
        let signed = signing.sign(Kind::Payment, status.seq, bob, amount, status.signer.as_ref(), &net.network)?;
        // Asking again for the very same payment hands back the same order
        // and spends nothing more; a different one under its number is
        // refused while it is unfinished.
        assert_eq!(signing.sign(Kind::Payment, status.seq, bob, amount, None, &net.network).unwrap(), signed);
        assert!(signing.sign(Kind::Payment, status.seq, bob, amount + 1, None, &net.network).is_err());
        assert!(signing.signed_by_this_wallet(&signed, &net.network));
        client.transfer(&signed).unwrap();
        signing.confirm(status.seq)
    };
    pay(&mut signing, 4).unwrap();
    pay(&mut signing, 5).unwrap();
    assert_eq!(net.view(0, &bob).balance, START as i128 + 9);

    // The limit is used up: even the smallest payment is refused, and
    // nothing is signed for it.
    let refused = pay(&mut signing, 1).unwrap_err().to_string();
    assert!(refused.contains("allows only 0"), "{refused}");
    assert_eq!(signing.next_seq(), 2);
    assert!(signing.pending().is_none());
    let session = Session::find(&path).unwrap();
    assert_eq!(session.status().unwrap().left(), 0);

    // The helper kept the wallet file up to date, still sealed.
    assert_eq!(Wallet::peek(&path).unwrap().1, 2);
    assert!(Wallet::load(&path).is_err(), "the file still needs the password");

    // Locking ends the session and removes its description.
    session.lock().unwrap();
    helper.join().unwrap().unwrap();
    assert!(Session::find(&path).is_none());
    assert!(!unlock::session_path(&path).exists());
}

#[test]
fn an_unlock_ends_on_time() {
    use orien_net::unlock::{self, Limits as Unlock, Session};
    let net = TestNet::start("unlock-ends");
    let path = net.dir.join("alice.wallet");
    let until = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 2;
    let wallet = net.wallet("alice");
    let (ready, started) = std::sync::mpsc::channel();
    let helper =
        thread::spawn(move || unlock::serve(wallet, Unlock { until, spend: 1 }, move || ready.send(()).unwrap()));
    started.recv().unwrap();
    assert!(Session::find(&path).is_some());
    // Nobody asks anything; the session ends by itself.
    helper.join().unwrap().unwrap();
    assert!(Session::find(&path).is_none());
    assert!(!unlock::session_path(&path).exists());
}

/// A swap's ORN side: Alice locks for Bob behind a secret; Bob, whose
/// wallet has never held anything, claims with it; anyone can then read the
/// secret, as the other side of a swap would.
#[test]
fn a_locked_payment_is_claimed_across_the_network() {
    use orien_ledger::lock_hash;
    let net = TestNet::start("lock-claim");
    let client = net.client();
    let alice = net.address("alice");
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let mut payee = Wallet::create(&net.dir.join("newcomer.wallet")).unwrap();
    let newcomer = payee.address();
    let secret = [7u8; 32];
    let hash = lock_hash(&secret);

    // Alice locks 300 for the newcomer, claimable until two epochs on.
    client.follow();
    let until = client.epoch() + 2;
    let status = client.account(&alice, sender.next_seq());
    let lock = sender
        .sign(Kind::Lock { hash, until }, status.seq, newcomer, 300, status.signer.as_ref(), &net.network)
        .unwrap();
    assert!(matches!(client.pay(&lock, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    sender.confirm(status.seq).unwrap();
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, START as i128 - 300, "validator {v}");
    }
    let waiting = client.locked(&alice, lock.order.seq, &hash);
    assert_eq!(waiting.waiting.as_ref().map(|e| (e.to, e.amount)), Some((newcomer, 300)));
    assert_eq!(waiting.secret, None);

    // The wrong secret is refused by every validator.
    let wrong = payee
        .sign(Kind::Claim { from: alice, seq: lock.order.seq, secret: [8; 32] }, 0, newcomer, 300, None, &net.network)
        .unwrap();
    assert!(matches!(net.ask(0, &Request::SubmitOrder(wrong)), Response::Rejected(Reject::WrongSecret)));
    payee.forget(0).unwrap();

    // The newcomer claims with the right one. A key that signed something
    // else is never used again, so the claim is order 0 with a fresh wallet.
    let mut payee = Wallet::create(&net.dir.join("newcomer-2.wallet")).unwrap();
    let newcomer = payee.address();
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let status = client.account(&alice, 1);
    let second = sender
        .sign(Kind::Lock { hash, until }, status.seq, newcomer, 200, status.signer.as_ref(), &net.network)
        .unwrap();
    assert!(matches!(client.pay(&second, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    let claim = payee
        .sign(Kind::Claim { from: alice, seq: second.order.seq, secret }, 0, newcomer, 200, None, &net.network)
        .unwrap();
    assert!(matches!(client.pay(&claim, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!(net.view(v, &newcomer).balance, 200, "validator {v}");
    }
    // Anyone can now read the secret, and it is checked against the lock.
    let claimed = client.locked(&alice, second.order.seq, &hash);
    assert_eq!((claimed.waiting, claimed.secret), (None, Some(secret)));
}

/// Once claims are open (from the start on a test network), anyone holding
/// the secret claims for the payee: here Carol, for a newcomer whose wallet
/// never signs anything. The coins reach the newcomer, and only them.
#[test]
fn anyone_with_the_secret_claims_for_the_payee() {
    use orien_ledger::lock_hash;
    let net = TestNet::start("open-claim");
    let client = net.client();
    let alice = net.address("alice");
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let newcomer = Wallet::create(&net.dir.join("newcomer.wallet")).unwrap().address();
    let mut stranger = Wallet::create(&net.dir.join("stranger.wallet")).unwrap();
    let secret = [9u8; 32];
    client.follow();
    let until = client.epoch() + 2;
    let status = client.account(&alice, sender.next_seq());
    let lock = sender
        .sign(Kind::Lock { hash: lock_hash(&secret), until }, status.seq, newcomer, 250, status.signer.as_ref(), &net.network)
        .unwrap();
    assert!(matches!(client.pay(&lock, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    // The stranger, who holds nothing, claims it for the newcomer.
    let claim = stranger
        .sign(Kind::Claim { from: alice, seq: lock.order.seq, secret }, 0, newcomer, 250, None, &net.network)
        .unwrap();
    assert!(matches!(client.pay(&claim, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    net.wait_until_level();
    let fee = net.genesis.fee.min;
    for v in 0..4 {
        assert_eq!(net.view(v, &newcomer).balance, (250 - fee) as i128, "validator {v}");
        assert_eq!(net.view(v, &stranger.address()).balance, 0, "validator {v}");
    }
    assert_eq!(client.locked(&alice, lock.order.seq, &lock_hash(&secret)).secret, Some(secret));
}

/// A locked payment nobody claims goes back to its sender at the checkpoint
/// closing the last epoch it could be claimed in, and a claim after that is
/// refused.
#[test]
fn an_unclaimed_lock_comes_back_after_its_deadline() {
    use orien_ledger::lock_hash;
    let net = TestNet::with_epochs("lock-refund", 2);
    let client = net.client();
    let alice = net.address("alice");
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let mut payee = Wallet::create(&net.dir.join("late.wallet")).unwrap();
    let secret = [9u8; 32];

    net.wait_until_level();
    client.follow();
    let until = client.epoch() + 2;
    let status = client.account(&alice, sender.next_seq());
    let lock = sender
        .sign(Kind::Lock { hash: lock_hash(&secret), until }, status.seq, payee.address(), 400, status.signer.as_ref(), &net.network)
        .unwrap();
    assert!(matches!(client.pay(&lock, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    assert_eq!(net.view(0, &alice).balance, START as i128 - 400);

    net.wait_for_epoch(until);
    net.wait_until_level();
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, START as i128, "validator {v} refunded it");
    }
    let late = payee
        .sign(Kind::Claim { from: alice, seq: lock.order.seq, secret }, 0, payee.address(), 400, None, &net.network)
        .unwrap();
    assert!(matches!(
        net.ask(0, &Request::SubmitOrder(late)),
        Response::Rejected(Reject::NoSuchLock | Reject::LockExpired { .. })
    ));
}

/// What a swap's buyer relies on: a locked payment proven by signatures,
/// first by its certificate, then by the checkpoint's state root, and its
/// secret proven once claimed. Nothing is proven of a payment never made.
#[test]
fn a_locked_payment_and_its_secret_are_proven_by_checkpoints() {
    use orien_ledger::lock_hash;
    let net = TestNet::with_epochs("lock-proof", 3);
    let client = net.client();
    let alice = net.address("alice");
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let mut payee = Wallet::create(&net.dir.join("proof.wallet")).unwrap();
    let secret = [11u8; 32];
    let hash = lock_hash(&secret);

    net.wait_until_level();
    client.follow();
    let until = client.epoch() + 20;
    let status = client.account(&alice, sender.next_seq());
    let lock = sender
        .sign(Kind::Lock { hash, until }, status.seq, payee.address(), 500, status.signer.as_ref(), &net.network)
        .unwrap();
    assert!(matches!(client.pay(&lock, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    let seq = lock.order.seq;
    // Proven at once, by whichever proof the epoch allows.
    let proven = client.proven_lock(&alice, seq).expect("proven");
    assert_eq!(proven.made.map(|e| (e.to, e.amount, e.hash, e.until)), Some((payee.address(), 500, hash, until)));
    // Proven by the checkpoint once an epoch has closed.
    let epoch = client.epoch();
    net.wait_for_epoch(epoch + 1);
    let proven = client.proven_lock(&alice, seq).expect("proven by the checkpoint");
    assert!(proven.epoch > epoch && proven.made.is_some() && proven.secret.is_none());
    // Nothing is proven of an order that was never a lock.
    assert!(client.proven_lock(&alice, seq + 5).is_none());

    let claim = payee
        .sign(Kind::Claim { from: alice, seq, secret }, 0, payee.address(), 500, None, &net.network)
        .unwrap();
    assert!(matches!(client.pay(&claim, Duration::from_secs(20)), Ok(Done::Receipt(_))));
    let epoch = client.epoch();
    net.wait_for_epoch(epoch + 1);
    let proven = client.proven_lock(&alice, seq).expect("the secret is proven");
    assert_eq!((proven.made, proven.secret), (None, Some(secret)));
}


/// Two locks made one after the other, as a seller makes when two of their
/// orders are taken at once: the older is proven straight away too, not
/// only once a checkpoint has passed.
#[test]
fn an_older_lock_is_proven_at_once_after_a_newer_payment() {
    use orien_ledger::lock_hash;
    let net = TestNet::with_epochs("lock-proof-older", 3);
    let client = net.client();
    let alice = net.address("alice");
    let mut sender = net.wallet("alice");
    sender.accept_fee(net.genesis.fee);
    let payee = Wallet::create(&net.dir.join("older.wallet")).unwrap();
    net.wait_until_level();
    client.follow();
    let until = client.epoch() + 20;
    let mut seqs = Vec::new();
    for secret in [[21u8; 32], [22u8; 32]] {
        let status = client.account(&alice, sender.next_seq());
        let lock = sender
            .sign(Kind::Lock { hash: lock_hash(&secret), until }, status.seq, payee.address(), 300, status.signer.as_ref(), &net.network)
            .unwrap();
        assert!(matches!(client.pay(&lock, Duration::from_secs(20)), Ok(Done::Receipt(_))));
        seqs.push(lock.order.seq);
    }
    let older = client.proven_lock(&alice, seqs[0]).expect("the older lock is proven at once");
    assert_eq!(older.made.map(|e| (e.to, e.amount, e.hash)), Some((payee.address(), 300, lock_hash(&[21u8; 32]))));
    let newer = client.proven_lock(&alice, seqs[1]).expect("the newer lock is proven");
    assert_eq!(newer.made.map(|e| e.hash), Some(lock_hash(&[22u8; 32])));
    // Still nothing of a payment never made.
    assert!(client.proven_lock(&alice, seqs[1] + 5).is_none());
}

/// How many work shares validator `v` holds for `epoch`, by asking it page
/// by page.
fn shares_held(net: &TestNet, v: usize, epoch: u64) -> usize {
    let (mut after, mut count) = (None, 0);
    loop {
        match net.ask(v, &Request::Shares { epoch, after, max: 20_000 }) {
            Response::Shares { epoch: theirs, shares } if theirs == epoch && !shares.is_empty() => {
                count += shares.len();
                after = shares.last().copied();
            }
            _ => return count,
        }
    }
}

/// Waits for the start of a fresh epoch and returns it with its seed and target.
fn fresh_epoch(client: &Client) -> (u64, orien_crypto::Hash32, u64) {
    client.follow();
    let (now, ..) = client.followed_work();
    let mut work = client.followed_work();
    wait_up_to(300, "a new epoch begins", || {
        client.follow();
        work = client.followed_work();
        work.0 > now
    });
    work
}

/// Whether `account` was paid for work done in `epoch`, by every validator's records.
fn mined_in(client: &Client, account: &orien_ledger::AccountId, epoch: u64) -> bool {
    client
        .history(account, 0, 1000)
        .is_some_and(|page| page.entries.iter().any(|e| e.income == orien_ledger::Income::Mining && e.epoch == epoch))
}

#[test]
fn ten_validators_close_an_epoch_of_forty_thousand_shares() {
    // Before shares were written once per proposal, ten statements of
    // 40,000 shares at 40 bytes each made a 16 MB proposal, past the frame
    // limit, and each validator's share of 12 MB held only 30,000.
    let options = Options {
        validators: 10,
        epoch: 120,
        cap: orien_ledger::release::CAP,
        first_target: u64::MAX >> 2,
        shares: 40_000,
        sync: Duration::from_millis(200),
        ..Options::default()
    };
    let net = TestNet::start_with("ten-validators", options);
    let carol = net.address("carol");
    let client = net.client();
    let (epoch, seed, target) = fresh_epoch(&client);
    let math = client.committee().work().math(epoch);
    let started = Instant::now();
    let shares: Vec<orien_ledger::close::Share> = (0..)
        .map(|nonce| orien_ledger::close::Share { account: carol, nonce })
        .filter(|share| share.counts(math, &seed, target))
        .take(40_000)
        .collect();
    // Each validator passes every share on to the others, so each share
    // goes to one, from ten senders at once.
    thread::scope(|scope| {
        for (v, part) in shares.chunks(4_000).enumerate() {
            let client = &client;
            scope.spawn(move || {
                for share in part {
                    match client.call(v, &Request::SubmitShare(*share)) {
                        Ok(Response::Share { .. }) => {}
                        other => panic!("share refused by validator {v}: {other:?}"),
                    }
                }
            });
        }
    });
    println!("sent 40,000 shares in {:?}", started.elapsed());
    wait_up_to(120, "every validator holds every share", || (0..10).all(|v| shares_held(&net, v, epoch) == 40_000));
    println!("all ten hold all 40,000 after {:?}", started.elapsed());
    wait_up_to(240, "the epoch closes and pays carol for it", || {
        client.follow();
        mined_in(&client, &carol, epoch)
    });
    net.wait_until_level();
}

#[test]
fn a_restarted_validator_keeps_its_shares_and_fetches_the_ones_it_lost() {
    use orien_ledger::work::Math;
    // RandomX from the start, at an easy target, and long epochs, so the
    // restarts all happen inside one.
    let randomx = orien_ledger::RandomXFrom { epoch: 0, target: u64::MAX >> 3 };
    let options = Options {
        epoch: 150,
        cap: orien_ledger::release::CAP,
        first_target: u64::MAX >> 3,
        randomx: Some(randomx),
        ..Options::default()
    };
    let mut net = TestNet::start_with("restarts", options);
    let (carol, bob) = (net.address("carol"), net.address("bob"));
    let client = net.client();
    client.follow();
    let (epoch, seed, target) = client.followed_work();
    assert_eq!(client.committee().work().math(epoch), Math::RandomX);
    let ready = orien_ledger::work::Prepared::light_only(Math::RandomX, &seed);
    let hasher = ready.hasher();
    let mine = |account, from: u64, count: usize| -> Vec<orien_ledger::close::Share> {
        (from..)
            .map(|nonce| orien_ledger::close::Share { account, nonce })
            .filter(|share| hasher.number(share) < target)
            .take(count)
            .collect()
    };
    for share in mine(carol, 0, 40) {
        assert_eq!(client.submit_share(share), 4);
    }
    wait_until("all four hold carol's shares", || (0..4).all(|v| shares_held(&net, v, epoch) == 40));

    // Restarted with its share file intact, a validator still has them all.
    net.stop(2);
    net.restart(2);
    assert_eq!(shares_held(&net, 2, epoch), 40, "kept on disk");

    // Restarted without it, as a validator moved without its share file,
    // it fetches them from the others.
    net.stop(3);
    fs::remove_file(net.dir.join("data-3").join("epochs").join(epoch.to_string()).join("shares.log")).unwrap();
    net.restart(3);
    wait_until("the restarted validator fetches the shares it lost", || shares_held(&net, 3, epoch) == 40);

    // All four restarted at once, mid-epoch: nobody holds carol's shares in
    // memory any more, and still none are lost.
    for v in 0..4 {
        net.stop(v);
    }
    for v in 0..4 {
        net.restart(v);
    }
    for share in mine(bob, 0, 10) {
        client.submit_share(share);
    }
    wait_until("all four hold every share after restarting together", || {
        (0..4).all(|v| shares_held(&net, v, epoch) == 50)
    });
    println!("epoch {epoch}: all four restarted together and still hold all 50 shares");
    wait_up_to(300, "the epoch closes and pays both miners", || {
        client.follow();
        mined_in(&client, &carol, epoch) && mined_in(&client, &bob, epoch)
    });
    net.wait_until_level();
}

#[test]
#[ignore = "a soak run of about seven minutes; run it by name"]
fn many_hours_of_mining_survive_restarts_lost_files_and_absences() {
    use orien_ledger::work::Math;
    use std::sync::atomic::{AtomicBool, Ordering};
    let randomx = orien_ledger::RandomXFrom { epoch: 0, target: u64::MAX >> 3 };
    let options = Options {
        epoch: 12,
        cap: orien_ledger::release::CAP,
        first_target: u64::MAX >> 3,
        randomx: Some(randomx),
        ..Options::default()
    };
    let mut net = TestNet::start_with("soak", options);
    let carol = net.address("carol");
    let genesis = net.genesis.clone();
    let stop = AtomicBool::new(false);
    let hours = 30u64;
    thread::scope(|scope| {
        // A miner that never stops: each epoch's seed, light hashing, every
        // share to every validator that answers.
        scope.spawn(|| {
            let client = Client::new(&genesis);
            let mut nonce = 0u64;
            while !stop.load(Ordering::SeqCst) {
                client.follow();
                let (epoch, seed, target) = client.followed_work();
                let ready = orien_ledger::work::Prepared::light_only(Math::RandomX, &seed);
                let hasher = ready.hasher();
                let started = Instant::now();
                while !stop.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(2) {
                    let share = orien_ledger::close::Share { account: carol, nonce };
                    nonce += 1;
                    if hasher.number(&share) < target {
                        client.submit_share(share);
                    }
                }
                let _ = epoch;
            }
        });
        let client = net.client();
        let first = fresh_epoch(&client).0;
        for hour in 0..hours {
            let (epoch, ..) = client.followed_work();
            // Somewhere mid-epoch, one validator goes down.
            thread::sleep(Duration::from_millis(2_000 + (hour * 1_700) % 7_000));
            let v = (hour % 4) as usize;
            net.stop(v);
            if hour % 3 == 1 {
                // As if moved without its share file.
                let file = net.dir.join(format!("data-{v}")).join("epochs").join(epoch.to_string()).join("shares.log");
                let _ = fs::remove_file(file);
            }
            if hour == 12 {
                // Away through three closes.
                let back = epoch + 3;
                wait_up_to(120, "three closes without one validator", || {
                    client.follow();
                    client.followed_work().0 >= back
                });
            } else {
                thread::sleep(Duration::from_secs(1));
            }
            net.restart(v);
            wait_up_to(120, "the next epoch", || {
                client.follow();
                client.followed_work().0 > epoch
            });
        }
        stop.store(true, Ordering::SeqCst);
        let last = client.followed_work().0;
        println!("epochs {first} to {last} closed with a validator knocked out in each");
        let paid = (first..last).filter(|epoch| mined_in(&client, &carol, *epoch)).count() as u64;
        println!("carol paid for {paid} of {} epochs", last - first);
        assert!(paid + 3 >= last - first, "nearly every epoch paid her work");
    });
    net.wait_until_level();
}
