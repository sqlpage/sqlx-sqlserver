use bytes::{Buf, Bytes, BytesMut};
use sqlx_core::Error;
use std::sync::Arc;

use super::col_meta_data::ColMetaData;
use super::done::{Done, Status};
use super::packet::{encode_message, PacketFrameError, PacketType};
use super::read::{read_len_prefixed, read_u32_le, read_u8, ReadError};
use super::return_value::ReturnValue;
use super::row::Row;
use super::token::{parse_env_change, parse_server_error, EnvChange};
use crate::{error::server_error, MssqlColumn, MssqlQueryResult, MssqlRow, MssqlValue};

const TOKEN_COL_METADATA: u8 = 0x81;
const TOKEN_ORDER: u8 = 0xa9;
const TOKEN_ERROR: u8 = 0xaa;
const TOKEN_INFO: u8 = 0xab;
const TOKEN_RETURN_STATUS: u8 = 0x79;
const TOKEN_RETURN_VALUE: u8 = 0xac;
const TOKEN_ROW: u8 = 0xd1;
const TOKEN_NBCROW: u8 = 0xd2;
const TOKEN_ENVCHANGE: u8 = 0xe3;
const TOKEN_DONE: u8 = 0xfd;
const TOKEN_DONEPROC: u8 = 0xfe;
const TOKEN_DONEINPROC: u8 = 0xff;

#[derive(Debug)]
pub(crate) enum QueryEvent {
    Columns(Vec<MssqlColumn>),
    Row(MssqlRow),
    Result(MssqlQueryResult),
    ReturnValue(MssqlValue),
    EnvChange(EnvChange),
}

#[derive(Debug)]
enum PartialToken {
    Columns(ColMetaData),
    Row(Row),
    ReturnValue(ReturnValue),
}

#[derive(Debug, Default)]
pub(crate) struct QueryEvents {
    payload: Bytes,
    partial: Option<PartialToken>,
    columns: Arc<[MssqlColumn]>,
}

impl QueryEvents {
    #[cfg(test)]
    fn new(payload: impl Into<Bytes>) -> Self {
        let mut events = Self::default();
        events.push_packet(payload.into());
        events
    }

    pub(crate) fn push_packet(&mut self, packet: Bytes) {
        if self.payload.is_empty() {
            self.payload = packet;
        } else {
            // Only an incomplete field crosses this packet boundary.
            let mut joined = BytesMut::from(std::mem::take(&mut self.payload));
            joined.extend_from_slice(&packet);
            self.payload = joined.freeze();
        }
    }

    pub(crate) fn finish(&self) -> Result<(), Error> {
        if self.partial.is_some() || !self.payload.is_empty() {
            return Err(Error::Protocol(ReadError::UnexpectedEof.to_string()));
        }
        Ok(())
    }

    pub(crate) fn next(&mut self) -> Result<Option<QueryEvent>, Error> {
        loop {
            match self.read_token() {
                Ok(None) => continue,
                Ok(Some(event)) => return Ok(Some(event)),
                Err(ReadError::UnexpectedEof) => return Ok(None),
                Err(ReadError::Database(error)) => return Err(server_error(error)),
                Err(error) => return Err(Error::Protocol(error.to_string())),
            }
        }
    }

    fn read_token(&mut self) -> Result<Option<QueryEvent>, ReadError> {
        if let Some(token) = &mut self.partial {
            let event = match token {
                PartialToken::Columns(metadata) => {
                    let columns = metadata.read(&mut self.payload)?;
                    self.columns = columns.clone().into();
                    QueryEvent::Columns(columns)
                }
                PartialToken::Row(row) => {
                    QueryEvent::Row(row.read(&mut self.payload, &self.columns)?)
                }
                PartialToken::ReturnValue(value) => {
                    QueryEvent::ReturnValue(value.read(&mut self.payload)?)
                }
            };
            self.partial = None;
            return Ok(Some(event));
        }

        let mut input = self.payload.as_ref();
        let token = read_u8(&mut input)?;
        let event = match token {
            TOKEN_COL_METADATA => {
                self.partial = Some(PartialToken::Columns(ColMetaData::get(&mut input)?));
                None
            }
            TOKEN_ROW | TOKEN_NBCROW => {
                self.partial = Some(PartialToken::Row(Row::get(
                    &mut input,
                    token == TOKEN_NBCROW,
                    self.columns.len(),
                )?));
                None
            }
            TOKEN_RETURN_VALUE => {
                self.partial = Some(PartialToken::ReturnValue(ReturnValue::get(&mut input)?));
                None
            }
            TOKEN_RETURN_STATUS => {
                let _ = read_u32_le(&mut input)?;
                None
            }
            TOKEN_DONE | TOKEN_DONEPROC | TOKEN_DONEINPROC => {
                let done = Done::get(&mut input)?;
                done.status
                    .contains(Status::DONE_COUNT)
                    .then(|| QueryEvent::Result(MssqlQueryResult::new(done.affected_rows)))
            }
            TOKEN_ERROR => {
                let error = parse_server_error(read_len_prefixed(&mut input)?)?;
                self.payload.advance(self.payload.len() - input.len());
                return Err(ReadError::Database(error));
            }
            TOKEN_ENVCHANGE => Some(QueryEvent::EnvChange(parse_env_change(read_len_prefixed(
                &mut input,
            )?)?)),
            TOKEN_INFO | TOKEN_ORDER => {
                let _ = read_len_prefixed(&mut input)?;
                None
            }
            other => return Err(ReadError::UnsupportedToken(other)),
        };
        self.payload.advance(self.payload.len() - input.len());
        Ok(event)
    }
}

pub(crate) fn build_sql_batch_packet(
    sql: &str,
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, PacketFrameError> {
    let mut payload = Vec::with_capacity(22 + sql.len() * 2);
    write_all_headers(&mut payload, transaction_descriptor);

    for unit in sql.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }

    encode_message(PacketType::SQL_BATCH, &payload, packet_size)
}

pub(crate) fn write_all_headers(out: &mut Vec<u8>, transaction_descriptor: u64) {
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&18_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&transaction_descriptor.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mssql;
    use sqlx_core::row::Row;
    use sqlx_core::value::{Value, ValueRef};

    #[test]
    fn sql_batch_packet_starts_with_all_headers_and_utf16_sql() {
        let packet = build_sql_batch_packet("SELECT 1", 512, 0).unwrap();
        let payload = &packet[8..];

        assert_eq!(22, u32::from_le_bytes(payload[0..4].try_into().unwrap()));
        assert_eq!(18, u32::from_le_bytes(payload[4..8].try_into().unwrap()));
        assert_eq!(2, u16::from_le_bytes(payload[8..10].try_into().unwrap()));
        assert_eq!(0, u64::from_le_bytes(payload[10..18].try_into().unwrap()));
        assert_eq!(1, u32::from_le_bytes(payload[18..22].try_into().unwrap()));
        assert_eq!(
            &[b'S', 0, b'E', 0, b'L', 0, b'E', 0, b'C', 0, b'T', 0, b' ', 0, b'1', 0],
            &payload[22..]
        );
    }

    #[test]
    fn sql_batch_packet_writes_transaction_descriptor() {
        let packet = build_sql_batch_packet("SELECT 1", 512, 0x0102_0304_0506_0708).unwrap();
        let payload = &packet[8..];

        assert_eq!(
            0x0102_0304_0506_0708,
            u64::from_le_bytes(payload[10..18].try_into().unwrap())
        );
    }

    #[test]
    fn parses_select_one_response() {
        let response = [col_metadata_int(""), row_int(1), done(0x10, 0, 1)].concat();
        let events = parse_events(response).unwrap();
        let [QueryEvent::Columns(_), QueryEvent::Row(row), QueryEvent::Result(result)] =
            &events[..]
        else {
            panic!("unexpected SELECT response: {events:?}");
        };
        assert_eq!(1, result.rows_affected());
        assert_eq!(1_i32, row.try_get::<i32, _>(0).unwrap());
    }

    #[test]
    fn skips_order_metadata_before_rows() {
        let response = [
            col_metadata_int("value"),
            vec![TOKEN_ORDER, 2, 0, 1, 0],
            row_int(1),
            done(0x10, 0, 1),
        ]
        .concat();
        let rows = parse_rows(&response);

        assert_eq!(1, rows.len());
        assert_eq!(1_i32, rows[0].try_get::<i32, _>(0).unwrap());
    }

    #[test]
    fn parses_variable_length_int_response() {
        let response = [col_metadata_intn(""), row_intn(7), done(0x10, 0, 1)].concat();
        let rows = parse_rows(&response);

        assert_eq!(7_i32, rows[0].try_get::<i32, _>(0).unwrap());
    }

    #[test]
    fn parses_null_typed_value_as_null() {
        let response = [col_metadata_null("value"), row_null(), done(0x10, 0, 1)].concat();
        let rows = parse_rows(&response);

        assert!(rows[0].try_get_raw(0).unwrap().is_null());
    }

    #[test]
    fn parses_nbcrow_null_bitmap() {
        let response = [col_metadata_intn("value"), nbcrow_null(1), done(0x10, 0, 1)].concat();
        let rows = parse_rows(&response);

        assert!(rows[0].try_get_raw(0).unwrap().is_null());
    }

    #[test]
    fn parses_return_value_response() {
        let response = [return_status(0), return_value_int(42), done(0x10, 0, 1)].concat();
        let events = parse_events(response).unwrap();
        let [QueryEvent::ReturnValue(value), QueryEvent::Result(_)] = &events[..] else {
            panic!("unexpected RPC response: {events:?}");
        };
        assert_eq!(
            42_i32,
            <i32 as sqlx_core::decode::Decode<Mssql>>::decode(value.as_ref()).unwrap()
        );
    }

    #[test]
    fn collects_envchange_tokens_from_query_response() {
        let response = [
            env_change(4, &[4, b'8', 0, b'1', 0, b'9', 0, b'2', 0]),
            env_change(8, &[8, 8, 7, 6, 5, 4, 3, 2, 1]),
            done(0, 0, 0),
        ]
        .concat();
        assert!(matches!(
            parse_events(response).unwrap().as_slice(),
            [
                QueryEvent::EnvChange(EnvChange::PacketSize(8192)),
                QueryEvent::EnvChange(EnvChange::BeginTransaction(0x0102_0304_0506_0708))
            ]
        ));
    }

    #[test]
    fn parses_error_token_as_database_error() {
        let response = [error(208, 1, 16, "Invalid object name", "dbhost", "", 3)].concat();
        let error = parse_events(response).unwrap_err();
        let db_error = error.as_database_error().unwrap();
        let mssql_error = db_error
            .as_error()
            .downcast_ref::<crate::MssqlDatabaseError>()
            .unwrap();

        assert_eq!(208, mssql_error.number());
        assert_eq!("Invalid object name", mssql_error.message());
        assert_eq!("dbhost", mssql_error.server_name());
        assert_eq!(3, mssql_error.line_number());
    }

    #[test]
    fn reads_end_of_batch_after_an_error() {
        let response = [
            error(208, 1, 16, "Invalid object name", "dbhost", "", 3),
            done(0, 0, 0),
        ]
        .concat();
        let mut events = QueryEvents::new(response);

        assert!(matches!(events.next(), Err(Error::Database(_))));
        assert!(events.next().unwrap().is_none());
    }

    #[test]
    fn reads_columns_rows_and_chunked_values_across_packet_boundaries() {
        use crate::protocol::type_info::DataType;

        // One fixed integer followed by a varbinary(max) value of unknown length.
        let mut batch = col_metadata_int("id");
        batch[1] = 2;
        batch.extend_from_slice(&[0; 6]);
        batch.extend_from_slice(&[DataType::BigVarBinary as u8, 0xff, 0xff]);
        push_b_varchar(&mut batch, "blob");
        batch.extend_from_slice(&[TOKEN_NBCROW, 0]);
        batch.extend_from_slice(&42_i32.to_le_bytes());
        batch.extend_from_slice(&(u64::MAX - 1).to_le_bytes());
        let blob = (0..240).cycle().take(8192).collect::<Vec<u8>>();
        for part in blob.chunks(67) {
            batch.extend_from_slice(&(part.len() as u32).to_le_bytes());
            batch.extend_from_slice(part);
        }
        batch.extend_from_slice(&0_u32.to_le_bytes());
        batch.extend_from_slice(&done(0x10, 0, 1));

        let mut events = QueryEvents::default();
        let mut rows = Vec::new();
        let mut results = Vec::new();
        for packet in batch.chunks(3) {
            events.push_packet(Bytes::copy_from_slice(packet));
            while let Some(event) = events.next().unwrap() {
                match event {
                    QueryEvent::Row(row) => rows.push(row),
                    QueryEvent::Result(result) => results.push(result.rows_affected()),
                    _ => (),
                }
            }
            // Finished PLP chunks have already been consumed.
            assert!(events.payload.len() < 4 + 67);
        }
        events.finish().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].try_get::<i32, _>("id").unwrap(), 42);
        assert_eq!(rows[0].try_get::<&[u8], _>("blob").unwrap(), blob);
        assert_eq!(results, [1]);
    }

    #[test]
    fn reports_incomplete_tokens_at_end_of_message() {
        let int_column = col_metadata_int("value");
        for partial in [
            &int_column[..4],
            &[TOKEN_DONE, 0][..],
            &[TOKEN_RETURN_VALUE][..],
        ] {
            let mut events = QueryEvents::new(partial.to_vec());
            assert!(events.next().unwrap().is_none());
            assert!(events.finish().is_err());
        }
        let mut events = QueryEvents::new([int_column, vec![TOKEN_ROW]].concat());
        assert!(matches!(
            events.next().unwrap(),
            Some(QueryEvent::Columns(_))
        ));
        assert!(events.next().unwrap().is_none());
        assert!(events.finish().is_err());
    }

    #[test]
    fn can_drain_state_changes_after_a_server_error() {
        let mut events = QueryEvents::new(
            [
                error(208, 1, 16, "Invalid object name", "dbhost", "", 3),
                env_change(8, &[8, 8, 7, 6, 5, 4, 3, 2, 1]),
            ]
            .concat(),
        );
        assert!(matches!(events.next(), Err(Error::Database(_))));
        assert!(matches!(
            events.next().unwrap(),
            Some(QueryEvent::EnvChange(EnvChange::BeginTransaction(
                0x0102_0304_0506_0708
            )))
        ));
        assert!(events.next().unwrap().is_none());
        events.finish().unwrap();
    }

    #[test]
    fn malformed_length_prefixed_token_fails_immediately() {
        let mut events = QueryEvents::new(vec![TOKEN_ERROR, 1, 0, 0]);
        assert!(matches!(events.next(), Err(Error::Protocol(_))));
    }

    fn col_metadata_int(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::Int as u8);
        push_b_varchar(&mut out, name);
        out
    }

    fn col_metadata_intn(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::IntN as u8);
        out.push(4);
        push_b_varchar(&mut out, name);
        out
    }

    fn col_metadata_null(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::Null as u8);
        push_b_varchar(&mut out, name);
        out
    }

    fn row_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_ROW);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn row_intn(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_ROW);
        out.push(4);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn row_null() -> Vec<u8> {
        vec![TOKEN_ROW]
    }

    fn nbcrow_null(column_count: usize) -> Vec<u8> {
        let mut out = vec![TOKEN_NBCROW];
        out.resize(1 + column_count.div_ceil(8), 0xff);
        out
    }

    fn return_value_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_RETURN_VALUE);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.push(0);
        out.push(1);
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::IntN as u8);
        out.push(4);
        out.push(4);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn return_status(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_RETURN_STATUS);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn env_change(change_type: u8, data: &[u8]) -> Vec<u8> {
        let len = 1 + data.len();
        let mut out = Vec::new();
        out.push(TOKEN_ENVCHANGE);
        out.extend_from_slice(&u16::try_from(len).unwrap().to_le_bytes());
        out.push(change_type);
        out.extend_from_slice(data);
        out
    }

    fn error(
        number: i32,
        state: u8,
        class: u8,
        message: &str,
        server: &str,
        procedure: &str,
        line: u32,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&number.to_le_bytes());
        payload.push(state);
        payload.push(class);
        push_us_varchar(&mut payload, message);
        push_b_varchar(&mut payload, server);
        push_b_varchar(&mut payload, procedure);
        payload.extend_from_slice(&line.to_le_bytes());

        let mut out = Vec::new();
        out.push(TOKEN_ERROR);
        out.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn parse_events(response: Vec<u8>) -> Result<Vec<QueryEvent>, Error> {
        let mut decoder = QueryEvents::new(response);
        let mut events = Vec::new();
        while let Some(event) = decoder.next()? {
            events.push(event);
        }
        decoder.finish()?;
        Ok(events)
    }

    fn parse_rows(response: &[u8]) -> Vec<MssqlRow> {
        parse_events(response.to_vec())
            .unwrap()
            .into_iter()
            .filter_map(|event| match event {
                QueryEvent::Row(row) => Some(row),
                _ => None,
            })
            .collect()
    }

    fn done(status: u16, current_command: u16, row_count: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_DONE);
        out.extend_from_slice(&status.to_le_bytes());
        out.extend_from_slice(&current_command.to_le_bytes());
        out.extend_from_slice(&row_count.to_le_bytes());
        out
    }

    fn push_us_varchar(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(
            &u16::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes(),
        );
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn push_b_varchar(out: &mut Vec<u8>, value: &str) {
        out.push(u8::try_from(value.encode_utf16().count()).unwrap());
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
}
