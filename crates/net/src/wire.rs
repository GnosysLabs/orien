//! The wire protocol: one request and one response per frame, over TCP.
//! A frame is a 4-byte little-endian length followed by that many bytes.

use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::Hash32;
use orien_crypto::merkle;
use orien_ledger::close::{Decided, Message, Share};
use crate::history::HistoryPage;
use orien_ledger::{Account, AccountId, Certificate, Confirmation, Escrow, LocksProof, Reject, SignedOrder, Signer, Vote};
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 16 << 20;

// Signatures are over a kilobyte and travel inline; these values are short-lived.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Request {
    Info,
    Account(AccountId),
    /// Ask the validator to vote for an order.
    SubmitOrder(SignedOrder),
    /// Hand the validator a certificate to apply. It does not sign anything
    /// in return; `Confirm` is the request for that.
    SubmitCertificate(Certificate),
    /// A certificate applied in the current epoch.
    GetCertificate { account: AccountId, seq: u64 },
    /// The signed order this validator has voted for under the account's
    /// current payment number, if that payment is not final yet.
    Pending(AccountId),
    /// The fingerprint of every account as this validator holds them.
    Root,
    /// Two different orders signed with the account's current key under its
    /// current payment number, if this validator has seen both.
    Evidence(AccountId),
    /// Hand the validator a certificate to apply, and ask it to sign a
    /// confirmation that it has. A payment is final once validators holding
    /// more than two thirds of the stake have confirmed it.
    Confirm(Certificate),
    /// One validator catching up with another. It names its epoch, and how
    /// much of this validator's certificate log and closing messages for
    /// that epoch it has read already.
    Sync { epoch: u64, certs: u64, messages: u64, max: u32 },
    /// A message about closing `epoch`, from one validator to another.
    Close { epoch: u64, message: Message },
    /// The orders in the log that a closing statement pins, as digests,
    /// from position `from`.
    Log { epoch: u64, statement: Hash32, from: u64, max: u32 },
    /// Certificates applied in `epoch`, by the digests of their orders.
    Payments { epoch: u64, orders: Vec<Hash32> },
    /// Part of the state `epoch` started from.
    Snapshot { epoch: u64, offset: u64, max: u32 },
    /// The checkpoints that closed epochs `from` onwards, with their proofs.
    Checkpoints { from: u64, max: u32 },
    /// An account as the latest checkpoint left it, with a proof against
    /// that checkpoint's state root.
    Proof(AccountId),
    /// The current epoch's seed and work target, for mining.
    Work,
    /// A work share for the current epoch.
    SubmitShare(Share),
    /// The locked payment `from` made as order `seq`: still waiting, or the
    /// secret that claimed it.
    Locked { from: AccountId, seq: u64 },
    /// Every locked payment and revealed secret as the latest checkpoint
    /// left them, with proof against that checkpoint's state root.
    Locks,
    /// What came into `account` from epoch `from` on, as this validator
    /// recorded it: about `max` entries, in whole epochs.
    History { account: AccountId, from: u64, max: u32 },
    /// `History`, with what the account sent too (as `Income::Sent`).
    /// Validators recorded that only from release 1.7.0 on.
    Activity { account: AccountId, from: u64, max: u32 },
    /// Up to `max` of the work shares this validator holds for `epoch`, in
    /// order, from just after `after`: for a validator that started partway
    /// through the epoch to catch up on the shares it missed.
    Shares { epoch: u64, after: Option<Share>, max: u32 },
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct NodeInfo {
    pub network: Hash32,
    /// Its place in this epoch's list of validators, if it is in it.
    pub validator: Option<u16>,
    pub name: String,
    pub accounts: u64,
    pub certificates: u64,
    pub keys_used: u32,
    pub keys_total: u32,
    pub epoch: u64,
    /// True while the validator is closing its epoch and takes no payments.
    pub closing: bool,
    /// When the current epoch began, in seconds since 1970.
    pub epoch_started: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AccountView {
    pub balance: i128,
    pub seq: u64,
    /// The key that must sign the account's next order.
    pub signer: Signer,
    /// The root of the account's spare keys, all zeros until its first
    /// order reveals it.
    pub spare: Hash32,
    /// Digest of the order this validator has voted for at `seq`, if any.
    pub lock: Option<Hash32>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Response {
    Info(NodeInfo),
    Account(Option<AccountView>),
    Vote(Vote),
    /// The certificate is applied. `fresh` is false if it already was.
    Applied { fresh: bool },
    Certificate(Option<Certificate>),
    Certs { certs: Vec<Certificate>, total: u64 },
    /// The validator refuses to vote, for a reason the ledger rules define.
    Rejected(Reject),
    /// The request was malformed or failed verification.
    Invalid(String),
    /// The validator cannot serve the request right now.
    Unavailable(String),
    Pending(Option<SignedOrder>),
    Root(Hash32),
    Evidence(Option<(SignedOrder, SignedOrder)>),
    Confirmed(Confirmation),
    /// The validator is closing its epoch. It votes and confirms nothing
    /// until the checkpoint is agreed, which takes a few seconds.
    Closing,
    /// What was sent belongs to another epoch. This is the validator's own.
    Epoch(u64),
    Received,
    Sync {
        /// The answering validator's epoch.
        epoch: u64,
        certs: Vec<Certificate>,
        certs_total: u64,
        messages: Vec<Message>,
        messages_total: u64,
        /// The checkpoint that closed the asker's epoch, when the answering
        /// validator is already past it.
        decided: Option<Decided>,
    },
    Log(Option<Vec<Hash32>>),
    Payments(Vec<Certificate>),
    /// Some bytes of a state, and the size of the whole.
    Snapshot(Option<(Vec<u8>, u64)>),
    Checkpoints(Vec<Decided>),
    Proof { epoch: u64, account: Option<(Account, merkle::Path)> },
    Work { epoch: u64, seed: Hash32, target: u64 },
    /// The share is taken; `fresh` when this validator had not had it.
    Share { fresh: bool },
    /// A locked payment as this validator holds it now: waiting, or claimed
    /// with this secret, or neither (refunded, long claimed, or unknown).
    Locked { epoch: u64, escrow: Option<Escrow>, secret: Option<[u8; 32]> },
    Locks { epoch: u64, proof: LocksProof },
    History(HistoryPage),
    /// Shares held for `epoch`, the answering validator's current one. Empty
    /// when the asker named another epoch, or there are no more.
    Shares { epoch: u64, shares: Vec<Share> },
}

pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()
}

/// Reads one frame. Returns `None` if the peer closed the connection cleanly.
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}
