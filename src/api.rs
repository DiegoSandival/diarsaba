use axum::{
    extract::{ State, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
    body::Bytes,
};

use axum_extra::extract::Host;
use tokio::sync::oneshot;

use crate::types::{AppState, EngineCommand};
use crate::opcodes::Opcode;

/// Construye el enrutador principal y le inyecta el estado global.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Rutas raíz (Dashboard o Index del hosting)
        .route("/", get(handle_root))
        // Rutas de API / Mutación
        .route("/do", post(handle_do))
        .route("/sign", post(handle_sign))
        .route("/verify", post(handle_verify))
        // Ruta comodín para archivos del hosting descentralizado (ej. /css/style.css)
       .route("/{*file_path}", get(handle_file_request))
        .with_state(state)
}

pub struct MutateRequest {
    pub payload: Vec<u8>,
    pub pubkey: [u8; 32],
    pub signature: [u8; 64],
}
// --- HANDLERS ---

/// Maneja la raíz "/" dependiendo del dominio que haga la petición.
async fn handle_root(Host(hostname): Host) -> impl IntoResponse {
    if is_api_domain(&hostname) {
        // Aquí podrías devolver el HTML de tu interfaz web administrativa
        (StatusCode::OK, "🖥️ Bienvenido a la Interfaz Web de diarsaba (API)").into_response()
    } else if is_hosting_domain(&hostname) {
        // Aquí podrías disparar una consulta a core.rs para pedir el "index.html" de la DB
        (StatusCode::OK, "🌐 Bienvenido al Hosting Descentralizado diarsaba").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Dominio no reconocido por el nodo").into_response()
    }
}


async fn handle_do(
    State(state): State<AppState>,
    body_bytes: Bytes, 
) -> Response {
    
    // Validación mínima: 2(len) + 136(ticket min) + 32(llave) + 32(v_sec) + 32(n_sec) + 1(payload) = 235 bytes
    if body_bytes.len() < 235 {
        return (StatusCode::BAD_REQUEST, "Paquete /do demasiado corto").into_response();
    }

    let mut offset = 0;

    // 1. Extraer Ticket
    let mut ticket_len_bytes = [0u8; 2];
    ticket_len_bytes.copy_from_slice(&body_bytes[offset..offset+2]);
    let ticket_len = u16::from_le_bytes(ticket_len_bytes) as usize;
    offset += 2;
    
    if body_bytes.len() < offset + ticket_len { return (StatusCode::BAD_REQUEST, "Ticket truncado").into_response(); }
    let ticket = body_bytes[offset..offset+ticket_len].to_vec();
    offset += ticket_len;

    // 2. Extraer Llave Fija (32 bytes)
    let mut llave = [0u8; 32];
    llave.copy_from_slice(&body_bytes[offset..offset+32]);
    offset += 32;

    // 3. Extraer Secretos (32 bytes cada uno)
    let mut viejo_secreto = [0u8; 32];
    viejo_secreto.copy_from_slice(&body_bytes[offset..offset+32]);
    offset += 32;

    let mut nuevo_secreto = [0u8; 32];
    nuevo_secreto.copy_from_slice(&body_bytes[offset..offset+32]);
    offset += 32;

    // 4. El resto es el Payload
    let payload = body_bytes[offset..].to_vec();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    
    let cmd = EngineCommand::Mutate {
        ticket,
        llave,
        viejo_secreto,
        nuevo_secreto,
        payload,
        reply_to: reply_tx,
    };

    let _ = state.engine_tx.send(cmd).await;

    match reply_rx.await {
        Ok(Ok(record_index)) => (StatusCode::OK, format!("✅ Mutación exitosa. Index: {}", record_index)).into_response(),
        Ok(Err(e)) => (StatusCode::UNAUTHORIZED, format!("❌ Rechazado: {}", e)).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Motor colapsó").into_response(),
    }
}

/// Sirve archivos estáticos para el hosting descentralizado.
async fn handle_file_request(
    Host(hostname): Host,
    State(state): State<AppState>,
    Path(file_path): Path<String>,
) -> Response {
    if !is_hosting_domain(&hostname) {
        return (StatusCode::NOT_FOUND, "No encontrado").into_response();
    }

    // Lógica asíncrona similar a handle_do, pero usando EngineCommand::Query
    // ...
    (StatusCode::OK, format!("Sirviendo archivo '{}' desde Ouroboros", file_path)).into_response()
}



// Endpoint POST /sign
async fn handle_sign(
    State(state): State<AppState>,
    body_bytes: axum::body::Bytes, 
) -> axum::response::Response {
    
    // El cliente manda: [FirmaCliente(64) | Pubkey(32) | DatosArbitrarios(N)]
    if body_bytes.len() < 96 {
        return (StatusCode::BAD_REQUEST, "Paquete /sign demasiado corto").into_response();
    }

    let mut client_signature = [0u8; 64];
    client_signature.copy_from_slice(&body_bytes[0..64]);

    let mut client_pubkey = [0u8; 32];
    client_pubkey.copy_from_slice(&body_bytes[64..96]);

    let arbitrary_data = body_bytes[96..].to_vec();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    
    let cmd = EngineCommand::Sign {
        client_pubkey,
        client_signature,
        arbitrary_data,
        reply_to: reply_tx,
    };

    if state.engine_tx.send(cmd).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Motor desconectado").into_response();
    }

    match reply_rx.await {
        Ok(Ok(certificado_binario)) => {
            // Devolvemos los bytes puros al cliente
            (StatusCode::OK, certificado_binario).into_response()
        }
        Ok(Err(e)) => (StatusCode::UNAUTHORIZED, e).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Fallo en motor").into_response(),
    }
}
async fn handle_verify(Host(hostname): Host) -> impl IntoResponse { /* ... */ }

// --- UTILIDADES DE ENRUTAMIENTO ---

fn is_api_domain(hostname: &str) -> bool {
    hostname.starts_with("api.diarsaba.com") || hostname.starts_with("localhost") || hostname.starts_with("127.0.0.1")
}

fn is_hosting_domain(hostname: &str) -> bool {
    hostname.starts_with("diarsaba.com") && !hostname.starts_with("api.")
}
