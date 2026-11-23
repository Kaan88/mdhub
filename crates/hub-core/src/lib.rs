//! Hub's headless core. No window, no threads of its own except where a module says so.
//! Module owners: catalog (fixed), manifest, state+detect, transport+check, install.
pub mod autostart;
pub mod catalog;
pub mod check;
pub mod detect;
pub mod install;
pub mod manifest;
pub mod model;
pub mod notes;
pub mod notify;
pub mod selfupdate;
pub mod state;
pub mod transport;
