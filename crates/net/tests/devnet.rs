//! Real validators talking over real sockets on this machine.

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Barrier;
use std::thread;
use std::time::{Duration, Instant};
use token_crypto::keytree::KeyTree;
use token_crypto::{Hash32, Kdf, ots, prf};
use token_ledger::{AccountId, Allocation, Genesis, Order, Reject, SignedOrder, ValidatorInfo};
use token_net::client::{Client, Receipt, TransferError, rpc};
use token_net::keys::{ValidatorKey, Wallet};
use token_net::node::{Node, NodeConfig};
use token_net::wire::{AccountView, NodeInfo, Request, Response};

const START: u64 = 1000;

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
}

impl TestNet {
    fn start(name: &str) -> TestNet {
        TestNet::start_with(name, Options::default())
    }

    fn start_with(name: &str, options: Options) -> TestNet {
        let dir = std::env::temp_dir().join(format!("token-net-{name}-{}", std::process::id()));
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
        let genesis = Genesis {
            network: name.into(),
            validators: keys
                .iter()
                .zip(&listeners)
                .enumerate()
                .map(|(i, (key, listener))| ValidatorInfo {
                    name: format!("v{}", i + 1),
                    root: KeyTree::generate(key.seed, key.height).root(),
                    height: key.height,
                    stake: 1,
                    address: listener.local_addr().unwrap().to_string(),
                })
                .collect(),
            allocations,
        };
        let network = genesis.id();
        let mut net = TestNet { dir, genesis, network, keys, nodes: Vec::new(), options };
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
        config
    }

    fn stop(&mut self, i: usize) {
        self.nodes[i].take().unwrap().stop();
    }

    fn restart(&mut self, i: usize) {
        let listener = TcpListener::bind(&self.genesis.validators[i].address).unwrap();
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
        rpc(&self.genesis.validators[validator].address, request, Duration::from_secs(5)).unwrap()
    }

    fn view(&self, validator: usize, id: &AccountId) -> AccountView {
        match self.ask(validator, &Request::Account(*id)) {
            Response::Account(Some(view)) => view,
            Response::Account(None) => AccountView { balance: 0, seq: 0, key: *id, lock: None },
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
    fn try_pay(&self, from: &str, to: AccountId, amount: u64) -> Result<Receipt, TransferError> {
        let mut wallet = self.wallet(from);
        let client = self.client();
        let status = client.account(&wallet.address(), wallet.next_seq());
        let signed = wallet.sign_order(status.seq, to, amount, &self.network).unwrap();
        let receipt = client.transfer(&signed)?;
        wallet.confirm(status.seq).unwrap();
        Ok(receipt)
    }

    fn pay(&self, from: &str, to: AccountId, amount: u64) -> Receipt {
        self.try_pay(from, to, amount).unwrap()
    }

    /// Signs an order directly with a wallet's key, skipping the wallet's
    /// guard against using a key twice. This is what a cheater does.
    fn raw_order(&self, from: &str, seq: u64, to: AccountId, amount: u64) -> SignedOrder {
        let wallet = self.wallet(from);
        let order = Order {
            from: wallet.address(),
            seq,
            to,
            amount,
            next_key: wallet.key(seq + 1).commitment(),
        };
        let digest = order.digest(&self.network);
        SignedOrder { signature: wallet.key(seq).sign(digest.as_bytes()), order }
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

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
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
    assert_eq!(receipt.acknowledged, 4);
    assert_eq!(receipt.certificate.votes.len(), 3, "three of four votes are enough");
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 750);
        assert_eq!(net.view(v, &alice).seq, 1);
        assert_eq!(net.view(v, &bob).balance, 1250);
    }
    // The receipt stands on its own: genesis plus hashing verifies it.
    assert!(net.client().committee().verify_certificate(&receipt.certificate).is_ok());
    let status = net.client().account(&alice, 0);
    assert_eq!((status.seq, status.spendable), (1, Some(750)));
}

#[test]
fn keys_rotate_through_a_run_of_payments() {
    let net = TestNet::start("run");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let mut keys = vec![net.view(0, &alice).key];
    for round in 0..5 {
        net.pay("alice", carol, 10);
        let key = net.view(0, &alice).key;
        assert!(!keys.contains(&key), "payment {round} reused a key");
        keys.push(key);
    }
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
    assert_eq!(client.transfer(&to_bob).unwrap().acknowledged, 4);
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
    assert_eq!(net.pay("bob", carol, 5).acknowledged, 4);
}

#[test]
fn the_network_works_with_a_validator_down_and_it_catches_up() {
    let mut net = TestNet::start("down");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    net.stop(3);

    let receipt = net.pay("alice", bob, 100);
    assert_eq!((receipt.votes, receipt.acknowledged), (3, 3));
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
    assert_eq!((receipt.votes, receipt.acknowledged), (4, 4));
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
    assert_eq!(log, borsh::to_vec(&to_bob).unwrap().len() as u64);
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

    assert_eq!(net.pay("alice", bob, 5).acknowledged, 4);
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

    let too_few = token_ledger::Certificate { order: order.order.clone(), votes: votes[..2].to_vec() };
    let mut inflated = token_ledger::Certificate { order: order.order.clone(), votes: votes[..3].to_vec() };
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
        assert!(info.keys_used <= 8, "validator {v} signed {} times for {crowd} payments", info.keys_used);
    }
}

#[test]
fn a_single_validator_is_already_a_network() {
    let net = TestNet::start_with("solo", Options { validators: 1, ..Options::default() });
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let receipt = net.pay("alice", bob, 1);
    assert_eq!((receipt.votes, receipt.acknowledged), (1, 1));
    assert_eq!(net.view(0, &alice).balance, 999);
}

#[test]
fn a_wallet_never_signs_two_orders_with_one_key() {
    let net = TestNet::start_with("wallet", Options { validators: 1, ..Options::default() });
    let (bob, carol) = (net.address("bob"), net.address("carol"));
    let mut wallet = net.wallet("alice");

    let first = wallet.sign_order(0, bob, 5, &net.network).unwrap();
    assert!(wallet.sign_order(0, carol, 5, &net.network).is_err());
    assert!(wallet.sign_order(0, bob, 6, &net.network).is_err());
    assert_eq!(wallet.sign_order(0, bob, 5, &net.network).unwrap(), first);

    // The promise is on disk: a fresh load of the file remembers it.
    let mut reloaded = net.wallet("alice");
    assert!(reloaded.sign_order(0, carol, 5, &net.network).is_err());
    assert_eq!(reloaded.sign_order(0, bob, 5, &net.network).unwrap(), first);

    reloaded.confirm(0).unwrap();
    assert!(reloaded.sign_order(0, bob, 5, &net.network).is_err(), "payment #0 is done");
    assert!(reloaded.sign_order(1, carol, 5, &net.network).is_ok());
}

/// Not a pass/fail test: prints how many payments per second this machine
/// makes final, with every validator syncing its votes to disk. Run with
///
///   cargo test --release -p token-net --test devnet throughput -- --ignored --nocapture
#[test]
fn another_copy_of_a_wallet_finishes_a_payment() {
    let net = TestNet::start("copy");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    // A second device holds a copy of alice's wallet, made before she paid anyone.
    fs::copy(net.dir.join("alice.wallet"), net.dir.join("alice-copy.wallet")).unwrap();

    // The first device signs a payment, reaches two validators, and is lost.
    let signed = net.wallet("alice").sign_order(0, bob, 100, &net.network).unwrap();
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
    assert_eq!((receipt.votes, receipt.acknowledged), (4, 4));
    for v in 0..4 {
        assert_eq!(net.view(v, &alice).balance, 900);
        assert_eq!(net.view(v, &bob).balance, 1100);
        assert!(matches!(net.ask(v, &Request::Pending(alice)), Response::Pending(None)));
    }
    // And it carries on with the next key, which both copies derive alike.
    assert_eq!(net.pay("alice-copy", bob, 1).acknowledged, 4);
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
    assert_eq!((receipt.votes, receipt.acknowledged), (3, 4));
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
    let accounts: Vec<AccountId> = seeds.iter().map(|seed| key(seed, 0).commitment()).collect();
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
        .zip(&accounts)
        .map(|(seed, account)| {
            let order = Order { from: *account, seq: 0, to: carol, amount: 1, next_key: key(seed, 1).commitment() };
            let digest = order.digest(&net.network);
            SignedOrder { signature: key(seed, 0).sign(digest.as_bytes()), order }
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
