use cosmic_config::CosmicConfigEntry;
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u64 = 2;

#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    cosmic_config::cosmic_config_derive::CosmicConfigEntry,
)]
#[version = 2]
pub struct QuakeConfig {
    pub terminal_args: Vec<String>,
}
