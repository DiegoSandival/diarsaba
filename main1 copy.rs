
impl CircularDbCore {
    fn open(path: &str) -> io::Result<(Self, DbReader)> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        if file.metadata()?.len() == 0 {
            file.set_len(RECORD_SIZE * MAX_RECORDS as u64)?;
        }

        let (cursor, phase) = Self::recover_state(&file)?;
        println!("CicliDB Iniciada -> Cursor: {} | Fase: {}", cursor, phase);

        let shared_file = Arc::new(file);

        let core = Self {
            file: shared_file.clone(),
            cursor,
            phase,
        };

        let reader = DbReader { file: shared_file };

        Ok((core, reader))
    }

    fn append(&mut self, data: &[u8; DATA_SIZE]) -> io::Result<u32> {
        let mut buffer = [0u8; RECORD_SIZE as usize];
        buffer[0] = self.phase;
        buffer[1..].copy_from_slice(data);

        let offset = (self.cursor as u64) * RECORD_SIZE;
        self.file.write_all_at(&buffer, offset)?;

        let written_index = self.cursor;
        self.cursor += 1;

        if self.cursor >= MAX_RECORDS {
            self.cursor = 0;
            self.phase ^= 1;
        }

        Ok(written_index)
    }

    fn overwrite(&mut self, index: u32, data: &[u8; DATA_SIZE]) -> io::Result<()> {
        if index >= MAX_RECORDS {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Índice fuera de rango"));
        }

        let offset = (index as u64) * RECORD_SIZE;
        let mut phase_buf = [0u8; 1];
        self.file.read_exact_at(&mut phase_buf, offset)?;

        let mut buffer = [0u8; RECORD_SIZE as usize];
        buffer[0] = phase_buf[0]; 
        buffer[1..].copy_from_slice(data);

        self.file.write_all_at(&buffer, offset)?;
        Ok(())
    }

    fn recover_state(file: &File) -> io::Result<(u32, u8)> {
        let mut low = 0;
        let mut high = MAX_RECORDS - 1;

        let phase_first = Self::read_phase_bit(file, 0)?;
        let phase_last = Self::read_phase_bit(file, high)?;

        if phase_first == phase_last {
            return Ok((0, phase_first ^ 1));
        }

        while low < high {
            let mid = low + (high - low) / 2;
            let phase_mid = Self::read_phase_bit(file, mid)?;

            if phase_mid == phase_first {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok((low, phase_first))
    }

    fn read_phase_bit(file: &File, index: u32) -> io::Result<u8> {
        let offset = (index as u64) * RECORD_SIZE;
        let mut buf = [0u8; 1];
        file.read_exact_at(&mut buf, offset)?;
        Ok(buf[0] & 1)
    }
}



// ==========================================
// 1. EL ADN (GENES Y PODERES - 32 bits)
// ==========================================
pub const CMD_LEER_SELF:       u32 = 0x00000001;
pub const CMD_LEER_ANY:        u32 = 0x00000002;
pub const CMD_ESCRIBIR_SELF:   u32 = 0x00000004;
pub const CMD_ESCRIBIR_ANY:    u32 = 0x00000008;
pub const CMD_BORRAR_SELF:     u32 = 0x00000010;
pub const CMD_BORRAR_ANY:      u32 = 0x00000020;
pub const CMD_DIVIDIR:         u32 = 0x00000040;
pub const CMD_FUCIONAR:        u32 = 0x00000080;
pub const CMD_CLONAR:          u32 = 0x00000100; 

pub const CMD_DOMINANTE:       u32 = 0x00000200;
pub const CMD_LEER_LIBRE:      u32 = 0x00004000;

pub const CMD_MIGRADA:         u32 = 0x00080000;
pub const GHOST_FLAG:          u32 = 0x80000000;

pub const MASK_SEGURIDAD: u32 = !(GHOST_FLAG | CMD_LINK_L_VERIF | CMD_LINK_R_VERIF | CMD_MIGRADA);



pub fn validar_candado(salt: &[u8; 32], challenge: &[u8; 32], solucion: &[u8; 32]) -> bool {
    if let Ok(mut mac) = <Blake2bMac256 as KeyInit>::new_from_slice(solucion) {
        mac.update(salt);
        return mac.verify_slice(challenge).is_ok();
    }
    false
}

pub fn ciclidb_read_estricto(
    reader: &DbReader, 
    index_inicial: u32, 
    solucion_cliente: &[u8; 32]
) -> Result<(Cell, u32), String> {
    let mut index_actual = index_inicial;

    loop {
        let raw_bytes = reader.read_raw(index_actual)
            .map_err(|_| "Error de lectura en disco".to_string())?;
        
        let cell = Cell::from_bytes(&raw_bytes);

        if !validar_candado(&cell.salt, &cell.challenge, solucion_cliente) {
            return Err("Rastro perdido o Challenge incorrecto (HTTP 401)".to_string());
        }

        if (cell.cmd & CMD_MIGRADA) != 0 {
            index_actual = cell.index_l;
        } else {
            return Ok((cell, index_actual));
        }
    }
}

pub fn ciclidb_read_libre(
    reader: &DbReader, 
    index_inicial: u32
) -> Result<(Cell, u32), String> {
    let mut index_actual = index_inicial;
    loop {
        let raw_bytes = reader.read_raw(index_actual)
            .map_err(|_| "Error de lectura en disco".to_string())?;
        
        let cell = Cell::from_bytes(&raw_bytes);
        
        if (cell.cmd & CMD_MIGRADA) != 0 {
            index_actual = cell.index_l;
        } else {
            return Ok((cell, index_actual));
        }
    }
}

/// Ejecuta el ciclo de vida post-acción y RETORNA EL NUEVO ÍNDICE (u32)
async fn procesar_ciclo_de_vida(
    state: &AppState,
    celula: Cell,
    index_real: u32,
    solucion_cliente: &[u8; 32],
) -> Result<u32, StatusCode> {
    if (celula.cmd & CMD_EFIMERA) != 0 && (celula.cmd & CMD_INMORTAL) == 0 {
        // Se vuelve fantasma: No migra de posición
        let mut ghost = celula.clone();
        ghost.cmd |= GHOST_FLAG;
        
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.db_writer_tx.send(DbMessage::Overwrite { index: index_real, data: ghost.to_bytes(), responder: tx }).await.unwrap();
        rx.await.unwrap().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(index_real)
    } else {
        // Renueva el candado y MIGRA al final de la base de datos
        let (nuevo_salt, nuevo_challenge) = {
            let mut rng = rand::thread_rng();
            let mut s = [0u8; 32];
            rng.fill(&mut s);
            
            let mut mac = <Blake2bMac256 as KeyInit>::new_from_slice(solucion_cliente).unwrap();
            mac.update(&s);
            let chall: [u8; 32] = mac.finalize().into_bytes().into();
            
            (s, chall)
        };

        let celula_fresca = Cell {
            salt: nuevo_salt,
            challenge: nuevo_challenge,
            ..celula.clone()
        };

        let (tx1, rx1) = tokio::sync::oneshot::channel();
        state.db_writer_tx.send(DbMessage::Append { data: celula_fresca.to_bytes(), responder: tx1 }).await.unwrap();
        
        // ¡Este es el nuevo índice de la célula viva!
        let nuevo_index = rx1.await.unwrap().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let mut celula_apuntador = celula;
        celula_apuntador.index_l = nuevo_index;
        celula_apuntador.index_r = 0;
        celula_apuntador.cmd |= CMD_MIGRADA;

        let (tx2, rx2) = tokio::sync::oneshot::channel();
        state.db_writer_tx.send(DbMessage::Overwrite { index: index_real, data: celula_apuntador.to_bytes(), responder: tx2 }).await.unwrap();
        rx2.await.unwrap().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        // Retornamos el nuevo índice para comunicarlo al cliente
        Ok(nuevo_index)
    }
}



async fn handle_api_do(
    State(state): State<AppState>,
    body: Bytes, 
) -> Result<Bytes, (StatusCode, String)> {
    if body.len() < 130 {
        return Err((StatusCode::BAD_REQUEST, format!("Payload muy corto: {} bytes (mínimo 130)", body.len())));
    }

    let firma_servidor_bytes = &body[0..64];
    let firma_peticion_bytes = &body[64..128];
    
    let tamano_cert = u16::from_le_bytes(body[128..130].try_into().unwrap()) as usize;
    
    if body.len() < 130 + tamano_cert + 37 { 
        return Err((StatusCode::BAD_REQUEST, "El payload no contiene la petición CicliDB completa".to_string()));
    }

    let certificado_bytes = &body[130 .. 130 + tamano_cert];
    let peticion_bytes    = &body[130 + tamano_cert ..];

    let signature_servidor = Signature::from_slice(firma_servidor_bytes)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Firma del servidor malformada".to_string()))?;
    
    if state.server_keypair.verifying_key().verify(certificado_bytes, &signature_servidor).is_err() {
        return Err((StatusCode::UNAUTHORIZED, "Certificado falso o alterado".to_string())); 
    }

    if certificado_bytes.len() < 40 { return Err((StatusCode::BAD_REQUEST, "Certificado truncado".to_string())); }
    let user_pub_key_bytes: [u8; 32] = certificado_bytes[0..32].try_into().unwrap();
    let expiracion = u64::from_le_bytes(certificado_bytes[32..40].try_into().unwrap());

    let ahora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    if ahora > expiracion {
        return Err((StatusCode::UNAUTHORIZED, "El certificado ha expirado".to_string())); 
    }

    let user_pub_key = VerifyingKey::from_bytes(&user_pub_key_bytes)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Llave pública de usuario inválida".to_string()))?;
    let signature_peticion = Signature::from_slice(firma_peticion_bytes)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Firma de petición malformada".to_string()))?;

    if user_pub_key.verify(peticion_bytes, &signature_peticion).is_err() {
        return Err((StatusCode::UNAUTHORIZED, "La firma de la petición no coincide con tu certificado".to_string())); 
    }

    let target_index = u32::from_le_bytes(peticion_bytes[0..4].try_into().unwrap());
    let solucion_32b: [u8; 32] = peticion_bytes[4..36].try_into().unwrap();
    let action_opcode = peticion_bytes[36];
    let _params_bytes = &peticion_bytes[37..];

    let (celula, _index_real) = ciclidb_read_estricto(&state.db_reader, target_index, &solucion_32b)
        .map_err(|e| (StatusCode::UNAUTHORIZED, format!("Genética rechazada: {}", e)))?;

    match action_opcode {
        0x01 => { // OP: LEER
            if _params_bytes.is_empty() { return Err((StatusCode::BAD_REQUEST, "Falta la llave de redb".to_string())); }
            let key_len = _params_bytes[0] as usize;
            if _params_bytes.len() < 1 + key_len { return Err((StatusCode::BAD_REQUEST, "Llave de redb truncada".to_string())); }
            let key_str = std::str::from_utf8(&_params_bytes[1 .. 1+key_len]).map_err(|_| (StatusCode::BAD_REQUEST, "Llave no es UTF-8 válido".to_string()))?;

            let read_txn = state.temp_db.begin_read().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error de BD temporal".to_string()))?;
            let table = read_txn.open_table(TEMP_TABLE).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error de tabla temporal".to_string()))?;

            if let Some(entry) = table.get(key_str).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error al leer entrada".to_string()))? {
                let value_bytes = entry.value();
                if value_bytes.len() < 4 { return Err((StatusCode::INTERNAL_SERVER_ERROR, "Datos corruptos en redb".to_string())); }
                
                let owner_index_original = u32::from_le_bytes(value_bytes[0..4].try_into().unwrap());
                let data_real = &value_bytes[4..];

                // ¡LA MAGIA EVOLUTIVA!: Rastreamos dónde vive el dueño original hoy.
                let owner_current_index = if let Ok((_, idx)) = ciclidb_read_libre(&state.db_reader, owner_index_original) { idx } else { owner_index_original };

                if (celula.cmd & CMD_LEER_ANY) != 0 {
                    return Ok(Bytes::copy_from_slice(data_real));
                } else if (celula.cmd & CMD_LEER_SELF) != 0 {
                    // Ahora comparamos contra la dirección actual del dueño
                    let valid_self = owner_current_index == _index_real;
                    let valid_l = owner_current_index == celula.index_l && (celula.cmd & CMD_LINK_L_VERIF) != 0;
                    let valid_r = owner_current_index == celula.index_r && (celula.cmd & CMD_LINK_R_VERIF) != 0;
                    
                    if valid_self || valid_l || valid_r {
                        return Ok(Bytes::copy_from_slice(data_real));
                    }
                }
                return Err((StatusCode::FORBIDDEN, "No tienes permiso para leer esta llave".to_string()));
            }
            Err((StatusCode::NOT_FOUND, "La llave no existe en la base de datos".to_string()))
        },

        0x02 => { // OP: ESCRIBIR
            if _params_bytes.is_empty() { return Err((StatusCode::BAD_REQUEST, "Falta la llave de redb".to_string())); }
            let key_len = _params_bytes[0] as usize;
            if _params_bytes.len() < 1 + key_len { return Err((StatusCode::BAD_REQUEST, "Llave de redb truncada".to_string())); }
            let key_str = std::str::from_utf8(&_params_bytes[1 .. 1+key_len]).map_err(|_| (StatusCode::BAD_REQUEST, "Llave no es UTF-8".to_string()))?;
            let payload_bytes = &_params_bytes[1+key_len ..];

            let mut is_overwrite = false;
            let mut is_owner = false;

            let read_txn = state.temp_db.begin_read().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error de BD temporal".to_string()))?;
            let table = read_txn.open_table(TEMP_TABLE).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error de tabla temporal".to_string()))?;
            
            if let Some(entry) = table.get(key_str).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error al buscar llave".to_string()))? {
                let existing_val = entry.value();
                if existing_val.len() >= 4 {
                    is_overwrite = true;
                    let owner_index_original = u32::from_le_bytes(existing_val[0..4].try_into().unwrap());
                    
                    // Rastreamos al dueño para ver si la congeló o si somos nosotros
                    let owner_current_index = if let Ok((owner_cell, idx)) = ciclidb_read_libre(&state.db_reader, owner_index_original) {
                        if (owner_cell.cmd & CMD_CONGELAMIENTO) != 0 {
                            return Err((StatusCode::FORBIDDEN, "El dueño de esta llave la ha congelado".to_string())); 
                        }
                        idx
                    } else { owner_index_original };
                    
                    is_owner = owner_current_index == _index_real ||
                               (owner_current_index == celula.index_l && (celula.cmd & CMD_LINK_L_VERIF) != 0) ||
                               (owner_current_index == celula.index_r && (celula.cmd & CMD_LINK_R_VERIF) != 0);
                }
            }
            drop(table); drop(read_txn);

            let can_write_any = (celula.cmd & CMD_ESCRIBIR_ANY) != 0;
            // Solo puedes sobrescribir con SELF si eres el dueño actual de la cadena
            let can_write_self = (celula.cmd & CMD_ESCRIBIR_SELF) != 0 && (!is_overwrite || is_owner);

            if can_write_any || can_write_self {
                let write_txn = state.temp_db.begin_write().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al iniciar transacción".to_string()))?;
                {
                    let mut table = write_txn.open_table(TEMP_TABLE).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al abrir tabla".to_string()))?;
                    let mut final_value = Vec::with_capacity(4 + payload_bytes.len());
                    // Guardamos la dirección actual como nueva dueña
                    final_value.extend_from_slice(&_index_real.to_le_bytes()); 
                    final_value.extend_from_slice(payload_bytes);
                    table.insert(key_str, final_value.as_slice()).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al insertar dato".to_string()))?;
                }
                write_txn.commit().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al guardar transacción".to_string()))?;
                
                let nuevo_idx = procesar_ciclo_de_vida(&state, celula, _index_real, &solucion_32b).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error en el ciclo de vida celular".to_string()))?;
                
                return Ok(Bytes::from(nuevo_idx.to_le_bytes().to_vec()));
            }
            Err((StatusCode::FORBIDDEN, "Careces del gen ESCRIBIR o no eres dueño del dato".to_string()))
        },

        0x03 => { // OP: BORRAR
            if _params_bytes.is_empty() { return Err((StatusCode::BAD_REQUEST, "Falta la llave de redb".to_string())); }
            let key_len = _params_bytes[0] as usize;
            if _params_bytes.len() < 1 + key_len { return Err((StatusCode::BAD_REQUEST, "Llave de redb truncada".to_string())); }
            let key_str = std::str::from_utf8(&_params_bytes[1 .. 1+key_len]).map_err(|_| (StatusCode::BAD_REQUEST, "Llave no es UTF-8".to_string()))?;

            let write_txn = state.temp_db.begin_write().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al iniciar transacción".to_string()))?;
            let mut table = write_txn.open_table(TEMP_TABLE).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al abrir tabla".to_string()))?;

            let owner_index_opt = if let Some(entry) = table.get(key_str).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al buscar llave".to_string()))? {
                let val = entry.value();
                if val.len() >= 4 { Some(u32::from_le_bytes(val[0..4].try_into().unwrap())) } else { None }
            } else { None }; 

            if let Some(owner_index_original) = owner_index_opt {
                // Rastreamos al dueño
                let owner_current_index = if let Ok((owner_cell, idx)) = ciclidb_read_libre(&state.db_reader, owner_index_original) { 
                    if (owner_cell.cmd & CMD_CONGELAMIENTO) != 0 {
                        return Err((StatusCode::FORBIDDEN, "El dueño congeló esta entrada".to_string()));
                    }
                    idx 
                } else { owner_index_original };

                let can_delete_any = (celula.cmd & CMD_BORRAR_ANY) != 0;
                let can_delete_self = (celula.cmd & CMD_BORRAR_SELF) != 0 && (
                    owner_current_index == _index_real ||
                    (owner_current_index == celula.index_l && (celula.cmd & CMD_LINK_L_VERIF) != 0) ||
                    (owner_current_index == celula.index_r && (celula.cmd & CMD_LINK_R_VERIF) != 0)
                );

                if can_delete_any || can_delete_self {
                    table.remove(key_str).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al eliminar".to_string()))?;
                    drop(table); 
                    write_txn.commit().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Fallo al guardar transacción".to_string()))?;
                    
                    let nuevo_idx = procesar_ciclo_de_vida(&state, celula, _index_real, &solucion_32b).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error en ciclo".to_string()))?;
                    return Ok(Bytes::from(nuevo_idx.to_le_bytes().to_vec()));
                } else {
                    Err((StatusCode::FORBIDDEN, "Careces del gen BORRAR o no eres dueño".to_string()))
                }
            } else {
                Err((StatusCode::NOT_FOUND, "La llave no existe".to_string()))
            }
        },

       0x04 => { // OP: DERIVAR
            if _params_bytes.len() < 76 { 
                return Err((StatusCode::BAD_REQUEST, format!("DERIVAR exige 76 bytes de parámetros, recibimos {}", _params_bytes.len()))); 
            }
            
            let nuevo_salt: [u8; 32]      = _params_bytes[0..32].try_into().unwrap();
            let nuevo_challenge: [u8; 32] = _params_bytes[32..64].try_into().unwrap();
            let nuevos_cmd   = u32::from_le_bytes(_params_bytes[64..68].try_into().unwrap());
            let user_index_l = u32::from_le_bytes(_params_bytes[68..72].try_into().unwrap());
            let user_index_r = u32::from_le_bytes(_params_bytes[72..76].try_into().unwrap());

            if (celula.cmd & CMD_DERIVAR) == 0 { return Err((StatusCode::FORBIDDEN, "La madre carece del Gen de Mitosis (DERIVAR)".to_string())); }

            let l_is_verified = user_index_l == _index_real;
            let r_is_verified = user_index_r == _index_real;

            if (celula.cmd & CMD_ANCLA) != 0 && (user_index_l != 0 || user_index_r != 0) {
                return Err((StatusCode::FORBIDDEN, "Regla ANCLA violada".to_string()));
            }
            if (celula.cmd & CMD_ESTRICTA) != 0 && ((user_index_l != 0 && !l_is_verified) || (user_index_r != 0 && !r_is_verified)) {
                return Err((StatusCode::FORBIDDEN, "Regla ESTRICTA violada: Vínculos huérfanos".to_string()));
            }

            // 1. Aplicamos la máscara de seguridad para evitar genes fantasma/migrada inyectados manualmente
            let mut nuevos_cmd_limpios = nuevos_cmd & MASK_SEGURIDAD;
            let cmd_madre_limpios = celula.cmd & MASK_SEGURIDAD;

            // 2. EVOLUCIÓN PURA: Validamos absolutamente todos los poderes pedidos.
            // Si la hija pide un gen y la madre lo tiene, se transfiere intacto.
            if (nuevos_cmd_limpios & cmd_madre_limpios) == nuevos_cmd_limpios {
                
                if (celula.cmd & CMD_TERMINAL) != 0 { nuevos_cmd_limpios &= !(CMD_DERIVAR | CMD_TERMINAL); }
                if l_is_verified { nuevos_cmd_limpios |= CMD_LINK_L_VERIF; }
                if r_is_verified { nuevos_cmd_limpios |= CMD_LINK_R_VERIF; }

                let cmd_final = nuevos_cmd_limpios | (celula.cmd & GHOST_FLAG);

                let nueva_celula = Cell {
                    salt: nuevo_salt, challenge: nuevo_challenge,
                    index_l: user_index_l, index_r: user_index_r,
                    cmd: cmd_final, padding: [0u8; 20],
                };

                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                state.db_writer_tx.send(DbMessage::Append { data: nueva_celula.to_bytes(), responder: resp_tx }).await.unwrap();
                let nuevo_index = resp_rx.await.unwrap().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error en disco al guardar hija".to_string()))?;

                procesar_ciclo_de_vida(&state, celula, _index_real, &solucion_32b)
                    .await
                    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error renovando célula madre".to_string()))?;

                return Ok(Bytes::from(nuevo_index.to_le_bytes().to_vec()));
            }
            Err((StatusCode::FORBIDDEN, "Intento de heredar poderes que la madre no posee".to_string()))
        },

        0x05 => { // OP: COMBINAR
            if _params_bytes.len() < 108 { 
                return Err((StatusCode::BAD_REQUEST, format!("COMBINAR exige 108 bytes de parámetros, recibimos {}", _params_bytes.len()))); 
            }
            let index2 = u32::from_le_bytes(_params_bytes[0..4].try_into().unwrap());
            let solucion2: [u8; 32] = _params_bytes[4..36].try_into().unwrap();
            let nuevo_salt: [u8; 32] = _params_bytes[36..68].try_into().unwrap();
            let nuevo_challenge: [u8; 32] = _params_bytes[68..100].try_into().unwrap();
            let user_index_l = u32::from_le_bytes(_params_bytes[100..104].try_into().unwrap());
            let user_index_r = u32::from_le_bytes(_params_bytes[104..108].try_into().unwrap());

            if (celula.cmd & CMD_COMBINAR) == 0 { return Err((StatusCode::FORBIDDEN, "La madre 1 carece del gen COMBINAR".to_string())); }

            let (celula2, _index_real2) = ciclidb_read_estricto(&state.db_reader, index2, &solucion2)
                .map_err(|e| (StatusCode::UNAUTHORIZED, format!("Genética madre 2 rechazada: {}", e)))?;

            if (celula2.cmd & CMD_COMBINAR) == 0 { return Err((StatusCode::FORBIDDEN, "La madre 2 carece del gen COMBINAR".to_string())); }
            if (celula.cmd & CMD_DOMINANTE) != 0 || (celula2.cmd & CMD_DOMINANTE) != 0 { return Err((StatusCode::FORBIDDEN, "Las células dominantes no pueden combinarse".to_string())); }

            let l_is_verified = user_index_l == _index_real || user_index_l == _index_real2;
            let r_is_verified = user_index_r == _index_real || user_index_r == _index_real2;

            if ((celula.cmd & CMD_ANCLA) != 0 || (celula2.cmd & CMD_ANCLA) != 0) && (user_index_l != 0 || user_index_r != 0) {
                return Err((StatusCode::FORBIDDEN, "Regla ANCLA violada en combinación".to_string()));
            }
            if ((celula.cmd & CMD_ESTRICTA) != 0 || (celula2.cmd & CMD_ESTRICTA) != 0) && 
               ((user_index_l != 0 && !l_is_verified) || (user_index_r != 0 && !r_is_verified)) {
                return Err((StatusCode::FORBIDDEN, "Regla ESTRICTA violada en combinación".to_string()));
            }

            let mut poderes_combinados = (celula.cmd | celula2.cmd) & MASK_SEGURIDAD;
            if l_is_verified { poderes_combinados |= CMD_LINK_L_VERIF; }
            if r_is_verified { poderes_combinados |= CMD_LINK_R_VERIF; }

            let nueva_celula = Cell {
                salt: nuevo_salt, challenge: nuevo_challenge,
                index_l: user_index_l, index_r: user_index_r,
                cmd: poderes_combinados, padding: [0u8; 20],
            };

            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            state.db_writer_tx.send(DbMessage::Append { data: nueva_celula.to_bytes(), responder: resp_tx }).await.unwrap();
            let nuevo_index = resp_rx.await.unwrap().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error en disco al guardar fusión".to_string()))?;

            let mut madre1 = celula.clone();
            madre1.index_l = nuevo_index; madre1.index_r = 0; madre1.cmd |= CMD_MIGRADA;
            let (tx1, rx1) = tokio::sync::oneshot::channel();
            state.db_writer_tx.send(DbMessage::Overwrite { index: _index_real, data: madre1.to_bytes(), responder: tx1 }).await.unwrap();
            rx1.await.unwrap().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error renovando madre 1".to_string()))?;

            let mut madre2 = celula2.clone();
            madre2.index_l = nuevo_index; madre2.index_r = 0; madre2.cmd |= CMD_MIGRADA;
            let (tx2, rx2) = tokio::sync::oneshot::channel();
            state.db_writer_tx.send(DbMessage::Overwrite { index: _index_real2, data: madre2.to_bytes(), responder: tx2 }).await.unwrap();
            rx2.await.unwrap().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error renovando madre 2".to_string()))?;

            return Ok(Bytes::from(nuevo_index.to_le_bytes().to_vec()));
        },
0x06 => { // OP: INSPECCIONAR ADN (Microscopio)
            // Queremos ver la celda EXACTA que el usuario pidió, sin importar si migró.
            let raw_bytes = state.db_reader.read_raw(target_index)
                .map_err(|_| (StatusCode::NOT_FOUND, "Índice fuera de rango o vacío".to_string()))?;
            
            let celda_raw = Cell::from_bytes(&raw_bytes);
            
            // Requerimos el secreto para evitar que cualquiera escanee la base de datos
            if !validar_candado(&celda_raw.salt, &celda_raw.challenge, &solucion_32b) {
                return Err((StatusCode::UNAUTHORIZED, "Candado genético incorrecto para este índice".to_string()));
            }
            
            // Empaquetamos la metadata celular: [cmd(4)] + [L(4)] + [R(4)] + [Indice_Vivo_Real(4)] = 16 bytes
            let mut resp = Vec::with_capacity(16);
            resp.extend_from_slice(&celda_raw.cmd.to_le_bytes());
            resp.extend_from_slice(&celda_raw.index_l.to_le_bytes());
            resp.extend_from_slice(&celda_raw.index_r.to_le_bytes());
            resp.extend_from_slice(&_index_real.to_le_bytes()); // _index_real viene de ciclidb_read_estricto al inicio
            
            return Ok(Bytes::from(resp));
        },

        _ => Err((StatusCode::BAD_REQUEST, format!("Opcode 0x{:02x} no reconocido", action_opcode))), 
    }
}

// ==========================================
// INYECCIÓN DE LA CÉLULA GÉNESIS
// ==========================================
async fn inyectar_genesis_si_vacio(db_reader: &DbReader, db_writer_tx: &mpsc::Sender<DbMessage>) {
    if let Ok(raw) = db_reader.read_raw(0) {
        let cell = Cell::from_bytes(&raw);
        if cell.cmd != 0 { return; } 
    }

    println!("Inyectando Célula Génesis en el Índice 0...");

    // Convertimos la palabra "diarsaba" en 32 bytes
    let mut hasher = blake2::Blake2b::<blake2::digest::consts::U32>::new();
    hasher.update(b"diarsaba");
    let solucion_genesis: [u8; 32] = hasher.finalize().into();

    let salt_genesis = [1u8; 32];

    let mut mac = <Blake2bMac256 as KeyInit>::new_from_slice(&solucion_genesis).unwrap();
    mac.update(&salt_genesis);
    let challenge_genesis: [u8; 32] = mac.finalize().into_bytes().into();

    // El ADN definitivo de la Célula Génesis.
    // Posee todos los poderes activos, pero omitimos deliberadamente:
    // - CMD_TERMINAL (para que pueda derivar)
    // - CMD_EFIMERA (para que no se vuelva fantasma)
    // - CMD_ANCLA y CMD_ESTRICTA (para no asfixiar sus topologías)
    let poderes_dios = 
        CMD_LEER_SELF | CMD_LEER_ANY | 
        CMD_ESCRIBIR_SELF | CMD_ESCRIBIR_ANY | 
        CMD_BORRAR_SELF | CMD_BORRAR_ANY | 
        CMD_DERIVAR | CMD_COMBINAR | 
        CMD_DOMINANTE | CMD_INMORTAL | 
        CMD_CONGELAMIENTO | CMD_EJECUTOR | 
        CMD_LEER_LIBRE;

    let celula_genesis = Cell {
        salt: salt_genesis,
        challenge: challenge_genesis,
        index_l: 0,
        index_r: 0,
        cmd: poderes_dios,
        padding: [0u8; 20],
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    db_writer_tx.send(DbMessage::Append { data: celula_genesis.to_bytes(), responder: tx }).await.unwrap();
    rx.await.unwrap().unwrap();
    println!("¡Génesis creada exitosamente! Tu secreto (llave) es la frase 'diarsaba'.");
}