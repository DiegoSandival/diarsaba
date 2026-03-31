use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey}; 
use redb::{Database, TableDefinition, ReadableTable, ReadableDatabase};

// Importamos Ouroboros y el tipo RecordIndex
use ouroboros_db::{OuroborosDB, RecordIndex}; 

use crate::types::{CellId, EngineCommand, SignatureBytes};
use crate::opcodes::Opcode;

// La misma definición de tabla que pusimos en main.rs
const INDEX_TABLE: TableDefinition<&[u8; 32], u32> = TableDefinition::new("index_table");

pub async fn run_engine(
    mut cmd_rx: mpsc::Receiver<EngineCommand>,
    ouroboros: Arc<RwLock<OuroborosDB>>,
    temp_db: Arc<Database>,
) {
    println!("⚙️ Motor Genético iniciado. Esperando comandos...");

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            EngineCommand::Mutate { opcode, payload, pubkey, signature, reply_to } => {
                let db_clone = Arc::clone(&ouroboros);
                let temp_clone = Arc::clone(&temp_db);
                let result = process_mutation(opcode, payload, pubkey, signature, db_clone, temp_clone).await;
                let _ = reply_to.send(result);
            }
            EngineCommand::Query { id, reply_to } => {
                let db_clone = Arc::clone(&ouroboros);
                let temp_clone = Arc::clone(&temp_db); // Necesitamos temp_db para buscar el índice
                let result = process_query(id, db_clone, temp_clone).await;
                let _ = reply_to.send(result);
            }
        }
    }
}

async fn process_mutation(
    opcode: Opcode,
    payload: Vec<u8>,
    pubkey_bytes: [u8; 32],
    signature_bytes: SignatureBytes,
    ouroboros: Arc<RwLock<OuroborosDB>>,
    temp_db: Arc<Database>,
) -> Result<CellId, String> {
    
    // 1. VALIDACIÓN CRIPTOGRÁFICA (Fail-Fast)
    let public_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| "Rechazado: Formato de Llave Pública inválido".to_string())?;

    let signature = Signature::from_bytes(&signature_bytes);

    if let Err(_) = public_key.verify(&payload, &signature) {
        return Err("Rechazado: Firma Ed25519 incorrecta.".to_string());
    }

    println!("🔐 Firma validada. Procediendo con el Opcode: {:?}", opcode);

    // 2. GENERAR EL IDENTIFICADOR DE LA CÉLULA (CellId)
    // En producción, esto debería ser el hash (ej. SHA256 o Blake3) del payload.
    // Por ahora, simularemos un CellId llenándolo con los primeros bytes del payload.
    let mut cell_id = [0u8; 32];
    let len = payload.len().min(32);
    cell_id[..len].copy_from_slice(&payload[..len]);

    match opcode {
        Opcode::WriteCell => {
            // A. ESCRIBIR EN OUROBOROS (Hilo bloqueante)
            // Clonamos el payload para moverlo al hilo bloqueante sin pelear con el borrow checker
            let payload_to_write = payload.clone(); 
            
            let record_index = tokio::task::spawn_blocking(move || {
                let mut db_lock = ouroboros.write().map_err(|_| "Error RwLock Ouroboros")?;
                
                // Asumiendo que tu método append recibe &[u8] y devuelve un Result<RecordIndex>
                // Si tu RecordIndex es un tuple struct (ej. RecordIndex(u32)), extraemos el u32
                let idx = db_lock.append(&payload_to_write).map_err(|e| format!("Error disco: {:?}", e))?;
                
                // Si tu RecordIndex es una struct con un campo '0', usamos idx.0. 
                // Si es solo un alias de u32, usamos idx directamente. Asumo idx.0 por tu archivo basico.rs
                Ok::<u32, String>(idx.0) 
            })
            .await
            .map_err(|_| "El hilo de disco colapsó".to_string())??;

            // B. GUARDAR EL ÍNDICE EN REDB (Rápido, en el hilo asíncrono)
            let write_txn = temp_db.begin_write().map_err(|_| "Error iniciando transacción Redb")?;
            {
                let mut table = write_txn.open_table(INDEX_TABLE).map_err(|_| "Error abriendo tabla de índices")?;
                table.insert(&cell_id, record_index).map_err(|_| "Error guardando en Redb")?;
            }
            write_txn.commit().map_err(|_| "Error en commit de Redb")?;

            println!("💾 Célula guardada. CellId: {:?} -> RecordIndex: {}", &cell_id[0..4], record_index);
            
            Ok(cell_id)
        },
        _ => Err("Opcode no válido para mutación".to_string()),
    }
}

async fn process_query(
    id: CellId,
    ouroboros: Arc<RwLock<OuroborosDB>>,
    temp_db: Arc<Database>,
) -> Result<Vec<u8>, String> {
    
    // 1. BUSCAR EN REDB EL RECORD INDEX
    let read_txn = temp_db.begin_read().map_err(|_| "Error tx read redb")?;
    let table = read_txn.open_table(INDEX_TABLE).map_err(|_| "Error tabla redb")?;
    
    // Forzamos explícitamente a que Rust sepa que extraemos un u32
    let record_index: u32 = match table.get(&id).map_err(|_| "Error leyendo de redb")? {
        Some(access) => access.value(),
        None => return Err("Célula no encontrada en el índice".to_string()),
    };

    // 2. LEER EL PAYLOAD DE OUROBOROS (Hilo bloqueante)
    let data = tokio::task::spawn_blocking(move || {
        let db_lock = ouroboros.read().map_err(|_| "Error RwLock Ouroboros")?;
        
        // Reconstruimos tu tipo RecordIndex
        let idx = RecordIndex(record_index);
        
        // Leemos la data usando tu método
        let result = db_lock.read(idx).map_err(|e| format!("Error lectura disco: {:?}", e))?;
        
        Ok::<Vec<u8>, String>(result)
    })
    .await
    .map_err(|_| "El hilo de lectura colapsó".to_string())??;

    println!("📖 Célula leída exitosamente desde Ouroboros");
    Ok(data)
}