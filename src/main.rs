use std::sync::Arc;

use diarsaba::db_handler::CellEngineLike;
use diarsaba::net_handler::{NodeClientLike, PeerId};
use diarsaba::server::{serve, AppState};
use tokio::sync::broadcast;

struct NoopDb;

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

struct NoopNet;

impl NodeClientLike for NoopNet {
    type Error = &'static str;

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
async fn main() -> std::io::Result<()> {
    let (event_tx, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        db: Arc::new(NoopDb),
        net: Arc::new(NoopNet),
        event_tx,
    });

    serve("127.0.0.1:3000", state).await
}
