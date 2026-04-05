use std::sync::Arc;

#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
use diarsaba::db_handler::CellEngineLike;
#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
use diarsaba::net_handler::{Multiaddr, NodeClientLike, PeerId};
use diarsaba::server::{serve, AppState};
use tokio::sync::broadcast;

#[cfg(feature = "correspondence")]
use correspondence::CellEngine;

#[cfg(all(feature = "correspondence", feature = "synap2p"))]
use std::{env, io};

#[cfg(all(feature = "correspondence", feature = "synap2p"))]
use std::path::PathBuf;

#[cfg(all(feature = "correspondence", feature = "synap2p"))]
use diarsaba::server::forward_network_events;

#[cfg(feature = "synap2p")]
use synap2p::{NodeClient, NodeConfig};

#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
struct NoopDb;

#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
impl CellEngineLike for NoopDb {
    type Error = &'static str;

    async fn ejecutar(
        &self,
        _target_index: u32,
        _solucion: &[u8; 32],
        _opcode: u8,
        _params: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        Err("correspondence engine not configured")
    }
}

#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
struct NoopNet;

#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
impl NodeClientLike for NoopNet {
    type Error = &'static str;

    async fn connect_to_node(&self, _peer: PeerId, _addr: Multiaddr) -> Result<(), Self::Error> {
        Err("synap2p client not configured")
    }

    async fn subscribe(&self, _topic: String) -> Result<(), Self::Error> {
        Err("synap2p client not configured")
    }

    async fn send_direct_message(&self, _peer: PeerId, _data: Vec<u8>) -> Result<(), Self::Error> {
        Err("synap2p client not configured")
    }

    async fn publish_message(&self, _topic: String, _data: Vec<u8>) -> Result<(), Self::Error> {
        Err("synap2p client not configured")
    }

    async fn announce_provider(&self, _key: String) -> Result<(), Self::Error> {
        Err("synap2p client not configured")
    }

    async fn find_providers(&self, _key: String) -> Result<Vec<PeerId>, Self::Error> {
        Err("synap2p client not configured")
    }
}

#[tokio::main]
#[cfg(all(feature = "correspondence", feature = "synap2p"))]
async fn main() -> std::io::Result<()> {
    run_real_server().await
}

#[tokio::main]
#[cfg(not(all(feature = "correspondence", feature = "synap2p")))]
async fn main() -> std::io::Result<()> {
    let (event_tx, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        db: Arc::new(NoopDb),
        net: Arc::new(NoopNet),
        event_tx,
    });

    serve("127.0.0.1:3000", state).await
}

#[cfg(all(feature = "correspondence", feature = "synap2p"))]
async fn run_real_server() -> io::Result<()> {
    let bind_addr = env::var("DIARSABA_BIND_ADDR").unwrap_or_else(|_| String::from("127.0.0.1:3000"));
    let circular_db_path = env::var("DIARSABA_DB_PATH").unwrap_or_else(|_| String::from("./diarsaba.circular.db"));
    let kv_path = env::var("DIARSABA_KV_PATH").unwrap_or_else(|_| String::from("./diarsaba.kv.db"));
    let mut node_config = NodeConfig::default();

    if let Ok(listen_port) = env::var("DIARSABA_P2P_LISTEN_PORT") {
        node_config.listen_port = parse_env_number::<u16>("DIARSABA_P2P_LISTEN_PORT", &listen_port)?;
    }

    if let Ok(identity_path) = env::var("DIARSABA_P2P_IDENTITY_PATH") {
        node_config.identity_path = PathBuf::from(identity_path);
    }

    if let Ok(command_channel_size) = env::var("DIARSABA_P2P_COMMAND_CHANNEL_SIZE") {
        node_config.command_channel_size = parse_env_number::<usize>(
            "DIARSABA_P2P_COMMAND_CHANNEL_SIZE",
            &command_channel_size,
        )?;
    }

    if let Ok(event_channel_size) = env::var("DIARSABA_P2P_EVENT_CHANNEL_SIZE") {
        node_config.event_channel_size = parse_env_number::<usize>(
            "DIARSABA_P2P_EVENT_CHANNEL_SIZE",
            &event_channel_size,
        )?;
    }

    let db = CellEngine::new(&circular_db_path, &kv_path)
        .await
        .map_err(|error| io::Error::other(format!("failed to initialize correspondence: {error}")))?;

    let (net, network_events) = NodeClient::start(node_config)
        .await
        .map_err(|error| io::Error::other(format!("failed to initialize synap2p: {error}")))?;

    let (event_tx, _) = broadcast::channel(256);
    tokio::spawn(forward_network_events(network_events, event_tx.clone()));

    let state = Arc::new(AppState {
        db: Arc::new(db),
        net: Arc::new(net),
        event_tx,
    });

    serve(&bind_addr, state).await
}

#[cfg(all(feature = "correspondence", feature = "synap2p"))]
fn parse_env_number<T>(name: &str, value: &str) -> io::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid {name}: {error}")))
}
