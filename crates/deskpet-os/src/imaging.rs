//! 图片压缩引擎(参考 PicLite 的智能择优思路)。
//!
//! 流程:解码(应用 EXIF 方向)→ 长边缩放(Lanczos3)→ 水印 → 编码 → 节省阈值判定。
//! 编码器:JPEG(image 精确质量)、WebP(libwebp)、PNG(png crate,Best 压缩;
//! 低质量走 NeuQuant 调色板有损路径)。智能择优模式下多格式并行竞速取最小,
//! PNG 候选带写入早停(超过当前最优即中止编码)。

use std::io::{Cursor, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::{
    DynamicImage, ExtendedColorType, ImageBuffer, ImageDecoder, ImageEncoder, ImageReader, Rgba,
    RgbaImage,
};

use crate::animation;
use crate::watermark::{self, WatermarkParams};
use crate::{OsError, Result};

/// 目标体积搜索的外层缩放梯度(先保尺寸、后降分辨率)。
const SCALE_LADDER: [f32; 8] = [1.0, 0.78, 0.6, 0.46, 0.34, 0.25, 0.18, 0.12];

/// 手动模式"防变大"的质量回退梯度:首次编码不小于原图时逐级降质重编。
const GUARDED_STEPS: [u8; 9] = [4, 8, 14, 22, 32, 44, 58, 72, 99];

/// 输出格式选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// 智能择优:多格式竞速取最小
    Auto,
    /// 保持源格式(静态 gif/bmp 回退 PNG)
    Keep,
    Jpeg,
    Png,
    Webp,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "keep" => Some(Self::Keep),
            "jpeg" | "jpg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// 静态图可编码的目标格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncFmt {
    Jpeg,
    Png,
    Webp,
}

impl EncFmt {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    /// 源扩展名 → 保持原格式时的编码格式;静态 gif/bmp 回退 PNG。
    pub fn keep_from_ext(ext: &str) -> Self {
        match ext {
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            _ => Self::Png,
        }
    }
}

/// 压缩参数(与前端 DTO 对应,由命令层构造)。
#[derive(Debug, Clone)]
pub struct CompressParams {
    /// 编码质量 1-100(JPEG/WebP 质量;PNG <90 走调色板有损)
    pub quality: u8,
    /// 长边上限,0 = 不限
    pub max_dim: u32,
    pub format: OutputFormat,
    /// 目标体积(KB,十进制),0 = 关闭
    pub target_kb: u32,
    pub watermark: Option<WatermarkParams>,
}

impl Default for CompressParams {
    fn default() -> Self {
        Self {
            quality: 82,
            max_dim: 0,
            format: OutputFormat::Auto,
            target_kb: 0,
            watermark: None,
        }
    }
}

/// 压缩结果。
#[derive(Debug, Clone)]
pub struct CompressOutcome {
    pub data: Vec<u8>,
    /// 输出扩展名:jpg / png / webp / gif
    pub ext: String,
    /// 实际输出格式名:jpeg / png / webp / gif
    pub format: String,
    pub width: u32,
    pub height: u32,
    /// 未达节省阈值,原样返回源字节
    pub kept: bool,
    pub animated: bool,
}

/// 扩展名 → 规范格式名(jpg→jpeg)。
fn ext_format_name(ext: &str) -> String {
    match ext {
        "jpg" | "jpeg" => "jpeg".into(),
        "png" => "png".into(),
        "webp" => "webp".into(),
        "gif" => "gif".into(),
        "bmp" => "bmp".into(),
        other => other.into(),
    }
}

/// 统一入口:静态图与动图自动路由。
/// 动图源 + 显式 JPEG/PNG 输出时按首帧走静态路径(与旧版单图行为一致)。
pub fn optimize(bytes: &[u8], src_ext: &str, params: &CompressParams) -> Result<CompressOutcome> {
    let animated = animation::detect_animated(bytes, src_ext);
    if animated && !matches!(params.format, OutputFormat::Jpeg | OutputFormat::Png) {
        return animation::optimize_animation(bytes, src_ext, params);
    }
    let rgba = decode_rgba(bytes)?;
    let mut outcome = optimize_static(rgba, src_ext, params, bytes)?;
    // 静态路径处理动图源时,kept 返回的是原始动图字节,标记需保持
    if animated && outcome.kept {
        outcome.animated = true;
    }
    Ok(outcome)
}

/// 静态图压缩主流程(已解码 RGBA)。
pub(crate) fn optimize_static(
    mut rgba: RgbaImage,
    src_ext: &str,
    params: &CompressParams,
    original: &[u8],
) -> Result<CompressOutcome> {
    let (orig_w, orig_h) = rgba.dimensions();
    rgba = apply_max_dim(rgba, params.max_dim);
    if let Some(wm) = &params.watermark {
        watermark::apply(&mut rgba, wm)?;
    }
    let (w, h) = rgba.dimensions();
    let pixels_changed = (w, h) != (orig_w, orig_h) || params.watermark.is_some();

    if params.target_kb > 0 {
        return optimize_target(&rgba, src_ext, params, original, w, h, pixels_changed);
    }

    match params.format {
        OutputFormat::Auto => {
            let (fmt, data) = optimize_auto(&rgba, src_ext, params.quality)?;
            Ok(finish(fmt, data, w, h, original, src_ext, pixels_changed, true))
        }
        OutputFormat::Keep => {
            let fmt = EncFmt::keep_from_ext(src_ext);
            let data = encode_guarded(&rgba, fmt, params.quality, original.len(), pixels_changed)?;
            Ok(finish(fmt, data, w, h, original, src_ext, pixels_changed, false))
        }
        OutputFormat::Jpeg | OutputFormat::Png | OutputFormat::Webp => {
            let fmt = match params.format {
                OutputFormat::Jpeg => EncFmt::Jpeg,
                OutputFormat::Png => EncFmt::Png,
                OutputFormat::Webp => EncFmt::Webp,
                _ => unreachable!(),
            };
            let data = encode_guarded(&rgba, fmt, params.quality, original.len(), pixels_changed)?;
            Ok(finish(fmt, data, w, h, original, src_ext, pixels_changed, false))
        }
    }
}

/// 结果收口:未改动像素时按节省阈值决定是否保留原图。
/// auto 模式要求"有意义节省"(绝对字节 + 2% 双阈值);手动模式只要严格变小。
fn finish(
    fmt: EncFmt,
    data: Vec<u8>,
    w: u32,
    h: u32,
    original: &[u8],
    src_ext: &str,
    pixels_changed: bool,
    auto: bool,
) -> CompressOutcome {
    if pixels_changed {
        return CompressOutcome {
            data,
            ext: fmt.ext().to_string(),
            format: fmt.name().to_string(),
            width: w,
            height: h,
            kept: false,
            animated: false,
        };
    }
    let worthwhile = if auto {
        meaningful_savings(original.len(), data.len())
    } else {
        data.len() < original.len()
    };
    if worthwhile {
        CompressOutcome {
            data,
            ext: fmt.ext().to_string(),
            format: fmt.name().to_string(),
            width: w,
            height: h,
            kept: false,
            animated: false,
        }
    } else {
        CompressOutcome {
            data: original.to_vec(),
            ext: src_ext.to_string(),
            format: ext_format_name(src_ext),
            width: w,
            height: h,
            kept: true,
            animated: false,
        }
    }
}

/// 有意义节省:严格更小,且绝对省量达标(<32KB 文件 96B,否则 256B),且相对节省 ≥2%。
pub(crate) fn meaningful_savings(orig: usize, new: usize) -> bool {
    if new >= orig {
        return false;
    }
    let saved = orig - new;
    let min_abs = if orig < 32 * 1024 { 96 } else { 256 };
    saved >= min_abs && saved * 50 >= orig
}

/* ---------------- 智能择优(竞速) ---------------- */

/// 多格式并行竞速,取字节数最小者。
/// 候选:源格式(透明图排除 JPEG)→ WebP → PNG;不透明补 JPEG。
/// PNG 候选用 BoundedWriter 早停:写入超过当前最优即中止。
fn optimize_auto(rgba: &RgbaImage, src_ext: &str, quality: u8) -> Result<(EncFmt, Vec<u8>)> {
    let opaque = is_opaque(rgba);
    let mut candidates: Vec<EncFmt> = Vec::with_capacity(4);
    let src_fmt = EncFmt::keep_from_ext(src_ext);
    if !(src_fmt == EncFmt::Jpeg && !opaque) {
        candidates.push(src_fmt);
    }
    for f in [EncFmt::Webp, EncFmt::Png] {
        if !candidates.contains(&f) {
            candidates.push(f);
        }
    }
    if opaque && !candidates.contains(&EncFmt::Jpeg) {
        candidates.push(EncFmt::Jpeg);
    }

    let best_size = AtomicUsize::new(usize::MAX);
    let best: Mutex<Option<(EncFmt, Vec<u8>)>> = Mutex::new(None);
    std::thread::scope(|s| {
        let best_ref = &best;
        let size_ref = &best_size;
        for fmt in candidates {
            s.spawn(move || {
                let res = match fmt {
                    EncFmt::Png => encode_png_into(rgba, quality, Some(size_ref))
                        .map_err(|_| OsError::Other("PNG 编码失败".into())),
                    _ => encode_fmt(rgba, fmt, quality),
                };
                if let Ok(data) = res {
                    let n = data.len();
                    if n < size_ref.load(Ordering::Relaxed) {
                        let mut guard = best_ref.lock().expect("best lock");
                        if n < size_ref.load(Ordering::Relaxed) {
                            size_ref.store(n, Ordering::Relaxed);
                            *guard = Some((fmt, data));
                        }
                    }
                }
            });
        }
    });
    best.into_inner()
        .expect("best lock")
        .ok_or_else(|| OsError::Other("图片编码失败".into()))
}

/* ---------------- 目标体积模式 ---------------- */

/// 外层缩放梯度 × 内层质量二分搜索,返回首个达标的最大质量结果。
/// 全部失败时返回过程中最小候选。
fn optimize_target(
    rgba: &RgbaImage,
    src_ext: &str,
    params: &CompressParams,
    original: &[u8],
    w: u32,
    h: u32,
    pixels_changed: bool,
) -> Result<CompressOutcome> {
    let target = ((params.target_kb as u64 * 1000) as f64 * 0.98) as usize;
    let target = target.max(512);

    // 先无约束编码一次,锁定输出格式(auto 时即智能择优的胜者)
    let (fmt, initial) = match params.format {
        OutputFormat::Auto => optimize_auto(rgba, src_ext, params.quality)?,
        OutputFormat::Keep => {
            let f = EncFmt::keep_from_ext(src_ext);
            let d = encode_fmt(rgba, f, params.quality)?;
            (f, d)
        }
        f => {
            let fmt = match f {
                OutputFormat::Jpeg => EncFmt::Jpeg,
                OutputFormat::Png => EncFmt::Png,
                OutputFormat::Webp => EncFmt::Webp,
                _ => unreachable!(),
            };
            let d = encode_fmt(rgba, fmt, params.quality)?;
            (fmt, d)
        }
    };

    // 目标模式下 PNG 恒走调色板有损路径,给质量搜索留空间
    let enc = |img: &RgbaImage, q: u8| -> Result<Vec<u8>> {
        match fmt {
            EncFmt::Png => encode_png_opt(img, q, true),
            _ => encode_fmt(img, fmt, q),
        }
    };

    if initial.len() <= target {
        return Ok(finish(
            fmt, initial, w, h, original, src_ext, pixels_changed, false,
        ));
    }

    let mut scaled: Option<RgbaImage> = None;
    let mut fallback = (initial, w, h);
    for &scale in &SCALE_LADDER {
        let cur: &RgbaImage = if scale < 1.0 {
            let nw = ((w as f32 * scale).round() as u32).max(1);
            let nh = ((h as f32 * scale).round() as u32).max(1);
            scaled.insert(resize_rgba(rgba, nw, nh, FilterType::Lanczos3))
        } else {
            rgba
        };
        let (cw, ch) = cur.dimensions();
        // 可行性探测:最小质量仍超目标则直接降分辨率
        let probe = enc(cur, 8)?;
        if probe.len() > target {
            if probe.len() < fallback.0.len() {
                fallback = (probe, cw, ch);
            }
            continue;
        }
        // 质量二分 [9, user_quality],取 ≤target 的最大质量
        let mut lo: u8 = 9;
        let mut hi: u8 = params.quality.max(9);
        let mut best_data = probe;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let d = enc(cur, mid)?;
            if d.len() <= target {
                best_data = d;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        return Ok(CompressOutcome {
            data: best_data,
            ext: fmt.ext().to_string(),
            format: fmt.name().to_string(),
            width: cw,
            height: ch,
            kept: false,
            animated: false,
        });
    }
    // 全部梯度未达标:返回过程中最小候选
    let (data, fw, fh) = fallback;
    Ok(CompressOutcome {
        data,
        ext: fmt.ext().to_string(),
        format: fmt.name().to_string(),
        width: fw,
        height: fh,
        kept: false,
        animated: false,
    })
}

/* ---------------- 手动模式防变大 ---------------- */

/// 手动/保持原格式编码:结果不小于原图且像素未变时,按梯度降质重编取首个更小者。
fn encode_guarded(
    rgba: &RgbaImage,
    fmt: EncFmt,
    quality: u8,
    original_size: usize,
    pixels_changed: bool,
) -> Result<Vec<u8>> {
    let data = encode_fmt(rgba, fmt, quality)?;
    if pixels_changed || data.len() < original_size {
        return Ok(data);
    }
    for step in GUARDED_STEPS {
        let q = quality.saturating_sub(step).max(1);
        if q == quality {
            continue;
        }
        let d = encode_fmt(rgba, fmt, q)?;
        if d.len() < original_size {
            return Ok(d);
        }
    }
    Ok(data)
}

/* ---------------- 编码器 ---------------- */

/// 按格式编码(缩略图生成等外部使用)。
pub fn encode_fmt(img: &RgbaImage, fmt: EncFmt, quality: u8) -> Result<Vec<u8>> {
    match fmt {
        EncFmt::Jpeg => encode_jpeg(img, quality),
        EncFmt::Webp => encode_webp(img, quality),
        EncFmt::Png => encode_png_opt(img, quality, false),
    }
}

fn encode_jpeg(img: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let rgb = flatten_white(img);
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, quality.max(1));
    encoder
        .write_image(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)
        .map_err(|e| OsError::Other(format!("JPEG 编码失败: {e}")))?;
    Ok(out)
}

fn encode_webp(img: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let mut cfg = webp::WebPConfig::new().map_err(|_| OsError::Other("WebP 配置失败".into()))?;
    if quality >= 100 {
        cfg.lossless = 1;
    } else {
        cfg.quality = f32::from(quality.max(1));
        cfg.method = 4;
        cfg.alpha_quality = i32::from(quality).clamp(35, 100);
        cfg.thread_level = 1;
    }
    let encoder = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height());
    let mem = encoder
        .encode_advanced(&cfg)
        .map_err(|e| OsError::Other(format!("WebP 编码失败: {e:?}")))?;
    if mem.is_empty() {
        return Err(OsError::Other("WebP 编码失败".into()));
    }
    Ok(mem.to_vec())
}

/// PNG 编码:quality ≥90 无损(Best 压缩 + 自适应过滤);<90 NeuQuant 调色板有损。
/// force_lossy 用于目标体积模式:恒走调色板路径,让质量参与二分搜索。
fn encode_png_opt(img: &RgbaImage, quality: u8, force_lossy: bool) -> Result<Vec<u8>> {
    let q = if force_lossy { quality.min(89) } else { quality };
    encode_png_into(img, q, None).map_err(|e| OsError::Other(format!("PNG 编码失败: {e}")))
}

/// 实际 PNG 编码入口:可选写入早停(超过 best_size 即中止)。
fn encode_png_into(
    img: &RgbaImage,
    quality: u8,
    limit: Option<&AtomicUsize>,
) -> std::io::Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let mut sink = ByteSink {
        limit,
        buf: Vec::new(),
    };
    let mut encoder = png::Encoder::new(&mut sink, w, h);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder.set_filter(png::Filter::Adaptive);

    let lossless = quality >= 90;
    if lossless {
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(img.as_raw())?;
        writer.finish()?;
    } else {
        // 有损调色板:NeuQuant,颜色数随质量映射(64-256,1.35 次幂曲线)
        let colors = palette_colors(quality);
        let (palette, trns, indexed) = quantize_indexed(img, colors);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_palette(&palette);
        encoder.set_trns(&trns);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&indexed)?;
        writer.finish()?;
    }
    Ok(sink.buf)
}

/// 质量滑杆 → NeuQuant 调色板颜色数。
pub(crate) fn palette_colors(quality: u8) -> usize {
    let q = f64::from(quality) / 100.0;
    (64.0 + 192.0 * q.powf(1.35)).round().clamp(64.0, 256.0) as usize
}

/// RGBA 图 → (PLTE 字节, tRNS 字节, 索引数据)。
fn quantize_indexed(img: &RgbaImage, colors: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let nq = color_quant::NeuQuant::new(10, colors, img.as_raw());
    let mut indexed = Vec::with_capacity(img.as_raw().len() / 4);
    for px in img.as_raw().chunks_exact(4) {
        indexed.push(nq.index_of(px) as u8);
    }
    let rgba_map = nq.color_map_rgba();
    let mut palette = Vec::with_capacity(rgba_map.len() / 4 * 3);
    let mut trns = Vec::with_capacity(rgba_map.len() / 4);
    for px in rgba_map.chunks_exact(4) {
        palette.extend_from_slice(&px[..3]);
        trns.push(px[3]);
    }
    (palette, trns, indexed)
}

/// 带可选上限的写入缓冲:超限立即报错,中止 PNG 编码(竞速早停)。
struct ByteSink<'a> {
    limit: Option<&'a AtomicUsize>,
    buf: Vec<u8>,
}

impl Write for ByteSink<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let next = self.buf.len() + data.len();
        if let Some(limit) = self.limit {
            if next > limit.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("png candidate aborted"));
            }
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/* ---------------- 像素工具 ---------------- */

/// 解码任意图片为 RGBA,并应用 EXIF 方向(其余元数据因重编码自然剥离)。
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| OsError::Other(format!("图片格式识别失败: {e}")))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| OsError::Other(format!("图片解码失败: {e}")))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|e| OsError::Other(format!("图片解码失败: {e}")))?;
    img.apply_orientation(orientation);
    Ok(img.to_rgba8())
}

/// 长边超限时等比缩小(不放大)。
pub(crate) fn apply_max_dim(img: RgbaImage, max_dim: u32) -> RgbaImage {
    if max_dim == 0 {
        return img;
    }
    let (w, h) = img.dimensions();
    let m = w.max(h);
    if m == 0 || m <= max_dim {
        return img;
    }
    let scale = f64::from(max_dim) / f64::from(m);
    let nw = ((f64::from(w) * scale).round() as u32).max(1);
    let nh = ((f64::from(h) * scale).round() as u32).max(1);
    resize_rgba(&img, nw, nh, FilterType::Lanczos3)
}

/// 高质量缩放(fast_image_resize)。
pub fn resize_rgba(img: &RgbaImage, nw: u32, nh: u32, filter: FilterType) -> RgbaImage {
    let src = FirImage::from_vec_u8(img.width(), img.height(), img.as_raw().clone(), PixelType::U8x4)
        .expect("resize source buffer");
    let mut dst = FirImage::new(nw, nh, PixelType::U8x4);
    let mut resizer = Resizer::new();
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(filter));
    resizer
        .resize(&src, &mut dst, Some(&opts))
        .expect("resize target buffer");
    ImageBuffer::from_raw(nw, nh, dst.buffer().to_vec()).expect("resize output")
}

/// 是否完全不透明(JPEG 编码无需铺底判断依据)。
pub(crate) fn is_opaque(img: &RgbaImage) -> bool {
    img.pixels().all(|p: &Rgba<u8>| p[3] == 255)
}

/// 透明区域铺白底,输出 RGB(JPEG 无 alpha 通道)。
pub(crate) fn flatten_white(img: &RgbaImage) -> ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    let (w, h) = img.dimensions();
    let mut out = ImageBuffer::new(w, h);
    for (x, y, p) in img.enumerate_pixels() {
        let a = f32::from(p[3]) / 255.0;
        let blend = |s: u8| -> u8 { (f32::from(s) * a + 255.0 * (1.0 - a)).round() as u8 };
        out.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let img: RgbaImage = ImageBuffer::from_fn(w, h, |_, _| Rgba(color));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn gradient_jpeg(w: u32, h: u32, quality: u8) -> Vec<u8> {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(
                    x,
                    y,
                    Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128]),
                );
            }
        }
        let mut out = Vec::new();
        let enc = JpegEncoder::new_with_quality(&mut out, quality);
        enc.write_image(img.as_raw(), w, h, ExtendedColorType::Rgb8).unwrap();
        out
    }

    #[test]
    fn decode_applies_size() {
        let img = decode_rgba(&solid_png(7, 5, [255, 0, 0, 255])).unwrap();
        assert_eq!(img.dimensions(), (7, 5));
        assert!(is_opaque(&img));
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_rgba(b"not an image").is_err());
    }

    #[test]
    fn jpeg_encoder_smaller_at_lower_quality() {
        let big = encode_jpeg(&decode_rgba(&gradient_jpeg(200, 200, 95)).unwrap(), 95).unwrap();
        let small = encode_jpeg(&decode_rgba(&gradient_jpeg(200, 200, 95)).unwrap(), 40).unwrap();
        assert!(small.len() < big.len());
    }

    #[test]
    fn webp_encode_valid_header() {
        let img = decode_rgba(&gradient_jpeg(64, 64, 90)).unwrap();
        let data = encode_webp(&img, 80).unwrap();
        assert_eq!(&data[..4], b"RIFF");
        assert_eq!(&data[8..12], b"WEBP");
    }

    #[test]
    fn png_lossless_vs_palette() {
        let img = decode_rgba(&gradient_jpeg(128, 128, 90)).unwrap();
        let lossless = encode_png_opt(&img, 95, false).unwrap();
        let lossy = encode_png_opt(&img, 30, true).unwrap();
        assert_eq!(&lossless[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&lossy[..8], b"\x89PNG\r\n\x1a\n");
        assert!(lossy.len() < lossless.len());
    }

    #[test]
    fn palette_colors_mapping() {
        assert_eq!(palette_colors(0), 64);
        assert_eq!(palette_colors(100), 256);
        assert!(palette_colors(50) > 64 && palette_colors(50) < 256);
    }

    #[test]
    fn meaningful_savings_threshold() {
        // <32KB:省 ≥96B 且 ≥2%
        assert!(!meaningful_savings(10_000, 9_910)); // 省 90B 绝对值不足
        assert!(!meaningful_savings(10_000, 9_900)); // 省 100B = 1%,相对不足
        assert!(meaningful_savings(10_000, 9_700)); // 省 300B = 3%
        // ≥32KB:省 ≥256B 且 ≥2%
        assert!(!meaningful_savings(40_000, 39_750)); // 省 250B 绝对值不足
        assert!(!meaningful_savings(40_000, 39_700)); // 省 300B = 0.75%,相对不足
        assert!(meaningful_savings(40_000, 38_000)); // 省 2000B = 5%
        assert!(!meaningful_savings(1_000, 1_000));
    }

    #[test]
    fn auto_race_picks_smallest_format() {
        // 平滑渐变图:JPEG/WebP 应显著小于无损 PNG
        let params = CompressParams {
            format: OutputFormat::Auto,
            ..CompressParams::default()
        };
        let original = gradient_jpeg(300, 200, 92);
        let out = optimize(&original, "jpg", &params).unwrap();
        assert!(!out.kept);
        assert!(matches!(out.format.as_str(), "jpeg" | "webp"));
        assert!(out.data.len() < original.len());
    }

    #[test]
    fn keep_mode_tiny_png_stays_original() {
        // 1x1 PNG 重编码必然更大,应保留原图
        let original = solid_png(1, 1, [0, 0, 0, 255]);
        let params = CompressParams {
            format: OutputFormat::Keep,
            ..CompressParams::default()
        };
        let out = optimize(&original, "png", &params).unwrap();
        assert!(out.kept);
        assert_eq!(out.data.len(), original.len());
    }

    #[test]
    fn max_dim_scales_down() {
        let img = decode_rgba(&solid_png(400, 200, [0, 0, 0, 255])).unwrap();
        let scaled = apply_max_dim(img, 100);
        assert_eq!(scaled.dimensions(), (100, 50));
        let img = decode_rgba(&solid_png(50, 50, [0, 0, 0, 255])).unwrap();
        assert_eq!(apply_max_dim(img, 100).dimensions(), (50, 50));
    }

    #[test]
    fn target_size_converges() {
        let original = gradient_jpeg(1200, 800, 95);
        let params = CompressParams {
            format: OutputFormat::Auto,
            target_kb: 30,
            ..CompressParams::default()
        };
        let out = optimize(&original, "jpg", &params).unwrap();
        assert!(out.data.len() <= 30 * 1000, "got {} bytes", out.data.len());
        assert!(!out.kept);
    }

    #[test]
    fn target_size_easy_case() {
        // 宽松目标:初次编码即达标(原图 q100 偏大,重编码明显更小)
        let original = gradient_jpeg(600, 400, 100);
        let params = CompressParams {
            format: OutputFormat::Keep,
            target_kb: 200,
            ..CompressParams::default()
        };
        let out = optimize(&original, "jpg", &params).unwrap();
        assert!(!out.kept);
        assert!(out.data.len() < original.len());
        assert!(out.data.len() <= 200 * 1000);
    }

    #[test]
    fn resize_produces_requested_size() {
        let img = decode_rgba(&gradient_jpeg(320, 240, 90)).unwrap();
        let out = resize_rgba(&img, 160, 120, FilterType::Lanczos3);
        assert_eq!(out.dimensions(), (160, 120));
    }

    /// 大图端到端冒烟:2000×1400 渐变+噪声 JPEG,智能择优默认参数应明显变小;
    /// 目标体积模式应收敛到目标附近。
    #[test]
    fn large_photo_smoke() {
        let w = 2000usize;
        let h = 1400usize;
        let mut img = RgbImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7 + y * 13 + x * y / 97) % 31) as u8;
                img.put_pixel(
                    x as u32,
                    y as u32,
                    Rgb([
                        ((x * 255 / w) as u8).saturating_sub(n / 2),
                        ((y * 255 / h) as u8).saturating_add(n / 3),
                        128u8.saturating_sub(n),
                    ]),
                );
            }
        }
        let mut original = Vec::new();
        let enc = JpegEncoder::new_with_quality(&mut original, 95);
        enc.write_image(img.as_raw(), w as u32, h as u32, ExtendedColorType::Rgb8).unwrap();
        assert!(original.len() > 200 * 1024, "source too small: {}", original.len());

        let t0 = std::time::Instant::now();
        let out = optimize(&original, "jpg", &CompressParams::default()).unwrap();
        let elapsed = t0.elapsed();
        assert!(!out.kept);
        assert!(out.data.len() < original.len() / 2, "auto: {} → {}", original.len(), out.data.len());

        let params = CompressParams {
            target_kb: 150,
            ..CompressParams::default()
        };
        let out2 = optimize(&original, "jpg", &params).unwrap();
        assert!(out2.data.len() <= 155 * 1000, "target: {} bytes", out2.data.len());
        // 智能择优耗时合理(竞速 + 目标体积搜索)
        assert!(elapsed.as_secs() < 30, "too slow: {elapsed:?}");
    }
}
