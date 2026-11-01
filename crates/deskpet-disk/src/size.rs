//! 计量口径：默认按磁盘分配大小（删除后真正能释放的字节），不用表观长度。
//!
//! `ls -l` 的 `st_size` 对稀疏文件、APFS 克隆、压缩文件都会高估；
//! `st_blocks * 512` 才是删除后能回来的空间。硬链接去重在扫描层做。

use std::fs;

/// 文件在磁盘上的分配大小（字节）。
///
/// Unix 用 `st_blocks * 512`；其他平台暂用表观长度 `st_len` 回退，
/// Windows 分配大小后续按 cluster 细化。
pub fn allocated_size(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

/// 目录体积统计结果。`complete = false` 表示达到访问上限提前截断，
/// 字节只是已扫描部分的下界，UI 必须标注"大小不完整"。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirSize {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    pub errors: u64,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn allocated_size_never_exceeds_apparent_for_sparse_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sparse.bin");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"x").unwrap();
        // 先写 1 字节再扩到 1MB：支持稀疏的文件系统上只分配 1 个块
        file.set_len(1024 * 1024).unwrap();
        drop(file);

        let meta = fs::metadata(&path).unwrap();
        let allocated = allocated_size(&meta);
        assert!(
            allocated < 1024 * 1024,
            "稀疏文件的分配大小应远小于表观长度（当前文件系统分配了 {allocated}）"
        );
    }

    #[test]
    fn allocated_size_matches_file_size_for_dense_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("dense.bin");
        fs::write(&path, vec![0u8; 4096]).unwrap();
        let meta = fs::metadata(&path).unwrap();
        let allocated = allocated_size(&meta);
        assert!(allocated >= 4096, "4K 全写文件至少占一个块：{allocated}");
        assert!(allocated <= 4096 * 4, "4K 文件不应分配超过几块：{allocated}");
    }
}
