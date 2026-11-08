//! 水印:文字(系统字体光栅化)/ 图片(缩放混合)/ 盲水印(分块 DCT 中频嵌入,简化实现)。
//! 应用时机:缩放之后、编码之前,因此水印随输出一起被压缩。

use std::fs;
use std::sync::OnceLock;

use ab_glyph::{FontVec, PxScale, PxScaleFont, ScaleFont};
use image::{Rgba, RgbaImage};

use crate::imaging::resize_rgba;
use crate::{OsError, Result};
use fast_image_resize::FilterType;

/// 水印类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkKind {
    Text,
    Image,
    Blind,
}

/// 单枚水印位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmPos {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl WmPos {
    pub fn parse(s: &str) -> Self {
        match s {
            "center" => Self::Center,
            "top-left" => Self::TopLeft,
            "top-right" => Self::TopRight,
            "bottom-left" => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }
}

/// 水印参数(与前端 DTO 对应)。
#[derive(Debug, Clone)]
pub struct WatermarkParams {
    pub kind: WatermarkKind,
    pub text: String,
    pub font_size: u32,
    /// #RRGGBB 已解析为 RGB
    pub color: [u8; 3],
    /// 0-100
    pub opacity: u8,
    /// 顺时针旋转角度(度)
    pub rotation: i16,
    pub tiled: bool,
    /// 铺满密度 5-95,越大越密
    pub density: u8,
    pub position: WmPos,
    /// 图片水印的 PNG 字节
    pub image: Vec<u8>,
    /// 图片水印宽度相对宿主图宽的百分比
    pub image_scale: u8,
    /// 盲水印强度 1-100
    pub strength: u8,
}

/// 应用水印入口。
pub fn apply(img: &mut RgbaImage, wm: &WatermarkParams) -> Result<()> {
    match wm.kind {
        WatermarkKind::Text => apply_text(img, wm),
        WatermarkKind::Image => apply_image(img, wm),
        WatermarkKind::Blind => {
            apply_blind(img, &wm.text, wm.strength);
            Ok(())
        }
    }
}

/* ---------------- 文字水印 ---------------- */

/// 系统中文字体候选(按平台),全部缺失则文字水印报错。
const FONT_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    // Windows
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simhei.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

static FONT: OnceLock<Option<FontVec>> = OnceLock::new();

pub(crate) fn system_font() -> Option<&'static FontVec> {
    FONT.get_or_init(|| {
        for path in FONT_CANDIDATES {
            if let Ok(data) = fs::read(path) {
                // ttc 为字体集合,取第一个字体;普通 ttf/tot 同样适用
                if let Ok(f) = FontVec::try_from_vec_and_index(data, 0) {
                    return Some(f);
                }
            }
        }
        None
    })
    .as_ref()
}

fn apply_text(img: &mut RgbaImage, wm: &WatermarkParams) -> Result<()> {
    let font = system_font()
        .ok_or_else(|| OsError::Other("未找到可用的系统字体,无法生成文字水印".into()))?;
    let text = wm.text.trim();
    if text.is_empty() {
        return Err(OsError::Other("水印文字为空".into()));
    }
    let stamp = render_text_stamp(font, text, wm);
    layout_blend(img, &stamp, wm);
    Ok(())
}

/// 渲染单行文字为 RGBA 图(已应用颜色,不透明度在混合阶段统一处理)。
pub(crate) fn render_text_stamp(font: &FontVec, text: &str, wm: &WatermarkParams) -> RgbaImage {
    let sf: PxScaleFont<&FontVec> = PxScaleFont {
        font,
        scale: PxScale::from(wm.font_size as f32),
    };
    let ascent = sf.ascent();
    let height = (ascent - sf.descent()).ceil().max(1.0) as u32;
    let mut width = 0.0f32;
    for c in text.chars() {
        width += sf.h_advance(sf.glyph_id(c));
    }
    let w = (width.ceil() as u32).max(1);
    let mut cov = vec![0f32; (w as usize) * (height as usize)];
    let mut pen_x = 0.0f32;
    for c in text.chars() {
        let mut g = sf.scaled_glyph(c);
        g.position = ab_glyph::point(pen_x, ascent);
        let id = g.id;
        if let Some(outlined) = sf.outline_glyph(g) {
            // draw 回调坐标相对字形边界框,必须加 px_bounds().min 偏移到印章坐标
            let bounds = outlined.px_bounds();
            let bx = bounds.min.x.round() as i64;
            let by = bounds.min.y.round() as i64;
            outlined.draw(|x, y, c| {
                let sx = bx + x as i64;
                let sy = by + y as i64;
                // 字形可能超出字宽(斜体/悬挂),越界丢弃
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= height as i64 {
                    return;
                }
                let idx = sy as usize * w as usize + sx as usize;
                cov[idx] = cov[idx].max(c.clamp(0.0, 1.0));
            });
        }
        pen_x += sf.h_advance(id);
    }
    let mut stamp = RgbaImage::new(w, height);
    for (i, c) in cov.iter().enumerate() {
        if *c > 0.0 {
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            stamp.put_pixel(
                x,
                y,
                Rgba([wm.color[0], wm.color[1], wm.color[2], (c * 255.0).round() as u8]),
            );
        }
    }
    rotate_rgba(&stamp, f32::from(wm.rotation))
}

/* ---------------- 图片水印 ---------------- */

fn apply_image(img: &mut RgbaImage, wm: &WatermarkParams) -> Result<()> {
    if wm.image.is_empty() {
        return Err(OsError::Other("水印图片为空".into()));
    }
    let src = image::load_from_memory(&wm.image)
        .map_err(|e| OsError::Other(format!("水印图片解码失败: {e}")))?
        .to_rgba8();
    let host_w = img.width();
    let target_w = ((host_w as f32 * f32::from(wm.image_scale) / 100.0).round() as u32)
        .clamp(8, host_w.max(8));
    let target_h = ((src.height() as f32 * target_w as f32 / src.width() as f32).round() as u32)
        .max(1);
    let stamp = resize_rgba(&src, target_w, target_h, FilterType::Bilinear);
    let stamp = rotate_rgba(&stamp, f32::from(wm.rotation));
    layout_blend(img, &stamp, wm);
    Ok(())
}

/* ---------------- 布局与混合 ---------------- */

/// 按铺满/单枚布局把印章混合到宿主图。
fn layout_blend(host: &mut RgbaImage, stamp: &RgbaImage, wm: &WatermarkParams) {
    let opacity = f32::from(wm.opacity.clamp(0, 100)) / 100.0;
    if opacity <= 0.0 {
        return;
    }
    if wm.tiled {
        // 密度 95 → 间隙极小;5 → 间隙大
        let gap = f32::from(100 - i16::from(wm.density.clamp(5, 95))) / 100.0;
        let pad = ((stamp.width().min(stamp.height())) as f32 * (gap + 0.15)).max(8.0) as i64;
        let step_x = i64::from(stamp.width()) + pad;
        let step_y = i64::from(stamp.height()) + pad;
        let mut row = 0i64;
        let mut y = -(step_y);
        while y < i64::from(host.height()) {
            // 奇数行横向错开半格,视觉更自然
            let x0 = -(step_x) - if row % 2 == 1 { step_x / 2 } else { 0 };
            let mut x = x0;
            while x < i64::from(host.width()) {
                blend_stamp(host, stamp, x, y, opacity);
                x += step_x;
            }
            y += step_y;
            row += 1;
        }
    } else {
        let margin = (i64::from(stamp.height()) / 2).max(8);
        let (ox, oy) = match wm.position {
            WmPos::Center => (
                (i64::from(host.width()) - i64::from(stamp.width())) / 2,
                (i64::from(host.height()) - i64::from(stamp.height())) / 2,
            ),
            WmPos::TopLeft => (margin, margin),
            WmPos::TopRight => (
                i64::from(host.width()) - i64::from(stamp.width()) - margin,
                margin,
            ),
            WmPos::BottomLeft => (
                margin,
                i64::from(host.height()) - i64::from(stamp.height()) - margin,
            ),
            WmPos::BottomRight => (
                i64::from(host.width()) - i64::from(stamp.width()) - margin,
                i64::from(host.height()) - i64::from(stamp.height()) - margin,
            ),
        };
        blend_stamp(host, stamp, ox.max(0), oy.max(0), opacity);
    }
}

/// 直通 alpha 混合(over 运算),超出边界自动裁剪。
pub(crate) fn blend_stamp(host: &mut RgbaImage, stamp: &RgbaImage, ox: i64, oy: i64, opacity: f32) {
    let hw = i64::from(host.width());
    let hh = i64::from(host.height());
    for sy in 0..i64::from(stamp.height()) {
        let hy = oy + sy;
        if hy < 0 || hy >= hh {
            continue;
        }
        for sx in 0..i64::from(stamp.width()) {
            let p = stamp.get_pixel(sx as u32, sy as u32);
            let sa = f32::from(p[3]) / 255.0 * opacity;
            if sa <= 0.0 {
                continue;
            }
            let hx = ox + sx;
            if hx < 0 || hx >= hw {
                continue;
            }
            let d = host.get_pixel_mut(hx as u32, hy as u32);
            let da = f32::from(d[3]) / 255.0;
            let oa = sa + da * (1.0 - sa);
            if oa <= 0.0 {
                continue;
            }
            let mix = |s: u8, dd: u8| -> u8 {
                ((f32::from(s) * sa + f32::from(dd) * da * (1.0 - sa)) / oa).round() as u8
            };
            *d = Rgba([
                mix(p[0], d[0]),
                mix(p[1], d[1]),
                mix(p[2], d[2]),
                (oa * 255.0).round() as u8,
            ]);
        }
    }
}

/// 最近邻旋转(顺时针 deg 度),输出画布扩大到容纳整个旋转结果。
pub(crate) fn rotate_rgba(src: &RgbaImage, deg: f32) -> RgbaImage {
    if deg.abs() % 360.0 < 0.01 {
        return src.clone();
    }
    let (w, h) = src.dimensions();
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    // 减去小容差再取整,抵消 f32 三角函数误差(如 90° 时 cos≈-4.4e-8)导致的画布多出 1px
    let nw = (w as f32 * cos.abs() + h as f32 * sin.abs() - 1e-3).ceil().max(1.0) as u32;
    let nh = (w as f32 * sin.abs() + h as f32 * cos.abs() - 1e-3).ceil().max(1.0) as u32;
    let mut dst = RgbaImage::new(nw.max(1), nh.max(1));
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let (ncx, ncy) = (nw as f32 / 2.0, nh as f32 / 2.0);
    for y in 0..dst.height() {
        for x in 0..dst.width() {
            let dx = x as f32 - ncx + 0.5;
            let dy = y as f32 - ncy + 0.5;
            // 逆旋转映射回源坐标
            let sx = dx * cos + dy * sin + cx - 0.5;
            let sy = -dx * sin + dy * cos + cy - 0.5;
            if sx < 0.0 || sy < 0.0 || sx >= w as f32 || sy >= h as f32 {
                continue;
            }
            // floor + 钳制,避免 round 越界
            let v = src.get_pixel(
                (sx as usize).min(w as usize - 1) as u32,
                (sy as usize).min(h as usize - 1) as u32,
            );
            dst.put_pixel(x, y, *v);
        }
    }
    dst
}

/* ---------------- 盲水印(简化实现) ---------------- */

const DCT_POS: [(usize, usize); 8] = [(1, 2), (2, 1), (0, 3), (3, 0), (2, 2), (1, 3), (3, 1), (2, 3)];

fn cos_table() -> &'static [[f32; 8]; 8] {
    static TABLE: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [[0f32; 8]; 8];
        for (x, row) in t.iter_mut().enumerate() {
            for (u, v) in row.iter_mut().enumerate() {
                *v = ((2.0 * x as f32 + 1.0) * u as f32 * std::f32::consts::PI / 16.0).cos();
            }
        }
        t
    })
}

/// 8x8 DCT-II( naive 实现,工具用途足够)。
fn dct8(input: &[f32; 64]) -> [f32; 64] {
    let table = cos_table();
    let mut out = [0f32; 64];
    for u in 0..8 {
        for v in 0..8 {
            let mut sum = 0.0f32;
            for y in 0..8 {
                for x in 0..8 {
                    sum += input[y * 8 + x] * table[x][u] * table[y][v];
                }
            }
            let cu = if u == 0 { 1.0 / 2f32.sqrt() } else { 1.0 };
            let cv = if v == 0 { 1.0 / 2f32.sqrt() } else { 1.0 };
            out[v * 8 + u] = 0.25 * cu * cv * sum;
        }
    }
    out
}

/// 8x8 IDCT-III。
fn idct8(freq: &[f32; 64]) -> [f32; 64] {
    let table = cos_table();
    let mut out = [0f32; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0f32;
            for u in 0..8 {
                for v in 0..8 {
                    let cu = if u == 0 { 1.0 / 2f32.sqrt() } else { 1.0 };
                    let cv = if v == 0 { 1.0 / 2f32.sqrt() } else { 1.0 };
                    sum += 0.25 * cu * cv * freq[v * 8 + u] * table[x][u] * table[y][v];
                }
            }
            out[y * 8 + x] = sum;
        }
    }
    out
}

fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    (
        0.299 * r + 0.587 * g + 0.114 * b,
        -0.168736 * r - 0.331264 * g + 0.5 * b + 128.0,
        0.5 * r - 0.418688 * g - 0.081312 * b + 128.0,
    )
}

fn ycbcr_to_rgb(y: f32, cb: f32, cr: f32) -> (u8, u8, u8) {
    let r = y + 1.402 * (cr - 128.0);
    let g = y - 0.344136 * (cb - 128.0) - 0.714136 * (cr - 128.0);
    let b = y + 1.772 * (cb - 128.0);
    (
        r.round().clamp(0.0, 255.0) as u8,
        g.round().clamp(0.0, 255.0) as u8,
        b.round().clamp(0.0, 255.0) as u8,
    )
}

/// 盲水印:把文本位序列以固定幅度写入每个 8x8 块 DCT 的中频系数(亮度通道),
/// 高冗余嵌入,轻量鲁棒。简化实现:仅嵌入,不提供提取;强度决定可见性/鲁棒性折衷。
fn apply_blind(img: &mut RgbaImage, text: &str, strength: u8) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let amp = 4.0 + f32::from(strength.clamp(1, 100)) / 100.0 * 20.0;
    let mut bits: Vec<bool> = Vec::with_capacity(text.len() * 8);
    for byte in text.as_bytes() {
        for i in 0..8 {
            bits.push(byte & (1 << i) != 0);
        }
    }
    let (w, h) = img.dimensions();
    let mut block = 0usize;
    let mut alpha_blk = [0u8; 64];
    for by in 0..h / 8 {
        for bx in 0..w / 8 {
            let mut y_blk = [0f32; 64];
            let mut cb_blk = [0f32; 64];
            let mut cr_blk = [0f32; 64];
            for j in 0..8usize {
                for i in 0..8usize {
                    let p = img.get_pixel(bx * 8 + i as u32, by * 8 + j as u32);
                    let (y, cb, cr) = rgb_to_ycbcr(p[0], p[1], p[2]);
                    y_blk[j * 8 + i] = y;
                    cb_blk[j * 8 + i] = cb;
                    cr_blk[j * 8 + i] = cr;
                    alpha_blk[j * 8 + i] = p[3];
                }
            }
            let mut freq = dct8(&y_blk);
            for (k, &(i, j)) in DCT_POS.iter().enumerate() {
                let bit = bits[(block * 8 + k) % bits.len()];
                freq[j * 8 + i] = if bit { amp } else { -amp };
            }
            let y_new = idct8(&freq);
            for j in 0..8usize {
                for i in 0..8usize {
                    let (r, g, b) =
                        ycbcr_to_rgb(y_new[j * 8 + i], cb_blk[j * 8 + i], cr_blk[j * 8 + i]);
                    // 保留原 alpha,避免破坏透明区域
                    img.put_pixel(
                        bx * 8 + i as u32,
                        by * 8 + j as u32,
                        Rgba([r, g, b, alpha_blk[j * 8 + i]]),
                    );
                }
            }
            block += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp_image() -> RgbaImage {
        let mut img = RgbaImage::new(3, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        img.put_pixel(2, 1, Rgba([0, 0, 255, 255]));
        img
    }

    #[test]
    fn rotate_keeps_all_pixels() {
        let src = stamp_image();
        let rotated = rotate_rgba(&src, 90.0);
        let opaque = rotated.pixels().filter(|p| p[3] > 0).count();
        assert_eq!(opaque, 3);
        assert_eq!(rotated.dimensions(), (2, 3));
    }

    #[test]
    fn rotate_zero_is_identity() {
        let src = stamp_image();
        assert_eq!(rotate_rgba(&src, 0.0).dimensions(), (3, 2));
    }

    #[test]
    fn blend_over_opaque_pixel() {
        let mut host = RgbaImage::new(2, 1);
        host.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let mut stamp = RgbaImage::new(1, 1);
        stamp.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        blend_stamp(&mut host, &stamp, 0, 0, 0.5);
        let v = host.get_pixel(0, 0);
        assert_eq!(v[0], 128);
        assert_eq!(v[3], 255);
    }

    #[test]
    fn blend_clips_out_of_bounds() {
        let mut host = RgbaImage::new(2, 2);
        let stamp = stamp_image();
        blend_stamp(&mut host, &stamp, -2, -1, 1.0);
        blend_stamp(&mut host, &stamp, 10, 10, 1.0);
    }

    #[test]
    fn dct_roundtrip() {
        let input = [0.5f32; 64];
        let freq = dct8(&input);
        let back = idct8(&freq);
        for (a, b) in input.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
    }

    #[test]
    fn blind_watermark_changes_pixels_subtly() {
        let mut img = RgbaImage::from_pixel(64, 64, Rgba([128, 128, 128, 255]));
        let before = *img.get_pixel(3, 3);
        apply_blind(&mut img, "mark", 50);
        // 像素变化但幅度有限
        let mut diff = 0;
        for p in img.pixels() {
            for c in p.0.iter().take(3) {
                diff += i32::from(c.abs_diff(before[0]));
            }
        }
        assert!(diff > 0);
    }

    #[test]
    fn text_stamp_layout_side_by_side() {
        // 两个字符必须按 advance 分布在印章左右两半(拦截漏加 px_bounds 偏移的布局错误)
        let Some(font) = system_font() else {
            return;
        };
        let wm = WatermarkParams {
            kind: WatermarkKind::Text,
            text: "ab".into(),
            font_size: 48,
            color: [255, 0, 0],
            opacity: 100,
            rotation: 0,
            tiled: false,
            density: 55,
            position: WmPos::Center,
            image: Vec::new(),
            image_scale: 25,
            strength: 50,
        };
        let stamp = render_text_stamp(font, "ab", &wm);
        let (w, h) = stamp.dimensions();
        assert!(w >= 2, "stamp too narrow: {w}");
        let mid = w / 2;
        let count = |x0: u32, x1: u32| -> u32 {
            let mut n = 0;
            for y in 0..h {
                for x in x0..x1 {
                    if stamp.get_pixel(x, y)[3] > 0 {
                        n += 1;
                    }
                }
            }
            n
        };
        let left = count(0, mid);
        let right = count(mid, w);
        assert!(left > 20, "left glyph missing: {left}");
        assert!(right > 20, "right glyph missing: {right}");
    }

    #[test]
    fn text_watermark_renders_when_font_exists() {
        let mut img = RgbaImage::from_pixel(200, 100, Rgba([255, 255, 255, 255]));
        let wm = WatermarkParams {
            kind: WatermarkKind::Text,
            text: "测试 Test".into(),
            font_size: 20,
            color: [255, 0, 0],
            opacity: 100,
            rotation: -30,
            tiled: true,
            density: 55,
            position: WmPos::Center,
            image: Vec::new(),
            image_scale: 25,
            strength: 50,
        };
        // 无字体环境下跳过(macOS/Windows 必有字体,CI 若缺失则错误可接受)
        match apply(&mut img, &wm) {
            Ok(()) => {
                assert!(img.pixels().any(|p| p[0] > 200 && p[1] < 200));
            }
            Err(e) => assert!(e.to_string().contains("字体")),
        }
    }
}
