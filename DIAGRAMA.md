# Diagrama de Flujo — Diarsaba Orchestrator

```mermaid
flowchart TD
    START([🚀 Inicio: main\(\)]) --> CANALES

    subgraph INIT["⚙️ Inicialización del Sistema"]
        CANALES["Crear canales de comunicación\nmpsc::channel → engine_tx / engine_rx\nbroadcast::channel → p2p_tx / p2p_rx"]
        CANALES --> DATABASES
        DATABASES["Levantar bases de datos\n• tempdb.redb  (ReDB — índice rápido)\n• ciclidb_produccion.db  (OuroborosDB — disco circular)"]
        DATABASES --> KEYPAIR
        KEYPAIR["Generar identidad criptográfica\nSigningKey  (Ed25519)"]
        KEYPAIR --> P2P_INIT
        P2P_INIT["Inicializar nodo QUIC\nSynap2p — suscribir p2p_event_rx"]
        P2P_INIT --> STATE
        STATE["Construir AppState\n(temp_db, server_keypair,\nengine_tx, p2p_tx)"]
    end

    STATE --> SPAWN_ENGINE
    STATE --> SPAWN_HTTP

    subgraph ACTORS["🎭 Actores concurrentes (tokio::spawn)"]
        SPAWN_ENGINE["🧬 Actor A — Motor Genético\ncore::run_engine(engine_rx, ouro_db, temp_db, keypair)"]
        SPAWN_HTTP["🌐 Actor B — Servidor HTTP\nAxum en 0.0.0.0:8080"]
    end

    SPAWN_ENGINE --> LOOP
    SPAWN_HTTP --> LOOP

    subgraph LOOP["🛡️ Bucle Maestro (tokio::select!)"]
        SEL{Evento recibido}
        SEL -->|Ctrl+C / SIGTERM| SHUTDOWN
        SEL -->|P2pEvent| P2P_EVENT
        SEL -->|Error canal| FAIL_FAST

        P2P_EVENT{Tipo de evento P2P}
        P2P_EVENT -->|IncomingCell| CELL_IN["📡 Célula entrante\nlog del peer origen\n(pendiente: reenviar al Motor)"]
        P2P_EVENT -->|PeerDiscovered| PEER_NEW["🤝 Nuevo peer conectado\nlog del peer"]

        CELL_IN --> SEL
        PEER_NEW --> SEL
    end

    FAIL_FAST["💥 Fail-Fast activado\nSalir del bucle"] --> SHUTDOWN

    subgraph SHUTDOWN_BLOCK["🧹 Graceful Shutdown"]
        SHUTDOWN["Sincronizar bases de datos\nDrop de todos los Arc\n(OuroborosDB::drop → flush a disco)"]
        SHUTDOWN --> END([💤 Sistema apagado])
    end

    %% ── Flujo HTTP ─────────────────────────────────────────────────────────────
    subgraph HTTP["🌐 Manejo de Peticiones HTTP (api.rs)"]
        REQ["Petición HTTP entrante"] --> ROUTER
        ROUTER{Ruta y dominio}

        ROUTER -->|"GET /"| ROOT{¿Dominio?}
        ROOT -->|"api.*"| ROOT_API["200 — Interfaz Admin"]
        ROOT -->|"diarsaba.com"| ROOT_HOST["200 — Hosting Descentralizado"]
        ROOT -->|Otro| ROOT_404["404 — Dominio no reconocido"]

        ROUTER -->|"POST /do"| DO_HANDLER
        ROUTER -->|"POST /sign"| SIGN_HANDLER
        ROUTER -->|"GET /{*path}"| FILE_HANDLER

        subgraph DO_FLOW["POST /do — Mutación"]
            DO_HANDLER["Parsear paquete binario\n(ticket, llave, secretos, payload)"]
            DO_HANDLER -->|"< 235 bytes"| DO_400["400 Bad Request"]
            DO_HANDLER -->|OK| DO_CMD["Crear EngineCommand::Mutate\nenviar por engine_tx"]
            DO_CMD --> DO_WAIT["Esperar respuesta (oneshot)"]
            DO_WAIT -->|OK| DO_200["200 — Index de mutación"]
            DO_WAIT -->|Error| DO_401["401 Unauthorized / 500"]
        end

        subgraph SIGN_FLOW["POST /sign — Certificación"]
            SIGN_HANDLER["Parsear paquete binario\n(firma_cliente 64B + pubkey 32B + datos)"]
            SIGN_HANDLER -->|"< 96 bytes"| SIGN_400["400 Bad Request"]
            SIGN_HANDLER -->|OK| SIGN_CMD["Crear EngineCommand::Sign\nenviar por engine_tx"]
            SIGN_CMD --> SIGN_WAIT["Esperar respuesta (oneshot)"]
            SIGN_WAIT -->|OK| SIGN_200["200 — Certificado binario"]
            SIGN_WAIT -->|Error| SIGN_401["401 / 500"]
        end

        FILE_HANDLER -->|"is_hosting_domain?"| FILE_CHECK{¿Dominio hosting?}
        FILE_CHECK -->|No| FILE_404["404"]
        FILE_CHECK -->|Sí| FILE_200["200 — Archivo desde OuroborosDB"]
    end

    %% ── Motor Genético ──────────────────────────────────────────────────────────
    subgraph ENGINE["🧬 Motor Genético (core.rs)"]
        ENG_LOOP["Esperar EngineCommand\ncmd_rx.recv()"]
        ENG_LOOP --> CMD{Tipo de comando}

        CMD -->|Mutate| MUTATE
        CMD -->|Query| QUERY
        CMD -->|Sign| SIGN

        subgraph MUTATE_FLOW["Mutate — process_mutation()"]
            MUTATE["Validar Ticket de Oro\n(firma Ed25519 + timestamp)"]
            MUTATE -->|Inválido| MUT_ERR["Err('Ticket falsificado')"]
            MUTATE -->|Expirado| MUT_EXP["Err('Ticket expirado')"]
            MUTATE -->|Válido| MUT_REDB["Buscar llave en ReDB (index_table)"]
            MUT_REDB -->|Existe| MUT_UPD["Validar secreto anterior"]
            MUT_REDB -->|Nueva| MUT_NEW["Crear célula inicial"]
            MUT_UPD --> MUT_WRITE
            MUT_NEW --> MUT_WRITE
            MUT_WRITE["Escribir payload en OuroborosDB\n(spawn_blocking)"]
            MUT_WRITE --> MUT_IDX["Actualizar índice en ReDB\n(llave → record_index)"]
            MUT_IDX --> MUT_OK["Ok(record_index)"]
        end

        subgraph QUERY_FLOW["Query — process_query()"]
            QUERY["Buscar CellId en ReDB\n→ obtener record_index"]
            QUERY -->|No encontrado| Q_ERR["Err('Célula no encontrada')"]
            QUERY -->|Encontrado| Q_READ["Leer payload de OuroborosDB\ncon RecordIndex (spawn_blocking)"]
            Q_READ --> Q_OK["Ok(Vec<u8>)"]
        end

        subgraph SIGN_FLOW_ENG["Sign — process_sign()"]
            SIGN["Verificar firma del cliente\n(VerifyingKey::verify)"]
            SIGN -->|Inválida| S_ERR["Err('Firma inválida')"]
            SIGN -->|Válida| S_BUILD["Construir certificado:\n• Random 32B\n• Timestamp exp +24h\n• Pubkey cliente 32B\n• Datos arbitrarios"]
            S_BUILD --> S_SIGN["Servidor firma certificado\n(SigningKey::sign)"]
            S_SIGN --> S_OK["Ok([FirmaServidor(64) | Certificado])"]
        end

        MUT_OK --> ENG_LOOP
        MUT_ERR --> ENG_LOOP
        Q_OK --> ENG_LOOP
        Q_ERR --> ENG_LOOP
        S_OK --> ENG_LOOP
        S_ERR --> ENG_LOOP
    end

    %% Conexiones entre flujos
    DO_CMD -.->|engine_tx| ENG_LOOP
    SIGN_CMD -.->|engine_tx| ENG_LOOP

    %% Estilos
    classDef actor fill:#1e40af,color:#fff,stroke:#1e3a8a
    classDef db fill:#065f46,color:#fff,stroke:#064e3b
    classDef error fill:#991b1b,color:#fff,stroke:#7f1d1d
    classDef ok fill:#065f46,color:#fff,stroke:#064e3b
    classDef event fill:#78350f,color:#fff,stroke:#92400e

    class SPAWN_ENGINE,SPAWN_HTTP actor
    class DATABASES,MUT_WRITE,MUT_IDX,Q_READ db
    class DO_400,DO_401,SIGN_400,SIGN_401,FILE_404,ROOT_404,MUT_ERR,MUT_EXP,Q_ERR,S_ERR,FAIL_FAST error
    class DO_200,SIGN_200,FILE_200,ROOT_API,ROOT_HOST,MUT_OK,Q_OK,S_OK ok
    class P2P_EVENT,CELL_IN,PEER_NEW event
```
