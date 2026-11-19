#[cfg(feature = "bigdecimal")]
use bigdecimal::BigDecimal;
#[cfg(feature = "chrono")]
use chrono::{
    DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Offset, Timelike, Utc,
};
#[cfg(feature = "bigdecimal")]
use num_bigint::{BigInt, Sign};
#[cfg(feature = "decimal")]
use rust_decimal::Decimal;
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};
#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
use sqlx_core::decode::Decode;
#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
use sqlx_core::encode::{Encode, IsNull};
#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
use sqlx_core::error::BoxDynError;
#[cfg(feature = "json")]
use sqlx_core::types::Json;
#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
use sqlx_core::types::Type;
#[cfg(feature = "uuid")]
use uuid::Uuid;

#[cfg(any(feature = "bigdecimal", feature = "decimal"))]
use crate::decimal_tools::{decode_money_bytes, decode_numeric_bytes};
#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
use crate::{Mssql, MssqlType, MssqlTypeInfo, MssqlValueRef};

#[cfg(feature = "decimal")]
impl Type<Mssql> for Decimal {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DECIMAL
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Decimal | MssqlType::Money)
    }
}

#[cfg(feature = "decimal")]
impl Encode<'_, Mssql> for Decimal {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        Some(MssqlTypeInfo::decimal_with_scale(
            u8::try_from(self.scale()).unwrap_or(u8::MAX),
        ))
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.push(if self.is_sign_negative() { 0 } else { 1 });
        let mantissa = if self.scale() <= u32::from(u8::MAX) {
            self.mantissa().saturating_abs() as u128
        } else {
            0
        };
        buf.extend_from_slice(&mantissa.to_le_bytes());
        Ok(IsNull::No)
    }
}

#[cfg(feature = "decimal")]
impl Decode<'_, Mssql> for Decimal {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "Decimal")?;

        match value.mssql_type_info().kind() {
            MssqlType::Decimal => {
                let (sign, numerator) = decode_numeric_bytes(bytes)?;
                let signed_numerator = i128::try_from(numerator)? * i128::from(sign);

                Ok(Decimal::try_from_i128_with_scale(
                    signed_numerator,
                    u32::from(value.mssql_type_info().scale()),
                )?)
            }
            MssqlType::Money => Ok(Decimal::new(decode_money_bytes(bytes)?, 4)),
            other => Err(format!("expected SQL Server numeric type, got {other:?}").into()),
        }
    }
}

#[cfg(feature = "bigdecimal")]
impl Type<Mssql> for BigDecimal {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DECIMAL
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Decimal | MssqlType::Money)
    }
}

#[cfg(feature = "bigdecimal")]
impl Encode<'_, Mssql> for BigDecimal {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        let (_, exponent) = self.as_bigint_and_exponent();
        Some(MssqlTypeInfo::decimal_with_scale(
            u8::try_from(exponent).unwrap_or(0),
        ))
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        let (mut bigint, exponent) = self.as_bigint_and_exponent();
        buf.push(if bigint.sign() == Sign::Minus { 0 } else { 1 });

        if exponent < 0 {
            if let Ok(abs_exponent) = u32::try_from(-exponent) {
                bigint *= BigInt::from(10).pow(abs_exponent);
            }
        }

        let (_, bytes) = bigint.to_bytes_le();
        let mut mantissa = [0_u8; 16];
        if exponent <= i64::from(u8::MAX) && bytes.len() <= mantissa.len() {
            mantissa[..bytes.len()].copy_from_slice(&bytes);
        }

        buf.extend_from_slice(&mantissa);
        Ok(IsNull::No)
    }
}

#[cfg(feature = "bigdecimal")]
impl Decode<'_, Mssql> for BigDecimal {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "BigDecimal")?;

        match value.mssql_type_info().kind() {
            MssqlType::Decimal => {
                let (sign, numerator) = decode_numeric_bytes(bytes)?;
                let numerator = if sign < 0 {
                    -BigInt::from(numerator)
                } else {
                    BigInt::from(numerator)
                };

                Ok(BigDecimal::new(
                    numerator,
                    i64::from(value.mssql_type_info().scale()),
                ))
            }
            MssqlType::Money => Ok(BigDecimal::new(BigInt::from(decode_money_bytes(bytes)?), 4)),
            other => Err(format!("expected SQL Server numeric type, got {other:?}").into()),
        }
    }
}

#[cfg(feature = "json")]
impl<T> Type<Mssql> for Json<T> {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::VARBINARY
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::VarBinary | MssqlType::VarChar)
    }
}

#[cfg(feature = "json")]
impl<T> Encode<'_, Mssql> for Json<T>
where
    T: Serialize,
{
    fn produces(&self) -> Option<MssqlTypeInfo> {
        Some(MssqlTypeInfo::with_size(MssqlType::VarBinary, u16::MAX))
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        serde_json::to_writer(buf, &self.0)?;
        Ok(IsNull::No)
    }
}

#[cfg(feature = "json")]
impl<'r, T> Decode<'r, Mssql> for Json<T>
where
    T: Deserialize<'r>,
{
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(Json(serde_json::from_slice(non_null_bytes(
            value, "Json",
        )?)?))
    }
}

#[cfg(feature = "uuid")]
impl Type<Mssql> for Uuid {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::UNIQUEIDENTIFIER
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::UniqueIdentifier)
    }
}

#[cfg(feature = "uuid")]
impl Encode<'_, Mssql> for Uuid {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_bytes_le());
        Ok(IsNull::No)
    }
}

#[cfg(feature = "uuid")]
impl Decode<'_, Mssql> for Uuid {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = <[u8; 16]>::try_from(non_null_bytes(value, "Uuid")?)?;
        Ok(Uuid::from_bytes_le(bytes))
    }
}

#[cfg(feature = "chrono")]
impl Type<Mssql> for NaiveDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATETIME2
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::DateTime | MssqlType::DateTime2 | MssqlType::DateTimeOffset
        )
    }
}

#[cfg(feature = "chrono")]
impl Type<Mssql> for NaiveDate {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATE
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Date)
    }
}

#[cfg(feature = "chrono")]
impl Type<Mssql> for NaiveTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::TIME
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Time)
    }
}

#[cfg(feature = "chrono")]
impl<T> Type<Mssql> for DateTime<T>
where
    T: chrono::TimeZone,
{
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATETIMEOFFSET
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::DateTime | MssqlType::DateTimeOffset)
    }
}

#[cfg(feature = "chrono")]
impl Encode<'_, Mssql> for NaiveDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_chrono_datetime2(self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "chrono")]
impl Encode<'_, Mssql> for NaiveDate {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_chrono_date(self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "chrono")]
impl Encode<'_, Mssql> for NaiveTime {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_chrono_time(self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "chrono")]
impl<T> Encode<'_, Mssql> for DateTime<T>
where
    T: chrono::TimeZone,
{
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_chrono_datetime2(&self.naive_utc()));
        let offset_minutes = self.offset().fix().local_minus_utc() / 60;
        buf.extend_from_slice(&i16::try_from(offset_minutes)?.to_le_bytes());
        Ok(IsNull::No)
    }
}

#[cfg(feature = "chrono")]
impl Decode<'_, Mssql> for NaiveDateTime {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "NaiveDateTime")?;
        match value.mssql_type_info().kind() {
            MssqlType::DateTime2 => decode_chrono_datetime2(value.mssql_type_info().scale(), bytes),
            _ => Ok(<DateTime<FixedOffset> as Decode<Mssql>>::decode(value)?.naive_local()),
        }
    }
}

#[cfg(feature = "chrono")]
impl Decode<'_, Mssql> for NaiveDate {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        decode_chrono_date(non_null_bytes(value, "NaiveDate")?)
    }
}

#[cfg(feature = "chrono")]
impl Decode<'_, Mssql> for NaiveTime {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        decode_chrono_time(
            value.mssql_type_info().scale(),
            non_null_bytes(value, "NaiveTime")?,
        )
    }
}

#[cfg(feature = "chrono")]
impl Decode<'_, Mssql> for DateTime<FixedOffset> {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "DateTime<FixedOffset>")?;
        let scale = value.mssql_type_info().scale();

        match value.mssql_type_info().kind() {
            MssqlType::DateTime if bytes.len() == 4 => decode_chrono_smalldatetime(bytes),
            MssqlType::DateTime if bytes.len() == 8 => decode_chrono_datetime(bytes),
            MssqlType::DateTimeOffset => decode_chrono_datetimeoffset(scale, bytes),
            other => Err(format!("unsupported SQL Server datetime type {other:?}").into()),
        }
    }
}

#[cfg(feature = "chrono")]
impl Decode<'_, Mssql> for DateTime<Utc> {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(<DateTime<FixedOffset> as Decode<Mssql>>::decode(value)?.with_timezone(&Utc))
    }
}

#[cfg(feature = "time")]
impl Type<Mssql> for time::PrimitiveDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATETIME2
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::DateTime2 | MssqlType::DateTimeOffset)
    }
}

#[cfg(feature = "time")]
impl Type<Mssql> for time::Date {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATE
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Date)
    }
}

#[cfg(feature = "time")]
impl Type<Mssql> for time::Time {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::TIME
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Time)
    }
}

#[cfg(feature = "time")]
impl Type<Mssql> for time::OffsetDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::DATETIMEOFFSET
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::DateTimeOffset)
    }
}

#[cfg(feature = "time")]
impl Encode<'_, Mssql> for time::Date {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_time_date(*self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "time")]
impl Encode<'_, Mssql> for time::Time {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_time_time(*self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "time")]
impl Encode<'_, Mssql> for time::PrimitiveDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&encode_time_datetime2(*self));
        Ok(IsNull::No)
    }
}

#[cfg(feature = "time")]
impl Encode<'_, Mssql> for time::OffsetDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        let utc = self
            .checked_to_offset(time::UtcOffset::UTC)
            .ok_or("SQL Server datetimeoffset has an out-of-range UTC date")?;
        let datetime = time::PrimitiveDateTime::new(utc.date(), utc.time());
        buf.extend_from_slice(&encode_time_datetime2(datetime));
        buf.extend_from_slice(&self.offset().whole_minutes().to_le_bytes());
        Ok(IsNull::No)
    }
}

#[cfg(feature = "time")]
impl Decode<'_, Mssql> for time::Date {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        decode_time_date(non_null_bytes(value, "time::Date")?)
    }
}

#[cfg(feature = "time")]
impl Decode<'_, Mssql> for time::Time {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        decode_time_time(
            value.mssql_type_info().scale(),
            non_null_bytes(value, "time::Time")?,
        )
    }
}

#[cfg(feature = "time")]
impl Decode<'_, Mssql> for time::PrimitiveDateTime {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "time::PrimitiveDateTime")?;
        match value.mssql_type_info().kind() {
            MssqlType::DateTime2 => decode_time_datetime2(value.mssql_type_info().scale(), bytes),
            MssqlType::DateTimeOffset => {
                let datetime = decode_time_datetimeoffset(value.mssql_type_info().scale(), bytes)?;
                Ok(Self::new(datetime.date(), datetime.time()))
            }
            other => Err(format!("unsupported SQL Server datetime type {other:?}").into()),
        }
    }
}

#[cfg(feature = "time")]
impl Decode<'_, Mssql> for time::OffsetDateTime {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = non_null_bytes(value, "time::OffsetDateTime")?;
        if !matches!(value.mssql_type_info().kind(), MssqlType::DateTimeOffset) {
            return Err("unsupported SQL Server datetimeoffset type".into());
        }

        decode_time_datetimeoffset(value.mssql_type_info().scale(), bytes)
    }
}

#[cfg(any(
    feature = "bigdecimal",
    feature = "chrono",
    feature = "decimal",
    feature = "json",
    feature = "time",
    feature = "uuid"
))]
fn non_null_bytes<'r>(value: MssqlValueRef<'r>, rust_type: &str) -> Result<&'r [u8], BoxDynError> {
    value
        .as_bytes()
        .ok_or_else(|| format!("cannot decode SQL Server NULL as {rust_type}").into())
}

#[cfg(any(feature = "chrono", feature = "time"))]
fn read_i24_le(bytes: &[u8]) -> i32 {
    let mut value = i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
    if value & 0x80_0000 != 0 {
        value |= !0x00ff_ffff;
    }
    value
}

#[cfg(any(feature = "chrono", feature = "time"))]
fn write_i24_le(value: i32) -> [u8; 3] {
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

#[cfg(feature = "chrono")]
fn encode_chrono_date(date: &NaiveDate) -> [u8; 3] {
    write_i24_le(date.num_days_from_ce() - 1)
}

#[cfg(feature = "chrono")]
fn encode_chrono_time(time: &NaiveTime) -> [u8; 5] {
    let total = u64::from(time.num_seconds_from_midnight()) * 10_000_000
        + u64::from(time.nanosecond() / 100);
    let bytes = total.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]]
}

#[cfg(feature = "chrono")]
fn encode_chrono_datetime2(datetime: &NaiveDateTime) -> [u8; 8] {
    let mut buf = [0_u8; 8];
    buf[..5].copy_from_slice(&encode_chrono_time(&datetime.time()));
    buf[5..].copy_from_slice(&encode_chrono_date(&datetime.date()));
    buf
}

#[cfg(feature = "chrono")]
fn decode_chrono_time(scale: u8, data: &[u8]) -> Result<NaiveTime, BoxDynError> {
    let mut acc = 0_u64;
    for byte in data.iter().rev() {
        acc <<= 8;
        acc |= u64::from(*byte);
    }

    acc *= 10_u64.pow(9 - u32::from(scale));
    let seconds = u32::try_from(acc / 1_000_000_000)?;
    let nanos = u32::try_from(acc % 1_000_000_000)?;

    NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos)
        .ok_or_else(|| format!("invalid time: seconds={seconds} nanoseconds={nanos}").into())
}

#[cfg(feature = "chrono")]
fn decode_chrono_date(bytes: &[u8]) -> Result<NaiveDate, BoxDynError> {
    let days_from_ce = read_i24_le(bytes);
    NaiveDate::from_num_days_from_ce_opt(days_from_ce + 1)
        .ok_or_else(|| format!("invalid days offset in date: {days_from_ce}").into())
}

#[cfg(feature = "chrono")]
fn decode_chrono_datetime2(scale: u8, bytes: &[u8]) -> Result<NaiveDateTime, BoxDynError> {
    let time_size = bytes.len() - 3;
    Ok(decode_chrono_date(&bytes[time_size..])?
        .and_time(decode_chrono_time(scale, &bytes[..time_size])?))
}

#[cfg(feature = "chrono")]
fn decode_chrono_datetime(bytes: &[u8]) -> Result<DateTime<FixedOffset>, BoxDynError> {
    let days = i32::from_le_bytes(bytes[..4].try_into()?);
    let ticks = u32::from_le_bytes(bytes[4..].try_into()?);
    let date = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap() + chrono::Duration::days(days.into());
    let datetime = date.and_time(NaiveTime::default())
        + chrono::Duration::milliseconds(i64::from(ticks) * 1000 / 300);
    Ok(datetime.and_utc().fixed_offset())
}

#[cfg(feature = "chrono")]
fn decode_chrono_smalldatetime(bytes: &[u8]) -> Result<DateTime<FixedOffset>, BoxDynError> {
    let days = u16::from_le_bytes(bytes[..2].try_into()?);
    let minutes = u16::from_le_bytes(bytes[2..].try_into()?);
    let date =
        NaiveDate::from_ymd_opt(1900, 1, 1).unwrap() + chrono::Duration::days(i64::from(days));
    let datetime =
        date.and_time(NaiveTime::default()) + chrono::Duration::minutes(i64::from(minutes));
    Ok(datetime.and_utc().fixed_offset())
}

#[cfg(feature = "chrono")]
fn decode_chrono_datetimeoffset(
    scale: u8,
    bytes: &[u8],
) -> Result<DateTime<FixedOffset>, BoxDynError> {
    let naive = decode_chrono_datetime2(scale, &bytes[..bytes.len() - 2])?;
    let offset_minutes = i16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]);
    let offset = FixedOffset::east_opt(i32::from(offset_minutes) * 60)
        .ok_or_else(|| format!("invalid offset {offset_minutes} in DateTimeOffset"))?;
    Ok(DateTime::from_naive_utc_and_offset(naive, offset))
}

#[cfg(feature = "time")]
fn date_one_ce_julian_day() -> i32 {
    time::Date::from_calendar_date(1, time::Month::January, 1)
        .unwrap()
        .to_julian_day()
}

#[cfg(feature = "time")]
fn encode_time_date(date: time::Date) -> [u8; 3] {
    write_i24_le(date.to_julian_day() - date_one_ce_julian_day())
}

#[cfg(feature = "time")]
fn encode_time_time(time: time::Time) -> [u8; 5] {
    let total = u64::from(time.hour()) * 36_000_000_000
        + u64::from(time.minute()) * 600_000_000
        + u64::from(time.second()) * 10_000_000
        + u64::from(time.nanosecond() / 100);
    let bytes = total.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]]
}

#[cfg(feature = "time")]
fn encode_time_datetime2(datetime: time::PrimitiveDateTime) -> [u8; 8] {
    let mut buf = [0_u8; 8];
    buf[..5].copy_from_slice(&encode_time_time(datetime.time()));
    buf[5..].copy_from_slice(&encode_time_date(datetime.date()));
    buf
}

#[cfg(feature = "time")]
fn decode_time_date(bytes: &[u8]) -> Result<time::Date, BoxDynError> {
    let days = read_i24_le(bytes);
    Ok(time::Date::from_julian_day(
        date_one_ce_julian_day() + days,
    )?)
}

#[cfg(feature = "time")]
fn decode_time_time(scale: u8, data: &[u8]) -> Result<time::Time, BoxDynError> {
    let mut acc = 0_u64;
    for byte in data.iter().rev() {
        acc <<= 8;
        acc |= u64::from(*byte);
    }

    acc *= 10_u64.pow(9 - u32::from(scale));
    let seconds = acc / 1_000_000_000;
    let nanos = u32::try_from(acc % 1_000_000_000)?;
    let hour = u8::try_from(seconds / 3600)?;
    let minute = u8::try_from((seconds % 3600) / 60)?;
    let second = u8::try_from(seconds % 60)?;

    Ok(time::Time::from_hms_nano(hour, minute, second, nanos)?)
}

#[cfg(feature = "time")]
fn decode_time_datetime2(scale: u8, bytes: &[u8]) -> Result<time::PrimitiveDateTime, BoxDynError> {
    let (time, date) = bytes
        .split_last_chunk::<3>()
        .ok_or("invalid SQL Server datetime2 length")?;
    Ok(time::PrimitiveDateTime::new(
        decode_time_date(date)?,
        decode_time_time(scale, time)?,
    ))
}

#[cfg(feature = "time")]
fn decode_time_datetimeoffset(
    scale: u8,
    bytes: &[u8],
) -> Result<time::OffsetDateTime, BoxDynError> {
    let (datetime, offset) = bytes
        .split_last_chunk::<2>()
        .ok_or("invalid SQL Server datetimeoffset length")?;
    let offset_minutes = i16::from_le_bytes(*offset);
    let offset = time::UtcOffset::from_whole_seconds(i32::from(offset_minutes) * 60)?;
    decode_time_datetime2(scale, datetime)?
        .assume_utc()
        .checked_to_offset(offset)
        .ok_or_else(|| "SQL Server datetimeoffset has an out-of-range local date".into())
}

#[cfg(all(test, any(feature = "decimal", feature = "time")))]
mod tests {
    use super::*;
    use crate::MssqlValue;
    use sqlx_core::value::Value;

    #[cfg(feature = "decimal")]
    #[test]
    fn rejects_sql_decimal_outside_rust_decimal_range() {
        let mut data = vec![1];
        data.extend_from_slice(&(10_u128.pow(38) - 1).to_le_bytes());
        let value = MssqlValue::new(MssqlTypeInfo::DECIMAL, Some(data.into()));

        assert!(Decimal::decode(value.as_ref()).is_err());
    }

    #[cfg(feature = "time")]
    #[test]
    fn encodes_and_decodes_datetimeoffset_with_utc_wire_date() {
        use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

        let local = PrimitiveDateTime::new(
            Date::from_calendar_date(2016, Month::October, 23).unwrap(),
            Time::from_hms_nano(0, 45, 37, 123_456_700).unwrap(),
        );
        let utc = PrimitiveDateTime::new(
            Date::from_calendar_date(2016, Month::October, 22).unwrap(),
            Time::from_hms_nano(22, 45, 37, 123_456_700).unwrap(),
        );
        let offset = UtcOffset::from_hms(2, 0, 0).unwrap();
        let mut wire = encode_time_datetime2(utc).to_vec();
        wire.extend_from_slice(&120_i16.to_le_bytes());

        let mut encoded = Vec::new();
        let _ = local
            .assume_offset(offset)
            .encode_by_ref(&mut encoded)
            .unwrap();
        assert_eq!(wire, encoded);

        let value = MssqlValue::new(MssqlTypeInfo::DATETIMEOFFSET, Some(wire.into()));
        let decoded = OffsetDateTime::decode(value.as_ref()).unwrap();
        assert_eq!(utc.assume_utc(), decoded);
        assert_eq!(offset, decoded.offset());
        assert_eq!(local, PrimitiveDateTime::decode(value.as_ref()).unwrap());
    }
}
