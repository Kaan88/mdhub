//! How bytes come from github.com. A trait, so tests and the dev loop never touch the network.
//! OWNER: the transport+check agent. Implement `Net` over `mdshared_fetch::fetch_file`, and
//! `FileRoot` (https://github.com/X -> <root>/X); keep the trait signature.
use std::time::Duration;

pub const USER_AGENT: &str = "Hub/0.1";

pub trait Transport: Send + Sync {
    /// GET `url` (https only), at most `max_bytes`, within `timeout`. Err is a short reason.
    fn get(&self, url: &str, max_bytes: usize, timeout: Duration) -> Result<Vec<u8>, String>;
}
