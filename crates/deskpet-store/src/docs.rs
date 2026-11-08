//! 文档管理数据访问层:文档目录、文档索引(与图库的 image_item 分表,互不干扰)。

use rusqlite::params;

use crate::store::Store;
use crate::Result;

/// 文档目录(监控的扫描根目录)。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DocDir {
    pub id: i64,
    pub path: String,
    pub added_at: u64,
    pub last_scan_at: u64,
    /// 已索引条目数(JOIN 计数,非表字段)
    pub item_count: u64,
}

/// 文档条目。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DocItem {
    pub id: i64,
    pub dir_id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub mtime: u64,
    pub added_at: u64,
}

/// 文档查询过滤条件。
#[derive(Debug, Default, Clone)]
pub struct DocQuery {
    pub dir_id: Option<i64>,
    /// 文件名子串搜索
    pub search: String,
    /// 扩展名精确过滤(小写)
    pub ext: Option<String>,
    /// 排序列:None/"mtime"/"name"/"size"/"ext"(白名单,非法值回退 mtime)
    pub sort_by: Option<String>,
    /// "asc" | "desc"(默认 desc)
    pub sort_order: Option<String>,
    pub limit: u32,
    pub offset: u64,
}

/// 文档类型统计。
#[derive(Debug, serde::Serialize, Clone)]
pub struct DocExtStat {
    pub ext: String,
    pub count: u64,
}

/// 扫描时单文件的最新元数据。
#[derive(Debug, Clone)]
pub struct DocItemMeta {
    pub dir_id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub mtime: u64,
}

const DOC_COLS: &str = "id, dir_id, path, name, ext, size, mtime, added_at";

fn row_doc(row: &rusqlite::Row) -> rusqlite::Result<DocItem> {
    Ok(DocItem {
        id: row.get(0)?,
        dir_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        ext: row.get(4)?,
        size: row.get::<_, i64>(5)?.max(0) as u64,
        mtime: row.get::<_, i64>(6)?.max(0) as u64,
        added_at: row.get::<_, i64>(7)?.max(0) as u64,
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    /* ---------------- 文档目录 ---------------- */

    /// 添加文档目录(已存在则原样返回)。
    pub fn add_doc_dir(&self, path: &str) -> Result<DocDir> {
        let now = now_secs();
        self.conn.execute(
            "INSERT OR IGNORE INTO doc_dir (path, added_at) VALUES (?1, ?2)",
            params![path, now as i64],
        )?;
        let (id, added_at): (i64, i64) = self.conn.query_row(
            "SELECT id, added_at FROM doc_dir WHERE path = ?1",
            params![path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(DocDir {
            id,
            path: path.to_string(),
            added_at: added_at.max(0) as u64,
            last_scan_at: 0,
            item_count: 0,
        })
    }

    /// 列出文档目录(带条目计数)。
    pub fn list_doc_dirs(&self) -> Result<Vec<DocDir>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, d.path, d.added_at, d.last_scan_at, COUNT(i.id)
             FROM doc_dir d LEFT JOIN doc_item i ON i.dir_id = d.id
             GROUP BY d.id ORDER BY d.added_at, d.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DocDir {
                id: row.get(0)?,
                path: row.get(1)?,
                added_at: row.get::<_, i64>(2)?.max(0) as u64,
                last_scan_at: row.get::<_, i64>(3)?.max(0) as u64,
                item_count: row.get::<_, i64>(4)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 移除文档目录及其索引(不动用户文件)。
    pub fn remove_doc_dir(&self, id: i64) -> Result<u64> {
        let n = self.conn.execute("DELETE FROM doc_item WHERE dir_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM doc_dir WHERE id = ?1", params![id])?;
        Ok(n as u64)
    }

    /// 记录目录扫描完成时间。
    pub fn touch_doc_dir_scan(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE doc_dir SET last_scan_at = ?1 WHERE id = ?2",
            params![now_secs() as i64, id],
        )?;
        Ok(())
    }

    /* ---------------- 文档条目 ---------------- */

    /// 扫描比对用:按路径取 (id, size, mtime)。
    pub fn get_doc_item_stamp(&self, path: &str) -> Result<Option<(i64, u64, u64)>> {
        let r = self.conn.query_row(
            "SELECT id, size, mtime FROM doc_item WHERE path = ?1",
            params![path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?.max(0) as u64,
                    row.get::<_, i64>(2)?.max(0) as u64,
                ))
            },
        );
        match r {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 新增条目,返回 (id, 是否新建)。文档索引只增不改:内容变化由调用方先删旧行再入新行。
    pub fn add_doc_item(&self, m: &DocItemMeta) -> Result<(i64, bool)> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM doc_item WHERE path = ?1",
                params![m.path],
                |r| r.get(0),
            )
            .ok();
        if let Some(id) = existing {
            return Ok((id, false));
        }
        self.conn.execute(
            "INSERT INTO doc_item (dir_id, path, name, ext, size, mtime, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                m.dir_id,
                m.path,
                m.name,
                m.ext,
                m.size as i64,
                m.mtime as i64,
                now_secs() as i64
            ],
        )?;
        Ok((self.conn.last_insert_rowid(), true))
    }

    /// 按 id 取条目。
    pub fn get_doc_item(&self, id: i64) -> Result<Option<DocItem>> {
        let r = self.conn.query_row(
            &format!("SELECT {DOC_COLS} FROM doc_item WHERE id = ?1"),
            params![id],
            row_doc,
        );
        match r {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 删除条目(内容变化/文件消失时调用)。
    pub fn delete_doc_item(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM doc_item WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 按过滤条件查询文档,返回 (条目, 是否还有更多)。
    pub fn query_doc_items(&self, q: &DocQuery) -> Result<(Vec<DocItem>, bool)> {
        let mut sql = format!("SELECT {DOC_COLS} FROM doc_item WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(dir_id) = q.dir_id {
            sql.push_str(" AND dir_id = ?");
            args.push(Box::new(dir_id));
        }
        if let Some(ext) = &q.ext {
            sql.push_str(" AND ext = ?");
            args.push(Box::new(ext.clone()));
        }
        if !q.search.is_empty() {
            sql.push_str(" AND name LIKE ? ESCAPE '\\'");
            args.push(Box::new(format!(
                "%{}%",
                q.search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
            )));
        }
        // 排序白名单:列名与方向都不进字符串拼接的非法值
        let order_col = match q.sort_by.as_deref() {
            Some("name") => "name COLLATE NOCASE",
            Some("ext") => "ext",
            Some("size") => "size",
            _ => "mtime",
        };
        let order_dir = if q.sort_order.as_deref() == Some("asc") {
            "ASC"
        } else {
            "DESC"
        };
        sql.push_str(&format!(
            " ORDER BY {order_col} {order_dir}, id {order_dir} LIMIT ? OFFSET ?"
        ));
        let limit = q.limit.max(1);
        args.push(Box::new(limit as i64 + 1));
        args.push(Box::new(q.offset as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), row_doc)?;
        let mut items: Vec<DocItem> = rows.filter_map(|r| r.ok()).collect();
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        Ok((items, has_more))
    }

    /// 按过滤条件计数。
    pub fn count_doc_items(&self, q: &DocQuery) -> Result<u64> {
        let mut sql = String::from("SELECT COUNT(*) FROM doc_item WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(dir_id) = q.dir_id {
            sql.push_str(" AND dir_id = ?");
            args.push(Box::new(dir_id));
        }
        if let Some(ext) = &q.ext {
            sql.push_str(" AND ext = ?");
            args.push(Box::new(ext.clone()));
        }
        if !q.search.is_empty() {
            sql.push_str(" AND name LIKE ? ESCAPE '\\'");
            args.push(Box::new(format!(
                "%{}%",
                q.search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
            )));
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let n: i64 = stmt.query_row(refs.as_slice(), |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// 按扩展名统计文档数量(数量降序)。
    pub fn list_doc_exts(&self) -> Result<Vec<DocExtStat>> {
        let mut stmt = self.conn.prepare(
            "SELECT ext, COUNT(*) FROM doc_item GROUP BY ext ORDER BY COUNT(*) DESC, ext ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DocExtStat {
                ext: row.get(0)?,
                count: row.get::<_, i64>(1)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Store {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        Store::open(&path).unwrap()
    }

    fn meta(dir_id: i64, path: &str, name: &str) -> DocItemMeta {
        DocItemMeta {
            dir_id,
            path: path.into(),
            name: name.into(),
            ext: "pdf".into(),
            size: 100,
            mtime: 1_700_000_000,
        }
    }

    #[test]
    fn doc_dir_crud_and_counts() {
        let s = open();
        let d1 = s.add_doc_dir("/tmp/docs").unwrap();
        let d2 = s.add_doc_dir("/tmp/docs").unwrap();
        assert_eq!(d1.id, d2.id, "重复添加返回既有目录");

        s.add_doc_item(&meta(d1.id, "/tmp/docs/a.pdf", "a.pdf")).unwrap();
        s.add_doc_item(&meta(d1.id, "/tmp/docs/b.pdf", "b.pdf")).unwrap();
        let dirs = s.list_doc_dirs().unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].item_count, 2);

        let removed = s.remove_doc_dir(d1.id).unwrap();
        assert_eq!(removed, 2);
        assert!(s.list_doc_dirs().unwrap().is_empty());
    }

    #[test]
    fn doc_stamp_and_query() {
        let s = open();
        let d = s.add_doc_dir("/x").unwrap();
        let (id, created) = s.add_doc_item(&meta(d.id, "/x/a.pdf", "a.pdf")).unwrap();
        assert!(created);
        let stamp = s.get_doc_item_stamp("/x/a.pdf").unwrap().unwrap();
        assert_eq!(stamp.0, id);
        assert_eq!(stamp, (1, 100, 1_700_000_000));
        assert!(s.get_doc_item_stamp("/x/none.pdf").unwrap().is_none());

        let mut m = meta(d.id, "/x/older.pdf", "older.pdf");
        m.mtime = 1_600_000_000;
        s.add_doc_item(&m).unwrap();

        let (items, more) = s
            .query_doc_items(&DocQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(!more);
        assert_eq!(items[0].name, "a.pdf", "按 mtime 倒序");

        let (items, _) = s
            .query_doc_items(&DocQuery {
                search: "old".into(),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "older.pdf");
    }

    #[test]
    fn doc_query_sorting() {
        let s = open();
        let d = s.add_doc_dir("/x").unwrap();
        for (name, size) in [("b.pdf", 30), ("a.pdf", 20), ("c.pdf", 10)] {
            let mut m = meta(d.id, &format!("/x/{name}"), name);
            m.size = size;
            s.add_doc_item(&m).unwrap();
        }
        // 按名称升序
        let (items, _) = s
            .query_doc_items(&DocQuery {
                sort_by: Some("name".into()),
                sort_order: Some("asc".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.pdf", "b.pdf", "c.pdf"]);
        // 按大小降序
        let (items, _) = s
            .query_doc_items(&DocQuery {
                sort_by: Some("size".into()),
                sort_order: Some("desc".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items[0].size, 30);
        // 非法排序键回退 mtime
        let (items, _) = s
            .query_doc_items(&DocQuery {
                sort_by: Some("evil; DROP TABLE".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn doc_delete_row() {
        let s = open();
        let d = s.add_doc_dir("/x").unwrap();
        let (id, _) = s.add_doc_item(&meta(d.id, "/x/a.pdf", "a.pdf")).unwrap();
        s.delete_doc_item(id).unwrap();
        assert!(s.get_doc_item_stamp("/x/a.pdf").unwrap().is_none());
        assert_eq!(s.count_doc_items(&DocQuery::default()).unwrap(), 0);
    }
}
