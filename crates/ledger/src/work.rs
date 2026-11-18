//! The math a work share is judged by.
//!
//! At launch a share was judged by BLAKE3: fast, and just as fast for a
//! graphics card, which could have taken mining from every ordinary
//! computer. From an announced epoch on, shares are judged by RandomX, the
//! proof of work Monero has used since 2019: each try runs a small random
//! program over 2 GB of memory, the work an ordinary processor is built for
//! and a graphics card or custom chip gains little at.
//!
//! RandomX's key is the epoch's seed, the checkpoint that opened it, so
//! nobody can prepare for an epoch before it begins. Miners build the full
//! 2 GB dataset for each key (a few seconds to a minute, once an hour).
//! Validators check shares in light mode, with a 256 MB cache: slower per
//! share, so each share is checked once and the answer remembered.

use crate::close::Share;
use orien_crypto::Hash32;
use orien_crypto::{Tag, hash};
#[cfg(feature = "randomx")]
use randomx_rs::{RandomXCache, RandomXDataset, RandomXFlag, RandomXVM};
#[cfg(feature = "randomx")]
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

// Without the `randomx` feature (the iPhone wallet, which neither mines nor
// checks shares), RandomX's types stand empty and asking for it stops.
#[cfg(feature = "randomx")]
type Dataset = Shared<RandomXDataset>;
#[cfg(feature = "randomx")]
type Cache = Shared<RandomXCache>;
#[cfg(feature = "randomx")]
type Machine = RandomXVM;
#[cfg(not(feature = "randomx"))]
enum Dataset {}
#[cfg(not(feature = "randomx"))]
enum Cache {}
#[cfg(not(feature = "randomx"))]
enum Machine {}

#[cfg(not(feature = "randomx"))]
fn no_randomx() -> ! {
    panic!("this build of Orien leaves out RandomX, so it can neither mine nor check RandomX shares")
}

/// Which math judges an epoch's shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Math {
    Blake3,
    RandomX,
}

/// The Orien network that went live on 2026-11-14.
pub const LIVE_NETWORK: &str = "8f1833742a76431b7e0912ebbab49f3669dd423fb2fbbf0612b8ade911ac1452";

/// The first epoch on the live network whose shares RandomX judges: epoch
/// 21, which begins at 2026-11-15 18:11 UTC. Release 1.5.0, which brings
/// RandomX, reaches every validator before epoch 20 closes and sets 21's
/// target.
pub const LIVE_RANDOMX_FROM: u64 = 21;

/// The live network's target for its first RandomX epoch: about one try in
/// 400,000 makes a share. Its whole network did about 1.2 billion BLAKE3
/// tries a second before the switch, which on processors is in the order
/// of 100,000 RandomX tries a second, so about 1,000 shares an hour. From
/// there the target moves each epoch, as before.
pub const LIVE_RANDOMX_TARGET: u64 = u64::MAX / 400_000;

/// The first epoch in which the live network's four launch seats are
/// reached by name rather than at 217.77.11.197, so that each can move to
/// its own server by changing its name record. Set to an epoch the release
/// bringing it reaches every validator before.
pub const LIVE_SEAT_NAMES_FROM: u64 = 117;

/// The live network's launch seats' names, in genesis order.
pub const LIVE_SEAT_NAMES: [&str; 4] =
    ["v1.orien.network:7101", "v2.orien.network:7102", "v3.orien.network:7103", "v4.orien.network:7104"];

/// What RandomX hashes for a share: the seed, the account and the nonce.
#[cfg(feature = "randomx")]
fn input(seed: &Hash32, share: &Share) -> [u8; 72] {
    let mut bytes = [0u8; 72];
    bytes[..32].copy_from_slice(seed.as_bytes());
    bytes[32..64].copy_from_slice(share.account.as_bytes());
    bytes[64..].copy_from_slice(&share.nonce.to_le_bytes());
    bytes
}

/// A hash read as a number: its first eight bytes.
fn number(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().expect("a hash has at least 8 bytes"))
}

/// A share's number by BLAKE3, as at launch.
pub fn blake3(seed: &Hash32, share: &Share) -> u64 {
    let digest = hash(Tag::Work, &[seed.as_bytes(), share.account.as_bytes(), &share.nonce.to_le_bytes()]);
    number(&digest.0)
}

/// A RandomX cache or dataset, which RandomX lets many threads read at
/// once once it is made: each thread runs its own machine over it.
#[cfg(feature = "randomx")]
struct Shared<T>(T);
// SAFETY: RandomX caches and datasets are written only while they are made
// and only read afterwards, by any number of machines on any threads; the
// wrapper's reference count is atomic.
#[cfg(feature = "randomx")]
unsafe impl<T> Send for Shared<T> {}
#[cfg(feature = "randomx")]
unsafe impl<T> Sync for Shared<T> {}

/// The light cache for the key validators are checking against now.
#[cfg(feature = "randomx")]
static LIGHT: Mutex<Option<(Hash32, Shared<RandomXCache>)>> = Mutex::new(None);

/// Shares already judged, by seed: each is checked once however often it
/// arrives. Holds a couple of epochs at most.
static JUDGED: Mutex<Option<HashMap<(Hash32, Share), u64>>> = Mutex::new(None);

#[cfg(feature = "randomx")]
thread_local! {
    /// This thread's light machine, and the key it was made for.
    static MACHINE: RefCell<Option<(Hash32, RandomXVM)>> = const { RefCell::new(None) };
}

#[cfg(feature = "randomx")]
fn light_flags() -> RandomXFlag {
    RandomXFlag::get_recommended_flags()
}

/// The light cache for `seed`, made if it is not the current one.
#[cfg(feature = "randomx")]
fn light_cache(seed: &Hash32) -> RandomXCache {
    let mut light = LIGHT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((key, cache)) = light.as_ref()
        && key == seed
    {
        return cache.0.clone();
    }
    let cache = RandomXCache::new(light_flags(), seed.as_bytes()).expect("RandomX cannot allocate its 256 MB cache");
    *light = Some((*seed, Shared(cache.clone())));
    cache
}

/// A share's number by RandomX, in light mode, as validators check it.
#[cfg(not(feature = "randomx"))]
fn randomx_light(_: &Hash32, _: &Share) -> u64 {
    no_randomx()
}

/// A share's number by RandomX, in light mode, as validators check it.
#[cfg(feature = "randomx")]
fn randomx_light(seed: &Hash32, share: &Share) -> u64 {
    MACHINE.with(|machine| {
        let mut machine = machine.borrow_mut();
        if machine.as_ref().is_none_or(|(key, _)| key != seed) {
            let vm = RandomXVM::new(light_flags(), Some(light_cache(seed)), None).expect("RandomX cannot make a machine");
            *machine = Some((*seed, vm));
        }
        let (_, vm) = machine.as_ref().expect("made above");
        number(&vm.calculate_hash(&input(seed, share)).expect("RandomX hashes any input"))
    })
}

/// A share's number if it has been judged already, or is cheap to judge:
/// whether checking it would cost nothing.
pub fn judged(math: Math, seed: &Hash32, share: &Share) -> Option<u64> {
    match math {
        Math::Blake3 => Some(blake3(seed, share)),
        Math::RandomX => JUDGED.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|j| j.get(&(*seed, *share)).copied()),
    }
}

/// A share's number under `math`, as a validator judges it. RandomX
/// answers are remembered, so a share that arrives again costs nothing.
pub fn judge(math: Math, seed: &Hash32, share: &Share) -> u64 {
    match math {
        Math::Blake3 => blake3(seed, share),
        Math::RandomX => {
            let key = (*seed, *share);
            if let Some(known) = JUDGED.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|j| j.get(&key)) {
                return *known;
            }
            let found = randomx_light(seed, share);
            let mut judged = JUDGED.lock().unwrap_or_else(|p| p.into_inner());
            let judged = judged.get_or_insert_with(HashMap::new);
            // Far more than two epochs' worth means an old epoch's are in it.
            if judged.len() >= 400_000 {
                judged.retain(|(other, _), _| other == seed);
            }
            judged.insert(key, found);
            found
        }
    }
}

/// What a miner prepares for an epoch: nothing for BLAKE3; for RandomX the
/// full dataset if this computer can hold it, or else the light cache.
pub struct Prepared {
    math: Math,
    seed: Hash32,
    dataset: Option<Dataset>,
    cache: Option<Cache>,
}

impl Prepared {
    /// Makes ready to mine against `seed`. For RandomX this takes a few
    /// seconds to a minute and 2 GB of memory; with less memory it falls
    /// back to light mode, which is several times slower.
    pub fn for_mining(math: Math, seed: &Hash32) -> Prepared {
        #[allow(unused_mut)]
        let (mut dataset, mut cache) = (None, None);
        #[cfg(not(feature = "randomx"))]
        if math == Math::RandomX {
            no_randomx();
        }
        #[cfg(feature = "randomx")]
        if math == Math::RandomX {
            let flags = RandomXFlag::get_recommended_flags();
            let made = RandomXCache::new(flags, seed.as_bytes()).expect("RandomX cannot allocate its 256 MB cache");
            match RandomXDataset::new(flags | RandomXFlag::FLAG_FULL_MEM, made.clone(), 0) {
                Ok(full) => dataset = Some(Shared(full)),
                Err(_) => cache = Some(Shared(made)),
            }
        }
        Prepared { math, seed: *seed, dataset, cache }
    }

    /// Makes ready to mine in light mode only: no 2 GB dataset, a quick
    /// start, several times slower per try. For tests and small machines.
    pub fn light_only(math: Math, seed: &Hash32) -> Prepared {
        #[cfg(feature = "randomx")]
        let cache = (math == Math::RandomX).then(|| Shared(light_cache(seed)));
        #[cfg(not(feature = "randomx"))]
        let cache = if math == Math::RandomX { no_randomx() } else { None };
        Prepared { math, seed: *seed, dataset: None, cache }
    }

    pub fn math(&self) -> Math {
        self.math
    }

    /// Whether RandomX runs in light mode here, short of memory.
    pub fn light(&self) -> bool {
        self.math == Math::RandomX && self.dataset.is_none()
    }

    /// How many tries to make between looks at whether to stop.
    pub fn batch(&self) -> u64 {
        match self.math {
            Math::Blake3 => 4096,
            Math::RandomX => 8,
        }
    }

    /// A hasher for one thread.
    pub fn hasher(&self) -> Hasher {
        #[cfg(not(feature = "randomx"))]
        let vm: Option<Machine> = match (&self.dataset, &self.cache) {
            (Some(never), _) => match *never {},
            (None, Some(never)) => match *never {},
            (None, None) => None,
        };
        #[cfg(feature = "randomx")]
        let flags = RandomXFlag::get_recommended_flags();
        #[cfg(feature = "randomx")]
        let vm = match (&self.dataset, &self.cache) {
            (Some(dataset), _) => Some(
                RandomXVM::new(flags | RandomXFlag::FLAG_FULL_MEM, None, Some(dataset.0.clone())).expect("RandomX cannot make a machine"),
            ),
            (None, Some(cache)) => Some(RandomXVM::new(flags, Some(cache.0.clone()), None).expect("RandomX cannot make a machine")),
            (None, None) => None,
        };
        Hasher { seed: self.seed, vm }
    }
}

/// One mining thread's hasher.
pub struct Hasher {
    seed: Hash32,
    vm: Option<Machine>,
}

impl Hasher {
    /// The share's number: below the target, it counts.
    pub fn number(&self, share: &Share) -> u64 {
        match &self.vm {
            None => blake3(&self.seed, share),
            #[cfg(feature = "randomx")]
            Some(vm) => number(&vm.calculate_hash(&input(&self.seed, share)).expect("RandomX hashes any input")),
            #[cfg(not(feature = "randomx"))]
            Some(never) => match *never {},
        }
    }
}

#[cfg(all(test, feature = "randomx"))]
mod tests {
    use super::*;

    #[test]
    fn miners_and_validators_agree_on_every_share() {
        let seed = Hash32([5; 32]);
        let shares: Vec<Share> = (0..4).map(|nonce| Share { account: Hash32([9; 32]), nonce }).collect();
        let light = Prepared { math: Math::RandomX, seed, dataset: None, cache: Some(Shared(light_cache(&seed))) };
        let hasher = light.hasher();
        for share in &shares {
            let judged = judge(Math::RandomX, &seed, share);
            assert_eq!(judged, hasher.number(share), "the miner's number is the validator's");
            assert_eq!(judge(Math::RandomX, &seed, share), judged, "and the same again, remembered");
            assert_ne!(judged, blake3(&seed, share), "RandomX is not BLAKE3");
        }
        // Another seed, another key: other numbers.
        let other = Hash32([6; 32]);
        assert_ne!(judge(Math::RandomX, &other, &shares[0]), judge(Math::RandomX, &seed, &shares[0]));
        let blake = Prepared::for_mining(Math::Blake3, &seed).hasher();
        assert_eq!(blake.number(&shares[0]), blake3(&seed, &shares[0]));
    }
}
