# Protocolo Binario de Diarsaba

Este documento describe la codificación binaria observada en el código actual del servidor. No es una especificación aspiracional; refleja la implementación presente del crate.

## 1. Header

Todo frame Diarsaba tiene un header fijo de 8 bytes:

| Offset | Tamaño | Campo | Endianness |
| --- | --- | --- | --- |
| 0 | 1 | `domain` | n/a |
| 1 | 1 | `flags` | n/a |
| 2 | 4 | `req_id` | little-endian |
| 6 | 2 | `payload_len` | big-endian |

Longitud total del frame:

$$
frame\_len = 8 + payload\_len
$$

### Ejemplo de header

Si:

- `domain = 0x20`
- `flags = 0x02`
- `req_id = 0x12345678`
- `payload_len = 0x0005`

Entonces el header es:

```text
20 02 78 56 34 12 00 05
```

Observación crítica:

- `req_id` y `payload_len` usan endianness distintos. Esto no es accidental; está probado por las unit tests del proyecto.

## 2. Dominios

| Dominio | Valor | Uso |
| --- | --- | --- |
| `DOMAIN_DB` | `0x10` | Solicitudes de base/celdas |
| `DOMAIN_NET` | `0x20` | Solicitudes P2P |
| `DOMAIN_AUTH` | `0x30` | Reservado, no implementado realmente |
| `DOMAIN_EVENT` | `0x80` | Eventos push emitidos por el servidor |

## 3. Flags

| Flag | Valor | Significado actual |
| --- | --- | --- |
| `FLAG_FALLBACK_P2P` | `0b0000_0001` | Declarado, no usado por el router actual |
| `FLAG_HAS_PASSPORT` | `0b0000_0010` | Declarado, no usado por el router actual |
| `FLAG_ERROR` | `0b1000_0000` | Marca respuestas de error |

El servidor actual no toma decisiones de negocio basadas en `FLAG_FALLBACK_P2P` ni `FLAG_HAS_PASSPORT`.

## 4. Reglas generales de respuesta

### Éxito

- mismo dominio de la operación atendida;
- `flags = 0x00`;
- mismo `req_id` recibido;
- payload dependiente de la operación.

### Error

- mismo dominio de la operación cuando es recuperable;
- `flags = FLAG_ERROR`;
- `req_id` recibido o `0` si no pudo extraerse;
- payload textual UTF-8 con el mensaje de error.

### Límite de tamaño

`payload_len` es `u16`, por lo que:

$$
0 \le payload\_len \le 65535
$$

Si una respuesta supera ese máximo, el servidor intenta sustituirla por un error textual más pequeño.

## 5. Dominio DB

### 5.1 Estructura del payload de request

| Orden | Campo | Tamaño |
| --- | --- | --- |
| 1 | `opcode` | 1 byte |
| 2 | `target_index` | 4 bytes little-endian |
| 3 | `solucion` | 32 bytes |
| 4 | `params` | resto del payload |

La longitud mínima del payload DB es 37 bytes.

Representación:

```text
+--------+-------------------+----------------------------------+-----------+
| opcode | target_index (LE) | solucion[32]                     | params... |
+--------+-------------------+----------------------------------+-----------+
```

### 5.2 Opcodes DB declarados

| Constante | Valor |
| --- | --- |
| `DB_READ` | `0x01` |
| `DB_WRITE` | `0x02` |
| `DB_DELETE` | `0x03` |
| `DB_DERIVE` | `0x04` |
| `DB_MERGE` | `0x05` |
| `DB_INSPECT` | `0x06` |

Punto importante:

- el servidor no implementa un `match` sobre estos opcodes;
- simplemente extrae los campos y llama a `ejecutar(target_index, solucion, opcode, params)`.

Por tanto, la semántica efectiva pertenece al backend `correspondence`.

### 5.3 Respuesta DB

En éxito:

```text
domain = 0x10
flags  = 0x00
req_id = request.req_id
payload = bytes retornados por el motor
```

En error:

```text
domain = 0x10
flags  = 0x80
req_id = request.req_id
payload = mensaje UTF-8
```

### 5.4 Ejemplo esquemático

Request:

```text
Header:
10 00 01 00 00 00 00 29

Payload:
02                ; opcode DB_WRITE
2A 00 00 00       ; target_index = 42
<32 bytes>        ; solucion
DE AD BE EF       ; params
```

`0x0029 = 41`, que corresponde a `1 + 4 + 32 + 4` bytes de payload.

## 6. Dominio NET

El dominio NET sí interpreta opcodes en el servidor.

### 6.1 Opcodes declarados

| Constante | Valor | Estado |
| --- | --- | --- |
| `NET_CONNECT` | `0x21` | implementado |
| `NET_DIRECT_MSG` | `0x22` | implementado |
| `NET_SUBSCRIBE` | `0x23` | implementado |
| `NET_PUBLISH` | `0x24` | implementado |
| `NET_ANNOUNCE` | `0x25` | implementado |
| `NET_FIND` | `0x26` | implementado |

### 6.2 Primitive length-prefixed

Varias operaciones usan un campo con esta forma:

```text
+-----------+------------------+-----------+
| field_len | field[field_len] | tail[...] |
+-----------+------------------+-----------+
```

`field_len` ocupa un byte, así que el tamaño máximo del campo principal es 255 bytes.

### 6.3 `NET_CONNECT` (`0x21`)

Payload:

```text
+--------+----------+----------------------+----------+----------------------+ 
| opcode | peer_len | peer_bytes[peer_len] | addr_len | addr_bytes[addr_len] |
+--------+----------+----------------------+----------+----------------------+ 
```

Semántica:

- `peer_bytes` se convierten a `PeerId`.
- `addr_bytes` se convierten a `Multiaddr`.
- no debe haber trailing bytes después de `addr_bytes`.

Respuesta exitosa:

- payload vacío.

### 6.4 `NET_DIRECT_MSG` (`0x22`)

Payload:

```text
+--------+----------+----------------------+---------+
| opcode | peer_len | peer_bytes[peer_len] | data...  |
+--------+----------+----------------------+---------+
```

Semántica:

- `peer_bytes` se convierten a `PeerId`.
- en builds con `synap2p`, el parseo usa `PeerId::from_str`.
- en builds sin `synap2p`, el peer se trata como `String`.

Respuesta exitosa:

- payload vacío.

### 6.5 `NET_SUBSCRIBE` (`0x23`)

Payload:

```text
+--------+-----------+------------------------+
| opcode | topic_len | topic_bytes[topic_len] |
+--------+-----------+------------------------+
```

Restricción:

- no debe haber trailing bytes después del topic;
- si existen, el servidor devuelve `unexpected trailing bytes in subscribe request`.

Respuesta exitosa:

- payload vacío.

### 6.6 `NET_PUBLISH` (`0x24`)

Payload:

```text
+--------+-----------+------------------------+---------+
| opcode | topic_len | topic_bytes[topic_len] | data...  |
+--------+-----------+------------------------+---------+
```

`topic_bytes` se interpretan con `String::from_utf8_lossy`.

### 6.7 `NET_ANNOUNCE` (`0x25`)

Payload:

```text
+--------+---------+--------------------+
| opcode | key_len | key_bytes[key_len] |
+--------+---------+--------------------+
```

Restricción:

- no debe haber trailing bytes después de la clave;
- si existen, el servidor devuelve `unexpected trailing bytes in announce request`.

### 6.8 `NET_FIND` (`0x26`)

Payload:

```text
+--------+---------+--------------------+
| opcode | key_len | key_bytes[key_len] |
+--------+---------+--------------------+
```

Restricción idéntica a `NET_ANNOUNCE`: no admite bytes sobrantes.

Respuesta exitosa:

lista de peers serializada así:

```text
+----------+----------------------+----------+----------------------+ ...
| peer_len | peer_bytes[peer_len] | peer_len | peer_bytes[peer_len] |
+----------+----------------------+----------+----------------------+ ...
```

No se incluye contador total; el cliente debe consumir hasta agotar el payload.

## 7. Dominio AUTH

### 7.1 Constantes declaradas

| Constante | Valor |
| --- | --- |
| `AUTH_GEN_KEYPAIR` | `0x31` |
| `AUTH_SIGN` | `0x32` |
| `AUTH_VERIFY` | `0x33` |

### 7.2 Comportamiento real

Cualquier frame con `domain = 0x30` produce una respuesta exitosa con el payload ASCII/UTF-8:

```text
auth not implemented
```

No hay parseo de opcodes AUTH ni validación de payload.

## 8. Dominio EVENT

Los frames de `DOMAIN_EVENT` son generados por el servidor y enviados a clientes WebSocket. No forman parte del ciclo request/response tradicional.

### 8.1 Header de eventos push

Todos los eventos push se envían con:

```text
domain = 0x80
flags  = 0x00
req_id = 0
```

### 8.2 Tipos de evento

| Constante | Valor |
| --- | --- |
| `EVT_DIRECT_MSG` | `0x01` |
| `EVT_GOSSIP_MSG` | `0x02` |
| `EVT_PROVIDER_FOUND` | `0x03` |

### 8.3 `EVT_DIRECT_MSG`

Estructura:

```text
+--------+----------+----------------------+---------+
| opcode | peer_len | peer_bytes[peer_len] | data...  |
+--------+----------+----------------------+---------+
```

Proviene de `NetworkEvent::DirectMessageReceived`.

### 8.4 `EVT_GOSSIP_MSG`

Estructura:

```text
+--------+-----------+------------------------+---------+
| opcode | topic_len | topic_bytes[topic_len] | data...  |
+--------+-----------+------------------------+---------+
```

Proviene de `NetworkEvent::GossipMessageReceived`.

Nota:

- el `source` del evento de gossip no se serializa en el payload actual.

### 8.5 `EVT_PROVIDER_FOUND`

Estructura:

```text
+--------+---------+--------------------+----------+----------------------+
| opcode | key_len | key_bytes[key_len] | peer_len | peer_bytes[peer_len] |
+--------+---------+--------------------+----------+----------------------+
```

Semántica importante:

- si el backend reporta varios proveedores para la misma clave, el servidor no los agrupa en un solo frame;
- genera un frame independiente por cada proveedor.

## 9. Transporte HTTP y WebSocket

### 9.1 HTTP

Endpoint:

```text
POST /do
```

Contrato:

- body request: frame binario completo;
- body response: frame binario completo;
- el servidor siempre devuelve `200 OK` a nivel HTTP cuando logra producir una respuesta de aplicación, incluso si el frame contiene `FLAG_ERROR`.

### 9.2 WebSocket

Endpoint:

```text
GET /ws
```

Contrato:

- los mensajes binarios de cliente se interpretan como frames completos;
- cada mensaje binario genera exactamente una respuesta binaria;
- adicionalmente, el cliente puede recibir eventos push `DOMAIN_EVENT` sin haberlos solicitado directamente en ese instante.

## 10. Errores frecuentes de cliente

| Situación | Resultado |
| --- | --- |
| header con menos de 8 bytes | `header requires at least 8 bytes` |
| `payload_len` mayor al tamaño real del body | `frame payload shorter than declared length` |
| dominio desconocido | `unknown diarsaba domain` |
| request NET sin campo length-prefixed completo | `missing length-prefixed field` o `length-prefixed field exceeds payload` |
| bytes extra en `NET_CONNECT`, `NET_SUBSCRIBE`, `NET_ANNOUNCE` o `NET_FIND` | error por trailing bytes |
| opcode NET no soportado | `unsupported net opcode` |

## 11. Consideraciones para implementadores de clientes

- Tratar `req_id` como correlación de request/response, excepto en `DOMAIN_EVENT` donde siempre llega como `0`.
- No asumir que todos los errores se reflejan en códigos HTTP o en cierres de WebSocket; deben inspeccionarse los bits del frame.
- Soportar payloads vacíos en respuestas exitosas de algunas operaciones NET.
- Soportar múltiples frames `EVT_PROVIDER_FOUND` para una sola búsqueda lógica.
- No depender de `AUTH` para flujos reales hasta que exista implementación más allá del placeholder.
