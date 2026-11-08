//! 动图压缩管线(GIF / 动图 WebP,参考 PicLite 的分级降载思路):
//! 解码保留累计时间戳 → 质量×帧率分级 × 分辨率收缩,每级 WebP/GIF 双编码取小,
//! 达到目标体积即停。GIF 编码用均匀调色板 + Bayer 有序抖动 + 变化区域裁剪。

use std::borrow::Cow;
use std::io::Cursor;

use fast_image_resize::FilterType;
use image::RgbaImage;

use crate::imaging::{resize_rgba, CompressOutcome, CompressParams};
use crate::{OsError, Result};

/// 单帧解码结果:完整合成画布 + 累计起始时间戳。
struct AnimFrame {
    rgba: RgbaImage,
    start_ms: i32,
}

/// 参与编码的一帧(引用源帧或缩放后的帧)。
struct PickedFrame<'a> {
    rgba: &'a RgbaImage,
    start_ms: i32,
    dur_ms: i32,
}

/// 动图输出格式过滤。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimFmt {
    /// WebP / GIF 竞速取小
    Race,
    WebpOnly,
    GifOnly,
}

/// 帧数上限,防止异常文件耗尽内存。
const MAX_FRAMES: usize = 500;
/// 总像素预算(帧数 × 面积):64M 像素 ≈ 256MB RGBA,超出即停止解码。
const MAX_TOTAL_PIXELS: u64 = 64_000_000;

/* ---------------- 检测 ---------------- */

/// 是否为多帧动图。
pub(crate) fn detect_animated(bytes: &[u8], src_ext: &str) -> bool {
    match src_ext {
        "gif" => gif_frame_count(bytes) > 1,
        "webp" => webp_is_animated(bytes),
        _ => false,
    }
}

/// 扫描 RIFF chunk 是否含 ANMF(动画帧)。
fn webp_is_animated(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let mut i = 12usize;
    while i + 8 <= bytes.len() {
        let fourcc = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap_or([0; 4])) as usize;
        if fourcc == b"ANMF" {
            return true;
        }
        i += 8 + size + (size & 1);
    }
    false
}

/// 利用帧头探测读取 GIF 帧数(读到 2 帧即判定动图)。
fn gif_frame_count(bytes: &[u8]) -> usize {
    let mut dec = match gif::Decoder::new(Cursor::new(bytes)) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let mut n = 0;
    while n < 2 {
        match dec.next_frame_info() {
            Ok(Some(_)) => n += 1,
            _ => break,
        }
    }
    n
}

/* ---------------- 入口 ---------------- */

pub(crate) fn optimize_animation(
    bytes: &[u8],
    src_ext: &str,
    params: &CompressParams,
) -> Result<CompressOutcome> {
    let (mut frames, total_span) = match src_ext {
        "gif" => decode_gif_frames(bytes)?,
        "webp" => decode_webp_frames(bytes)?,
        _ => return Err(OsError::Other("不支持的动图格式".into())),
    };
    if frames.is_empty() {
        return Err(OsError::Other("动图没有有效帧".into()));
    }
    // 归一化时间轴:首帧起始必须为 0(WebP muxer 约定;前几帧被并入时起始会偏移)
    let time_offset = frames[0].start_ms;
    let mut total_span = total_span;
    if time_offset > 0 {
        for f in frames.iter_mut() {
            f.start_ms -= time_offset;
        }
        total_span -= time_offset;
    }
    let (w0, h0) = frames[0].rgba.dimensions();

    let fmt_filter = match params.format {
        crate::imaging::OutputFormat::Webp => AnimFmt::WebpOnly,
        crate::imaging::OutputFormat::Keep => match src_ext {
            "webp" => AnimFmt::WebpOnly,
            _ => AnimFmt::GifOnly,
        },
        _ => AnimFmt::Race,
    };

    // 显式 target 或默认 500KB(十进制,0.98 安全裕度)
    let target = if params.target_kb > 0 {
        ((params.target_kb as usize * 1000) * 98 / 100).max(512)
    } else {
        500 * 1000 * 98 / 100
    };
    let q_cap = params.quality.max(1);
    let edge_cap: u32 = if target <= 256 * 1000 { 640 } else { 1024 };

    let quality_ladder = [72u8, 58, 44, 34, 25, 18];
    let fps_ladder = [20.0f32, 15.0, 12.0, 10.0, 8.0, 6.0];

    let mut scale = 1.0f32;
    let mut best: Option<(AnimFmt, Vec<u8>, u32, u32)> = None;
    'outer: loop {
        let (w, h) = fit_dims(w0, h0, scale, edge_cap);
        let needs_resize = (w, h) != (w0, h0);
        let mut stage_best: Option<(AnimFmt, Vec<u8>, u32, u32)> = None;
        for (i, &ql) in quality_ladder.iter().enumerate() {
            let q = ql.min(q_cap);
            let picked = pick_frames(&frames, total_span, fps_ladder[i]);
            if picked.is_empty() {
                continue;
            }
            // 按本轮尺寸重采样(时间戳选帧后再缩放,节省内存);水印叠加在输出尺寸的帧上
            let needs_own = needs_resize || params.watermark.is_some();
            let resized: Vec<RgbaImage> = if needs_own {
                picked
                    .iter()
                    .map(|p| {
                        let mut img = if needs_resize {
                            resize_rgba(p.rgba, w, h, FilterType::Bilinear)
                        } else {
                            p.rgba.clone()
                        };
                        if let Some(wm) = &params.watermark {
                            crate::watermark::apply(&mut img, wm)?;
                        }
                        Ok(img)
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            };
            let refs: Vec<PickedFrame> = picked
                .iter()
                .enumerate()
                .map(|(k, p)| PickedFrame {
                    rgba: if needs_own {
                        resized.get(k).unwrap_or(p.rgba)
                    } else {
                        p.rgba
                    },
                    start_ms: p.start_ms,
                    dur_ms: p.dur_ms,
                })
                .collect();

            let mut candidates: Vec<(AnimFmt, Vec<u8>)> = Vec::with_capacity(2);
            if fmt_filter != AnimFmt::GifOnly {
                if let Ok(d) = encode_webp_anim(&refs, w, h, q) {
                    candidates.push((AnimFmt::WebpOnly, d));
                }
            }
            if fmt_filter != AnimFmt::WebpOnly {
                if let Ok(d) = encode_gif_anim(&refs, w, h, q) {
                    candidates.push((AnimFmt::GifOnly, d));
                }
            }
            if let Some((fmt, data)) = candidates.into_iter().min_by_key(|(_, d)| d.len()) {
                let better = stage_best
                    .as_ref()
                    .is_none_or(|(_, b, _, _)| data.len() < b.len());
                if better {
                    let reached = data.len() <= target;
                    stage_best = Some((fmt, data, w, h));
                    if reached {
                        break;
                    }
                }
            }
        }
        if let Some(stage) = stage_best {
            let stage_len = stage.1.len();
            let reached = stage_len <= target;
            let is_better = best
                .as_ref()
                .is_none_or(|(_, b, _, _)| stage_len < b.len());
            if is_better {
                best = Some(stage);
            }
            if reached {
                break 'outer;
            }
            // 以本轮最小结果推算需要的缩小比例
            let factor = ((target as f32 / stage_len as f32).sqrt() * 0.92).clamp(0.5, 0.85);
            let next = scale * factor;
            let (nw, nh) = fit_dims(w0, h0, next, edge_cap);
            if (nw, nh) == (w, h) || nw < 32 || nh < 32 {
                break 'outer;
            }
            scale = next;
        } else {
            break 'outer;
        }
    }

    let (fmt, data, w, h) = best.ok_or_else(|| OsError::Other("动图编码失败".into()))?;
    let (ext, format) = match fmt {
        AnimFmt::WebpOnly => ("webp", "webp"),
        AnimFmt::GifOnly => ("gif", "gif"),
        AnimFmt::Race => unreachable!("Race 不应作为最终格式"),
    };
    // 编码结果不小于原图时保留原文件;有水印时像素已变,必须返回新结果
    if params.watermark.is_none() && data.len() >= bytes.len() {
        return Ok(CompressOutcome {
            data: bytes.to_vec(),
            ext: src_ext.to_string(),
            format: src_ext.to_string(),
            width: w0,
            height: h0,
            kept: true,
            animated: true,
        });
    }
    Ok(CompressOutcome {
        data,
        ext: ext.to_string(),
        format: format.to_string(),
        width: w,
        height: h,
        kept: false,
        animated: true,
    })
}

/* ---------------- 解码 ---------------- */

/// GIF → 逐帧完整合成画布(处理子帧与 dispose),累计时间戳;<50ms 帧并入下一帧。
fn decode_gif_frames(bytes: &[u8]) -> Result<(Vec<AnimFrame>, i32)> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts
        .read_info(Cursor::new(bytes))
        .map_err(|e| OsError::Other(format!("GIF 解码失败: {e}")))?;
    let (w, h) = (u32::from(dec.width()), u32::from(dec.height()));
    if w == 0 || h == 0 {
        return Err(OsError::Other("GIF 尺寸无效".into()));
    }
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let mut frames: Vec<AnimFrame> = Vec::new();
    let mut t_ms = 0i32;
    while let Some(frame) = dec
        .read_next_frame()
        .map_err(|e| OsError::Other(format!("GIF 帧读取失败: {e}")))?
    {
        if frames.len() >= MAX_FRAMES || (frames.len() as u64 + 1) * u64::from(w * h) > MAX_TOTAL_PIXELS {
            break;
        }
        let (fw, fh) = (u32::from(frame.width), u32::from(frame.height));
        let (fl, ft) = (u32::from(frame.left), u32::from(frame.top));
        for y in 0..fh {
            let cy = ft + y;
            if cy >= h {
                continue;
            }
            for x in 0..fw {
                let cx = fl + x;
                if cx >= w {
                    continue;
                }
                let src_off = ((y * fw + x) * 4) as usize;
                if frame.buffer[src_off + 3] == 0 {
                    continue;
                }
                let dst_off = ((cy * w + cx) * 4) as usize;
                canvas[dst_off..dst_off + 4].copy_from_slice(&frame.buffer[src_off..src_off + 4]);
            }
        }
        let delay_ms = i32::from(frame.delay) * 10;
        let start = t_ms;
        t_ms += delay_ms.max(20);
        if delay_ms >= 50 {
            frames.push(AnimFrame {
                rgba: RgbaImage::from_raw(w, h, canvas.clone()).expect("canvas size"),
                start_ms: start,
            });
        }
        if frame.dispose == gif::DisposalMethod::Background {
            for y in 0..fh {
                let cy = ft + y;
                if cy >= h {
                    continue;
                }
                for x in 0..fw {
                    let cx = fl + x;
                    if cx >= w {
                        continue;
                    }
                    let dst_off = ((cy * w + cx) * 4) as usize;
                    canvas[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
    }
    let total = last_span(&frames, t_ms);
    Ok((frames, total))
}

/// 动图 WebP → 逐帧完整画布(libwebp AnimDecoder 已合成)。
/// 注意:get_time_ms() 返回帧的**结束**时间(= 下一帧起始),需换算回起始时间。
fn decode_webp_frames(bytes: &[u8]) -> Result<(Vec<AnimFrame>, i32)> {
    let anim = webp::AnimDecoder::new(bytes)
        .decode()
        .map_err(|e| OsError::Other(format!("WebP 动图解码失败: {e}")))?;
    let total_raw = anim.len();
    let mut raw: Vec<(RgbaImage, i32)> = Vec::new();
    let mut last_end = 0i32;
    for i in 0..total_raw {
        if raw.len() >= MAX_FRAMES {
            break;
        }
        let f = anim
            .get_frame(i)
            .ok_or_else(|| OsError::Other("WebP 动图帧缺失".into()))?;
        let (w, h) = (f.width(), f.height());
        if w == 0 || h == 0 {
            continue;
        }
        if (raw.len() as u64 + 1) * u64::from(w * h) > MAX_TOTAL_PIXELS {
            break;
        }
        let rgba = match f.get_layout() {
            webp::PixelLayout::Rgba => RgbaImage::from_raw(w, h, f.get_image().to_vec())
                .ok_or_else(|| OsError::Other("WebP 帧数据无效".into()))?,
            webp::PixelLayout::Rgb => {
                let src = f.get_image();
                let mut buf = Vec::with_capacity(src.len() / 3 * 4);
                for px in src.chunks_exact(3) {
                    buf.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                RgbaImage::from_raw(w, h, buf).ok_or_else(|| OsError::Other("WebP 帧数据无效".into()))?
            }
        };
        let end = f.get_time_ms().max(last_end);
        last_end = end;
        raw.push((rgba, end));
    }
    // 与 GIF 解码一致:显示时长 <50ms 的帧并入下一帧(起始时间连续,时长不漂移)
    let mut frames: Vec<AnimFrame> = Vec::with_capacity(raw.len());
    let mut start = 0i32;
    for (i, (rgba, end)) in raw.into_iter().enumerate() {
        let dur = end - start;
        if i + 1 == total_raw || dur >= 50 {
            frames.push(AnimFrame { rgba, start_ms: start });
        }
        start = end;
    }
    let total = last_span(&frames, last_end + 100);
    Ok((frames, total))
}

/// 动画总时长:最后帧起始 + 平均帧间隔(下限 20ms)。
fn last_span(frames: &[AnimFrame], fallback_total: i32) -> i32 {
    if frames.len() < 2 {
        return fallback_total.max(20);
    }
    let avg = frames[frames.len() - 1].start_ms / (frames.len() as i32 - 1);
    (frames[frames.len() - 1].start_ms + avg.max(20)).max(frames[frames.len() - 1].start_ms + 20)
}

/// 按目标帧率选帧(首帧恒保留,间隔按时间戳判断,丢帧时长自动并入下一帧)。
fn pick_frames<'a>(all: &'a [AnimFrame], total_span: i32, fps: f32) -> Vec<PickedFrame<'a>> {
    let interval = 1000.0 / fps.max(1.0);
    let mut idxs: Vec<usize> = Vec::new();
    let mut last_start = i32::MIN / 2;
    for (i, f) in all.iter().enumerate() {
        if i == 0 || (f.start_ms - last_start) as f32 >= interval - 0.5 {
            idxs.push(i);
            last_start = f.start_ms;
        }
    }
    idxs.iter()
        .enumerate()
        .map(|(k, &i)| {
            let dur = if k + 1 < idxs.len() {
                all[idxs[k + 1]].start_ms - all[i].start_ms
            } else {
                (total_span - all[i].start_ms).max(20)
            };
            PickedFrame {
                rgba: &all[i].rgba,
                start_ms: all[i].start_ms,
                dur_ms: dur,
            }
        })
        .collect()
}

/// 按缩放与长边上限计算本轮尺寸(不放大)。
fn fit_dims(w0: u32, h0: u32, scale: f32, edge_cap: u32) -> (u32, u32) {
    let m = w0.max(h0) as f32;
    let mut s = scale.min(1.0);
    if m * s > edge_cap as f32 {
        s = edge_cap as f32 / m;
    }
    (
        ((w0 as f32 * s).round() as u32).max(1),
        ((h0 as f32 * s).round() as u32).max(1),
    )
}

/* ---------------- WebP 动图编码 ---------------- */

fn encode_webp_anim(frames: &[PickedFrame], w: u32, h: u32, quality: u8) -> Result<Vec<u8>> {
    let mut cfg = webp::WebPConfig::new().map_err(|_| OsError::Other("WebP 配置失败".into()))?;
    cfg.quality = f32::from(quality.max(1));
    cfg.method = 1;
    cfg.alpha_quality = 80;
    cfg.thread_level = 1;
    let mut enc = webp::AnimEncoder::new(w, h, &cfg);
    enc.set_loop_count(0);
    enc.set_bgcolor([0, 0, 0, 0]);
    for f in frames {
        enc.add_frame(webp::AnimFrame::from_rgba(
            f.rgba.as_raw(),
            w,
            h,
            f.start_ms,
        ));
    }
    let mem = enc
        .try_encode()
        .map_err(|e| OsError::Other(format!("WebP 动图编码失败: {e:?}")))?;
    if mem.is_empty() {
        return Err(OsError::Other("WebP 动图编码失败".into()));
    }
    let mut out = mem.to_vec();
    // muxer 给最后一帧写默认 100ms;按选帧后的真实时长修补最后一个 ANMF 的 24bit delay
    if let Some(last) = frames.last() {
        patch_last_webp_delay(&mut out, last.dur_ms);
    }
    Ok(out)
}

/// 定位最后一个 ANMF chunk,把其 24bit LE duration 修补为 delay_ms。
fn patch_last_webp_delay(data: &mut [u8], delay_ms: i32) {
    if data.len() < 20 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return;
    }
    let mut i = 12usize;
    let mut last_anmf: Option<usize> = None;
    while i + 8 <= data.len() {
        let fourcc = &data[i..i + 4];
        let size = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap_or([0; 4])) as usize;
        if fourcc == b"ANMF" && i + 19 <= data.len() {
            last_anmf = Some(i);
        }
        i += 8 + size + (size & 1);
    }
    if let Some(off) = last_anmf {
        // ANMF payload: X(2) Y(2) W(2) H(2) + duration(3, LE)
        let d = delay_ms.clamp(0, 0x00FF_FFFF) as u32;
        data[off + 16..off + 19].copy_from_slice(&d.to_le_bytes()[..3]);
    }
}

/* ---------------- GIF 编码 ---------------- */

const BAYER4: [[f32; 4]; 4] = [
    [0.0, 8.0, 2.0, 10.0],
    [12.0, 4.0, 14.0, 6.0],
    [3.0, 11.0, 1.0, 9.0],
    [15.0, 7.0, 13.0, 5.0],
];

fn encode_gif_anim(frames: &[PickedFrame], w: u32, h: u32, quality: u8) -> Result<Vec<u8>> {
    let levels: u32 = if quality >= 60 {
        6
    } else if quality >= 40 {
        4
    } else {
        3
    };
    // 全局调色板:levels³ 均匀色 + 末位透明索引,补齐 256 项
    let mut palette = vec![0u8; 768];
    let step = 255.0 / (levels - 1) as f32;
    let mut idx = 0usize;
    for r in 0..levels {
        for g in 0..levels {
            for b in 0..levels {
                palette[idx * 3] = (r as f32 * step).round() as u8;
                palette[idx * 3 + 1] = (g as f32 * step).round() as u8;
                palette[idx * 3 + 2] = (b as f32 * step).round() as u8;
                idx += 1;
            }
        }
    }
    let trans_idx = (levels * levels * levels) as u8;

    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, w as u16, h as u16, &palette)
            .map_err(|e| OsError::Other(format!("GIF 编码失败: {e}")))?;
        enc.set_repeat(gif::Repeat::Infinite)
            .map_err(|e| OsError::Other(format!("GIF 编码失败: {e}")))?;

    for (i, f) in frames.iter().enumerate() {
        let has_alpha = f.rgba.pixels().any(|p| p[3] < 128);
        let delay_cs = (f.dur_ms / 10).clamp(2, 6000) as u16;
        // 不透明帧与上一不透明源帧做变化区域裁剪;透明帧写全帧
        let prev = if i > 0 { Some(frames[i - 1].rgba) } else { None };
        let (left, top, sub_w, sub_h, sub, dispose) = if has_alpha {
            (
                0,
                0,
                w,
                h,
                quantize_frame(f.rgba, levels, trans_idx),
                gif::DisposalMethod::Keep,
            )
        } else if let Some(prev) = prev.filter(|p| !p.pixels().any(|q| q[3] < 128)) {
            let (bx, by, bw, bh) = diff_bbox(prev, f.rgba);
            let sub_img = crop_rgba(f.rgba, bx, by, bw, bh);
            (
                bx,
                by,
                bw,
                bh,
                quantize_frame(&sub_img, levels, trans_idx),
                gif::DisposalMethod::Any,
            )
        } else {
            (
                0,
                0,
                w,
                h,
                quantize_frame(f.rgba, levels, trans_idx),
                gif::DisposalMethod::Any,
            )
        };
        let frame = gif::Frame {
            width: sub_w as u16,
            height: sub_h as u16,
            left: left as u16,
            top: top as u16,
            buffer: Cow::Owned(sub),
            transparent: Some(trans_idx).filter(|_| has_alpha),
            dispose,
            delay: delay_cs,
            ..Default::default()
        };
        enc.write_frame(&frame)
            .map_err(|e| OsError::Other(format!("GIF 帧写入失败: {e}")))?;
    }
    }
    Ok(out)
}

/// RGBA → 均匀调色板索引(Bayer 4x4 有序抖动)。
fn quantize_frame(rgba: &RgbaImage, levels: u32, trans_idx: u8) -> Vec<u8> {
    let step = 255.0 / (levels - 1) as f32;
    let w = rgba.width() as usize;
    let mut out = Vec::with_capacity((rgba.width() * rgba.height()) as usize);
    for (pos, p) in rgba.pixels().enumerate() {
        if p[3] < 128 {
            out.push(trans_idx);
            continue;
        }
        let d = (BAYER4[(pos / w) % 4][(pos % w) % 4] / 16.0 - 0.5) * step;
        let q = |v: u8| -> u32 {
            ((f32::from(v) + d) / step).round().clamp(0.0, levels as f32 - 1.0) as u32
        };
        let (r, g, b) = (q(p[0]), q(p[1]), q(p[2]));
        out.push(((r * levels + g) * levels + b) as u8);
    }
    out
}

/// 与上一帧的变化区域(仅不透明帧间调用)。
fn diff_bbox(prev: &RgbaImage, cur: &RgbaImage) -> (u32, u32, u32, u32) {
    let (w, h) = cur.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for y in 0..h {
        for x in 0..w {
            if prev.get_pixel(x, y) != cur.get_pixel(x, y) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x > max_x || min_y > max_y {
        return (0, 0, w, h);
    }
    (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

fn crop_rgba(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for yy in 0..h.min(img.height() - y) {
        for xx in 0..w.min(img.width() - x) {
            out.put_pixel(xx, yy, *img.get_pixel(x + xx, y + yy));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imaging::OutputFormat;

    /// 3 帧 48x48 噪声动图(每帧 100ms),噪声保证编码后体积可观。
    fn sample_gif(dim: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = gif::Encoder::new(&mut out, dim as u16, dim as u16, &[]).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        let mut seed = 0x1234u32;
        for k in 0..3u8 {
            let mut buf = vec![0u8; (dim * dim * 4) as usize];
            for (i, px) in buf.chunks_exact_mut(4).enumerate() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                px[0] = (seed >> 24) as u8 ^ k;
                px[1] = (seed >> 16) as u8;
                px[2] = (seed >> 8) as u8;
                px[3] = if i % 97 == 0 { 0 } else { 255 };
            }
            let mut frame = gif::Frame::from_rgba_speed(dim as u16, dim as u16, &mut buf, 4);
            frame.delay = 10;
            enc.write_frame(&frame).unwrap();
        }
        drop(enc);
        out
    }

    /// 3 帧动图 WebP。
    fn sample_webp_anim(dim: u32) -> Vec<u8> {
        let mut cfg = webp::WebPConfig::new().unwrap();
        cfg.quality = 75.0;
        // AnimEncoder 持有帧数据的借用,像素缓冲需活到 encode 之后
        let mut bufs: Vec<Vec<u8>> = Vec::new();
        let mut seed = 0xabcdu32;
        for _ in 0..3 {
            let mut buf = vec![0u8; (dim * dim * 4) as usize];
            for px in buf.chunks_exact_mut(4) {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                px[0] = (seed >> 24) as u8;
                px[1] = (seed >> 16) as u8;
                px[2] = (seed >> 8) as u8;
                px[3] = 255;
            }
            bufs.push(buf);
        }
        let mut enc = webp::AnimEncoder::new(dim, dim, &cfg);
        for (i, ts) in [0i32, 100, 200].into_iter().enumerate() {
            enc.add_frame(webp::AnimFrame::from_rgba(&bufs[i], dim, dim, ts));
        }
        enc.try_encode().unwrap().to_vec()
    }

    #[test]
    fn detects_animated_gif_and_static_png() {
        let gif_bytes = sample_gif(48);
        assert!(detect_animated(&gif_bytes, "gif"));
        // 静态单帧 GIF
        let out = {
            let mut out = Vec::new();
            let mut enc = gif::Encoder::new(&mut out, 16, 16, &[]).unwrap();
            let mut buf = vec![255u8; 16 * 16 * 4];
            let f = gif::Frame::from_rgba_speed(16, 16, &mut buf, 4);
            enc.write_frame(&f).unwrap();
            drop(enc);
            out
        };
        assert!(!detect_animated(&out, "gif"));
        assert!(!detect_animated(b"not an image", "gif"));
        assert!(!detect_animated(&[], "png"));
    }

    #[test]
    fn detects_animated_webp() {
        let webp_bytes = sample_webp_anim(48);
        assert!(webp_is_animated(&webp_bytes));
        assert!(detect_animated(&webp_bytes, "webp"));
        assert!(!webp_is_animated(b"RIFFxxxxWEBPxxx"));
    }

    #[test]
    fn gif_animation_roundtrip() {
        let original = sample_gif(64);
        let params = CompressParams {
            format: OutputFormat::Keep,
            ..CompressParams::default()
        };
        let out = optimize_animation(&original, "gif", &params).unwrap();
        assert!(out.animated);
        if out.kept {
            assert_eq!(out.data.len(), original.len());
            return;
        }
        assert_eq!(out.ext, "gif");
        // 输出可被解码,帧数保留
        let mut dec = gif::DecodeOptions::new()
            .read_info(Cursor::new(&out.data))
            .unwrap();
        let mut frames = 0;
        while let Some(_) = dec.read_next_frame().unwrap() {
            frames += 1;
        }
        assert_eq!(frames, 3);
    }

    #[test]
    fn webp_animation_roundtrip() {
        let original = sample_webp_anim(64);
        let params = CompressParams {
            format: OutputFormat::Auto,
            ..CompressParams::default()
        };
        let out = optimize_animation(&original, "webp", &params).unwrap();
        assert!(out.animated);
        if out.kept {
            assert_eq!(out.data.len(), original.len());
            return;
        }
        assert!(matches!(out.ext.as_str(), "webp" | "gif"));
    }

    #[test]
    fn animation_target_size_forces_downscale() {
        let original = sample_gif(128);
        let params = CompressParams {
            format: OutputFormat::Auto,
            target_kb: 2,
            ..CompressParams::default()
        };
        let out = optimize_animation(&original, "gif", &params).unwrap();
        assert!(out.animated);
        // 2KB 极限目标:要么收敛,要么返回已最小化的候选(不应超过 4KB 太多)
        assert!(out.data.len() <= 6 * 1000, "got {} bytes", out.data.len());
    }

    #[test]
    fn webp_short_frames_are_merged() {
        // 4 帧:前 3 帧间隔 30ms(<50ms 应并入),最后一段 240ms 保留 → 剩 2 帧
        let mut cfg = webp::WebPConfig::new().unwrap();
        cfg.quality = 75.0;
        let bufs: Vec<Vec<u8>> = (0..4)
            .map(|k| vec![k as u8; 32 * 32 * 4])
            .collect();
        let mut enc = webp::AnimEncoder::new(32, 32, &cfg);
        for (i, ts) in [0i32, 30, 60, 300].into_iter().enumerate() {
            enc.add_frame(webp::AnimFrame::from_rgba(&bufs[i], 32, 32, ts));
        }
        let original = enc.try_encode().unwrap().to_vec();
        let params = CompressParams {
            format: OutputFormat::Keep,
            ..CompressParams::default()
        };
        let out = optimize_animation(&original, "webp", &params).unwrap();
        if out.kept {
            return; // 极小图可能保留原文件,此时不校验帧数
        }
        let out_frames = match out.ext.as_str() {
            "gif" => {
                let mut dec = gif::DecodeOptions::new()
                    .read_info(Cursor::new(&out.data))
                    .unwrap();
                let mut n = 0;
                while dec.read_next_frame().unwrap().is_some() {
                    n += 1;
                }
                n
            }
            _ => {
                let anim = webp::AnimDecoder::new(&out.data).decode().unwrap();
                anim.len()
            }
        };
        assert_eq!(out_frames, 2, "short frames should merge");
    }

    #[test]
    fn animation_watermark_never_returns_original() {
        // 有水印时即使编码结果不小于原图,也必须返回带水印的新结果
        let original = sample_gif(16);
        let wm = crate::watermark::WatermarkParams {
            kind: crate::watermark::WatermarkKind::Blind,
            text: "mark".into(),
            font_size: 24,
            color: [255, 255, 255],
            opacity: 100,
            rotation: 0,
            tiled: false,
            density: 55,
            position: crate::watermark::WmPos::Center,
            image: Vec::new(),
            image_scale: 25,
            strength: 80,
        };
        let params = CompressParams {
            format: OutputFormat::Keep,
            watermark: Some(wm),
            ..CompressParams::default()
        };
        let out = optimize_animation(&original, "gif", &params).unwrap();
        assert!(!out.kept, "watermarked result must not be replaced by original");
        assert_ne!(out.data, original);
    }
}
