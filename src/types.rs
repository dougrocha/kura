use std::fmt;

use jiff::civil::DateTime;
use rusqlite::{
    Result as RusqliteResult,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ImageHash(pub String);

impl std::hash::Hash for ImageHash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl std::ops::Deref for ImageHash {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl ToSql for ImageHash {
    fn to_sql(&self) -> RusqliteResult<ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl FromSql for ImageHash {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        String::column_result(value).map(ImageHash)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CreatedAt(DateTime);

impl ToSql for CreatedAt {
    fn to_sql(&self) -> RusqliteResult<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(
            self.0.strftime("%Y-%m-%d %H:%M:%S").to_string(),
        ))
    }
}

impl FromSql for CreatedAt {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = String::column_result(value)?;
        DateTime::strptime("%Y-%m-%d %H:%M:%S", &s)
            .map(CreatedAt)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}
