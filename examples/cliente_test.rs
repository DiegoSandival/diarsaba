use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct MutateRequest {
    pub payload: Vec<u8>,
    pub pubkey: [u8; 32],
    pub signature: [u8; 64],
}

#[tokio::main]
async fn main() {
    println!("🧪 Iniciando Cliente de Prueba para Diarsaba...");

    // 1. Generamos una identidad criptográfica real para el cliente
    let mut csprng = OsRng;
    let client_keypair = SigningKey::generate(&mut csprng);
    let pubkey_bytes = client_keypair.verifying_key().to_bytes();

    // 2. Creamos los datos que queremos guardar en Ouroboros (ej. un pequeño archivo)
    let payload = b"Hola, Diarsaba. Este es mi primer registro en la base de datos descentralizada.".to_vec();

    // 3. Firmamos los datos con nuestra llave privada
    let signature = client_keypair.sign(&payload);

    // 4. Preparamos el paquete JSON
    let request_data = MutateRequest {
        payload,
        pubkey: pubkey_bytes,
        signature: signature.to_bytes(),
    };

    println!("🔐 Datos firmados correctamente. Enviando al Orquestador...");

    // 5. Enviamos la petición HTTP POST al servidor local
    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:80/do")
        .timeout(Duration::from_secs(3))
        .json(&request_data)
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            println!("\n📥 Respuesta del Servidor [{}]:", status);
            println!("{}", text);
        }
        Err(e) => eprintln!("\n💥 Error conectando al servidor: ¿Está encendido? {}", e),
    }
}