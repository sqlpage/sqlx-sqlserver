use bytes::Bytes;

use super::read::{consume, read_b_varchar, read_u16_le, read_u32_le, ReadError};
use super::type_info::TypeInfo;
use crate::{MssqlColumn, MssqlTypeInfo};

#[derive(Debug)]
pub(crate) struct ColMetaData {
    count: usize,
    columns: Vec<MssqlColumn>,
}

impl ColMetaData {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, ReadError> {
        let count = read_u16_le(input)?;
        let count = if count == 0xffff {
            0
        } else {
            usize::from(count)
        };
        Ok(Self {
            count,
            columns: Vec::new(),
        })
    }

    pub(crate) fn read(&mut self, input: &mut Bytes) -> Result<Vec<MssqlColumn>, ReadError> {
        while self.columns.len() < self.count {
            let column = consume(input, |src| {
                let _user_type = read_u32_le(src)?;
                let _flags = read_u16_le(src)?;
                let type_info = TypeInfo::get(src)?;
                let name = read_b_varchar(src)?;
                Ok(MssqlColumn::new(
                    self.columns.len(),
                    name,
                    MssqlTypeInfo::from_protocol(&type_info),
                ))
            })?;
            self.columns.push(column);
        }
        Ok(std::mem::take(&mut self.columns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_core::column::Column;

    #[test]
    fn parses_column_metadata() {
        let input = &[
            1,
            0, // count
            0,
            0,
            0,
            0, // user type
            0,
            0, // flags
            super::super::type_info::DataType::Int as u8,
            6,
            b'a',
            0,
            b'n',
            0,
            b's',
            0,
            b'w',
            0,
            b'e',
            0,
            b'r',
            0,
        ][..];

        let mut bytes = Bytes::copy_from_slice(input);
        let mut columns = consume(&mut bytes, ColMetaData::get).unwrap();
        let columns = columns.read(&mut bytes).unwrap();

        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].name(), "answer");
    }
}
