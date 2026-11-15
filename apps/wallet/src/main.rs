// The desktop program: everything is in the library, which the iPhone app
// shares.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    orien_wallet_lib::run()
}
