use core::fmt::Display;
use core::future::Future;

use crate::protocol::{build_header, DiarsabaHeader, DOMAIN_DB, FLAG_ERROR};

#[cfg(feature = "correspondence")]
use correspondence::CellEngine;

const DB_REQUEST_PREFIX_LEN: usize = 37;

#[cfg(not(feature = "correspondence"))]
pub trait CellEngineLike {
    type Error: Display;

    fn ejecutar(
        &self,
        target_index: u32,
        solucion: &[u8; 32],
        opcode: u8,
        params: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

#[cfg(feature = "correspondence")]
pub trait CellEngineLike {
    type Error: Display;

    fn ejecutar(
        &self,
        target_index: u32,
        solucion: &[u8; 32],
        opcode: u8,
        params: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

#[cfg(feature = "correspondence")]
impl CellEngineLike for CellEngine {
    type Error = correspondence::CellError;

    async fn ejecutar(
        &self,
        target_index: u32,
        solucion: &[u8; 32],
        opcode: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        self.ejecutar(target_index, solucion, opcode, params).await
    }
}

pub async fn handle_db_request<E>(
    header: &DiarsabaHeader,
    payload: &[u8],
    engine: &E,
) -> Vec<u8>
where
    E: CellEngineLike,
{
    let Some((&opcode, rest)) = payload.split_first() else {
        return build_error_response(header.req_id, "payload too short for db request");
    };

    if payload.len() < DB_REQUEST_PREFIX_LEN {
        return build_error_response(header.req_id, "payload too short for db request");
    }

    let target_index = match rest
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
    {
        Some(value) => value,
        None => return build_error_response(header.req_id, "invalid target_index bytes"),
    };

    let solucion = match rest.get(4..36).and_then(|bytes| bytes.try_into().ok()) {
        Some(value) => value,
        None => return build_error_response(header.req_id, "invalid solucion bytes"),
    };

    let params = match rest.get(36..) {
        Some(value) => value,
        None => return build_error_response(header.req_id, "invalid params slice"),
    };

    match engine.ejecutar(target_index, &solucion, opcode, params).await {
        Ok(result_bytes) => build_success_response(header.req_id, result_bytes),
        Err(error) => build_error_response(header.req_id, &error.to_string()),
    }
}

fn build_success_response(req_id: u32, payload: Vec<u8>) -> Vec<u8> {
    match u16::try_from(payload.len()) {
        Ok(payload_len) => build_response(DiarsabaHeader {
            domain: DOMAIN_DB,
            flags: 0x00,
            req_id,
            payload_len,
        }, payload),
        Err(_) => build_error_response(req_id, "response payload exceeds u16 length"),
    }
}

fn build_error_response(req_id: u32, message: &str) -> Vec<u8> {
    let payload = message.as_bytes();
    let payload_len = u16::try_from(payload.len()).unwrap_or(u16::MAX);
    let payload = if payload.len() > usize::from(u16::MAX) {
        payload[..usize::from(u16::MAX)].to_vec()
    } else {
        payload.to_vec()
    };

    build_response(
        DiarsabaHeader {
            domain: DOMAIN_DB,
            flags: FLAG_ERROR,
            req_id,
            payload_len,
        },
        payload,
    )
}

fn build_response(header: DiarsabaHeader, payload: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&build_header(&header));
    frame.extend_from_slice(&payload);
    frame
}