use bytes::Bytes;

use super::read::{read_b_varchar, read_u16_le, read_u32_le, read_u8, ReadError};
use super::type_info::{TypeInfo, ValueLength};
use crate::{MssqlTypeInfo, MssqlValue};

#[derive(Debug)]
pub(crate) struct ReturnValue {
    type_info: TypeInfo,
    length: Option<ValueLength>,
}

impl ReturnValue {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, ReadError> {
        let _ordinal = read_u16_le(input)?;
        let _name = read_b_varchar(input)?;
        let _status = read_u8(input)?;
        let _user_type = read_u32_le(input)?;
        let _flags = read_u16_le(input)?;
        Ok(Self {
            type_info: TypeInfo::get(input)?,
            length: None,
        })
    }

    pub(crate) fn read(&mut self, input: &mut Bytes) -> Result<MssqlValue, ReadError> {
        let value = ValueLength::read(&mut self.length, input, &self.type_info)?;
        Ok(MssqlValue::new(
            MssqlTypeInfo::from_protocol(&self.type_info),
            value,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::type_info::DataType;
    use crate::Mssql;
    use sqlx_core::decode::Decode;
    use sqlx_core::value::Value;

    #[test]
    fn parses_return_value() {
        let payload = Bytes::copy_from_slice(&[
            0,
            0, // ordinal
            0, // name
            1, // status
            0,
            0,
            0,
            0, // user type
            0,
            0, // flags
            DataType::IntN as u8,
            4,
            4,
            1,
            0,
            0,
            0,
        ]);
        let mut input = payload;

        let mut field = super::super::read::consume(&mut input, ReturnValue::get).unwrap();
        let return_value = field.read(&mut input).unwrap();

        assert_eq!(
            <i32 as Decode<Mssql>>::decode(return_value.as_ref()).unwrap(),
            1
        );
    }
}
