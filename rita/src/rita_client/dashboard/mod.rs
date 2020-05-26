//! This file contains all the network endpoints used for the client dashbaord. This management dashboard
//! is for users to use to configure and manage their router and should be firewalled from the outside
//! world.
//!
//! For more documentation on specific functions see the router-dashboard file in the docs folder

pub mod auth;
pub mod backup_created;
pub mod eth_private_key;
pub mod exits;
pub mod installation_details;
pub mod interfaces;
pub mod localization;
pub mod logging;
pub mod mesh_ip;
pub mod neighbors;
pub mod notifications;
pub mod operator;
pub mod prices;
pub mod release_feed;
pub mod remote_access;
pub mod router;
pub mod system_chain;
pub mod usage;
pub mod wifi;
