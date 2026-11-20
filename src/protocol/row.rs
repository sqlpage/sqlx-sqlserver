use bytes::Bytes;
use sqlx_core::column::Column;
use std::sync::Arc;

use super::read::{take, ReadError};
use super::type_info::ValueLength;
use crate::{MssqlColumn, MssqlRow, MssqlValue};

#[derive(Debug)]
pub(crate) struct Row {
    nulls: Vec<u8>,
    values: Vec<MssqlValue>,
    length: Option<ValueLength>,
}

impl Row {
    pub(crate) fn get(
        input: &mut &[u8],
        nullable: bool,
        columns: usize,
    ) -> Result<Self, ReadError> {
        let nulls = if nullable {
            take(input, columns.div_ceil(8))?.to_vec()
        } else {
            Vec::new()
        };
        Ok(Self {
            nulls,
            values: Vec::with_capacity(columns),
            length: None,
        })
    }

    pub(crate) fn read(
        &mut self,
        input: &mut Bytes,
        columns: &Arc<[MssqlColumn]>,
    ) -> Result<MssqlRow, ReadError> {
        for (index, column) in columns.iter().enumerate().skip(self.values.len()) {
            let ty = column.type_info();
            let value = if !self.nulls.is_empty() && self.nulls[index / 8] & (1 << (index % 8)) != 0
            {
                None
            } else {
                let metadata = ty.protocol_type_info().ok_or(ReadError::MissingTypeInfo)?;
                ValueLength::read(&mut self.length, input, metadata)?
            };
            self.values.push(MssqlValue::new(ty.clone(), value));
        }
        Ok(MssqlRow::new_shared(
            Arc::clone(columns),
            std::mem::take(&mut self.values),
        ))
    }
}
