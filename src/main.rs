use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, broadcast};
use tokio::signal;
use ed25519_dalek::SigningKey;
use redb::{Database, TableDefinition};

// Importamos tus librerías externas
use ouroboros_db::{OuroborosConfig, OuroborosDB};
// use synap2p; // Tu librería QUIC

// Módulos internos
mod opcodes;
mod types;
mod core;
mod api;

use types::{AppState, EngineCommand, P2pEvent};

const INDEX_TABLE: TableDefinition<&[u8; 32], u32> = TableDefinition::new("index_table");

#[tokio::main]
async fn main() {
    // Te recomiendo usar la caja 'tracing' en lugar de println! para logs de producción
    println!("🚀 Iniciando Diarsaba Orchestrator...");

    // ==========================================
    // 1. CREACIÓN DEL SISTEMA NERVIOSO (CANALES)
    // ==========================================
    // mpsc: Múltiples productores (Axum, P2P), un consumidor (Engine). Buffer de 1000.
    let (engine_tx, engine_rx) = mpsc::channel::<EngineCommand>(1000);
    
    // broadcast: Un productor (P2P), múltiples consumidores. Buffer de 100.
    let (p2p_tx, _p2p_rx) = broadcast::channel::<P2pEvent>(100);

    // ==========================================
    // 2. INICIALIZACIÓN DE BASES DE DATOS
    // ==========================================
    println!("📦 Levantando motores de almacenamiento...");
    
    let temp_db = Database::create("tempdb.redb").expect("Error fatal: No se pudo crear tempdb.redb");
    let write_txn = temp_db.begin_write().unwrap();
    // 2. ABRIR LA NUEVA TABLA
    write_txn.open_table(INDEX_TABLE).unwrap();
    write_txn.commit().unwrap();
    let temp_db_arc = Arc::new(temp_db);

    // Ouroboros (Disco Circular)
    let ouro_config = OuroborosConfig::load_or_init("ciclidb_produccion.db")
        .expect("Error al cargar config de Ouroboros");
    let ouro_db = OuroborosDB::open("ciclidb_produccion.db", ouro_config)
        .expect("Error fatal: No se pudo iniciar OuroborosDB");
    let ouro_db_arc = Arc::new(RwLock::new(ouro_db));

    // ==========================================
    // 3. IDENTIDAD CRIPTOGRÁFICA
    // ==========================================
    // NOTA: En producción, cargar esto desde un archivo seguro o variables de entorno.
    let server_keypair = Arc::new(SigningKey::from_bytes(&[1u8; 32]));

    // ==========================================
    // 4. INICIALIZACIÓN DE LA RED P2P (Synap2p)
    // ==========================================
    println!("🌐 Iniciando nodo QUIC Synap2p...");
    // Simulamos la inicialización de tu red. 
    // Lo ideal es que synap2p reciba un clon de `p2p_tx` para emitir eventos hacia el orquestador,
    // o que retorne un Receiver de eventos. Aquí simulamos la suscripción:
    let mut p2p_event_rx = p2p_tx.subscribe();
    
    // tokio::spawn(synap2p::start_node(config, p2p_tx.clone()));

    // ==========================================
    // 5. PREPARACIÓN DEL ESTADO GLOBAL
    // ==========================================
    let app_state = AppState {
        temp_db: Arc::clone(&temp_db_arc),
       server_keypair: Arc::clone(&server_keypair),
        engine_tx,
        p2p_tx,
    };

    // ==========================================
    // 6. LEVANTAMIENTO DE ACTORES PRINCIPALES
    // ==========================================
    // Actor A: El Motor Genético
    tokio::spawn(crate::core::run_engine(
        engine_rx,
        Arc::clone(&ouro_db_arc),
        Arc::clone(&temp_db_arc),
        Arc::clone(&server_keypair),
    ));

    // Actor B: Servidor HTTP (Axum)
    let router = api::build_router(app_state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    
    tokio::spawn(async move {
        println!("🎧 Servidor web Axum escuchando en el puerto 80...");
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("💥 El servidor Axum colapsó: {}", e);
        }
    });

    // ==========================================
    // 7. EL BUCLE MAESTRO (Event Loop / Fail-Fast)
    // ==========================================
    println!("🛡️ Orquestador en línea. Entrando al bucle de eventos...");
    
    loop {
        tokio::select! {
            // EVENTO A: Apagado del Sistema (Ctrl+C o SIGTERM)
            _ = signal::ctrl_c() => {
                println!("\n🛑 Señal de apagado recibida. Iniciando Graceful Shutdown...");
                break;
            }
            
            // EVENTO B: Mensajes de la Red P2P (synap2p)
            Ok(event) = p2p_event_rx.recv() => {
                match event {
                    P2pEvent::IncomingCell { from_peer, data } => {
                        println!("📡 Célula recibida del peer {}", from_peer);
                        // Aquí enviarías la data al Motor Genético para validarla
                        // app_state.engine_tx.send(EngineCommand::Mutate {...}).await;
                    }
                    P2pEvent::PeerDiscovered(peer) => {
                        println!("🤝 Nuevo peer conectado: {}", peer);
                    }
                }
            }

            // EVENTO C: Fallo catastrófico en canales
            else => {
                // Si el canal `broadcast` se cierra inesperadamente, el nodo P2P murió.
                // Aplicamos Fail-Fast: salimos del loop para reiniciar el proceso a nivel de SO (Systemd/Docker).
                eprintln!("💥 Error crítico en la comunicación de red. Fail-Fast activado.");
                break;
            }
        }
    }

    // ==========================================
    // 8. LIMPIEZA FINAL (Graceful Shutdown)
    // ==========================================
    println!("🧹 Sincronizando bases de datos y cerrando conexiones...");
    // Al salir de main(), se hace "drop" de todos los Arc.
    // Si tu OuroborosDB implementa la trait `Drop`, aquí es donde hará el flush final a disco.
    println!("💤 Sistema apagado correctamente.");
}