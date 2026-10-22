/// 系统交互层错误
#[derive(Debug, thiserror::Error)]
pub enum OsError {
    #[error("剪贴板访问失败: {0}")]
    Clipboard(#[from] arboard::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("图像数据无效: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("{0}")]
    Other(String),
}
