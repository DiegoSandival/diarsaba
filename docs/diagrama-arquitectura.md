# Diagrama de Arquitectura — Diarsaba

## 1. Arquitectura general

```mermaid
graph TD
    subgraph Clientes
        C1[Cliente HTTP]
        C2[Cliente WebSocket]
    end

    subgraph Servidor["servidor (server.rs)"]
        R_INDEX["GET / → index_handler"]
        R_DO["POST /do → http_frame_handler"]
        R_WS["GET /ws → ws_upgrade_handler"]
        ROUTER[route_diarsaba_frame]
        FWD[forward_network_events]
        BCAST["broadcast::Sender&lt;Vec&lt;u8&gt;&gt;"]
    end

    subgraph Protocolo["protocolo (protocol.rs)"]
        PARSE[parse_header]
        BUILD[build_header]
        HDR["DiarsabaHeader\n domain | flags | req_id | payload_len"]
    end

    subgraph Dominios["despachador de dominio"]
        DOM_DB["DOMAIN_DB 0x10"]
        DOM_NET["DOMAIN_NET 0x20"]
        DOM_AUTH["DOMAIN_AUTH 0x30\n(stub)"]
        DOM_EVT["DOMAIN_EVENT 0x80\n(solo push)"]
    end

    subgraph DB["db_handler.rs"]
        CEL["trait CellEngineLike\n→ ejecutar()"]
        CEL_REAL["CellEngine\n(feature: correspondence)"]
        CEL_NOOP["NoopDb\n(sin feature)"]
    end

    subgraph NET["net_handler.rs"]
        NCL["trait NodeClientLike\n→ send_direct_message()\n→ publish_message()\n→ announce_provider()\n→ find_providers()"]
        NCL_REAL["NodeClient\n(feature: synap2p)"]
        NCL_NOOP["NoopNet\n(sin feature)"]
        PKG[pack_network_event]
    end

    subgraph Estado["AppState (Arc)"]
        STATE["db: Arc&lt;D: CellEngineLike&gt;\nnet: Arc&lt;N: NodeClientLike&gt;\nevent_tx: broadcast::Sender"]
    end

    C1 -->|"frame binario"| R_DO
    C2 -->|"upgrade"| R_WS
    R_DO --> ROUTER
    R_WS --> ROUTER
    ROUTER --> PARSE
    PARSE --> HDR
    ROUTER --> DOM_DB
    ROUTER --> DOM_NET
    ROUTER --> DOM_AUTH
    DOM_DB --> CEL
    DOM_NET --> NCL
    CEL --> CEL_REAL
    CEL --> CEL_NOOP
    NCL --> NCL_REAL
    NCL --> NCL_NOOP
    NCL_REAL -->|"NetworkEvent"| PKG
    PKG --> FWD
    FWD --> BCAST
    BCAST -->|"push frame"| C2
    ROUTER -->|"respuesta binaria"| BUILD
    BUILD --> HDR
    Estado --> R_DO
    Estado --> R_WS
```

## 2. Flujo de un frame de request/response

```mermaid
sequenceDiagram
    participant Cliente
    participant Servidor
    participant Protocol as protocol.rs
    participant Handler as db/net_handler.rs
    participant Backend as CellEngine / NodeClient

    Cliente->>Servidor: frame binario (HTTP POST /do  ó  WS mensaje)
    Servidor->>Protocol: parse_header(frame)
    Protocol-->>Servidor: DiarsabaHeader { domain, flags, req_id, payload_len }
    Servidor->>Servidor: extraer payload[HEADER_LEN..HEADER_LEN+payload_len]
    alt domain == DOMAIN_DB (0x10)
        Servidor->>Handler: handle_db_request(header, payload, db)
        Handler->>Backend: ejecutar(target_index, solucion, opcode, params)
        Backend-->>Handler: Ok(bytes) | Err(msg)
    else domain == DOMAIN_NET (0x20)
        Servidor->>Handler: handle_net_request(header, payload, net)
        Handler->>Backend: send_direct_message / publish_message / announce_provider / find_providers
        Backend-->>Handler: Ok(...) | Err(msg)
    else domain == DOMAIN_AUTH (0x30)
        Servidor->>Servidor: respuesta stub "auth not implemented"
    else dominio desconocido
        Servidor->>Servidor: build_error_frame("unknown diarsaba domain")
    end
    Handler-->>Servidor: Vec<u8> (frame de respuesta)
    Servidor->>Protocol: build_header(DiarsabaHeader)
    Servidor-->>Cliente: frame binario de respuesta
```

## 3. Flujo de eventos push (WebSocket)

```mermaid
sequenceDiagram
    participant P2P as synap2p / NodeClient
    participant FWD as forward_network_events
    participant TX as broadcast::Sender
    participant WS as WebSocket session
    participant Cliente

    P2P->>FWD: NetworkEvent (DirectMessage / GossipMessage / ProviderFound)
    FWD->>FWD: pack_network_event(event) → Vec<Vec<u8>>
    FWD->>TX: event_tx.send(frame)
    TX->>WS: event_rx.recv()
    WS-->>Cliente: Message::Binary(frame DOMAIN_EVENT 0x80)
```

## 4. Estructura del frame binario

```mermaid
packet-beta
  0-7: "domain (1 B)"
  8-15: "flags (1 B)"
  16-47: "req_id — little-endian (4 B)"
  48-63: "payload_len — big-endian (2 B)"
  64-127: "payload (0 … 65 535 B)"
```

## 5. Dominios y opcodes

```mermaid
mindmap
  root((Diarsaba\nProtocolo))
    DB 0x10
      DB_READ 0x01
      DB_WRITE 0x02
      DB_DELETE 0x03
      DB_DERIVE 0x04
      DB_MERGE 0x05
      DB_INSPECT 0x06
    NET 0x20
      NET_DIRECT_MSG 0x22 ✅
      NET_PUBLISH 0x24 ✅
      NET_ANNOUNCE 0x25 ✅
      NET_FIND 0x26 ✅
      NET_CONNECT 0x21 ⬜
      NET_SUBSCRIBE 0x23 ⬜
    AUTH 0x30
      AUTH_GEN_KEYPAIR 0x31 ⬜
      AUTH_SIGN 0x32 ⬜
      AUTH_VERIFY 0x33 ⬜
    EVENT 0x80
      EVT_DIRECT_MSG 0x01
      EVT_GOSSIP_MSG 0x02
      EVT_PROVIDER_FOUND 0x03
```

## 6. Modos de compilación (features)

```mermaid
flowchart LR
    subgraph Features
        F1["correspondence"]
        F2["synap2p"]
    end

    subgraph "DB backend"
        CE["CellEngine\n(correspondence::CellEngine)"]
        ND["NoopDb\n(stub — siempre Err)"]
    end

    subgraph "NET backend"
        NC["NodeClient\n(synap2p::NodeClient)"]
        NN["NoopNet\n(stub — siempre Err)"]
    end

    F1 -- habilitado --> CE
    F1 -- ausente --> ND
    F2 -- habilitado --> NC
    F2 -- ausente --> NN

    CE & NC --> REAL["run_real_server()\nlee env vars\nDIARSABA_BIND_ADDR\nDIARSABA_DB_PATH\nDIARSABA_KV_PATH\nDIARSABA_P2P_*"]
    ND & NN --> DEV["main() de desarrollo\nbind 127.0.0.1:3000"]
```
