use super::token::{ServerError, TokenParseError};
use super::type_info::DataType;
use bytes::{Buf, Bytes};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ReadError {
    #[error("SQL Server query token ended before expected length")]
    UnexpectedEof,
    #[error("unknown TDS data type 0x{0:02x}")]
    UnknownDataType(u8),
    #[error("unsupported TDS data type {0:?}")]
    UnsupportedDataType(DataType),
    #[error("invalid scale {scale} for type {ty:?}")]
    InvalidScale { ty: DataType, scale: u8 },
    #[error("TDS value declared {declared} bytes but contained {actual} bytes")]
    InvalidPlpLength { declared: u64, actual: usize },
    #[error("SQL Server string contained invalid UTF-16")]
    InvalidUtf16,
    #[error("unsupported SQL Server query token 0x{0:02x}")]
    UnsupportedToken(u8),
    #[error("missing protocol type information for SQL Server column")]
    MissingTypeInfo,
    #[error(transparent)]
    InvalidToken(#[from] TokenParseError),
    #[error("SQL Server returned an error: {0:?}")]
    Database(ServerError),
}

/// Advance past a field only after its entire header or payload is available.
pub(crate) fn consume<T>(
    bytes: &mut Bytes,
    parse: impl FnOnce(&mut &[u8]) -> Result<T, ReadError>,
) -> Result<T, ReadError> {
    let mut input = bytes.as_ref();
    let value = parse(&mut input)?;
    bytes.advance(bytes.len() - input.len());
    Ok(value)
}

pub(crate) fn read_u8(input: &mut &[u8]) -> Result<u8, ReadError> {
    Ok(take(input, 1)?[0])
}

pub(crate) fn read_u16_le(input: &mut &[u8]) -> Result<u16, ReadError> {
    let bytes = take(input, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(crate) fn read_u32_le(input: &mut &[u8]) -> Result<u32, ReadError> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub(crate) fn read_u64_le(input: &mut &[u8]) -> Result<u64, ReadError> {
    let bytes = take(input, 8)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

pub(crate) fn read_len_prefixed<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], ReadError> {
    let len = usize::from(read_u16_le(input)?);
    take(input, len)
}

pub(crate) fn read_b_varchar(input: &mut &[u8]) -> Result<String, ReadError> {
    let len_chars = usize::from(read_u8(input)?);
    read_utf16(input, len_chars)
}

fn read_utf16(input: &mut &[u8], len_chars: usize) -> Result<String, ReadError> {
    let len_bytes = len_chars * 2;
    let bytes = take(input, len_bytes)?;
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .collect::<Vec<_>>();

    String::from_utf16(&units).map_err(|_| ReadError::InvalidUtf16)
}

pub(crate) fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], ReadError> {
    let bytes = input.get(..len).ok_or(ReadError::UnexpectedEof)?;
    *input = &input[len..];
    Ok(bytes)
}
