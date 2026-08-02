// Release builds must not spawn a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wispr_lightning_lib::run();
}
