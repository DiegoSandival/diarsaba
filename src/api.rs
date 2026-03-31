use axum::{
    extract::{ State, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
    Json,
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
        .route("/*file_path", get(handle_file_request))
        .with_state(state)
}

#[derive(Deserialize)]
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

/// Endpoint de mutación: Solo accesible desde API o localhost.
async fn handle_do(
    Host(hostname): Host,
    State(state): State<AppState>,
    Json(req): Json<MutateRequest>
    // payload: Json<TuEstructuraRequest> -> Aquí extraes el body de la petición
) -> Response {
    // 1. FILTRO DE DOMINIO ESTRICTO
    if !is_api_domain(&hostname) {
        return (StatusCode::NOT_FOUND, "Endpoint exclusivo de la API").into_response();
    }

    // 2. CREAR EL CANAL DE RESPUESTA
    // oneshot permite que el Motor Genético nos responda a este handler específico.
    let (reply_tx, reply_rx) = oneshot::channel();

    // 3. EMPAQUETAR EL COMANDO
    // (En la realidad, extraerías el opcode, payload y firma del body del request)
    let cmd = EngineCommand::Mutate {
        opcode: Opcode::WriteCell, 
        payload: vec![1, 2, 3], // Simulación del body
        pubkey: [1u8; 32],
        signature: [0u8; 64],   // Simulación de la firma
        reply_to: reply_tx,
    };

    // 4. ENVIAR AL MOTOR GENÉTICO
    if state.engine_tx.send(cmd).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "El Motor Genético está desconectado").into_response();
    }

    // 5. ESPERAR RESULTADO SIN BLOQUEAR EL HILO
    // Mientras esperamos, Tokio usa este hilo de CPU para atender a otros usuarios web.
    match reply_rx.await {
        Ok(Ok(cell_id)) => {
            (StatusCode::OK, format!("✅ Mutación exitosa. CellID generado.")).into_response()
        }
        Ok(Err(e)) => {
            // El motor rechazó la operación (ej. firma inválida)
            (StatusCode::BAD_REQUEST, format!("❌ Mutación rechazada: {}", e)).into_response()
        }
        Err(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "El motor colapsó procesando la petición").into_response()
        }
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

// Stubs (Puedes implementarlos según tus necesidades)
async fn handle_sign(Host(hostname): Host) -> impl IntoResponse { /* ... */ }
async fn handle_verify(Host(hostname): Host) -> impl IntoResponse { /* ... */ }


// --- UTILIDADES DE ENRUTAMIENTO ---

fn is_api_domain(hostname: &str) -> bool {
    hostname.starts_with("api.diarsaba.com") || hostname.starts_with("localhost") || hostname.starts_with("127.0.0.1")
}

fn is_hosting_domain(hostname: &str) -> bool {
    hostname.starts_with("diarsaba.com") && !hostname.starts_with("api.")
}