use crate::five_nodes::five_node_config;
use crate::setup_utils::database::start_postgres;
use crate::setup_utils::namespaces::setup_ns;
use crate::setup_utils::namespaces::Namespace;
use crate::setup_utils::rita::thread_spawner;
use crate::utils::{generate_traffic, register_all_namespaces_to_exit, validate_debt_entry};
use crate::utils::{
    get_default_settings, send_eth_bulk, test_reach_all, test_routes, TEST_PAY_THRESH,
};
use clarity::Address as EthAddress;
use clarity::{PrivateKey as EthPrivateKey, Uint256};
use log::info;
use rita_common::debt_keeper::GetDebtsResult;
use settings::client::RitaClientSettings;
use settings::exit::RitaExitSettingsStruct;
use std::thread;
use std::time::Duration;
use web30::client::Web3;

/// Key with funds in the EVM that can be sent to routers
pub const ETH_MINER_KEY: &str =
    "0xb1bab011e03a9862664706fc3bbaa1b16651528e5f0e7fbfcbfdd8be302a13e7";

pub fn get_miner_key() -> EthPrivateKey {
    ETH_MINER_KEY.parse().unwrap()
}

pub fn get_miner_address() -> EthAddress {
    get_miner_key().to_address()
}

/// The chain id of the ethereum testnet
pub fn eth_chain_id() -> Uint256 {
    417834u64.into()
}

pub const WEB3_TIMEOUT: Duration = Duration::from_secs(1);
pub const ONE_ETH: u128 = 1_000_000_000_000_000_000;

/// Runs a five node fixed network map test scenario
pub async fn run_eth_payments_test_scenario() {
    info!("Starting eth payments test");
    let node_config = five_node_config();
    let namespaces = node_config.0;
    let expected_routes = node_config.1;

    let (mut client_settings, mut exit_settings) =
        get_default_settings("test".to_string(), namespaces.clone());

    // Set payment thresholds low enough so that they get triggered after an iperf
    let (client_settings, exit_settings) =
        eth_payments_map(&mut client_settings, &mut exit_settings);

    namespaces.validate();
    start_postgres();

    let res = setup_ns(namespaces.clone());
    info!("Namespaces setup: {res:?}");

    let rita_identities = thread_spawner(namespaces.clone(), client_settings, exit_settings)
        .expect("Could not spawn Rita threads");
    info!("Thread Spawner: {res:?}");

    test_reach_all(namespaces.clone());
    test_routes(namespaces.clone(), expected_routes);

    info!("Registering routers to the exit");
    register_all_namespaces_to_exit(namespaces.clone()).await;

    thread::sleep(Duration::from_secs(10));

    let from_node: Option<Namespace> = namespaces.get_namespace(1);
    let forward_node: Option<Namespace> = namespaces.get_namespace(3);
    let end_node: Option<Namespace> = namespaces.get_namespace(6);

    // start main test content
    let web3 = Web3::new("http://localhost:8545", WEB3_TIMEOUT);
    let mut to_top_up = Vec::new();
    for c in rita_identities.client_identities {
        to_top_up.push(c.eth_address);
    }
    for e in rita_identities.exit_identities {
        to_top_up.push(e.eth_address)
    }

    info!("Sending 50 eth to all routers");
    send_eth_bulk((ONE_ETH * 50).into(), &to_top_up, &web3).await;

    info!("Trying to generate traffic");
    generate_traffic(
        from_node.clone().unwrap(),
        end_node.clone(),
        "1G".to_string(),
    );

    validate_debt_entry(
        from_node.unwrap(),
        forward_node.unwrap(),
        &eth_payment_conditions,
    )
    .await;
}

fn eth_payment_conditions(debts: GetDebtsResult) -> bool {
    matches!(
        (
            debts.payment_details.total_payment_sent > TEST_PAY_THRESH.into(),
            debts.payment_details.debt < TEST_PAY_THRESH.into(),
        ),
        (true, true)
    )
}

fn eth_payments_map(
    c_set: &mut RitaClientSettings,
    exit_set: &mut RitaExitSettingsStruct,
) -> (RitaClientSettings, RitaExitSettingsStruct) {
    c_set.payment.payment_threshold = TEST_PAY_THRESH.into();
    exit_set.payment.payment_threshold = TEST_PAY_THRESH.into();
    (c_set.clone(), exit_set.clone())
}
