//! Unlocked sessions: a wallet with a password, opened once by its owner
//! and then able to sign for a while without the password being typed
//! again, so that a program the owner trusts (an AI assistant, a script)
//! can send payments without ever learning the password.
//!
//! `orien unlock` asks for the password and two limits, a time and an
//! amount, then leaves a small helper running in the background. The
//! helper holds the opened wallet in memory only and signs orders on
//! request, as long as both limits allow. It is the only process that
//! writes the wallet file while it runs, and it keeps the wallet's rule
//! that each payment number is signed for one payment only. It ends when
//! the time is up, on `orien lock`, or when the computer restarts.
//!
//! The helper listens on this computer's own loopback address only, and
//! answers only requests carrying a secret token, which it writes beside
//! the wallet in a file only the owner can read. Whatever runs as the owner
//! can therefore use it; the limits are what bound that.

use crate::keys::{Wallet, random_seed};
use crate::store::write_replacing;
use anyhow::{Context, Result, anyhow, bail};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::Hash32;
use orien_ledger::{AccountId, Fees, Kind, SignedOrder, Signer, coins};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Where a wallet's session is described while it is unlocked: beside the
/// wallet, as `wallet.unlocked`.
pub fn session_path(wallet: &Path) -> PathBuf {
    let mut name = wallet.as_os_str().to_owned();
    name.push(".unlocked");
    PathBuf::from(name)
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    port: u16,
    token: String,
}

/// How long and how much an unlocked wallet may sign for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Limits {
    /// When the session ends, in seconds since 1970.
    pub until: u64,
    /// The most it may spend, in units: payments and stakes with their fees.
    pub spend: u64,
}

/// What an unlocked session is and has done.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Status {
    pub address: AccountId,
    pub limits: Limits,
    pub spent: u64,
}

impl Status {
    pub fn left(&self) -> u64 {
        self.limits.spend.saturating_sub(self.spent)
    }
}

/// The public state of the unlocked wallet, which a program signing
/// through the session works from.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Info {
    pub address: AccountId,
    pub spare: Hash32,
    pub next_seq: u64,
    pub pending: Option<SignedOrder>,
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Request {
    Status,
    Lock,
    Info,
    Sign {
        kind: Kind,
        seq: u64,
        to: AccountId,
        amount: u64,
        signer: Option<Signer>,
        network: Hash32,
        /// The network's fee rule, as (min, max, rate).
        fees: (u64, u64, u32),
    },
    Confirm {
        seq: u64,
    },
    Forget {
        seq: u64,
    },
    Adopt {
        signed: SignedOrder,
        network: Hash32,
    },
    Owns {
        signed: SignedOrder,
        network: Hash32,
    },
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Response {
    Status(Status),
    Info(Info),
    Signed(SignedOrder),
    Owns(bool),
    Done,
    Refused(String),
}

/// The largest message either side accepts.
const MAX_MESSAGE: u32 = 1 << 20;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn write_message(stream: &mut TcpStream, message: &impl BorshSerialize) -> Result<()> {
    let bytes = borsh::to_vec(message)?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_message<T: BorshDeserialize>(stream: &mut TcpStream) -> Result<T> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    if length > MAX_MESSAGE {
        bail!("message too large");
    }
    let mut bytes = vec![0u8; length as usize];
    stream.read_exact(&mut bytes)?;
    Ok(borsh::from_slice(&bytes)?)
}

/// Runs an unlocked session for `wallet` until its time is up or it is
/// locked. Calls `ready` once requests can be made.
pub fn serve(mut wallet: Wallet, limits: Limits, ready: impl FnOnce()) -> Result<()> {
    let file = session_path(wallet.path());
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let token = random_seed();
    let description = SessionFile { port: listener.local_addr()?.port(), token: hex::encode(token) };
    write_replacing(&file, serde_json::to_string(&description)?.as_bytes(), true)
        .with_context(|| format!("cannot write {}", file.display()))?;

    // The session ends on time even if nothing asks anything.
    let ending = file.clone();
    std::thread::spawn(move || {
        loop {
            let left = limits.until.saturating_sub(now());
            if left == 0 {
                let _ = fs::remove_file(&ending);
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_secs(left.min(30)));
        }
    });
    ready();

    let mut spent = 0u64;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut presented = [0u8; 32];
        if stream.read_exact(&mut presented).is_err() || presented != token {
            continue;
        }
        let Ok(request) = read_message::<Request>(&mut stream) else { continue };
        if now() >= limits.until {
            break;
        }
        let lock = matches!(request, Request::Lock);
        let response = answer(&mut wallet, limits, &mut spent, request);
        let _ = write_message(&mut stream, &response);
        if lock {
            break;
        }
    }
    let _ = fs::remove_file(&file);
    Ok(())
}

fn answer(wallet: &mut Wallet, limits: Limits, spent: &mut u64, request: Request) -> Response {
    let outcome = |result: Result<()>| match result {
        Ok(()) => Response::Done,
        Err(error) => Response::Refused(error.to_string()),
    };
    match request {
        Request::Status => Response::Status(Status { address: wallet.address(), limits, spent: *spent }),
        Request::Lock => Response::Done,
        Request::Info => Response::Info(Info {
            address: wallet.address(),
            spare: wallet.spare(),
            next_seq: wallet.next_seq(),
            pending: wallet.pending().cloned(),
        }),
        Request::Sign { kind, seq, to, amount, signer, network, fees: (min, max, rate) } => {
            let fees = Fees { min, max, rate };
            // Signing again what is already signed under this number spends
            // nothing new: the wallet hands back the same order.
            let again = wallet.pending().is_some_and(|pending| {
                let order = &pending.order;
                order.seq == seq && order.kind == kind && order.to == to && order.amount == amount
            });
            let moved = match kind {
                Kind::Unstake => 0,
                _ => amount,
            };
            let cost = if again { 0 } else { moved.saturating_add(fees.on(&kind, amount)) };
            let left = limits.spend.saturating_sub(*spent);
            if cost > left {
                return Response::Refused(format!(
                    "this needs {} ORN with its fee, and the unlock allows only {} ORN more",
                    coins(cost as i128),
                    coins(left as i128)
                ));
            }
            wallet.accept_fee(fees);
            match wallet.sign(kind, seq, to, amount, signer.as_ref(), &network) {
                Ok(signed) => {
                    *spent += cost;
                    Response::Signed(signed)
                }
                Err(error) => Response::Refused(error.to_string()),
            }
        }
        Request::Confirm { seq } => outcome(wallet.confirm(seq)),
        Request::Forget { seq } => outcome(wallet.forget(seq)),
        Request::Adopt { signed, network } => outcome(wallet.adopt(&signed, &network)),
        Request::Owns { signed, network } => Response::Owns(wallet.signed_by_this_wallet(&signed, &network)),
    }
}

/// A running unlocked session, found from its wallet's path.
pub struct Session {
    file: PathBuf,
    port: u16,
    token: [u8; 32],
}

impl Session {
    /// The session unlocking the wallet at `wallet`, if one is running. A
    /// description left behind by a session that has ended is removed.
    pub fn find(wallet: &Path) -> Option<Session> {
        let file = session_path(wallet);
        let text = fs::read_to_string(&file).ok()?;
        let session = serde_json::from_str::<SessionFile>(&text).ok().and_then(|description| {
            let token = hex::decode(description.token).ok()?.try_into().ok()?;
            Some(Session { file: file.clone(), port: description.port, token })
        });
        match session {
            Some(session) if session.status().is_ok() => Some(session),
            _ => {
                let _ = fs::remove_file(&file);
                None
            }
        }
    }

    fn call(&self, request: &Request) -> Result<Response> {
        let mut stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_secs(2),
        )
        .map_err(|_| anyhow!("the wallet is no longer unlocked"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.write_all(&self.token)?;
        write_message(&mut stream, request)?;
        read_message(&mut stream).map_err(|_| anyhow!("the wallet is no longer unlocked"))
    }

    fn done(&self, request: Request) -> Result<()> {
        match self.call(&request)? {
            Response::Done => Ok(()),
            Response::Refused(reason) => bail!(reason),
            _ => bail!("unexpected answer from the unlocked wallet"),
        }
    }

    pub fn status(&self) -> Result<Status> {
        match self.call(&Request::Status)? {
            Response::Status(status) => Ok(status),
            _ => bail!("unexpected answer from the unlocked wallet"),
        }
    }

    /// Ends the session.
    pub fn lock(&self) -> Result<()> {
        self.done(Request::Lock)?;
        let _ = fs::remove_file(&self.file);
        Ok(())
    }

    pub fn info(&self) -> Result<Info> {
        match self.call(&Request::Info)? {
            Response::Info(info) => Ok(info),
            _ => bail!("unexpected answer from the unlocked wallet"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        &self,
        kind: Kind,
        seq: u64,
        to: AccountId,
        amount: u64,
        signer: Option<&Signer>,
        network: &Hash32,
        fees: Fees,
    ) -> Result<SignedOrder> {
        let request = Request::Sign {
            kind,
            seq,
            to,
            amount,
            signer: signer.cloned(),
            network: *network,
            fees: (fees.min, fees.max, fees.rate),
        };
        match self.call(&request)? {
            Response::Signed(signed) => Ok(signed),
            Response::Refused(reason) => bail!("the unlocked wallet refused: {reason}"),
            _ => bail!("unexpected answer from the unlocked wallet"),
        }
    }

    pub fn confirm(&self, seq: u64) -> Result<()> {
        self.done(Request::Confirm { seq })
    }

    pub fn forget(&self, seq: u64) -> Result<()> {
        self.done(Request::Forget { seq })
    }

    pub fn adopt(&self, signed: &SignedOrder, network: &Hash32) -> Result<()> {
        self.done(Request::Adopt { signed: signed.clone(), network: *network })
    }

    pub fn owns(&self, signed: &SignedOrder, network: &Hash32) -> Result<bool> {
        match self.call(&Request::Owns { signed: signed.clone(), network: *network })? {
            Response::Owns(owns) => Ok(owns),
            _ => bail!("unexpected answer from the unlocked wallet"),
        }
    }
}

/// A wallet to sign with: opened here with its secret, or reached through
/// the unlocked session that holds it. The two behave the same.
pub enum Signing {
    Here(Wallet),
    Unlocked { session: Session, info: Info, fees: Fees },
}

impl Signing {
    /// Reaches the wallet at `path` through its unlocked session, if one is
    /// running.
    pub fn unlocked(path: &Path) -> Result<Option<Signing>> {
        let Some(session) = Session::find(path) else { return Ok(None) };
        let info = session.info()?;
        Ok(Some(Signing::Unlocked { session, info, fees: Fees::NONE }))
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(self, Signing::Unlocked { .. })
    }

    pub fn address(&self) -> AccountId {
        match self {
            Signing::Here(wallet) => wallet.address(),
            Signing::Unlocked { info, .. } => info.address,
        }
    }

    pub fn next_seq(&self) -> u64 {
        match self {
            Signing::Here(wallet) => wallet.next_seq(),
            Signing::Unlocked { info, .. } => info.next_seq,
        }
    }

    pub fn pending(&self) -> Option<&SignedOrder> {
        match self {
            Signing::Here(wallet) => wallet.pending(),
            Signing::Unlocked { info, .. } => info.pending.as_ref(),
        }
    }

    pub fn accept_fee(&mut self, new: Fees) {
        match self {
            Signing::Here(wallet) => wallet.accept_fee(new),
            Signing::Unlocked { fees, .. } => *fees = new,
        }
    }

    /// Re-reads the wallet's state from the session after it changed there.
    fn refresh(&mut self) -> Result<()> {
        if let Signing::Unlocked { session, info, .. } = self {
            *info = session.info()?;
        }
        Ok(())
    }

    pub fn sign(
        &mut self,
        kind: Kind,
        seq: u64,
        to: AccountId,
        amount: u64,
        signer: Option<&Signer>,
        network: &Hash32,
    ) -> Result<SignedOrder> {
        let signed = match self {
            Signing::Here(wallet) => return wallet.sign(kind, seq, to, amount, signer, network),
            Signing::Unlocked { session, fees, .. } => session.sign(kind, seq, to, amount, signer, network, *fees)?,
        };
        self.refresh()?;
        Ok(signed)
    }

    pub fn confirm(&mut self, seq: u64) -> Result<()> {
        match self {
            Signing::Here(wallet) => wallet.confirm(seq),
            Signing::Unlocked { session, .. } => {
                session.confirm(seq)?;
                self.refresh()
            }
        }
    }

    pub fn forget(&mut self, seq: u64) -> Result<()> {
        match self {
            Signing::Here(wallet) => wallet.forget(seq),
            Signing::Unlocked { session, .. } => {
                session.forget(seq)?;
                self.refresh()
            }
        }
    }

    pub fn adopt(&mut self, signed: &SignedOrder, network: &Hash32) -> Result<()> {
        match self {
            Signing::Here(wallet) => wallet.adopt(signed, network),
            Signing::Unlocked { session, .. } => {
                session.adopt(signed, network)?;
                self.refresh()
            }
        }
    }

    /// Whether `signed` carries this wallet's own signature. An unlocked
    /// session that cannot be reached counts as no.
    pub fn signed_by_this_wallet(&self, signed: &SignedOrder, network: &Hash32) -> bool {
        match self {
            Signing::Here(wallet) => wallet.signed_by_this_wallet(signed, network),
            Signing::Unlocked { session, .. } => session.owns(signed, network).unwrap_or(false),
        }
    }
}
