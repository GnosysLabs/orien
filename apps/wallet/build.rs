//! Builds the network's genesis file into the app, so people never have to
//! find or choose it. By default that is the live Orien network, from
//! `web/public/network/genesis.json`. To build for a test network instead:
//! `ORIEN_GENESIS=/path/to/genesis.json`.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=ORIEN_GENESIS");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("genesis.json");
    let path = env::var("ORIEN_GENESIS").unwrap_or_else(|_| {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        manifest.join("../../web/public/network/genesis.json").display().to_string()
    });
    println!("cargo:rerun-if-changed={path}");
    let genesis = fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    fs::write(out, genesis).unwrap();
    tauri_build::build()
}
