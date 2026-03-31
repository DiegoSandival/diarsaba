use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🧪 Iniciando Prueba de Delegación P2P (Fábrica de Identidad)...");

    // ==========================================
    // PASO 1: El cliente crea sus claves locales
    // ==========================================
    let mut csprng = OsRng;
    let client_keypair = SigningKey::generate(&mut csprng);
    let pubkey_bytes = client_keypair.verifying_key().to_bytes();
    println!("🔑 Claves de cliente generadas.");

    // ==========================================
    // PASO 2: Cliente elige un dato arbitrario y lo firma
    // ==========================================
    // Esto puede ser un string, el hash de una imagen, un JSON, etc.
    let arbitrary_data = b"usuario_diarsaba_007".to_vec();
    
    // El cliente demuestra que es dueño de la llave pública firmando su propio dato
    let client_signature = client_keypair.sign(&arbitrary_data).to_bytes();

    // Armamos el paquete crudo: [Firma(64) | Pubkey(32) | Dato(N)]
    let mut paquete_peticion = Vec::new();
    paquete_peticion.extend_from_slice(&client_signature);
    paquete_peticion.extend_from_slice(&pubkey_bytes);
    paquete_peticion.extend_from_slice(&arbitrary_data);

    println!("📤 Enviando petición de firma al servidor ({} bytes)...", paquete_peticion.len());

    // ==========================================
    // PASO 3: Enviar petición al endpoint /sign
    // ==========================================
    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:8080/sign") // Asegúrate de que el puerto coincida con main.rs
        .timeout(Duration::from_secs(3))
        .body(paquete_peticion)
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                // Leemos los bytes crudos de la respuesta
                let certificado_bytes = response.bytes().await.unwrap_or_default();
                
                println!("\n📥 ¡ÉXITO! Certificado emitido por el servidor.");
                println!("📜 Tamaño total del Certificado: {} bytes", certificado_bytes.len());
                
                // Desglosamos el certificado para comprobar que el servidor hizo bien su trabajo:
                // Estructura: [FirmaServidor(64) | Random(32) | ExpTimestamp(8) | Pubkey(32) | DatoArbitrario(N)]
                if certificado_bytes.len() >= 136 { // 64+32+8+32 = 136 bytes mínimos
                    let firma_srv = &certificado_bytes[0..64];
                    let random_slice = &certificado_bytes[64..96];
                    
                    let mut exp_bytes = [0u8; 8];
                    exp_bytes.copy_from_slice(&certificado_bytes[96..104]);
                    let exp_timestamp = u64::from_le_bytes(exp_bytes);

                    println!("   ├─ Firma del Nodo: {:?}", &firma_srv[0..4]); // Mostramos un fragmento
                    println!("   ├─ Sal Aleatorio (Anti-Replay): {:?}", &random_slice[0..4]);
                    println!("   ├─ Expira en (UNIX): {}", exp_timestamp);
                    println!("   └─ Dato Arbitrario devuelto: {:?}", String::from_utf8_lossy(&certificado_bytes[136..]));
                    
                    println!("\n✅ Este paquete de bytes es tu 'Ticket de Oro'. Se usará en el Paso 4 para mutar.");
                }

            } else {
                let text = response.text().await.unwrap_or_default();
                println!("\n❌ El servidor rechazó la petición [{}]: {}", status, text);
            }
        }
        Err(e) => eprintln!("\n💥 Error conectando al servidor: {}", e),
    }
}