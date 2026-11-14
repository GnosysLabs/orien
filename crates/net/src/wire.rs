//! The wire protocol: one request and one response per frame, over TCP.
//! A frame is a 4-byte little-endian length followed by that many bytes.

use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::Hash32;
use orien_ledger::{AccountId, Certificate, Reject, SignedOrder, Vote};
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
    /// Hand the validator proof that an order is final.
    SubmitCertificate(Certificate),
    GetCertificate { account: AccountId, seq: u64 },
    /// Certificates in the order this validator applied them, from `index`.
    CertsSince { index: u64, max: u32 },
    /// The signed order this validator has voted for under the account's
    /// current payment number, if that payment is not final yet.
    Pending(AccountId),
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct NodeInfo {
    pub network: Hash32,
    pub validator: u16,
    pub name: String,
    pub accounts: u64,
    pub certificates: u64,
    pub keys_used: u32,
    pub keys_total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AccountView {
    pub balance: i128,
    pub seq: u64,
    pub key: Hash32,
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
