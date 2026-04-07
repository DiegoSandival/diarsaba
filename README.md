# Diarsaba

Diarsaba es un servidor Rust que expone un protocolo binario propio sobre HTTP y WebSocket. Su función es recibir frames binarios, enrutar cada solicitud por dominio y delegarla a dos backends externos opcionales:

- `correspondence` para operaciones de base de datos/celdas.
- `synap2p` para operaciones de red P2P y recepción de eventos asíncronos.

El proyecto está pensado como una capa de transporte y multiplexación. No implementa en detalle la lógica del motor de datos ni del stack P2P; esos comportamientos quedan detrás de dos traits (`CellEngineLike` y `NodeClientLike`) y de sus implementaciones reales cuando se compila con features.

## Estado actual

El servidor compila en dos modos de ejecución:

- Modo completo: requiere compilar con `--features correspondence,synap2p`.
- Modo stub: si falta cualquiera de esos dos features, el binario arranca con implementaciones `NoopDb` y `NoopNet` que responden errores de configuración.

Esto implica una consecuencia importante:

- Aunque los módulos individuales soportan compilación condicional por feature, el binario principal solo ofrece comportamiento funcional completo cuando ambos features están activos al mismo tiempo.

## Arquitectura

### Componentes principales

- `src/main.rs`: punto de entrada, lectura de variables de entorno, construcción del estado compartido y arranque del servidor.
- `src/server.rs`: servidor Axum, rutas HTTP/WebSocket, despacho de frames y difusión de eventos de red hacia clientes WebSocket.
- `src/protocol.rs`: definición del header binario, dominios, opcodes y flags del protocolo Diarsaba.
- `src/db_handler.rs`: adaptación del dominio DB hacia un motor compatible con `CellEngineLike`.
- `src/net_handler.rs`: adaptación del dominio NET hacia un cliente compatible con `NodeClientLike` y empaquetado de eventos push.
- `src/lib.rs`: exporta los módulos públicos.

### Flujo de ejecución

1. `main` decide si levanta el modo completo o el modo stub según los features activos.
2. En modo completo:
   - lee configuración desde variables de entorno;
   - inicializa `CellEngine` usando rutas de base circular y KV;
   - inicializa `NodeClient` y recibe un canal de eventos de red;
   - crea un `broadcast::Sender<Vec<u8>>` para replicar eventos a sesiones WebSocket;
   - lanza una tarea Tokio con `forward_network_events` para transformar eventos P2P en frames del dominio `EVENT`.
3. `serve` expone tres rutas:
   - `GET /` sirve `index.html` embebido con `include_str!`.
   - `POST /do` recibe un frame binario y devuelve otro frame binario.
   - `GET /ws` establece un WebSocket binario bidireccional.
4. Cada frame entrante se parsea con `parse_header`, se valida el tamaño de payload y se despacha por dominio:
   - `DOMAIN_DB` -> `handle_db_request`
   - `DOMAIN_NET` -> `handle_net_request`
   - `DOMAIN_AUTH` -> respuesta exitosa fija con el texto `auth not implemented`
   - cualquier otro dominio -> error

### Estado compartido

`AppState<D, N>` contiene:

- `db: Arc<D>`: backend de datos.
- `net: Arc<N>`: backend P2P.
- `event_tx: broadcast::Sender<Vec<u8>>`: canal fan-out para enviar eventos push a todas las sesiones WebSocket suscritas.

El diseño es genérico para facilitar pruebas y desacoplar la lógica del protocolo de implementaciones concretas.

## Transporte expuesto

### `POST /do`

Interfaz request/response binaria clásica:

- request body: un frame Diarsaba completo.
- response body: un frame Diarsaba completo.
- content type de respuesta: `application/octet-stream`.

Útil para clientes sin conexión persistente o para operaciones puntuales.

### `GET /ws`

WebSocket orientado a frames binarios:

- si el cliente envía `Message::Binary`, el servidor responde con el frame resultante del enrutamiento.
- si el cliente envía `Ping`, el servidor responde `Pong`.
- además, el servidor puede empujar mensajes binarios espontáneos con eventos del dominio `EVENT`.

El canal WebSocket es la única vía para recibir eventos asíncronos provenientes de la capa P2P.

## Compilación y ejecución

### Requisitos

- Rust toolchain con Cargo.
- Acceso a los repositorios Git de dependencias opcionales si se desea el modo completo.

### Modo stub

Levanta el servidor sin backends reales:

```bash
cargo run
```

Comportamiento esperado:

- el servidor escucha en `127.0.0.1:3000`;
- las operaciones DB/NET devuelven errores del tipo `... not configured`;
- sirve para validar transporte, parseo de frames e integración básica de clientes.

### Modo completo

```bash
cargo run --features correspondence,synap2p
```

En este modo el binario intenta:

- abrir o crear la base circular y KV;
- arrancar el nodo P2P;
- reenviar eventos P2P a clientes WebSocket.

### Validacion desde Windows con workspace en WSL

Si el proyecto se abre en VS Code desde Windows pero los archivos viven bajo `\\wsl.localhost\...`, ejecutar `cargo test` directamente en PowerShell puede fallar por el linker `x86_64-w64-mingw32-gcc` al intentar generar `.exe` dentro de una ruta UNC.

La forma correcta de validar este repositorio en ese escenario es ejecutar Cargo dentro de WSL:

```powershell
wsl.exe --cd /home/starnet/Code/diarsaba --exec /home/starnet/.cargo/bin/cargo test
```

Para el build completo con features:

```powershell
wsl.exe --cd /home/starnet/Code/diarsaba --exec /home/starnet/.cargo/bin/cargo test --features correspondence,synap2p
```

Si se prefiere no usar la ruta absoluta de Cargo, basta con asegurar que `~/.cargo/bin` este en el `PATH` de la sesion no interactiva de WSL.

## Configuración por entorno

El archivo `.env.example` documenta las variables previstas por el proyecto.

### Variables de Diarsaba

| Variable | Significado | Default |
| --- | --- | --- |
| `DIARSABA_BIND_ADDR` | Dirección de bind del servidor HTTP/WebSocket | `127.0.0.1:3000` |
| `DIARSABA_DB_PATH` | Ruta del archivo de base circular | `./diarsaba.circular.db` |
| `DIARSABA_KV_PATH` | Ruta del archivo de base key-value | `./diarsaba.kv.db` |
| `DIARSABA_P2P_LISTEN_PORT` | Puerto de escucha del nodo P2P | sin override sobre el default de `NodeConfig` salvo que se defina |
| `DIARSABA_P2P_IDENTITY_PATH` | Ruta del archivo de identidad del peer | sin override salvo que se defina |
| `DIARSABA_P2P_COMMAND_CHANNEL_SIZE` | Tamaño del canal de comandos P2P | sin override salvo que se defina |
| `DIARSABA_P2P_EVENT_CHANNEL_SIZE` | Tamaño del canal de eventos P2P | sin override salvo que se defina |

### Variables externas presentes en `.env.example`

| Variable | Observación |
| --- | --- |
| `GENESIS_SECRET` | No se usa directamente en este repositorio. Probablemente la consume alguna dependencia o tooling externo. |
| `OUROBOROS_DATA_SIZE` | No se lee desde `main.rs`; parece relevante para la creación de la base circular en la dependencia de datos. |
| `OUROBOROS_MAX_RECORDS` | Igual que la anterior: no hay lectura directa en este crate. |

### Validación de tipos

Las variables numéricas se parsean con `parse_env_number<T>`. Si el valor es inválido, el proceso falla al arranque con `io::ErrorKind::InvalidInput`.

## Protocolo Diarsaba

La especificación detallada está en [docs/protocolo-binario.md](docs/protocolo-binario.md).

Resumen rápido:

- header fijo de 8 bytes;
- `req_id` codificado en little-endian;
- `payload_len` codificado en big-endian;
- multiplexación por dominio (`DB`, `NET`, `AUTH`, `EVENT`);
- bit `FLAG_ERROR` para marcar errores de aplicación.

## Dominio DB

El dominio DB delega toda la operación al motor `CellEngineLike::ejecutar`.

### Forma del payload de request

El payload mínimo tiene 37 bytes:

- `opcode`: 1 byte.
- `target_index`: 4 bytes little-endian.
- `solucion`: 32 bytes.
- `params`: resto del payload.

Expresado como fórmula:

$$
payload_{db} = opcode(1) + target\_index(4) + solucion(32) + params(n)
$$

El servidor no interpreta semánticamente el opcode DB. Solo extrae campos y los pasa al backend.

### Respuesta

- éxito: dominio `DOMAIN_DB`, `flags = 0`, payload con bytes devueltos por el motor.
- error: dominio `DOMAIN_DB`, `flags = FLAG_ERROR`, payload UTF-8 con el mensaje de error.

### Opcodes declarados

Los siguientes opcodes están definidos en `protocol.rs`:

- `DB_READ = 0x01`
- `DB_WRITE = 0x02`
- `DB_DELETE = 0x03`
- `DB_DERIVE = 0x04`
- `DB_MERGE = 0x05`
- `DB_INSPECT = 0x06`

Importante:

- Diarsaba no diferencia estos opcodes a nivel de router. La semántica real depende de `correspondence::CellEngine`.

## Dominio NET

El dominio NET implementa una gramática propia de payloads y sí distingue opcodes en el servidor.

### Operaciones soportadas

| Opcode | Constante | Estado |
| --- | --- | --- |
| `0x21` | `NET_CONNECT` | implementado |
| `0x22` | `NET_DIRECT_MSG` | implementado |
| `0x23` | `NET_SUBSCRIBE` | implementado |
| `0x24` | `NET_PUBLISH` | implementado |
| `0x25` | `NET_ANNOUNCE` | implementado |
| `0x26` | `NET_FIND` | implementado |

### Convención de campo length-prefixed

Varias operaciones usan un campo inicial con esta forma:

- `field_len`: 1 byte.
- `field`: `field_len` bytes.
- `tail`: resto del payload.

#### `NET_DIRECT_MSG`

Payload:

- `opcode = 0x22`
- `peer_len`
- `peer_bytes`
- `data`

Acción:

- llama a `send_direct_message(peer, data)`.

Respuesta:

- éxito con payload vacío;
- error con mensaje textual.

#### `NET_CONNECT`

Payload:

- `opcode = 0x21`
- `peer_len`
- `peer_bytes`
- `addr_len`
- `addr_bytes`

No admite bytes sobrantes después de `addr_bytes`.

Acción:

- llama a `connect_to_node(peer, addr)`.

#### `NET_PUBLISH`

Payload:

- `opcode = 0x24`
- `topic_len`
- `topic_bytes`
- `data`

Acción:

- llama a `publish_message(topic, data)`.

#### `NET_SUBSCRIBE`

Payload:

- `opcode = 0x23`
- `topic_len`
- `topic_bytes`

No admite bytes sobrantes después de `topic_bytes`.

Acción:

- llama a `subscribe(topic)`.

#### `NET_ANNOUNCE`

Payload:

- `opcode = 0x25`
- `key_len`
- `key_bytes`

No admite bytes sobrantes después de `key_bytes`.

Acción:

- llama a `announce_provider(key)`.

#### `NET_FIND`

Payload:

- `opcode = 0x26`
- `key_len`
- `key_bytes`

No admite bytes sobrantes después de `key_bytes`.

Acción:

- llama a `find_providers(key)`.

Respuesta exitosa:

- lista codificada como secuencia repetida de `peer_len + peer_bytes`.

## Dominio AUTH

El dominio AUTH está reservado en la especificación, pero no tiene implementación real.

Comportamiento actual:

- cualquier request con `DOMAIN_AUTH` devuelve respuesta exitosa con el payload textual `auth not implemented`.

Constantes declaradas pero no utilizadas actualmente:

- `AUTH_GEN_KEYPAIR = 0x31`
- `AUTH_SIGN = 0x32`
- `AUTH_VERIFY = 0x33`

## Dominio EVENT

El dominio `EVENT` no se usa para solicitudes del cliente; se usa para mensajes push emitidos por el servidor hacia WebSocket.

Fuente de eventos:

- eventos recibidos desde `synap2p`;
- convertidos a frames por `pack_network_event`;
- enviados a todas las sesiones activas mediante `broadcast`.

Tipos de eventos soportados:

- `EVT_DIRECT_MSG = 0x01`
- `EVT_GOSSIP_MSG = 0x02`
- `EVT_PROVIDER_FOUND = 0x03`

Detalles exactos de serialización en [docs/protocolo-binario.md](docs/protocolo-binario.md).

## Manejo de errores

Hay dos niveles de error relevantes.

### Error de parseo o enrutamiento

Se construye un frame con:

- mismo dominio cuando es posible;
- `req_id` extraído del input si existe;
- `FLAG_ERROR` activo;
- payload textual con la descripción.

Ejemplos:

- `header requires at least 8 bytes`
- `frame payload shorter than declared length`
- `unknown diarsaba domain`

### Error del backend DB o NET

El error se serializa como texto en el payload del frame de respuesta con `FLAG_ERROR`.

### Límite de tamaño

Toda respuesta está limitada por `u16` para `payload_len`, por lo que el máximo teórico del payload es `65535` bytes.

Si una respuesta excede ese tamaño:

- se sustituye por un frame de error con el texto `response payload exceeds u16 length`.

## Pruebas y validación

El proyecto contiene pruebas unitarias al menos en:

- `protocol.rs` para serialización/deserialización del header;
- `server.rs` y `net_handler.rs` para routing y empaquetado de eventos.

Ejecución sugerida:

```bash
cargo test
```

Y, para el modo completo:

```bash
cargo test --features correspondence,synap2p
```

La validez de esta segunda variante depende de que las dependencias Git externas estén disponibles y sigan siendo compatibles.

## Limitaciones actuales

- `AUTH` está reservado pero no implementado.
- El binario en modo parcial no habilita solo una mitad funcional; si falta uno de los dos features principales, ambos backends quedan en stub.
- No hay documentación embebida tipo Rustdoc en los módulos principales.

## Recomendaciones de evolución

- Añadir una especificación formal versionada del protocolo para clientes externos.
- Definir semántica pública estable para los opcodes DB, hoy delegados por completo al backend.
- Implementar autenticación o eliminar temporalmente el dominio `AUTH` de la superficie pública si aún no se usará.
- Incorporar un ejemplo de cliente CLI o script de prueba que construya frames binarios reales.
