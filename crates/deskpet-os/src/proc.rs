use std::process::Command;

use crate::{OsError, Result};

/// 后台启动进程并立即返回（不阻塞、不收集输出）。
pub(crate) fn spawn_detached(cmd: &mut Command) -> Result<()> {
    let mut child = cmd.spawn().map_err(OsError::Io)?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
