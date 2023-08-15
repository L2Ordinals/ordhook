pub mod file;
pub mod generator;

use crate::core::OrdhookConfig;
pub use chainhook_sdk::indexer::IndexerConfig;
use chainhook_sdk::observer::EventObserverConfig;
use chainhook_sdk::types::{
    BitcoinBlockSignaling, BitcoinNetwork, StacksNetwork, StacksNodeConfig,
};
pub use file::ConfigFile;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

const DEFAULT_MAINNET_ORDINALS_SQLITE_ARCHIVE: &str =
    "https://archive.hiro.so/mainnet/chainhooks/hord.sqlite";
const DEFAULT_REDIS_URI: &str = "redis://localhost:6379/";

pub const DEFAULT_INGESTION_PORT: u16 = 20455;
pub const DEFAULT_CONTROL_PORT: u16 = 20456;
pub const STACKS_SCAN_THREAD_POOL_SIZE: usize = 10;
pub const BITCOIN_SCAN_THREAD_POOL_SIZE: usize = 10;
pub const STACKS_MAX_PREDICATE_REGISTRATION: usize = 50;
pub const BITCOIN_MAX_PREDICATE_REGISTRATION: usize = 50;

#[derive(Clone, Debug)]
pub struct Config {
    pub storage: StorageConfig,
    pub http_api: PredicatesApi,
    pub limits: LimitsConfig,
    pub network: IndexerConfig,
    pub bootstrap: BootstrapConfig,
    pub logs: LogConfig,
}

#[derive(Clone, Debug)]
pub struct LogConfig {
    pub ordinals_internals: bool,
    pub chainhook_internals: bool,
}

#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub working_dir: String,
}

#[derive(Clone, Debug)]
pub enum PredicatesApi {
    Off,
    On(PredicatesApiConfig),
}

#[derive(Clone, Debug)]
pub struct PredicatesApiConfig {
    pub http_port: u16,
    pub database_uri: String,
    pub display_logs: bool,
}

#[derive(Clone, Debug)]
pub enum BootstrapConfig {
    Build,
    Download(String),
}

#[derive(Clone, Debug)]
pub struct PathConfig {
    pub file_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct UrlConfig {
    pub file_url: String,
}

#[derive(Clone, Debug)]
pub struct LimitsConfig {
    pub max_number_of_bitcoin_predicates: usize,
    pub max_number_of_concurrent_bitcoin_scans: usize,
    pub max_number_of_stacks_predicates: usize,
    pub max_number_of_concurrent_stacks_scans: usize,
    pub max_number_of_processing_threads: usize,
    pub bitcoin_concurrent_http_requests_max: usize,
    pub max_caching_memory_size_mb: usize,
}

impl Config {
    pub fn from_file_path(file_path: &str) -> Result<Config, String> {
        let file = File::open(file_path)
            .map_err(|e| format!("unable to read file {}\n{:?}", file_path, e))?;
        let mut file_reader = BufReader::new(file);
        let mut file_buffer = vec![];
        file_reader
            .read_to_end(&mut file_buffer)
            .map_err(|e| format!("unable to read file {}\n{:?}", file_path, e))?;

        let config_file: ConfigFile = match toml::from_slice(&file_buffer) {
            Ok(s) => s,
            Err(e) => {
                return Err(format!("Config file malformatted {}", e.to_string()));
            }
        };
        Config::from_config_file(config_file)
    }

    pub fn is_http_api_enabled(&self) -> bool {
        match self.http_api {
            PredicatesApi::Off => false,
            PredicatesApi::On(_) => true,
        }
    }

    pub fn get_ordhook_config(&self) -> OrdhookConfig {
        OrdhookConfig {
            network_thread_max: self.limits.bitcoin_concurrent_http_requests_max,
            ingestion_thread_max: self.limits.max_number_of_processing_threads,
            ingestion_thread_queue_size: 4,
            cache_size: self.limits.max_caching_memory_size_mb,
            db_path: self.expected_cache_path(),
            first_inscription_height: match self.network.bitcoin_network {
                BitcoinNetwork::Mainnet => 767430,
                BitcoinNetwork::Regtest => 1,
                BitcoinNetwork::Testnet => 2413343,
                // BitcoinNetwork::Signet => 112402,
            },
            logs: self.logs.clone(),
        }
    }

    pub fn get_event_observer_config(&self) -> EventObserverConfig {
        EventObserverConfig {
            bitcoin_rpc_proxy_enabled: true,
            event_handlers: vec![],
            chainhook_config: None,
            ingestion_port: DEFAULT_INGESTION_PORT,
            bitcoind_rpc_username: self.network.bitcoind_rpc_username.clone(),
            bitcoind_rpc_password: self.network.bitcoind_rpc_password.clone(),
            bitcoind_rpc_url: self.network.bitcoind_rpc_url.clone(),
            bitcoin_block_signaling: self.network.bitcoin_block_signaling.clone(),
            display_logs: false,
            cache_path: self.storage.working_dir.clone(),
            bitcoin_network: self.network.bitcoin_network.clone(),
            stacks_network: self.network.stacks_network.clone(),
        }
    }

    pub fn from_config_file(config_file: ConfigFile) -> Result<Config, String> {
        let (stacks_network, bitcoin_network) = match config_file.network.mode.as_str() {
            "devnet" => (StacksNetwork::Devnet, BitcoinNetwork::Regtest),
            "testnet" => (StacksNetwork::Testnet, BitcoinNetwork::Testnet),
            "mainnet" => (StacksNetwork::Mainnet, BitcoinNetwork::Mainnet),
            _ => return Err("network.mode not supported".to_string()),
        };

        let bootstrap = match config_file.bootstrap {
            Some(bootstrap) => match bootstrap.download_url {
                Some(ref url) => BootstrapConfig::Download(url.to_string()),
                None => BootstrapConfig::Build,
            },
            None => BootstrapConfig::Build,
        };

        let config = Config {
            storage: StorageConfig {
                working_dir: config_file.storage.working_dir.unwrap_or("ordhook".into()),
            },
            http_api: match config_file.http_api {
                None => PredicatesApi::Off,
                Some(http_api) => match http_api.disabled {
                    Some(false) => PredicatesApi::Off,
                    _ => PredicatesApi::On(PredicatesApiConfig {
                        http_port: http_api.http_port.unwrap_or(DEFAULT_CONTROL_PORT),
                        display_logs: http_api.display_logs.unwrap_or(true),
                        database_uri: http_api
                            .database_uri
                            .unwrap_or(DEFAULT_REDIS_URI.to_string()),
                    }),
                },
            },
            bootstrap,
            limits: LimitsConfig {
                max_number_of_stacks_predicates: config_file
                    .limits
                    .max_number_of_stacks_predicates
                    .unwrap_or(STACKS_MAX_PREDICATE_REGISTRATION),
                max_number_of_bitcoin_predicates: config_file
                    .limits
                    .max_number_of_bitcoin_predicates
                    .unwrap_or(BITCOIN_MAX_PREDICATE_REGISTRATION),
                max_number_of_concurrent_stacks_scans: config_file
                    .limits
                    .max_number_of_concurrent_stacks_scans
                    .unwrap_or(STACKS_SCAN_THREAD_POOL_SIZE),
                max_number_of_concurrent_bitcoin_scans: config_file
                    .limits
                    .max_number_of_concurrent_bitcoin_scans
                    .unwrap_or(BITCOIN_SCAN_THREAD_POOL_SIZE),
                max_number_of_processing_threads: config_file
                    .limits
                    .max_number_of_processing_threads
                    .unwrap_or(1.max(num_cpus::get().saturating_sub(1))),
                bitcoin_concurrent_http_requests_max: config_file
                    .limits
                    .bitcoin_concurrent_http_requests_max
                    .unwrap_or(1.max(num_cpus::get().saturating_sub(1))),
                max_caching_memory_size_mb: config_file
                    .limits
                    .max_caching_memory_size_mb
                    .unwrap_or(2048),
            },
            network: IndexerConfig {
                bitcoind_rpc_url: config_file.network.bitcoind_rpc_url.to_string(),
                bitcoind_rpc_username: config_file.network.bitcoind_rpc_username.to_string(),
                bitcoind_rpc_password: config_file.network.bitcoind_rpc_password.to_string(),
                bitcoin_block_signaling: match config_file.network.bitcoind_zmq_url {
                    Some(ref zmq_url) => BitcoinBlockSignaling::ZeroMQ(zmq_url.clone()),
                    None => BitcoinBlockSignaling::Stacks(StacksNodeConfig::default_localhost(
                        config_file
                            .network
                            .stacks_events_ingestion_port
                            .unwrap_or(DEFAULT_INGESTION_PORT),
                    )),
                },
                stacks_network,
                bitcoin_network,
            },
            logs: LogConfig {
                ordinals_internals: config_file
                    .logs
                    .as_ref()
                    .and_then(|l| l.ordinals_internals)
                    .unwrap_or(true),
                chainhook_internals: config_file
                    .logs
                    .as_ref()
                    .and_then(|l| l.chainhook_internals)
                    .unwrap_or(true),
            },
        };
        Ok(config)
    }

    pub fn should_bootstrap_through_download(&self) -> bool {
        match &self.bootstrap {
            BootstrapConfig::Build => false,
            BootstrapConfig::Download(_) => true,
        }
    }

    pub fn expected_api_database_uri(&self) -> &str {
        &self.expected_api_config().database_uri
    }

    pub fn expected_api_config(&self) -> &PredicatesApiConfig {
        match self.http_api {
            PredicatesApi::On(ref config) => config,
            _ => unreachable!(),
        }
    }

    pub fn expected_cache_path(&self) -> PathBuf {
        let mut destination_path = PathBuf::new();
        destination_path.push(&self.storage.working_dir);
        destination_path
    }

    fn expected_remote_ordinals_sqlite_base_url(&self) -> &str {
        match &self.bootstrap {
            BootstrapConfig::Build => unreachable!(),
            BootstrapConfig::Download(url) => &url,
        }
    }

    pub fn expected_remote_ordinals_sqlite_sha256(&self) -> String {
        format!("{}.sha256", self.expected_remote_ordinals_sqlite_base_url())
    }

    pub fn expected_remote_ordinals_sqlite_url(&self) -> String {
        format!("{}.gz", self.expected_remote_ordinals_sqlite_base_url())
    }

    pub fn default(
        devnet: bool,
        testnet: bool,
        mainnet: bool,
        config_path: &Option<String>,
    ) -> Result<Config, String> {
        let config = match (devnet, testnet, mainnet, config_path) {
            (true, false, false, _) => Config::devnet_default(),
            (false, true, false, _) => Config::testnet_default(),
            (false, false, true, _) => Config::mainnet_default(),
            (false, false, false, Some(config_path)) => Config::from_file_path(&config_path)?,
            _ => Err("Invalid combination of arguments".to_string())?,
        };
        Ok(config)
    }

    pub fn devnet_default() -> Config {
        Config {
            storage: StorageConfig {
                working_dir: default_cache_path(),
            },
            http_api: PredicatesApi::Off,
            bootstrap: BootstrapConfig::Build,
            limits: LimitsConfig {
                max_number_of_bitcoin_predicates: BITCOIN_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_bitcoin_scans: BITCOIN_SCAN_THREAD_POOL_SIZE,
                max_number_of_stacks_predicates: STACKS_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_stacks_scans: STACKS_SCAN_THREAD_POOL_SIZE,
                max_number_of_processing_threads: 1.max(num_cpus::get().saturating_sub(1)),
                bitcoin_concurrent_http_requests_max: 1.max(num_cpus::get().saturating_sub(1)),
                max_caching_memory_size_mb: 2048,
            },
            network: IndexerConfig {
                bitcoind_rpc_url: "http://0.0.0.0:18443".into(),
                bitcoind_rpc_username: "devnet".into(),
                bitcoind_rpc_password: "devnet".into(),
                bitcoin_block_signaling: BitcoinBlockSignaling::Stacks(
                    StacksNodeConfig::default_localhost(DEFAULT_INGESTION_PORT),
                ),
                stacks_network: StacksNetwork::Devnet,
                bitcoin_network: BitcoinNetwork::Regtest,
            },
            logs: LogConfig {
                ordinals_internals: true,
                chainhook_internals: false,
            },
        }
    }

    pub fn testnet_default() -> Config {
        Config {
            storage: StorageConfig {
                working_dir: default_cache_path(),
            },
            http_api: PredicatesApi::Off,
            bootstrap: BootstrapConfig::Build,
            limits: LimitsConfig {
                max_number_of_bitcoin_predicates: BITCOIN_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_bitcoin_scans: BITCOIN_SCAN_THREAD_POOL_SIZE,
                max_number_of_stacks_predicates: STACKS_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_stacks_scans: STACKS_SCAN_THREAD_POOL_SIZE,
                max_number_of_processing_threads: 1.max(num_cpus::get().saturating_sub(1)),
                bitcoin_concurrent_http_requests_max: 1.max(num_cpus::get().saturating_sub(1)),
                max_caching_memory_size_mb: 2048,
            },
            network: IndexerConfig {
                bitcoind_rpc_url: "http://0.0.0.0:18332".into(),
                bitcoind_rpc_username: "devnet".into(),
                bitcoind_rpc_password: "devnet".into(),
                bitcoin_block_signaling: BitcoinBlockSignaling::Stacks(
                    StacksNodeConfig::default_localhost(DEFAULT_INGESTION_PORT),
                ),
                stacks_network: StacksNetwork::Testnet,
                bitcoin_network: BitcoinNetwork::Testnet,
            },
            logs: LogConfig {
                ordinals_internals: true,
                chainhook_internals: false,
            },
        }
    }

    pub fn mainnet_default() -> Config {
        Config {
            storage: StorageConfig {
                working_dir: default_cache_path(),
            },
            http_api: PredicatesApi::Off,
            bootstrap: BootstrapConfig::Download(
                DEFAULT_MAINNET_ORDINALS_SQLITE_ARCHIVE.to_string(),
            ),
            limits: LimitsConfig {
                max_number_of_bitcoin_predicates: BITCOIN_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_bitcoin_scans: BITCOIN_SCAN_THREAD_POOL_SIZE,
                max_number_of_stacks_predicates: STACKS_MAX_PREDICATE_REGISTRATION,
                max_number_of_concurrent_stacks_scans: STACKS_SCAN_THREAD_POOL_SIZE,
                max_number_of_processing_threads: 1.max(num_cpus::get().saturating_sub(1)),
                bitcoin_concurrent_http_requests_max: 1.max(num_cpus::get().saturating_sub(1)),
                max_caching_memory_size_mb: 2048,
            },
            network: IndexerConfig {
                bitcoind_rpc_url: "http://0.0.0.0:8332".into(),
                bitcoind_rpc_username: "devnet".into(),
                bitcoind_rpc_password: "devnet".into(),
                bitcoin_block_signaling: BitcoinBlockSignaling::Stacks(
                    StacksNodeConfig::default_localhost(DEFAULT_INGESTION_PORT),
                ),
                stacks_network: StacksNetwork::Mainnet,
                bitcoin_network: BitcoinNetwork::Mainnet,
            },
            logs: LogConfig {
                ordinals_internals: true,
                chainhook_internals: false,
            },
        }
    }
}

pub fn default_cache_path() -> String {
    let mut cache_path = std::env::current_dir().expect("unable to get current dir");
    cache_path.push("ordhook");
    format!("{}", cache_path.display())
}