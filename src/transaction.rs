use sqlx_core::database::Database;
use sqlx_core::error::Error;
use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr, SqlStr};
use sqlx_core::transaction::TransactionManager;
use std::borrow::Cow;

use crate::{Mssql, MssqlConnection};

/// SQL Server transaction manager.
pub struct MssqlTransactionManager;

impl TransactionManager for MssqlTransactionManager {
    type Database = Mssql;

    async fn begin(conn: &mut MssqlConnection, statement: Option<SqlStr>) -> Result<(), Error> {
        let sql = statement.unwrap_or_else(|| match conn.transaction_depth() {
            0 => SqlStr::from_static("BEGIN TRANSACTION"),
            depth => {
                AssertSqlSafe(savepoint_statement("SAVE TRANSACTION", depth + 1)).into_sql_str()
            }
        });

        conn.run_sql_batch(sql.as_str()).await?;
        conn.increment_transaction_depth();
        Ok(())
    }

    async fn commit(conn: &mut MssqlConnection) -> Result<(), Error> {
        if conn.transaction_depth() == 1 {
            conn.run_sql_batch("COMMIT TRANSACTION").await?;
            conn.decrement_transaction_depth();
        } else if conn.transaction_depth() > 1 {
            conn.decrement_transaction_depth();
        }

        Ok(())
    }

    async fn rollback(conn: &mut MssqlConnection) -> Result<(), Error> {
        let depth = conn.transaction_depth();
        if depth > 0 {
            conn.run_sql_batch(rollback_statement(depth).as_ref())
                .await?;
            conn.decrement_transaction_depth();
        }

        Ok(())
    }

    fn start_rollback(conn: &mut MssqlConnection) {
        conn.queue_rollback();
    }

    fn get_transaction_depth(conn: &<Self::Database as Database>::Connection) -> usize {
        conn.transaction_depth()
    }
}

pub(crate) fn rollback_statement(depth: usize) -> Cow<'static, str> {
    if depth == 1 {
        Cow::Borrowed("ROLLBACK TRANSACTION")
    } else {
        Cow::Owned(savepoint_statement("ROLLBACK TRANSACTION", depth))
    }
}

fn savepoint_statement(action: &str, depth: usize) -> String {
    // SQL Server limits savepoint names to 32 characters.
    format!("{action} sqlx_savepoint_{depth:x}")
}
