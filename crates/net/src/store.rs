//! A validator's files. Three things must survive a crash:
//!
//! - which orders it voted for, or it could vote for a conflicting one;
//! - which one-time keys it has used, or it could reuse one;
//! - the certificates it applied, from which balances are rebuilt.

use orien_crypto::ots;
use orien_ledger::{AccountId, Certificate, SignedOrder};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::wire::MAX_FRAME;

/// Writes `bytes` to `path` by way of a temporary file, so a crash leaves
/// either the old contents or the new, never a mixture.
pub fn write_replacing(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    if durable {
        file.sync_all()?;
    }
    drop(file);
    fs::rename(&temporary, path)?;
    if durable {
        // The rename lives in the directory, which needs its own sync to
        // survive a power cut.
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        sync_dir(parent)?;
    }
    Ok(())
}

/// Forces a directory's entries (new files, renames) to disk.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Where a validator keeps each epoch's data.
///
/// An epoch has its own folder, holding the state the epoch started from
/// and the certificates applied during it. Moving to the next epoch writes
/// the new folder first and then replaces one small file that names the
/// current epoch, so a crash leaves the validator wholly in the old epoch or
/// wholly in the new one. A restart reads only the current epoch's folder,
/// and so can never replay a payment that a checkpoint left out.
pub struct Epochs {
    dir: PathBuf,
    current: u64,
    durable: bool,
}

impl Epochs {
    pub fn open(dir: &Path, durable: bool) -> io::Result<Epochs> {
        let current = match fs::read_to_string(dir.join("epoch")) {
            Ok(text) => text
                .trim()
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "corrupt epoch number"))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        let epochs = Epochs { dir: dir.to_owned(), current, durable };
        fs::create_dir_all(epochs.folder(current))?;
        // Before epochs existed, the certificate log sat at the top of the
        // data directory. It is epoch 0's log.
        let old = dir.join("certs.log");
        if current == 0 && old.exists() && !epochs.log().exists() {
            fs::rename(&old, epochs.log())?;
        }
        if durable {
            epochs.sync()?;
        }
        Ok(epochs)
    }

    pub fn current(&self) -> u64 {
        self.current
    }

    fn folder(&self, epoch: u64) -> PathBuf {
        self.dir.join("epochs").join(epoch.to_string())
    }

    /// The current epoch's certificate log.
    pub fn log(&self) -> PathBuf {
        self.folder(self.current).join("certs.log")
    }

    /// The state the current epoch started from. Epoch 0 has none: it
    /// starts from the genesis file.
    pub fn snapshot(&self) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.folder(self.current).join("state")) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && self.current == 0 => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Starts the next epoch from `snapshot`, with an empty certificate log.
    pub fn advance(&mut self, snapshot: &[u8]) -> io::Result<()> {
        let next = self.current + 1;
        let folder = self.folder(next);
        // A crash during an earlier attempt can have left this folder behind.
        match fs::remove_dir_all(&folder) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::create_dir_all(&folder)?;
        write_replacing(&folder.join("state"), snapshot, self.durable)?;
        if self.durable {
            sync_dir(&self.dir.join("epochs"))?;
        }
        // The one step that commits the change.
        write_replacing(&self.dir.join("epoch"), next.to_string().as_bytes(), self.durable)?;
        self.current = next;
        Ok(())
    }

    /// Forces the folders themselves to disk, so that files created in them
    /// survive a power cut.
    pub fn sync(&self) -> io::Result<()> {
        sync_dir(&self.folder(self.current))?;
        sync_dir(&self.dir.join("epochs"))?;
        sync_dir(&self.dir)
    }
}

/// Append-only log of applied certificates, in the order they were applied.
pub struct CertLog {
    file: File,
    /// Offset and length of each certificate's bytes.
    entries: Vec<(u64, u32)>,
    by_payment: HashMap<(AccountId, u64), u32>,
    end: u64,
}

impl CertLog {
    /// Opens the log, calling `replay` for every certificate in it.
    pub fn open(path: &Path, mut replay: impl FnMut(&Certificate)) -> io::Result<CertLog> {
        let file = OpenOptions::new().read(true).append(true).create(true).open(path)?;
        let mut entries = Vec::new();
        let mut by_payment = HashMap::new();
        let mut end = 0u64;
        let mut reader = BufReader::new(&file);
        loop {
            let mut length = [0u8; 4];
            if reader.read_exact(&mut length).is_err() {
                break;
            }
            let length = u32::from_le_bytes(length);
            if length as usize > MAX_FRAME {
                break;
            }
            let mut bytes = vec![0u8; length as usize];
            if reader.read_exact(&mut bytes).is_err() {
                break;
            }
            let Ok(certificate) = borsh::from_slice::<Certificate>(&bytes) else { break };
            by_payment.insert((certificate.order.from, certificate.order.seq), entries.len() as u32);
            entries.push((end + 4, length));
            end += 4 + length as u64;
            replay(&certificate);
        }
        drop(reader);
        // A crash mid-write can leave a partial record at the end. Drop it.
        if file.metadata()?.len() != end {
            file.set_len(end)?;
        }
        Ok(CertLog { file, entries, by_payment, end })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn append(&mut self, certificate: &Certificate) -> io::Result<()> {
        let bytes = borsh::to_vec(certificate)?;
        let mut record = Vec::with_capacity(4 + bytes.len());
        record.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        record.extend_from_slice(&bytes);
        self.file.write_all(&record)?;
        let key = (certificate.order.from, certificate.order.seq);
        self.by_payment.insert(key, self.entries.len() as u32);
        self.entries.push((self.end + 4, bytes.len() as u32));
        self.end += record.len() as u64;
        Ok(())
    }

    pub fn get(&mut self, index: usize) -> io::Result<Option<Certificate>> {
        let Some(&(offset, length)) = self.entries.get(index) else { return Ok(None) };
        let mut bytes = vec![0u8; length as usize];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut bytes)?;
        borsh::from_slice(&bytes).map(Some)
    }

    pub fn find(&mut self, account: &AccountId, seq: u64) -> io::Result<Option<Certificate>> {
        match self.by_payment.get(&(*account, seq)) {
            Some(&index) => self.get(index as usize),
            None => Ok(None),
        }
    }

    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_data()
    }
}

/// The orders this validator has voted for, signatures included.
///
/// A record is the vote's promise: once it is on disk, the validator will
/// never vote for a different order under the same payment number. The
/// whole signed order is kept, not just its digest, so that any copy of the
/// owner's wallet can fetch it and finish the payment.
pub struct OrderLog {
    file: File,
}

/// A signed order has the same size every time: the order's five fields,
/// then the signature.
const ORDER_RECORD: usize = 32 + 8 + 32 + 8 + 32 + ots::SIGNATURE_BYTES;

impl OrderLog {
    /// Opens the log, calling `live` for every order in it. An order for
    /// which it returns false is dropped from the file: its payment number
    /// is final, so nothing will ever ask about it again. The log therefore
    /// holds only what the next restart still needs.
    pub fn open(
        path: &Path,
        durable: bool,
        mut live: impl FnMut(&SignedOrder) -> bool,
    ) -> io::Result<OrderLog> {
        let mut kept = Vec::new();
        let mut rewrite = false;
        match File::open(path) {
            Ok(file) => {
                let length = file.metadata()?.len();
                // A crash mid-write can leave a partial record at the end.
                rewrite = length % ORDER_RECORD as u64 != 0;
                let mut reader = BufReader::new(file);
                let mut record = [0u8; ORDER_RECORD];
                for _ in 0..length / ORDER_RECORD as u64 {
                    reader.read_exact(&mut record)?;
                    let order: SignedOrder = borsh::from_slice(&record)?;
                    if live(&order) {
                        kept.extend_from_slice(&record);
                    } else {
                        rewrite = true;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if rewrite {
            write_replacing(path, &kept, durable)?;
        }
        let file = OpenOptions::new().append(true).create(true).open(path)?;
        Ok(OrderLog { file })
    }

    /// Records orders. With `durable`, returns only once they are on disk.
    pub fn append(&mut self, orders: &[&SignedOrder], durable: bool) -> io::Result<()> {
        if orders.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(orders.len() * ORDER_RECORD);
        for order in orders {
            borsh::to_writer(&mut bytes, order)?;
        }
        self.file.write_all(&bytes)?;
        if durable {
            self.file.sync_data()?;
        }
        Ok(())
    }
}

/// Hands out one-time key numbers, never the same one twice.
///
/// Numbers are reserved on disk in blocks before any of them is used. After
/// a crash the validator resumes from the end of the reserved block, wasting
/// a few keys instead of risking a repeat.
pub struct LeafCounter {
    path: PathBuf,
    next: u32,
    reserved: u32,
    capacity: u32,
    durable: bool,
}

impl LeafCounter {
    const BLOCK: u32 = 64;

    pub fn open(path: PathBuf, capacity: u32, durable: bool) -> io::Result<LeafCounter> {
        let reserved = match fs::read(&path) {
            Ok(bytes) => {
                let bytes: [u8; 4] = bytes
                    .try_into()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "corrupt key counter"))?;
                u32::from_le_bytes(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        Ok(LeafCounter { path, next: reserved, reserved, capacity, durable })
    }

    /// The next unused key number, or `None` when the key tree is used up.
    pub fn take(&mut self) -> io::Result<Option<u32>> {
        self.take_leaving(0)
    }

    /// The next unused key number, unless taking it would leave fewer than
    /// `reserve` keys. Ordinary work stops short of the end of the tree so
    /// that the keys a validator needs to close an epoch are always there.
    pub fn take_leaving(&mut self, reserve: u32) -> io::Result<Option<u32>> {
        if self.next.saturating_add(reserve) >= self.capacity {
            return Ok(None);
        }
        if self.next >= self.reserved {
            let reserved = self.next.saturating_add(Self::BLOCK).min(self.capacity);
            write_replacing(&self.path, &reserved.to_le_bytes(), self.durable)?;
            self.reserved = reserved;
        }
        let leaf = self.next;
        self.next += 1;
        Ok(Some(leaf))
    }

    pub fn used(&self) -> u32 {
        self.next.min(self.capacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_crypto::Hash32;
    use orien_ledger::Order;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orien-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_numbers_never_repeat_across_restarts() {
        let dir = scratch("leaf");
        let path = dir.join("leaf");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..5 {
            let mut counter = LeafCounter::open(path.clone(), 1000, false).unwrap();
            for _ in 0..10 {
                assert!(seen.insert(counter.take().unwrap().unwrap()));
            }
            // Dropped without any shutdown step, as in a crash.
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reserve_of_keys_is_held_back() {
        let dir = scratch("leaf-reserve");
        let mut counter = LeafCounter::open(dir.join("leaf"), 5, false).unwrap();
        assert_eq!(counter.take_leaving(2).unwrap(), Some(0));
        assert_eq!(counter.take_leaving(2).unwrap(), Some(1));
        assert_eq!(counter.take_leaving(2).unwrap(), Some(2));
        assert_eq!(counter.take_leaving(2).unwrap(), None, "the last two are held back");
        assert_eq!(counter.used(), 3);
        // Work that the reserve exists for can still draw on it.
        assert_eq!(counter.take().unwrap(), Some(3));
        assert_eq!(counter.take().unwrap(), Some(4));
        assert_eq!(counter.take().unwrap(), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_epoch_change_survives_a_restart() {
        let dir = scratch("epochs");
        let mut epochs = Epochs::open(&dir, false).unwrap();
        assert_eq!(epochs.current(), 0);
        assert_eq!(epochs.snapshot().unwrap(), None, "epoch 0 starts from the genesis file");
        let first_log = epochs.log();
        fs::write(&first_log, b"certificates of epoch 0").unwrap();

        epochs.advance(b"state after epoch 0").unwrap();
        for epochs in [epochs, Epochs::open(&dir, false).unwrap()] {
            assert_eq!(epochs.current(), 1);
            assert_eq!(epochs.snapshot().unwrap().as_deref(), Some(&b"state after epoch 0"[..]));
            assert_ne!(epochs.log(), first_log);
            assert!(!epochs.log().exists(), "the new epoch starts with no certificates");
        }
        // The old epoch's log is left where it was.
        assert_eq!(fs::read(&first_log).unwrap(), b"certificates of epoch 0");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_half_made_epoch_is_ignored() {
        let dir = scratch("epochs-half");
        Epochs::open(&dir, false).unwrap();
        // A crash after the next folder was written but before the switch.
        fs::create_dir_all(dir.join("epochs/1")).unwrap();
        fs::write(dir.join("epochs/1/state"), b"never committed").unwrap();
        fs::write(dir.join("epochs/1/certs.log"), b"stray").unwrap();

        let mut epochs = Epochs::open(&dir, false).unwrap();
        assert_eq!(epochs.current(), 0);
        assert_eq!(epochs.snapshot().unwrap(), None);
        // Doing it again properly replaces what was left behind.
        epochs.advance(b"committed").unwrap();
        assert_eq!(epochs.snapshot().unwrap().as_deref(), Some(&b"committed"[..]));
        assert!(!epochs.log().exists());
        // An epoch past the first cannot start without its snapshot.
        fs::remove_file(dir.join("epochs/1/state")).unwrap();
        assert!(Epochs::open(&dir, false).unwrap().snapshot().is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_older_data_directory_is_adopted() {
        let dir = scratch("epochs-old");
        fs::write(dir.join("certs.log"), b"old certificates").unwrap();
        let epochs = Epochs::open(&dir, false).unwrap();
        assert_eq!(fs::read(epochs.log()).unwrap(), b"old certificates");
        assert!(!dir.join("certs.log").exists());
        // Opening again changes nothing.
        let again = Epochs::open(&dir, false).unwrap();
        assert_eq!(fs::read(again.log()).unwrap(), b"old certificates");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn key_numbers_stop_at_capacity() {
        let dir = scratch("leaf-cap");
        let mut counter = LeafCounter::open(dir.join("leaf"), 3, false).unwrap();
        assert_eq!(counter.take().unwrap(), Some(0));
        assert_eq!(counter.take().unwrap(), Some(1));
        assert_eq!(counter.take().unwrap(), Some(2));
        assert_eq!(counter.take().unwrap(), None);
        let mut reopened = LeafCounter::open(dir.join("leaf"), 3, false).unwrap();
        assert_eq!(reopened.take().unwrap(), None);
        let _ = fs::remove_dir_all(dir);
    }

    fn signed_order(n: u8) -> SignedOrder {
        let order = Order {
            from: Hash32([n; 32]),
            seq: n as u64,
            to: Hash32([n + 1; 32]),
            amount: 5,
            next_key: Hash32([n + 2; 32]),
        };
        SignedOrder { signature: ots::SecretKey::from_seed([n; 32]).sign(b"any message"), order }
    }

    fn replay(path: &Path, mut live: impl FnMut(&SignedOrder) -> bool) -> (OrderLog, Vec<SignedOrder>) {
        let mut seen = Vec::new();
        let log = OrderLog::open(path, false, |order| {
            seen.push(order.clone());
            live(order)
        })
        .unwrap();
        (log, seen)
    }

    #[test]
    fn order_log_survives_a_torn_write() {
        let dir = scratch("orders");
        let path = dir.join("orders.log");
        let (a, b) = (signed_order(1), signed_order(3));
        assert_eq!(borsh::to_vec(&a).unwrap().len(), ORDER_RECORD);
        let (mut log, seen) = replay(&path, |_| true);
        assert!(seen.is_empty());
        log.append(&[&a, &b], false).unwrap();
        // Simulate a crash partway through a third record.
        OpenOptions::new().append(true).open(&path).unwrap().write_all(&[9u8; 20]).unwrap();

        let (mut log, seen) = replay(&path, |_| true);
        assert_eq!(seen, vec![a.clone(), b.clone()]);
        assert_eq!(fs::metadata(&path).unwrap().len(), 2 * ORDER_RECORD as u64);
        // Appending still works after the tail was cut off.
        let c = signed_order(5);
        log.append(&[&c], false).unwrap();
        assert_eq!(replay(&path, |_| true).1, vec![a, b, c]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn order_log_drops_orders_that_are_final() {
        let dir = scratch("orders-final");
        let path = dir.join("orders.log");
        let (a, b, c) = (signed_order(1), signed_order(3), signed_order(5));
        replay(&path, |_| true).0.append(&[&a, &b, &c], false).unwrap();

        // On the next start, payments 1 and 5 turn out to be final.
        let (mut log, seen) = replay(&path, |order| order.order.seq == 3);
        assert_eq!(seen.len(), 3);
        assert_eq!(fs::metadata(&path).unwrap().len(), ORDER_RECORD as u64);
        let d = signed_order(7);
        log.append(&[&d], false).unwrap();
        assert_eq!(replay(&path, |_| true).1, vec![b, d]);
        let _ = fs::remove_dir_all(dir);
    }
}
