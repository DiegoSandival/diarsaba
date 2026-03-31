use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey}; 
use redb::{Database, TableDefinition, ReadableTable, ReadableDatabase};
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};
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
    server_keypair: Arc<ed25519_dalek::SigningKey>,
) {
    println!("⚙️ Motor Genético iniciado. Esperando comandos...");

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
          EngineCommand::Mutate { ticket, llave, viejo_secreto, nuevo_secreto, payload, reply_to } => {
                let db_clone = Arc::clone(&ouroboros);
                let temp_clone = Arc::clone(&temp_db);
                
                // Pasamos las variables correctas a process_mutation
                let result = process_mutation(
                    ticket, 
                    llave, 
                    viejo_secreto, 
                    nuevo_secreto, 
                    payload, 
                    db_clone, 
                    temp_clone, 
                    &server_keypair
                ).await;
                
                let _ = reply_to.send(result);
            }
            EngineCommand::Query { id, reply_to } => {
                let db_clone = Arc::clone(&ouroboros);
                let temp_clone = Arc::clone(&temp_db); // Necesitamos temp_db para buscar el índice
                let result = process_query(id, db_clone, temp_clone).await;
                let _ = reply_to.send(result);
            }
            EngineCommand::Sign { client_pubkey, client_signature, arbitrary_data, reply_to } => {
                // Pasamos la llave del servidor (deberás inyectarla en run_engine o tenerla accesible)
                // Para esto, en main.rs y types.rs asegúrate de pasar Arc<SigningKey> a run_engine
                // Simularemos la función process_sign por ahora:
                let result = process_sign(client_pubkey, client_signature, arbitrary_data, &server_keypair).await;
                let _ = reply_to.send(result);
            }
        }
    }
}
// 2. Actualiza la función process_mutation
async fn process_mutation(
    ticket: Vec<u8>,
    llave: [u8; 32], // 👈 Array puro
    viejo_secreto: [u8; 32],
    _nuevo_secreto: [u8; 32], 
    payload: Vec<u8>,
    ouroboros: Arc<RwLock<OuroborosDB>>,
    temp_db: Arc<Database>,
    server_keypair: &ed25519_dalek::SigningKey,
) -> Result<u32, String> {
    
    // ==========================================
    // PASO 5.1: VALIDAR EL TICKET DE ORO
    // ==========================================
    if ticket.len() < 136 { return Err("Ticket malformado".to_string()); }

    let firma_srv_bytes = &ticket[0..64];
    let contenido_ticket = &ticket[64..];

    let server_pubkey = server_keypair.verifying_key();
    let signature = ed25519_dalek::Signature::from_bytes(firma_srv_bytes.try_into().unwrap());
    
    if server_pubkey.verify(contenido_ticket, &signature).is_err() {
        return Err("Ticket falsificado o inválido".to_string());
    }

    let mut exp_bytes = [0u8; 8];
    exp_bytes.copy_from_slice(&ticket[96..104]);
    let exp_timestamp = u64::from_le_bytes(exp_bytes);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    
    if now > exp_timestamp { return Err("El Ticket ha expirado".to_string()); }

    println!("🎟️ Ticket válido. Procesando mutación para llave {:?}", &llave[0..4]);

    // ==========================================
    // PASOS 5.2 y 5.3: LOGICA REDB + OUROBOROS
    // ==========================================
    let read_txn = temp_db.begin_read().map_err(|_| "Error tx read")?;
    let table_result = read_txn.open_table(INDEX_TABLE);
    
    let index_existente = if let Ok(table) = table_result {
        table.get(&llave).map_err(|_| "Error DB")?.map(|a| a.value()) // 👈 Pasamos la llave directamente
    } else {
        None
    };

    if let Some(_idx) = index_existente {
        println!("🔍 Llave existente encontrada en Index {}. Validando secreto...", _idx);
        // Simulamos validación de secretos con Ouroboros
    } else {
        println!("✨ Nueva llave. Creando célula inicial...");
    }

    // FINAL: Escribir en Ouroboros
    let mut payload_to_write = payload.clone();
    if payload_to_write.len() < 96 {
        payload_to_write.resize(96, 0); 
    }

    let record_index = tokio::task::spawn_blocking(move || {
        let mut db_lock = ouroboros.write().map_err(|_| "Error RwLock Ouroboros")?;
        db_lock.append(&payload_to_write).map_err(|e| format!("Error disco: {:?}", e))?;
        Ok::<u32, String>(1) 
    })
    .await
    .map_err(|_| "El hilo de disco colapsó".to_string())??;

    // Actualizamos redb
    let write_txn = temp_db.begin_write().map_err(|_| "Error Redb TX")?;
    {
        let mut table = write_txn.open_table(INDEX_TABLE).map_err(|_| "Error Redb Table")?;
        table.insert(&llave, record_index).map_err(|_| "Error Redb Insert")?; // 👈 Pasamos la referencia
    }
    write_txn.commit().map_err(|_| "Error Redb Commit")?;

    Ok(record_index)
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

async fn process_sign(
    pubkey_bytes: [u8; 32],
    signature_bytes: [u8; 64],
    arbitrary_data: Vec<u8>,
    server_keypair: &ed25519_dalek::SigningKey,
) -> Result<Vec<u8>, String> {
    
    // 1. Verificar la firma del cliente (Paso 3)
    let public_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| "Llave Pública inválida".to_string())?;
    let signature = Signature::from_bytes(&signature_bytes);

    // El cliente tuvo que firmar el arbitrary_data con su llave privada
    if public_key.verify(&arbitrary_data, &signature).is_err() {
        return Err("Firma del cliente inválida".to_string());
    }

    // 2. Construir el Certificado (Paso 3.1)
    let mut cert_bytes = Vec::new();

    // 2a. Slice Random de 32 bytes
    let mut random_slice = [0u8; 32];
    OsRng.fill_bytes(&mut random_slice);
    cert_bytes.extend_from_slice(&random_slice);

    // 2b. Timestamp de expiración (ej. +24 horas)
    let exp_timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 86400;
    cert_bytes.extend_from_slice(&exp_timestamp.to_le_bytes()); // 8 bytes

    // 2c. Llave pública del cliente
    cert_bytes.extend_from_slice(&pubkey_bytes); // 32 bytes

    // 2d. Dato arbitrario
    cert_bytes.extend_from_slice(&arbitrary_data); // N bytes

    // 3. El Servidor Firma el Certificado
    use ed25519_dalek::Signer;
    let firma_servidor = server_keypair.sign(&cert_bytes);

    // 4. Empaquetar y enviar respuesta: [FirmaServidor(64) | Certificado(N)]
    let mut respuesta = Vec::with_capacity(64 + cert_bytes.len());
    respuesta.extend_from_slice(&firma_servidor.to_bytes());
    respuesta.extend_from_slice(&cert_bytes);

    println!("📜 Nuevo certificado emitido para cliente. Tamaño: {} bytes", respuesta.len());
    Ok(respuesta)
}