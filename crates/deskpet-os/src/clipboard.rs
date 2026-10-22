use arboard::ImageData;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{OsError, Result};

/// RGBA 原始图像，前后端通用载体。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImagePayload {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 系统剪贴板封装（文本与图像）。
pub struct Clipboard {
    inner: arboard::Clipboard,
}

impl Clipboard {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: arboard::Clipboard::new()?,
        })
    }

    /// 读取文本（剪贴板无文本时返回 Err）。
    pub fn read_text(&mut self) -> Result<String> {
        Ok(self.inner.get_text()?)
    }

    /// 写入文本。
    pub fn write_text(&mut self, text: &str) -> Result<()> {
        Ok(self.inner.set_text(text.to_string())?)
    }

    /// 读取图像（无图像或读取失败时返回 None，与历史行为一致）。
    pub fn read_image(&mut self) -> Result<Option<ImagePayload>> {
        match self.inner.get_image() {
            Ok(img) => Ok(Some(ImagePayload {
                width: img.width as u32,
                height: img.height as u32,
                rgba: img.bytes.into_owned(),
            })),
            Err(_) => Ok(None),
        }
    }

    /// 将 RGBA 图像写入剪贴板。
    pub fn write_image(&mut self, payload: &ImagePayload) -> Result<()> {
        let data = ImageData {
            width: payload.width as usize,
            height: payload.height as usize,
            bytes: Cow::Owned(payload.rgba.clone()),
        };
        self.inner.set_image(data)?;
        Ok(())
    }
}

/// 便捷函数：一次性写入文本（不保留剪贴板句柄）。
pub fn copy_text(text: &str) -> Result<()> {
    let mut board = Clipboard::new().map_err(|e| OsError::Other(e.to_string()))?;
    board.write_text(text)
}
