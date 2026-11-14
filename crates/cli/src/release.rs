//! Signed releases, and `orien update`.
//!
//! Every release publishes SHA256SUMS, the checksum of each build, and
//! SHA256SUMS.sig, a signature over it made with the Orien release key: a
//! hash-based key tree, quantum-proof like every other signature in Orien.
//! The public half of that key is built into this program, so `orien update`
//! installs only builds the release key vouches for, even if the website that
//! serves them were taken over.

use anyhow::{Context, Result, anyhow, bail};
use orien_crypto::keytree::{self, KeyTree, TreeSignature};
use orien_crypto::{Hash32, Tag, hash};
use orien_net::keys::random_seed;
use orien_net::store::write_replacing;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The public half of the Orien release key.
const RELEASE_ROOT: &str = "58f7ebd7712eeeca98f1a66d23046345b462f1a1f500443d362f5a94c116d25f";
const RELEASE_HEIGHT: u8 = 10;

/// Where releases are published.
const DOWNLOADS: &str = "https://orien.network/downloads";

/// The release key's secret, and how many of its one-time keys are used.
#[derive(Serialize, Deserialize)]
struct ReleaseKeyFile {
    seed: String,
    height: u8,
    /// The next one-time key to sign with. Never goes back.
    next: u32,
}

/// What a release signature covers: the release's checksums.
fn message(sums: &[u8]) -> Hash32 {
    hash(Tag::Release, &[sums])
}

/// Makes a new release key at `path`. Prints only its public half.
pub fn keygen(path: &Path) -> Result<()> {
    if path.exists() {
        bail!("{} already exists; refusing to overwrite a key", path.display());
    }
    let file = ReleaseKeyFile { seed: hex::encode(random_seed()), height: RELEASE_HEIGHT, next: 0 };
    let seed = parse_seed(&file.seed)?;
    let root = KeyTree::generate(seed, file.height).root();
    write_replacing(path, (serde_json::to_string_pretty(&file)? + "\n").as_bytes(), true)?;
    println!("created {} (keep it secret and backed up offline)", path.display());
    println!("public root {root}");
    println!("height      {}", file.height);
    Ok(())
}

/// Signs a release's SHA256SUMS with the release key, writing SHA256SUMS.sig
/// beside it. The key's counter moves on and is saved before the signature
/// exists, so no one-time key ever signs twice, even after a crash.
pub fn sign(key_path: &Path, sums_path: &Path) -> Result<()> {
    let text = fs::read_to_string(key_path).with_context(|| format!("cannot read {}", key_path.display()))?;
    let mut file: ReleaseKeyFile =
        serde_json::from_str(&text).with_context(|| format!("{} is not a release key", key_path.display()))?;
    let tree = KeyTree::generate(parse_seed(&file.seed)?, file.height);
    if file.next >= tree.capacity() {
        bail!("this release key has signed all {} releases it can", tree.capacity());
    }
    let leaf = file.next;
    file.next += 1;
    write_replacing(key_path, (serde_json::to_string_pretty(&file)? + "\n").as_bytes(), true)?;

    let sums = fs::read(sums_path).with_context(|| format!("cannot read {}", sums_path.display()))?;
    let signature = tree.sign_with_leaf(leaf, message(&sums).as_bytes());
    let out = signature_path(sums_path);
    fs::write(&out, hex::encode(borsh::to_vec(&signature)?) + "\n")?;
    println!("signed {} with release key {} (one-time key {leaf})", sums_path.display(), tree.root().short());
    Ok(())
}

fn signature_path(sums_path: &Path) -> PathBuf {
    let mut name = sums_path.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

fn parse_seed(text: &str) -> Result<[u8; 32]> {
    hex::decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| anyhow!("the key file's seed is not 64 hexadecimal characters"))
}

/// Whether `signature` is the release key's over `sums`.
pub fn verify(sums: &[u8], signature: &str, root: &Hash32) -> bool {
    let Ok(bytes) = hex::decode(signature.trim()) else { return false };
    let Ok(signature) = borsh::from_slice::<TreeSignature>(&bytes) else { return false };
    keytree::verify(root, RELEASE_HEIGHT, &signature, message(sums).as_bytes())
}

/// This computer's build name, as the release files name it.
fn platform() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x86_64",
        (os, arch) => bail!("there is no orien build for {os} on {arch}"),
    })
}

/// A version number as three numbers, for comparing.
fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.').map(|part| part.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Fetches a URL with the system's curl, which Mac, Linux and Windows 10 or
/// later all have.
fn fetch(url: &str, out: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(out)
        .arg(url)
        .status()
        .context("cannot run curl, which orien update needs")?;
    if !status.success() {
        bail!("could not download {url}");
    }
    Ok(())
}

/// Replaces this program with the newest signed release, if there is one.
pub fn update() -> Result<()> {
    let base = std::env::var("ORIEN_DOWNLOADS").unwrap_or_else(|_| DOWNLOADS.to_string());
    let root: Hash32 = RELEASE_ROOT.parse().map_err(|_| anyhow!("this build has no release key"))?;
    let current = env!("CARGO_PKG_VERSION");
    let work = std::env::temp_dir().join(format!("orien-update-{}", std::process::id()));
    fs::create_dir_all(&work)?;
    let outcome = update_in(&base, &root, current, &work);
    let _ = fs::remove_dir_all(&work);
    outcome
}

fn update_in(base: &str, root: &Hash32, current: &str, work: &Path) -> Result<()> {
    fetch(&format!("{base}/latest"), &work.join("latest"))?;
    let latest = fs::read_to_string(work.join("latest"))?.trim().to_string();
    match (version(&latest), version(current)) {
        (Some(new), Some(have)) if new > have => {}
        (Some(_), Some(_)) => {
            println!("orien {current} is the newest version");
            return Ok(());
        }
        _ => bail!("cannot read the version numbers {latest:?} and {current:?}"),
    }

    // The checksums must carry the release key's signature before anything
    // they list is trusted.
    fetch(&format!("{base}/SHA256SUMS"), &work.join("SHA256SUMS"))?;
    fetch(&format!("{base}/SHA256SUMS.sig"), &work.join("SHA256SUMS.sig"))?;
    let sums = fs::read(work.join("SHA256SUMS"))?;
    let signature = fs::read_to_string(work.join("SHA256SUMS.sig"))?;
    if !verify(&sums, &signature, root) {
        bail!("the release is not signed with the Orien release key; nothing was changed");
    }

    let platform = platform()?;
    let windows = platform.starts_with("windows");
    let archive = format!("orien-{latest}-{platform}.{}", if windows { "zip" } else { "tar.gz" });
    let expected = String::from_utf8_lossy(&sums)
        .lines()
        .find_map(|line| line.strip_suffix(&format!("  {archive}")).map(str::to_string))
        .ok_or_else(|| anyhow!("the signed checksums list no {archive}"))?;
    fetch(&format!("{base}/{archive}"), &work.join(&archive))?;
    let actual = hex::encode(Sha256::digest(fs::read(work.join(&archive))?));
    if actual != expected.trim() {
        bail!("{archive} does not match its signed checksum; nothing was changed");
    }

    let status = Command::new("tar").arg("-xf").arg(work.join(&archive)).arg("-C").arg(work).status()?;
    if !status.success() {
        bail!("could not unpack {archive}");
    }
    let program = if windows { "orien.exe" } else { "orien" };
    let me = std::env::current_exe()?;
    let new = me.with_extension("new");
    fs::copy(work.join(program), &new)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&new, fs::Permissions::from_mode(0o755))?;
    }
    if windows {
        // A running program cannot be overwritten on Windows, but it can be
        // moved aside.
        let old = me.with_extension("old.exe");
        let _ = fs::remove_file(&old);
        fs::rename(&me, &old)?;
    }
    fs::rename(&new, &me)?;
    println!("updated orien from {current} to {latest}");
    // The new version's skill replaces any copies of the old one.
    let _ = Command::new(&me).args(["skill", "install", "--refresh"]).status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(version("1.10.0") > version("1.9.3"));
        assert!(version("v2.0.0") > version("1.99.99"));
        assert_eq!(version("1.0"), None);
    }

    #[test]
    fn only_the_release_keys_signature_over_these_checksums_counts() {
        let tree = KeyTree::generate([7; 32], RELEASE_HEIGHT);
        let sums = b"abc  orien-1.0.1-macos-arm64.tar.gz\n";
        let signature = hex::encode(borsh::to_vec(&tree.sign_with_leaf(0, message(sums).as_bytes())).unwrap());
        assert!(verify(sums, &signature, &tree.root()));
        // Other checksums, another key, or no signature at all: refused.
        assert!(!verify(b"abd  orien-1.0.1-macos-arm64.tar.gz\n", &signature, &tree.root()));
        assert!(!verify(sums, &signature, &KeyTree::generate([8; 32], RELEASE_HEIGHT).root()));
        assert!(!verify(sums, "not a signature", &tree.root()));
    }
}
