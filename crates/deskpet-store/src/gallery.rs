//! 图片管理器数据访问层:图库目录、条目索引(照片/文档)、标签、去重哈希。
//! DDL 见 store.rs,增量扫描约定:path 唯一,size+mtime 未变则跳过。

use rusqlite::params;

use crate::models::{ImageDir, ImageItem, ImageTag};
use crate::store::Store;
use crate::Result;

/// 条目查询过滤条件(与前端视图一一对应)。
#[derive(Debug, Default, Clone)]
pub struct ImageQuery {
    pub dir_id: Option<i64>,
    pub fav_only: bool,
    pub doc_only: bool,
    /// 仅照片(doc=0);与 doc_only 互斥时以 doc_only 为准
    pub photo_only: bool,
    pub tag_id: Option<i64>,
    /// 文件名子串搜索
    pub search: String,
    pub limit: u32,
    pub offset: u64,
}

/// 扫描时单文件的最新元数据。
#[derive(Debug, Clone)]
pub struct ImageItemMeta {
    pub dir_id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub mtime: u64,
    pub width: u32,
    pub height: u32,
    pub taken_at: u64,
    pub camera: String,
    pub doc: bool,
}

/// 精确重复分组(同 hash)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImageDupGroup {
    pub hash: String,
    pub items: Vec<ImageItem>,
}

const ITEM_COLS: &str = "id, dir_id, path, name, ext, size, mtime, width, height, taken_at, camera, is_fav, doc, thumb, hash";

fn row_item(row: &rusqlite::Row) -> rusqlite::Result<ImageItem> {
    Ok(ImageItem {
        id: row.get(0)?,
        dir_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        ext: row.get(4)?,
        size: row.get::<_, i64>(5)?.max(0) as u64,
        mtime: row.get::<_, i64>(6)?.max(0) as u64,
        width: row.get::<_, i64>(7)?.max(0) as u32,
        height: row.get::<_, i64>(8)?.max(0) as u32,
        taken_at: row.get::<_, i64>(9)?.max(0) as u64,
        camera: row.get(10)?,
        is_fav: row.get::<_, i64>(11)? != 0,
        doc: row.get::<_, i64>(12)? != 0,
        thumb: row.get(13)?,
        hash: row.get(14)?,
    })
}

impl Store {
    /* ---------------- 图库目录 ---------------- */

    /// 添加图库目录(已存在则原样返回),返回目录行。
    pub fn add_image_dir(&self, path: &str, full_disk: bool) -> Result<ImageDir> {
        let now = now_secs();
        self.conn.execute(
            "INSERT OR IGNORE INTO image_dir (path, full_disk, added_at) VALUES (?1, ?2, ?3)",
            params![path, full_disk as i64, now as i64],
        )?;
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM image_dir WHERE path = ?1", params![path], |r| {
                r.get(0)
            })?;
        Ok(ImageDir {
            id,
            path: path.to_string(),
            full_disk,
            added_at: now,
            last_scan_at: 0,
            item_count: 0,
        })
    }

    /// 列出全部图库目录(带条目计数)。
    pub fn list_image_dirs(&self) -> Result<Vec<ImageDir>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, d.path, d.full_disk, d.added_at, d.last_scan_at,
                    COUNT(i.id)
             FROM image_dir d LEFT JOIN image_item i ON i.dir_id = d.id
             GROUP BY d.id ORDER BY d.added_at, d.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ImageDir {
                id: row.get(0)?,
                path: row.get(1)?,
                full_disk: row.get::<_, i64>(2)? != 0,
                added_at: row.get::<_, i64>(3)?.max(0) as u64,
                last_scan_at: row.get::<_, i64>(4)?.max(0) as u64,
                item_count: row.get::<_, i64>(5)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 移除图库目录及其索引(不动用户文件),返回被清理条目的缩略图文件名供上层删缓存。
    pub fn remove_image_dir(&self, id: i64) -> Result<Vec<String>> {
        let thumbs: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT thumb FROM image_item WHERE dir_id = ?1 AND thumb != ''")?;
            let rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        self.conn.execute(
            "DELETE FROM image_item_tag WHERE item_id IN (SELECT id FROM image_item WHERE dir_id = ?1)",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM image_item WHERE dir_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM image_dir WHERE id = ?1", params![id])?;
        Ok(thumbs)
    }

    /// 记录目录扫描完成时间。
    pub fn touch_image_dir_scan(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE image_dir SET last_scan_at = ?1 WHERE id = ?2",
            params![now_secs() as i64, id],
        )?;
        Ok(())
    }

    /* ---------------- 条目 ---------------- */

    /// 扫描比对用:按路径取 (id, size, mtime)。
    pub fn get_image_item_stamp(&self, path: &str) -> Result<Option<(i64, u64, u64)>> {
        let r = self.conn.query_row(
            "SELECT id, size, mtime FROM image_item WHERE path = ?1",
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

    /// 新增或更新条目,返回 (id, 是否新建)。调用方以返回 id 命名缩略图文件。
    pub fn upsert_image_item(&self, m: &ImageItemMeta) -> Result<(i64, bool)> {
        let existed = self
            .conn
            .query_row("SELECT id FROM image_item WHERE path = ?1", params![m.path], |r| {
                r.get::<_, i64>(0)
            });
        match existed {
            Ok(id) => {
                self.conn.execute(
                    "UPDATE image_item SET dir_id=?1, name=?2, ext=?3, size=?4, mtime=?5,
                     width=?6, height=?7, taken_at=?8, camera=?9, doc=?10, thumb='', hash=''
                     WHERE id=?11",
                    params![
                        m.dir_id,
                        m.name,
                        m.ext,
                        m.size as i64,
                        m.mtime as i64,
                        m.width as i64,
                        m.height as i64,
                        m.taken_at as i64,
                        m.camera,
                        m.doc as i64,
                        id
                    ],
                )?;
                Ok((id, false))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.conn.execute(
                    "INSERT INTO image_item (dir_id, path, name, ext, size, mtime, width, height,
                     taken_at, camera, doc) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        m.dir_id,
                        m.path,
                        m.name,
                        m.ext,
                        m.size as i64,
                        m.mtime as i64,
                        m.width as i64,
                        m.height as i64,
                        m.taken_at as i64,
                        m.camera,
                        m.doc as i64
                    ],
                )?;
                Ok((self.conn.last_insert_rowid(), true))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// 回写缩略图文件名。
    pub fn set_image_thumb(&self, id: i64, thumb: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE image_item SET thumb = ?1 WHERE id = ?2",
            params![thumb, id],
        )?;
        Ok(())
    }

    /// 按过滤条件查询条目,返回 (条目, 是否还有更多)。
    pub fn query_image_items(&self, q: &ImageQuery) -> Result<(Vec<ImageItem>, bool)> {
        let mut sql = format!("SELECT {ITEM_COLS} FROM image_item WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(dir_id) = q.dir_id {
            sql.push_str(" AND dir_id = ?");
            args.push(Box::new(dir_id));
        }
        if q.fav_only {
            sql.push_str(" AND is_fav = 1");
        }
        if q.doc_only {
            sql.push_str(" AND doc = 1");
        } else if q.photo_only {
            sql.push_str(" AND doc = 0");
        }
        if let Some(tag_id) = q.tag_id {
            sql.push_str(" AND id IN (SELECT item_id FROM image_item_tag WHERE tag_id = ?)");
            args.push(Box::new(tag_id));
        }
        if !q.search.is_empty() {
            sql.push_str(" AND name LIKE ? ESCAPE '\\'");
            args.push(Box::new(format!(
                "%{}%",
                q.search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
            )));
        }
        sql.push_str(" ORDER BY taken_at DESC, id DESC LIMIT ? OFFSET ?");
        let limit = q.limit.max(1);
        args.push(Box::new(limit as i64 + 1));
        args.push(Box::new(q.offset as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), row_item)?;
        let mut items: Vec<ImageItem> = rows.filter_map(|r| r.ok()).collect();
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        Ok((items, has_more))
    }

    /// 按过滤条件计数(侧边栏徽标/汇总用)。
    pub fn count_image_items(&self, q: &ImageQuery) -> Result<u64> {
        let mut sql = String::from("SELECT COUNT(*) FROM image_item WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(dir_id) = q.dir_id {
            sql.push_str(" AND dir_id = ?");
            args.push(Box::new(dir_id));
        }
        if q.fav_only {
            sql.push_str(" AND is_fav = 1");
        }
        if q.doc_only {
            sql.push_str(" AND doc = 1");
        } else if q.photo_only {
            sql.push_str(" AND doc = 0");
        }
        if let Some(tag_id) = q.tag_id {
            sql.push_str(" AND id IN (SELECT item_id FROM image_item_tag WHERE tag_id = ?)");
            args.push(Box::new(tag_id));
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

    /// 取单个条目。
    pub fn get_image_item(&self, id: i64) -> Result<Option<ImageItem>> {
        let r = self.conn.query_row(
            &format!("SELECT {ITEM_COLS} FROM image_item WHERE id = ?1"),
            params![id],
            row_item,
        );
        match r {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 收藏/取消收藏。
    pub fn set_image_fav(&self, id: i64, fav: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE image_item SET is_fav = ?1 WHERE id = ?2",
            params![fav as i64, id],
        )?;
        Ok(())
    }

    /// 删除索引行(不动用户文件),返回其缩略图文件名供上层清缓存。
    pub fn delete_image_item(&self, id: i64) -> Result<Option<String>> {
        let thumb: Option<String> = self.conn.query_row(
            "SELECT thumb FROM image_item WHERE id = ?1",
            params![id],
            |r| r.get(0),
        ).ok();
        self.conn
            .execute("DELETE FROM image_item_tag WHERE item_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM image_item WHERE id = ?1", params![id])?;
        Ok(thumb.filter(|t| !t.is_empty()))
    }

    /// 回写内容哈希。
    pub fn set_image_hash(&self, id: i64, hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE image_item SET hash = ?1 WHERE id = ?2",
            params![hash, id],
        )?;
        Ok(())
    }

    /* ---------------- 标签 ---------------- */

    /// 列出标签(带使用计数)。
    pub fn list_image_tags(&self) -> Result<Vec<ImageTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color, COUNT(it.item_id)
             FROM image_tag t LEFT JOIN image_item_tag it ON it.tag_id = t.id
             GROUP BY t.id ORDER BY t.name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ImageTag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                item_count: row.get::<_, i64>(3)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 新建标签(同名返回既有行)。
    pub fn add_image_tag(&self, name: &str, color: &str) -> Result<ImageTag> {
        self.conn.execute(
            "INSERT OR IGNORE INTO image_tag (name, color) VALUES (?1, ?2)",
            params![name, color],
        )?;
        let (id, color): (i64, String) = self.conn.query_row(
            "SELECT id, color FROM image_tag WHERE name = ?1",
            params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(ImageTag {
            id,
            name: name.to_string(),
            color,
            item_count: 0,
        })
    }

    /// 改名/改色。
    pub fn rename_image_tag(&self, id: i64, name: &str, color: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE image_tag SET name = ?1, color = ?2 WHERE id = ?3",
            params![name, color, id],
        )?;
        Ok(())
    }

    /// 删除标签及其关联。
    pub fn delete_image_tag(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM image_item_tag WHERE tag_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM image_tag WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 全部 item↔tag 关系对(前端据此在缩略图上渲染标签色点)。
    pub fn list_item_tag_pairs(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id, tag_id FROM image_item_tag ORDER BY item_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 条目的标签列表。
    pub fn get_item_tags(&self, item_id: i64) -> Result<Vec<ImageTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color, 0
             FROM image_tag t JOIN image_item_tag it ON it.tag_id = t.id
             WHERE it.item_id = ?1 ORDER BY t.name",
        )?;
        let rows = stmt.query_map(params![item_id], |row| {
            Ok(ImageTag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                item_count: 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 打上/摘除标签。
    pub fn set_item_tag(&self, item_id: i64, tag_id: i64, on: bool) -> Result<()> {
        if on {
            self.conn.execute(
                "INSERT OR IGNORE INTO image_item_tag (item_id, tag_id) VALUES (?1, ?2)",
                params![item_id, tag_id],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM image_item_tag WHERE item_id = ?1 AND tag_id = ?2",
                params![item_id, tag_id],
            )?;
        }
        Ok(())
    }

    /* ---------------- 精确去重 ---------------- */

    /// 同尺寸且尚未计算哈希的条目(id, path)。重复哈希只在这些文件上计算。
    pub fn image_hash_candidates(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM image_item
             WHERE size > 0 AND hash = ''
               AND size IN (SELECT size FROM image_item WHERE size > 0 GROUP BY size HAVING COUNT(*) > 1)
             ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 重复分组(同 hash 且组内 >1)。
    pub fn image_dup_groups(&self) -> Result<Vec<ImageDupGroup>> {
        let hashes: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT hash FROM image_item WHERE hash != ''
                 GROUP BY hash HAVING COUNT(*) > 1 ORDER BY COUNT(*) DESC",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        let mut groups = Vec::with_capacity(hashes.len());
        {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {ITEM_COLS} FROM image_item WHERE hash = ?1 ORDER BY id"
            ))?;
            for hash in hashes {
                let rows = stmt.query_map(params![hash], row_item)?;
                let items: Vec<ImageItem> = rows.filter_map(|r| r.ok()).collect();
                groups.push(ImageDupGroup { hash, items });
            }
        }
        Ok(groups)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Store {
        let dir = tempfile::tempdir().unwrap();
        // tempdir 在测试内保持存活:泄漏目录即可(进程结束回收)
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        Store::open(&path).unwrap()
    }

    fn meta(dir_id: i64, path: &str, name: &str, size: u64, doc: bool) -> ImageItemMeta {
        ImageItemMeta {
            dir_id,
            path: path.into(),
            name: name.into(),
            ext: if doc { "pdf" } else { "jpg" }.into(),
            size,
            mtime: 1_700_000_000,
            width: if doc { 0 } else { 800 },
            height: if doc { 0 } else { 600 },
            taken_at: 1_700_000_100,
            camera: String::new(),
            doc,
        }
    }

    #[test]
    fn image_dir_crud_and_counts() {
        let s = open();
        let d1 = s.add_image_dir("/tmp/pics", false).unwrap();
        let d2 = s.add_image_dir("/tmp/pics", false).unwrap();
        assert_eq!(d1.id, d2.id, "重复添加应返回既有目录");

        s.upsert_image_item(&meta(d1.id, "/tmp/pics/a.jpg", "a.jpg", 10, false))
            .unwrap();
        s.upsert_image_item(&meta(d1.id, "/tmp/pics/b.jpg", "b.jpg", 20, false))
            .unwrap();
        let dirs = s.list_image_dirs().unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].item_count, 2);

        let thumbs = s.remove_image_dir(d1.id).unwrap();
        assert!(thumbs.is_empty());
        assert!(s.list_image_dirs().unwrap().is_empty());
        assert_eq!(s.count_image_items(&ImageQuery::default()).unwrap(), 0);
    }

    #[test]
    fn upsert_updates_on_change_and_keeps_id() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        let (id1, created) = s
            .upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 10, false))
            .unwrap();
        assert!(created);
        // 同路径更新(size 变化):id 不变,thumb/hash 重置
        s.set_image_thumb(id1, "ab/cd.jpg").unwrap();
        s.set_image_hash(id1, "deadbeef").unwrap();
        let (id2, created2) = s
            .upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 99, false))
            .unwrap();
        assert!(!created2);
        assert_eq!(id1, id2);
        let item = s.get_image_item(id2).unwrap().unwrap();
        assert_eq!(item.size, 99);
        assert_eq!(item.thumb, "", "变更后缩略图应重置待重生成");
        assert_eq!(item.hash, "", "变更后哈希应重置待重算");
    }

    #[test]
    fn stamp_lookup() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        s.upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 10, false))
            .unwrap();
        let stamp = s.get_image_item_stamp("/x/a.jpg").unwrap().unwrap();
        assert_eq!(stamp, (1, 10, 1_700_000_000));
        assert!(s.get_image_item_stamp("/x/none.jpg").unwrap().is_none());
    }

    #[test]
    fn query_filters_and_paging() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        for (i, name) in ["a.jpg", "b.jpg", "c.png"].iter().enumerate() {
            let mut m = meta(d.id, &format!("/x/{name}"), name, 10, false);
            m.taken_at = 1_700_000_000 + i as u64;
            s.upsert_image_item(&m).unwrap();
        }
        let mut m = meta(d.id, "/x/r.pdf", "r.pdf", 10, true);
        m.taken_at = 1_700_000_500;
        s.upsert_image_item(&m).unwrap();

        // 全部,按 taken_at 倒序
        let (items, more) = s
            .query_image_items(&ImageQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 4);
        assert!(!more);
        assert_eq!(items[0].name, "r.pdf");

        // 仅照片
        let (items, _) = s
            .query_image_items(&ImageQuery {
                photo_only: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 3);

        // 分页 has_more
        let (items, more) = s
            .query_image_items(&ImageQuery {
                limit: 3,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 3);
        assert!(more);

        // 收藏过滤
        let first = items.last().unwrap().id;
        s.set_image_fav(first, true).unwrap();
        let (items, _) = s
            .query_image_items(&ImageQuery {
                fav_only: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, first);

        // 文件名搜索
        let (items, _) = s
            .query_image_items(&ImageQuery {
                search: ".png".into(),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "c.png");
    }

    #[test]
    fn tags_crud_and_filter() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        let (i1, _) = s
            .upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 10, false))
            .unwrap();
        let (i2, _) = s
            .upsert_image_item(&meta(d.id, "/x/b.jpg", "b.jpg", 10, false))
            .unwrap();

        let t1 = s.add_image_tag("风景", "#67c23a").unwrap();
        let t1b = s.add_image_tag("风景", "#ff0000").unwrap();
        assert_eq!(t1.id, t1b.id, "同名标签复用");

        s.set_item_tag(i1, t1.id, true).unwrap();
        s.set_item_tag(i2, t1.id, true).unwrap();
        s.set_item_tag(i2, t1.id, false).unwrap();

        let tags = s.list_image_tags().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].item_count, 1);

        let item_tags = s.get_item_tags(i1).unwrap();
        assert_eq!(item_tags.len(), 1);
        assert_eq!(item_tags[0].name, "风景");

        let (items, _) = s
            .query_image_items(&ImageQuery {
                tag_id: Some(t1.id),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, i1);

        s.rename_image_tag(t1.id, "山景", "#409eff").unwrap();
        s.delete_image_tag(t1.id).unwrap();
        assert!(s.list_image_tags().unwrap().is_empty());
        assert!(s.get_item_tags(i1).unwrap().is_empty());
    }

    #[test]
    fn dedup_candidates_and_groups() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        // 两个同尺寸(重复)+ 一个独立尺寸
        let (a, _) = s
            .upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 100, false))
            .unwrap();
        let (b, _) = s
            .upsert_image_item(&meta(d.id, "/x/b.jpg", "b.jpg", 100, false))
            .unwrap();
        s.upsert_image_item(&meta(d.id, "/x/c.jpg", "c.jpg", 200, false))
            .unwrap();

        let cands = s.image_hash_candidates().unwrap();
        assert_eq!(cands.len(), 2, "仅同尺寸文件参与哈希");

        s.set_image_hash(a, "h1").unwrap();
        s.set_image_hash(b, "h1").unwrap();

        let groups = s.image_dup_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].hash, "h1");
        assert_eq!(groups[0].items.len(), 2);
    }

    #[test]
    fn delete_item_removes_tag_links() {
        let s = open();
        let d = s.add_image_dir("/x", false).unwrap();
        let (id, _) = s
            .upsert_image_item(&meta(d.id, "/x/a.jpg", "a.jpg", 10, false))
            .unwrap();
        s.set_image_thumb(id, "00/1.jpg").unwrap();
        let t = s.add_image_tag("t", "").unwrap();
        s.set_item_tag(id, t.id, true).unwrap();

        let thumb = s.delete_image_item(id).unwrap();
        assert_eq!(thumb.as_deref(), Some("00/1.jpg"));
        assert!(s.get_image_item(id).unwrap().is_none());
        assert_eq!(s.list_image_tags().unwrap()[0].item_count, 0);
    }
}
