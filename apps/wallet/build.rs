//! Builds the network's genesis file into the app, so people never have to
//! find or choose it. Which network: `ORIEN_GENESIS=/path/to/genesis.json`
//! at build time. Without it the app is built for no network and says so.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=ORIEN_GENESIS");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("genesis.json");
    let genesis = match env::var("ORIEN_GENESIS") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"))
        }
        Err(_) => String::new(),
    };
    fs::write(out, genesis).unwrap();
    tauri_build::build()
}
