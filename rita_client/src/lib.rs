#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate failure;

pub mod dashboard;
pub mod exit_manager;
pub mod heartbeat;
pub mod light_client_manager;
pub mod logging;
pub mod operator_fee_manager;
pub mod operator_update;
pub mod rita_loop;
pub mod traffic_watcher;

use rita_common::READABLE_VERSION;

pub use crate::dashboard::auth::*;
pub use crate::dashboard::backup_created::*;
pub use crate::dashboard::bandwidth_limit::*;
pub use crate::dashboard::contact_info::*;
pub use crate::dashboard::contact_info::*;
pub use crate::dashboard::eth_private_key::*;
pub use crate::dashboard::exits::*;
pub use crate::dashboard::installation_details::*;
pub use crate::dashboard::interfaces::*;
pub use crate::dashboard::localization::*;
pub use crate::dashboard::logging::*;
pub use crate::dashboard::mesh_ip::*;
pub use crate::dashboard::neighbors::*;
pub use crate::dashboard::notifications::*;
pub use crate::dashboard::operator::*;
pub use crate::dashboard::prices::*;
pub use crate::dashboard::remote_access::*;
pub use crate::dashboard::router::*;
pub use crate::dashboard::system_chain::*;
pub use crate::dashboard::usage;
pub use crate::dashboard::wifi::*;
use settings::client::{default_config_path, RitaClientSettings, APP_NAME};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
pub struct Args {
    #[serde(default = "default_config_path")]
    pub flag_config: String,
    pub flag_platform: String,
    pub flag_future: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            flag_config: default_config_path(),
            flag_platform: "linux".to_string(),
            flag_future: false,
        }
    }
}

/// TODO platform is in the process of being removed as a support argument
/// as it's not even used. Config can still be used but has a sane default
/// and does not need to be specified.
pub fn get_client_usage(version: &str, git_hash: &str) -> String {
    format!(
        "Usage: {} [--config=<settings>] [--platform=<platform>] [--future]
Options:
    -c, --config=<settings>     Name of config file
    -p, --platform=<platform>   Platform (linux or OpenWrt)
    --future                    Enable B side of A/B releases
About:
    Version {} - {}
    git hash {}",
        APP_NAME, READABLE_VERSION, version, git_hash
    )
}

/// Some devices (n600/n750) will provide junk file reads during disk init
/// post flashing, this adds in retry for the settings file read for up to
/// two minutes
pub fn wait_for_settings(settings_file: &str) -> RitaClientSettings {
    let start = Instant::now();
    let timeout = Duration::from_secs(120);
    let mut res = RitaClientSettings::new(settings_file);
    while (Instant::now() - start) < timeout {
        if let Ok(val) = res {
            return val;
        }
        res = RitaClientSettings::new(settings_file);
    }
    match res {
        Ok(val) => val,
        Err(e) => panic!("Settings parse failure {:?}", e),
    }
}
