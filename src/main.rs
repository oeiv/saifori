#![deny(unsafe_op_in_unsafe_fn)]
// The only unsafe FFI is isolated in secret_store.rs and reviewed against libsecret.
mod ai;
mod charts;
mod db;
mod demo;
mod file_io;
mod i18n;
mod insights;
mod matching;
mod model;
mod secret_store;
mod ui;

fn main() {
    ui::launch();
}
