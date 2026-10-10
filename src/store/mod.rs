//! Persistent storage: the encrypted vault and plain settings.

pub mod atomic;
pub mod crypto;
pub mod model;
pub mod settings;
pub mod vault;

pub use model::*;
pub use settings::Settings;
pub use vault::{Protection, Vault};
