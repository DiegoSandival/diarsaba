use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🧪 Iniciando Prueba Maestra P2P (Identidad + Mutación)...");

    let client = reqwest::Client::new();
    let mut csprng = OsRng;
    
    // ==========================================
    // ETAPA A: CREACIÓN DE IDENTIDAD Y TICKET
    // ==========================================
    let client_keypair = SigningKey::generate(&mut csprng);
    let pubkey_bytes = client_keypair.verifying_key().to_bytes();
    
    let arbitrary_data = b"usuario_diarsaba_007".to_vec();
    let client_signature = client_keypair.sign(&arbitrary_data).to_bytes();

    let mut paquete_sign = Vec::new();
    paquete_sign.extend_from_slice(&client_signature);
    paquete_sign.extend_from_slice(&pubkey_bytes);
    paquete_sign.extend_from_slice(&arbitrary_data);

    println!("\n[1/4] 📤 Pidiendo Ticket de Oro al servidor (/sign)...");
    let res_sign = client
        .post("http://127.0.0.1:8080/sign")
        .timeout(Duration::from_secs(3))
        .body(paquete_sign)
        .send()
        .await
        .expect("Error al conectar con /sign");

    if !res_sign.status().is_success() {
        panic!("❌ El servidor rechazó la firma: {}", res_sign.text().await.unwrap());
    }

    let ticket = res_sign.bytes().await.unwrap().to_vec();
    println!("[2/4] 📥 Ticket recibido. Tamaño: {} bytes.", ticket.len());

    // ==========================================
    // ETAPA B: PREPARANDO LA MUTACIÓN EN BYTES
    // ==========================================
    // Estructura: [TamañoTicket(2) | Ticket(N) | Llave(32) | ViejoSec(32) | NuevoSec(32) | Payload(X)]
    
    let mut paquete_mutacion = Vec::new();

    // 1. Tamaño del Ticket (2 bytes, Little Endian)
    let ticket_len = ticket.len() as u16;
    paquete_mutacion.extend_from_slice(&ticket_len.to_le_bytes());

    // 2. El Ticket devuelto por el servidor
    paquete_mutacion.extend_from_slice(&ticket);

    // 3. La Llave (Estrictamente 32 bytes). Rellenamos "hola" con ceros al final.
    let mut llave = [0u8; 32];
    let llave_str = b"hola";
    llave[..llave_str.len()].copy_from_slice(llave_str);
    paquete_mutacion.extend_from_slice(&llave);

    // 4. Viejo Secreto (32 bytes) - Simularemos que usamos Blake2b o ceros
    let viejo_secreto = [1u8; 32]; // Puro 1s para la prueba
    paquete_mutacion.extend_from_slice(&viejo_secreto);

    // 5. Nuevo Secreto (32 bytes) - El candado para el futuro
    let nuevo_secreto = [2u8; 32]; // Puro 2s para la prueba
    paquete_mutacion.extend_from_slice(&nuevo_secreto);

    // 6. El Payload
    let payload = b"hola mundo".to_vec();
    paquete_mutacion.extend_from_slice(&payload);

    println!("[3/4] 🧳 Paquete de mutación construido. Tamaño total: {} bytes.", paquete_mutacion.len());
    println!("      ├─ Llave: {:?}", std::str::from_utf8(llave_str).unwrap());
    println!("      └─ Payload: {:?}", std::str::from_utf8(&payload).unwrap());

    // ==========================================
    // ETAPA C: DISPARAR LA MUTACIÓN
    // ==========================================
    println!("\n[4/4] 🚀 Disparando datos a /do...");
    let res_do = client
        .post("http://127.0.0.1:8080/do")
        .timeout(Duration::from_secs(3))
        .body(paquete_mutacion)
        .send()
        .await
        .expect("Error al conectar con /do");

    let status = res_do.status();
    let texto = res_do.text().await.unwrap_or_default();

    if status.is_success() {
        println!("\n🎉 ¡ÉXITO TOTAL! Respuesta del servidor:");
        println!("   {}", texto);
    } else {
        println!("\n💥 Fallo en la mutación [{}]:", status);
        println!("   {}", texto);
    }
}