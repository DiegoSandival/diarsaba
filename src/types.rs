use ed25519_dalek::SigningKey;
use redb::Database;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, broadcast};
use crate::opcodes::Opcode;

// --- TIPOS BASE ---
// Alias para darle semántica a los bytes crudos.
pub type CellId = [u8; 32]; 
pub type SignatureBytes = [u8; 64];

// --- ESTADO DE LA APLICACIÓN (AXUM) ---
/// Este es el estado que inyectaremos en nuestros endpoints HTTP.
/// Todo aquí adentro debe ser barato de clonar (por eso usamos Arc y canales).
#[derive(Clone)]
pub struct AppState {
    /// Nuestra base de datos transaccional / índices rápidos.
    pub temp_db: Arc<Database>,
    
    /// La llave privada del nodo para firmar células.
    /// Usamos Arc porque instanciar SigningKey puede ser costoso y no siempre implementa Clone por defecto.
    pub server_keypair: Arc<SigningKey>,
    
    /// Canal (Sender) para enviar intenciones de escritura/lectura al Motor Genético (core.rs).
    pub engine_tx: mpsc::Sender<EngineCommand>,
    
    /// Canal para emitir eventos hacia la red P2P (synap2p).
    pub p2p_tx: broadcast::Sender<P2pEvent>,
}

// --- MENSAJERÍA INTERNA (El "Pegamento") ---

/// Comandos que el servidor HTTP o la red P2P envían al Motor Genético.
#[derive(Debug)]
pub enum EngineCommand {
    /// Petición para validar y escribir una célula en Ouroboros.
    Mutate {
        opcode: Opcode,
        payload: Vec<u8>,
        pubkey: [u8; 32],
        signature: SignatureBytes,
        /// Canal de un solo uso para devolver la respuesta asíncrona al solicitante (ej. handler de Axum).
        reply_to: oneshot::Sender<Result<CellId, String>>,
    },
    
    /// Petición de lectura (ej. para servir un archivo en diarsaba.com).
    Query {
        id: CellId,
        reply_to: oneshot::Sender<Result<Vec<u8>, String>>,
    },
}

/// Eventos que ocurren en la capa de red (synap2p) y que el orquestador debe manejar.
#[derive(Debug, Clone)]
pub enum P2pEvent {
    /// Un nodo vecino nos empujó una célula nueva.
    IncomingCell {
        from_peer: String,
        data: Vec<u8>,
    },
    /// Un nuevo nodo se conectó mediante QUIC.
    PeerDiscovered(String),
}