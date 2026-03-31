use serde::{Deserialize, Serialize};

/// Define todas las operaciones permitidas en la red y en la base de datos.
/// #[repr(u8)] nos asegura que en memoria (y al serializar) ocupe exactamente 1 byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    /// Operación inicial del nodo o red
    Genesis = 0x00,
    /// Mutación: Insertar una nueva célula de datos
    WriteCell = 0x01,
    /// Mutación: Actualizar un índice o apuntador
    UpdateIndex = 0x02,
    /// Lectura/Query
    ReadCell = 0x10,
    /// Operación de control P2P (Ping/Pong)
    Ping = 0xFF,
}

impl TryFrom<u8> for Opcode {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Opcode::Genesis),
            0x01 => Ok(Opcode::WriteCell),
            0x02 => Ok(Opcode::UpdateIndex),
            0x10 => Ok(Opcode::ReadCell),
            0xFF => Ok(Opcode::Ping),
            _ => Err("Opcode desconocido o no soportado"),
        }
    }
}