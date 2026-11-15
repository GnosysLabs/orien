//! The address book: nicknames for addresses, kept in `contacts.json` in
//! the app's folder, and kept in step across devices through the open
//! wallet's vault (see `vault.rs`), sealed so only that wallet can read it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    /// 64 lowercase hex characters.
    pub address: String,
    /// When it was last added or changed, in seconds since 1970: across
    /// devices, the newest version of a contact wins.
    #[serde(default)]
    pub updated: u64,
}

/// The whole book as devices share it: contacts, and when each removed one
/// was removed, so a removal on one device isn't undone by another.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Book {
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub deleted: std::collections::BTreeMap<String, u64>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn deleted(dir: &Path) -> std::collections::BTreeMap<String, u64> {
    fs::read(dir.join("contacts-deleted.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// The book, with removals, as this device has it.
pub fn book(dir: &Path) -> Book {
    Book { contacts: list(dir), deleted: deleted(dir) }
}

/// Takes in another device's book: for each address, the newest of its
/// versions there and here, or its removal if that's newer. True if this
/// device's book changed.
pub fn merge(dir: &Path, other: &Book) -> Result<bool, String> {
    let mine = book(dir);
    let mut removed = mine.deleted.clone();
    for (address, at) in &other.deleted {
        let entry = removed.entry(address.clone()).or_insert(0);
        *entry = (*entry).max(*at);
    }
    let mut newest: std::collections::BTreeMap<String, Contact> = std::collections::BTreeMap::new();
    for contact in mine.contacts.iter().chain(&other.contacts) {
        match newest.get(&contact.address) {
            Some(kept) if kept.updated >= contact.updated => {}
            _ => {
                newest.insert(contact.address.clone(), contact.clone());
            }
        }
    }
    let merged: Vec<Contact> =
        newest.into_values().filter(|c| removed.get(&c.address).is_none_or(|at| *at < c.updated)).collect();
    let changed = merged.len() != mine.contacts.len()
        || merged.iter().any(|c| !mine.contacts.iter().any(|m| m.address == c.address && m.name == c.name))
        || removed != mine.deleted;
    if changed {
        store(dir, &merged)?;
        store_deleted(dir, &removed)?;
    }
    Ok(changed)
}

fn store_deleted(dir: &Path, deleted: &std::collections::BTreeMap<String, u64>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(deleted).map_err(|error| error.to_string())?;
    fs::write(dir.join("contacts-deleted.json"), text).map_err(|error| error.to_string())
}

pub fn list(dir: &Path) -> Vec<Contact> {
    let mut contacts: Vec<Contact> =
        fs::read(dir.join("contacts.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    contacts.sort_by_key(|contact| contact.name.to_lowercase());
    contacts
}

fn store(dir: &Path, contacts: &[Contact]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(contacts).map_err(|error| error.to_string())?;
    let temporary = dir.join("contacts.json.new");
    fs::write(&temporary, text).map_err(|error| error.to_string())?;
    fs::rename(temporary, dir.join("contacts.json")).map_err(|error| error.to_string())
}

/// Adds a contact, or renames the one with this address. `previous` is the
/// address being edited, when the edit changes it.
pub fn save(dir: &Path, name: &str, address: &str, previous: Option<&str>) -> Result<(), String> {
    let name = name.trim();
    let address = address.trim().to_lowercase();
    if name.is_empty() {
        return Err("Give the contact a name.".into());
    }
    if name.chars().count() > 40 {
        return Err("Keep the name under 40 characters.".into());
    }
    if address.len() != 64 || !address.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("That is not an Orien address. An address is 64 letters and digits.".into());
    }
    let mut contacts = list(dir);
    if let Some(previous) = previous {
        contacts.retain(|contact| contact.address != previous);
        if previous != address {
            let mut removed = deleted(dir);
            removed.insert(previous.to_string(), now());
            store_deleted(dir, &removed)?;
        }
    }
    // With the contact being edited set aside, no other may share its
    // address or its name.
    if let Some(other) = contacts.iter().find(|contact| contact.address == address) {
        return Err(format!("That address is already saved as {}.", other.name));
    }
    if let Some(other) = contacts.iter().find(|contact| contact.name.to_lowercase() == name.to_lowercase()) {
        return Err(format!("{} is already the name of another address. Choose a different name.", other.name));
    }
    let mut removed = deleted(dir);
    if removed.remove(&address).is_some() {
        store_deleted(dir, &removed)?;
    }
    contacts.push(Contact { name: name.to_owned(), address, updated: now() });
    store(dir, &contacts)
}

pub fn delete(dir: &Path, address: &str) -> Result<(), String> {
    let mut contacts = list(dir);
    contacts.retain(|contact| contact.address != address);
    let mut removed = deleted(dir);
    removed.insert(address.to_string(), now());
    store_deleted(dir, &removed)?;
    store(dir, &contacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_renames_and_deletes() {
        let dir = std::env::temp_dir().join(format!("orien-contacts-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("contacts.json"));
        let bob = "01487c022b1d56bcff496df629ddca5c1488e6dcffec284311c228eecdea321b";
        let carol = "25e6dcd54bf195cb5a9fa522ed03db563254fab0898e37d440498086e1995b91";

        save(&dir, "Bob", bob, None).unwrap();
        save(&dir, "carol", &carol.to_uppercase(), None).unwrap();
        assert!(save(&dir, "Bobby", bob, None).is_err(), "one address, one name");
        assert!(save(&dir, "bob", carol, Some(carol)).is_err(), "names stay unique");
        assert!(save(&dir, "", bob, None).is_err());
        assert!(save(&dir, "X", "1234", None).is_err());

        save(&dir, "Robert", bob, Some(bob)).unwrap(); // rename
        let names: Vec<String> = list(&dir).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["carol", "Robert"]);
        assert_eq!(list(&dir)[0].address, carol, "addresses are kept in lowercase");

        delete(&dir, bob).unwrap();
        assert_eq!(list(&dir).len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
