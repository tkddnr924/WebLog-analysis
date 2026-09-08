//! 저장된 뷰: 조회 조건·정렬·컬럼 프리셋. 프로젝트 DB의 saved_views에 저장한다.

use duckdb::{params, OptionalExt};
use serde::{Deserialize, Serialize};

use super::query::{LogFilter, LogQuery, SortOrder};
use super::Store;
use crate::error::{EngineError, EngineResult};

/// 뷰 정의.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewDefinition {
    /// 조건.
    pub filter: LogFilter,
    /// 정렬.
    #[serde(default)]
    pub sort: SortOrder,
    /// 표시 컬럼(허용 목록 이름). 비어 있으면 기본.
    #[serde(default)]
    pub columns: Vec<String>,
    /// 룰 원문(YARA풍 텍스트). 있으면 편집할 때 이 원문을 다시 연다.
    #[serde(default)]
    pub rule_source: Option<String>,
}

/// 저장된 뷰.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedView {
    /// ID.
    pub view_id: i64,
    /// 이름.
    pub name: String,
    /// 정의.
    pub definition: ViewDefinition,
}

/// 허용되는 컬럼 이름.
pub const VIEW_COLUMNS: &[&str] = &[
    "timestamp_utc",
    "client_ip",
    "method",
    "request_target",
    "status",
    "bytes_sent",
    "source",
];

/// 뷰 읽기. 읽기 연결에서도 쓸 수 있다.
pub trait ViewQuery: LogQuery {
    /// 뷰 목록(이름순).
    fn list_views(&self) -> EngineResult<Vec<SavedView>> {
        let mut stmt = self
            .query_conn()
            .prepare("SELECT view_id, name, definition_json FROM saved_views ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (view_id, name, json) in rows {
            let definition: ViewDefinition = serde_json::from_str(&json)?;
            out.push(SavedView {
                view_id,
                name,
                definition,
            });
        }
        Ok(out)
    }
}

impl<T: LogQuery> ViewQuery for T {}

impl Store {
    /// 뷰를 저장한다. 같은 이름이 있으면 덮어쓴다.
    pub fn save_view(&self, name: &str, definition: &ViewDefinition) -> EngineResult<SavedView> {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Err(EngineError::Query("뷰 이름은 1~64자".to_owned()));
        }
        if let Some(bad) = definition
            .columns
            .iter()
            .find(|c| !VIEW_COLUMNS.contains(&c.as_str()))
        {
            return Err(EngineError::Query(format!("허용되지 않는 컬럼: {bad}")));
        }
        let json = serde_json::to_string(definition)?;
        let existing: Option<i64> = self
            .conn()
            .query_row(
                "SELECT view_id FROM saved_views WHERE name = ?",
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        let view_id = match existing {
            Some(id) => {
                self.conn().execute(
                    "UPDATE saved_views SET definition_json = ? WHERE view_id = ?",
                    params![json, id],
                )?;
                id
            }
            None => {
                let id: i64 = self.conn().query_row(
                    "SELECT COALESCE(MAX(view_id), 0) + 1 FROM saved_views",
                    [],
                    |r| r.get(0),
                )?;
                self.conn().execute(
                    "INSERT INTO saved_views (view_id, name, definition_json) VALUES (?, ?, ?)",
                    params![id, name, json],
                )?;
                id
            }
        };
        Ok(SavedView {
            view_id,
            name: name.to_owned(),
            definition: definition.clone(),
        })
    }

    /// 뷰를 삭제한다.
    pub fn delete_view(&self, view_id: i64) -> EngineResult<bool> {
        Ok(self.conn().execute(
            "DELETE FROM saved_views WHERE view_id = ?",
            params![view_id],
        )? > 0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::store::StoreConfig;

    #[test]
    fn views_roundtrip_and_upsert_by_name() {
        let store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let def = ViewDefinition {
            filter: LogFilter {
                status_class: Some(5),
                ..LogFilter::default()
            },
            sort: SortOrder::TimeDesc,
            columns: vec!["timestamp_utc".to_owned(), "status".to_owned()],
            rule_source: Some("rule errors { condition: status >= 500 }".to_owned()),
        };
        let v = store.save_view("errors", &def).unwrap();
        let mut def2 = def.clone();
        def2.filter.status = Some(502);
        let v2 = store.save_view("errors", &def2).unwrap();
        assert_eq!(v.view_id, v2.view_id);
        let list = store.list_views().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].definition, def2);
        assert!(store.delete_view(v.view_id).unwrap());
        assert!(store.list_views().unwrap().is_empty());
    }

    #[test]
    fn invalid_column_and_name_are_rejected() {
        let store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let bad = ViewDefinition {
            filter: LogFilter::default(),
            sort: SortOrder::TimeAsc,
            columns: vec!["raw_line".to_owned()],
            rule_source: None,
        };
        assert!(store.save_view("x", &bad).is_err());
        assert!(store
            .save_view(
                "  ",
                &ViewDefinition {
                    filter: LogFilter::default(),
                    sort: SortOrder::TimeAsc,
                    columns: vec![],
                    rule_source: None,
                }
            )
            .is_err());
    }
}
