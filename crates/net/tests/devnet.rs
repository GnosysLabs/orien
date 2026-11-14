//! Real validators talking over real sockets on this machine.

use orien_crypto::keytree::KeyTree;
use orien_crypto::{Hash32, Kdf, ots, prf};
use orien_ledger::{AccountId, Allocation, Genesis, Order, Reject, SignedOrder, ValidatorInfo};
use orien_net::client::{Client, Final, TransferError, rpc};
use orien_net::keys::{ValidatorKey, Wallet};
use orien_net::node::{Node, NodeConfig};
use orien_net::store::Epochs;
use orien_net::wire::{AccountView, NodeInfo, Request, Response};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Barrier;
use std::thread;
use std::time::{Duration, Instant};

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
    fn try_pay(&self, from: &str, to: AccountId, amount: u64) -> Result<Final, TransferError> {
        let mut wallet = self.wallet(from);
        let client = self.client();
        let status = client.account(&wallet.address(), wallet.next_seq());
        let signed = wallet.sign_order(status.seq, to, amount, &self.network).unwrap();
        let receipt = client.transfer(&signed)?;
        wallet.confirm(status.seq).unwrap();
        Ok(receipt)
    }

    fn pay(&self, from: &str, to: AccountId, amount: u64) -> Final {
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
        assert!(info.keys_used <= 8, "validator {v} signed {} times for {crowd} payments", info.keys_used);
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
///   cargo test --release -p orien-net --test devnet throughput -- --ignored --nocapture
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
fn a_validator_restarts_from_a_snapshot() {
    let mut net = TestNet::start("snapshot");
    let (alice, bob, carol) = (net.address("alice"), net.address("bob"), net.address("carol"));
    let client = net.client();
    net.pay("alice", bob, 100);
    net.pay("bob", carol, 30);
    let before = client.roots()[0].unwrap();

    // Move validator 1 into a new epoch by hand, the way a checkpoint will:
    // its state so far becomes a snapshot and its certificate log starts empty.
    net.stop(0);
    let mut state = orien_ledger::State::from_genesis(&net.genesis);
    for (account, seq) in [(alice, 0), (bob, 0)] {
        state.apply(&client.certificate(&account, seq).unwrap().order).unwrap();
    }
    let mut epochs = Epochs::open(&net.dir.join("data-0"), false).unwrap();
    epochs.advance(&state.snapshot()).unwrap();
    assert_eq!(epochs.current(), 1);
    net.restart(0);

    // It holds the same balances, read from the snapshot and not replayed.
    assert_eq!(client.roots()[0], Some(before));
    assert_eq!(net.info(0).certificates, 0);
    assert_eq!(net.view(0, &alice).balance, 900);
    assert_eq!((net.view(0, &bob).balance, net.view(0, &bob).seq), (1070, 1));

    // And it carries on: it votes, applies, and still agrees with the others.
    assert_eq!(net.pay("alice", carol, 5).confirmations, 4);
    assert_eq!(net.info(0).certificates, 1);
    net.stop(0);
    net.restart(0);
    assert_eq!(net.view(0, &alice).balance, 895);
    let roots = client.roots();
    assert!(roots.iter().all(|root| *root == roots[0] && root.is_some()));
}

#[test]
fn a_validator_holds_keys_back_for_closing_an_epoch() {
    // A tree of 16 one-time keys, of which a quarter are held back. One
    // payment at a time takes two: one for the vote, one for the confirmation.
    let options = Options { validators: 1, height: 4, ..Options::default() };
    let net = TestNet::start_with("reserve", options);
    let bob = net.address("bob");
    for _ in 0..6 {
        net.pay("alice", bob, 1);
    }
    assert_eq!((net.info(0).keys_used, net.info(0).keys_total), (12, 16));

    // The seventh payment gets no vote, and nothing moves.
    match net.try_pay("alice", bob, 1) {
        Err(TransferError::NoQuorum { stake: 0, refusals, .. }) => {
            assert!(refusals[0].1.contains("no one-time keys left for payments"), "{refusals:?}");
        }
        other => panic!("expected a refusal, got {:?}", other.map(|r| r.votes)),
    }
    assert_eq!(net.info(0).keys_used, 12);
    assert_eq!(net.view(0, &bob).balance, START as i128 + 6);
}

/// Gathers votes for an order into a certificate, without going further.
fn certificate_for(client: &Client, signed: &SignedOrder) -> orien_ledger::Certificate {
    let (votes, _) = client.collect_votes(signed);
    orien_ledger::Certificate::new(0, signed, votes.into_values().take(3).collect())
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
    for other in [None, Some("1"), Some("3")] {
        match other {
            Some(format) => fs::write(data.join("format"), format).unwrap(),
            None => fs::remove_file(data.join("format")).unwrap(),
        }
        let refusal = Node::start(net.config(0), None).err().expect("must refuse").to_string();
        assert!(refusal.contains("data format"), "{refusal}");
    }
    // Nothing was touched: put the marker back and the validator is as it was.
    fs::write(data.join("format"), "2").unwrap();
    net.restart(0);
    assert_eq!(net.view(0, &bob).balance, 1100);
}

#[test]
fn a_payment_is_final_only_once_enough_validators_confirm() {
    let mut net = TestNet::start("confirm");
    let (alice, bob) = (net.address("alice"), net.address("bob"));
    let client = net.client();
    let signed = net.wallet("alice").sign_order(0, bob, 100, &net.network).unwrap();
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
    let signed = net.wallet("alice").sign_order(0, bob, 100, &net.network).unwrap();
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
    let signed = net.wallet("alice").sign_order(0, bob, 100, &net.network).unwrap();
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
