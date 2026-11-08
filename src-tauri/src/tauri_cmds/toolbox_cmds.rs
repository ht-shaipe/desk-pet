use base64::Engine as _;
use deskpet_os::clipboard::{Clipboard, ImagePayload};
use deskpet_os::files::ImageFileInfo;
use deskpet_os::imaging::{CompressOutcome, CompressParams, OutputFormat};
use deskpet_os::watermark::{WatermarkKind, WatermarkParams, WmPos};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

use crate::paths::data_dir;

/// 读取系统剪贴板中的图像（无图像时返回 None）。
#[tauri::command]
pub fn get_clipboard_image() -> Result<Option<ImagePayload>, String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    board
        .read_image()
        .map_err(|e| format!("读取剪贴板图像失败: {e}"))
}

/// 将 RGBA 图像写入系统剪贴板。
#[tauri::command]
pub fn set_clipboard_image(payload: ImagePayload) -> Result<(), String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    board
        .write_image(&payload)
        .map_err(|e| format!("写入剪贴板失败: {e}"))
}

/// 将 PNG dataURL 保存到应用数据目录 qrcodes/ 下，并在文件管理器中定位。
#[tauri::command]
pub fn save_qr_png(app: AppHandle, data_url: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let path = deskpet_os::files::save_qr_png(&dir, &data_url).map_err(|e| e.to_string())?;
    deskpet_os::files::reveal(&path);
    Ok(path.to_string_lossy().to_string())
}

/// 将任意图片 dataURL（png/jpeg/webp）保存到应用数据目录 images/ 下，并在文件管理器中定位。
#[tauri::command]
pub fn save_image(app: AppHandle, data_url: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let path = deskpet_os::files::save_image(&dir, &data_url).map_err(|e| e.to_string())?;
    deskpet_os::files::reveal(&path);
    Ok(path.to_string_lossy().to_string())
}

/// 列出目录下的图片文件（仅当前目录，跳过隐藏文件），按名称排序。
#[tauri::command]
pub fn list_image_files(dir: String) -> Result<Vec<ImageFileInfo>, String> {
    deskpet_os::files::list_images(Path::new(&dir)).map_err(|e| e.to_string())
}

/// 水印参数(与前端 WatermarkOptions 对应;未传字段取默认值)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatermarkDto {
    pub kind: String,
    pub text: String,
    pub font_size: u32,
    pub color: String,
    pub opacity: u8,
    pub rotation: i16,
    pub tiled: bool,
    pub density: u8,
    pub position: String,
    pub image_data_url: String,
    pub image_scale: u8,
    pub strength: u8,
}

/// 压缩参数(与前端 CompressImageParams 对应)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressParamsDto {
    pub quality: u8,
    pub max_dim: u32,
    pub format: String,
    pub target_kb: u32,
    pub watermark: Option<WatermarkDto>,
}

impl CompressParamsDto {
    fn into_params(self) -> Result<CompressParams, String> {
        let format = OutputFormat::parse(&self.format)
            .ok_or_else(|| format!("不支持的输出格式: {}", self.format))?;
        let watermark = match self.watermark {
            None => None,
            Some(w) if w.kind == "none" || w.kind.is_empty() => None,
            Some(w) => Some(w.into_params()?),
        };
        Ok(CompressParams {
            quality: self.quality.clamp(1, 100),
            max_dim: self.max_dim,
            format,
            target_kb: self.target_kb,
            watermark,
        })
    }
}

impl WatermarkDto {
    fn into_params(self) -> Result<WatermarkParams, String> {
        let kind = match self.kind.as_str() {
            "text" => WatermarkKind::Text,
            "image" => WatermarkKind::Image,
            "blind" => WatermarkKind::Blind,
            other => return Err(format!("不支持的水印类型: {other}")),
        };
        let color = parse_hex_color(&self.color).unwrap_or([255, 255, 255]);
        let image = if self.image_data_url.is_empty() {
            Vec::new()
        } else {
            deskpet_os::files::decode_data_url(&self.image_data_url)
                .map_err(|e| e.to_string())?
                .1
        };
        Ok(WatermarkParams {
            kind,
            text: self.text,
            font_size: self.font_size.clamp(6, 400).max(6),
            color,
            opacity: self.opacity.clamp(0, 100),
            rotation: self.rotation,
            tiled: self.tiled,
            density: self.density,
            position: WmPos::parse(&self.position),
            image,
            image_scale: self.image_scale.clamp(1, 100),
            strength: self.strength,
        })
    }
}

/// "#RRGGBB" → [r, g, b];解析失败返回 None。
fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// 压缩结果(与前端 CompressImageResult 对应)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressImageResult {
    /// 压缩结果 dataURL(include_data 时返回)
    pub data_url: Option<String>,
    /// out_dir 提供时的落盘路径
    pub out_path: Option<String>,
    pub original_size: u64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    /// 实际输出格式名:jpeg / png / webp / gif
    pub format: String,
    pub ext: String,
    /// 未达节省阈值,原样保留
    pub kept: bool,
    pub animated: bool,
}

fn ext_mime(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

fn outcome_to_result(
    outcome: CompressOutcome,
    original_size: u64,
    out_path: Option<PathBuf>,
    include_data: bool,
) -> CompressImageResult {
    let CompressOutcome {
        data,
        ext,
        format,
        width,
        height,
        kept,
        animated,
    } = outcome;
    let data_url = if include_data {
        Some(format!(
            "data:{};base64,{}",
            ext_mime(&ext),
            base64::engine::general_purpose::STANDARD.encode(&data)
        ))
    } else {
        None
    };
    CompressImageResult {
        data_url,
        out_path: out_path.map(|p| p.to_string_lossy().to_string()),
        original_size,
        size: data.len() as u64,
        width,
        height,
        format,
        ext,
        kept,
        animated,
    }
}

/// 压缩单张图片(dataURL 输入,用于单图预览),返回压缩结果与统计。
#[tauri::command]
pub async fn compress_image_data(
    data_url: String,
    params: CompressParamsDto,
) -> Result<CompressImageResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = params.into_params()?;
        let (ext, bytes) =
            deskpet_os::files::decode_data_url(&data_url).map_err(|e| e.to_string())?;
        let original_size = bytes.len() as u64;
        let outcome =
            deskpet_os::imaging::optimize(&bytes, ext, &p).map_err(|e| e.to_string())?;
        Ok(outcome_to_result(outcome, original_size, None, true))
    })
    .await
    .map_err(|e| format!("压缩任务失败: {e}"))?
}

/// 压缩本地图片文件:out_dir 为 None 时仅估算;为 Some 时写入 `{out_dir}/compressed/{stem}.min.{ext}`。
#[tauri::command]
pub async fn compress_image_file(
    path: String,
    params: CompressParamsDto,
    out_dir: Option<String>,
    include_data: bool,
) -> Result<CompressImageResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = params.into_params()?;
        let src = Path::new(&path);
        let bytes = std::fs::read(src).map_err(|e| format!("读取文件失败: {e}"))?;
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let ext = match ext.as_str() {
            "jpg" | "jpeg" => "jpg",
            "png" => "png",
            "webp" => "webp",
            "gif" => "gif",
            "bmp" => "bmp",
            other => return Err(format!("不支持的图片格式: {other}")),
        };
        let original_size = bytes.len() as u64;
        let outcome = deskpet_os::imaging::optimize(&bytes, ext, &p).map_err(|e| e.to_string())?;
        let src_name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let out_path = match out_dir {
            Some(dir) => Some(
                deskpet_os::files::write_compressed(
                    Path::new(&dir),
                    &src_name,
                    &outcome.ext,
                    &outcome.data,
                )
                .map_err(|e| e.to_string())?,
            ),
            None => None,
        };
        Ok(outcome_to_result(outcome, original_size, out_path, include_data))
    })
    .await
    .map_err(|e| format!("压缩任务失败: {e}"))?
}

/// 在文件管理器中打开目录或定位文件。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    deskpet_os::files::reveal_entry(Path::new(&path));
    Ok(())
}

/// 由 Rust 代理发送 HTTP 请求，绕开 WebView 跨域限制，并回传请求/响应头。
#[tauri::command]
pub async fn http_request(
    spec: deskpet_os::http::HttpRequestSpec,
) -> Result<deskpet_os::http::HttpResponseData, String> {
    deskpet_os::http::send(&spec).await
}

/// 文档转换结果。needs_ocr 单独标记：扫描版 PDF（无文本层）不是错误，而是需要 OCR 的明确状态。
#[derive(serde::Serialize)]
pub struct DocumentConvertResult {
    /// 转换出的 Markdown；needs_ocr 时为空串
    pub markdown: String,
    /// true：扫描版 PDF（无文本层），需要 OCR 才能提取内容
    pub needs_ocr: bool,
    /// needs_ocr 时：无文本层的页码（1 起）
    pub ocr_pages: Vec<u32>,
    /// needs_ocr 时：PDF 总页数
    pub page_count: u32,
}

/// 文档转 Markdown：按内容签名自动探测格式
/// （doc/docx、ppt 系、xls 系、odt/ods/odp、rtf、epub、csv、pdf —— 共 12 种 Format），
/// 扩展名仅作兜底。文本型 PDF 本地解析；扫描版 PDF 无文本层，以 needs_ocr 状态返回。
#[tauri::command]
pub async fn document_convert(path: String) -> Result<DocumentConvertResult, String> {
    let path = if path.starts_with('~') {
        // HOME 缺失（如部分 Windows 环境）时保留原样，避免静默展开成错误路径
        match std::env::var("HOME") {
            Ok(home) => path.replacen('~', &home, 1),
            Err(_) => path,
        }
    } else {
        path
    };
    match anydoc::to_markdown(std::path::Path::new(&path)) {
        Ok(markdown) => Ok(DocumentConvertResult {
            markdown,
            needs_ocr: false,
            ocr_pages: Vec::new(),
            page_count: 0,
        }),
        // 枚举 #[non_exhaustive]：变体字段可直接解构，但必须保留通配分支
        Err(anydoc::ConvertError::NeedsOcr { pages, page_count }) => {
            Ok(DocumentConvertResult {
                markdown: String::new(),
                needs_ocr: true,
                ocr_pages: pages,
                page_count,
            })
        }
        Err(e) => Err(format!("文档转换失败：{e}")),
    }
}

/// 保存文本内容到本地文件（用于下载转换结果）。文件写入可能落在慢盘/网络盘，async 避免阻塞主线程。
#[tauri::command]
pub async fn save_document_markdown(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}
