use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🧪 Iniciando Cliente de Prueba (Bytes Crudos)...");

    // 1. Identidad
    let mut csprng = OsRng;
    let client_keypair = SigningKey::generate(&mut csprng);
    let pubkey_bytes = client_keypair.verifying_key().to_bytes();

    // 2. Datos y Firma
    let payload = b"Hola, Diarsaba. Este es mi primer registro usando BYTES CRUDOS puros.".to_vec();
    let signature = client_keypair.sign(&payload).to_bytes();

    // 3. CONSTRUIR EL PAQUETE BINARIO [Pubkey(32) | Firma(64) | Payload(N)]
    let mut paquete_binario = Vec::new();
    paquete_binario.extend_from_slice(&pubkey_bytes);
    paquete_binario.extend_from_slice(&signature);
    paquete_binario.extend_from_slice(&payload);

    println!("🔐 Paquete binario construido ({} bytes). Enviando al Orquestador...", paquete_binario.len());

    // 4. Enviar mediante POST
    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:8080/do") 
        .timeout(Duration::from_secs(3))
        .body(paquete_binario) // 👈 Aquí inyectamos los bytes crudos directamente
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            println!("\n📥 Respuesta del Servidor [{}]:", status);
            println!("{}", text);
        }
        Err(e) => eprintln!("\n💥 Error conectando al servidor: {}", e),
    }
}