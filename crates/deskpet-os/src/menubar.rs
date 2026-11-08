//! macOS 菜单栏监控：独立 NSStatusItem，把采样绘制成双行芯片图（与悬浮条同款式）。
//!
//! `ChipSections` 是跨平台纯数据类型（悬浮条与菜单栏共用同一组分区开关），
//! 随模块在所有平台编译；AppKit 状态项与绘制仅在 macOS 编译。
//! AppKit 仅主线程可调用（约束），函数内部做主线程校验。

use serde::Deserialize;

/// 芯片分区开关（与悬浮条共用同一组设置，缺省全显示）。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipSections {
    pub cpu: bool,
    pub mem: bool,
    pub net: bool,
}

impl Default for ChipSections {
    fn default() -> Self {
        Self {
            cpu: true,
            mem: true,
            net: true,
        }
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    //! NSStatusItem 状态项：把采样绘制为双行芯片图并显示在菜单栏。

    use std::cell::RefCell;

    use image::RgbaImage;
    use objc2::rc::Retained;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSData, NSString};

    use super::ChipSections;
    use crate::monitor::SystemSample;
    use crate::watermark::{
        blend_stamp, render_text_stamp, system_font, WatermarkKind, WatermarkParams,
    };
    use crate::{OsError, Result};

    /// 状态项存储。AppKit 对象非线程安全，但所有访问都先经主线程校验，
    /// 实际不会并发访问，故可静态共享。
    struct ItemCell(RefCell<Option<Retained<NSStatusItem>>>);
    // SAFETY: 所有访问都要求当前处于主线程，见各函数入口校验。
    unsafe impl Sync for ItemCell {}

    static ITEM: ItemCell = ItemCell(RefCell::new(None));

    /// retina 2x 渲染，展示时尺寸减半。
    const SCALE: u32 = 2;
    /// 三色阈值（与前端 monitorFormat 一致）：≥85 红 / ≥60 黄。
    const WARN: f32 = 60.0;
    const DANGER: f32 = 85.0;
    const COLOR_UP: [u8; 3] = [138, 180, 248];
    const COLOR_DOWN: [u8; 3] = [129, 201, 149];

    fn main_thread() -> Result<MainThreadMarker> {
        MainThreadMarker::new().ok_or_else(|| OsError::Io(std::io::Error::other("必须在主线程调用")))
    }

    fn value_color(pct: f32) -> [u8; 3] {
        if pct >= DANGER {
            [245, 108, 108]
        } else if pct >= WARN {
            [230, 162, 60]
        } else {
            [255, 255, 255]
        }
    }

    /// 渲染单行文字图（size 为 2x 画布像素）。
    fn draw_text(font: &ab_glyph::FontVec, text: &str, size: u32, color: [u8; 3]) -> RgbaImage {
        let wm = WatermarkParams {
            kind: WatermarkKind::Text,
            text: text.to_string(),
            font_size: size,
            color,
            opacity: 100,
            rotation: 0,
            tiled: false,
            density: 50,
            position: crate::watermark::WmPos::Center,
            image: Vec::new(),
            image_scale: 50,
            strength: 50,
        };
        render_text_stamp(font, text, &wm)
    }

    /// 菜单栏用速率文本："39.4 K/s"（≥100 取整，其余一位小数）。
    fn rate_text(bytes: f64) -> String {
        if !bytes.is_finite() || bytes <= 0.0 {
            return "0 K/s".into();
        }
        const UNITS: [&str; 4] = ["B", "K", "M", "G"];
        let mut v = bytes;
        let mut i = 0;
        while v >= 1024.0 && i < UNITS.len() - 1 {
            v /= 1024.0;
            i += 1;
        }
        let num = if v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") };
        format!("{num} {}/s", UNITS[i])
    }

    /// 把采样绘制为双行芯片（透明底，2x 像素），只渲染启用的分区。
    /// 列宽按参考文本（各列最宽可能内容）固定，总尺寸逐帧稳定，
    /// 避免数值位数变化导致菜单栏整排图标左右晃动。
    fn render_chip(sample: &SystemSample, sections: &ChipSections) -> Result<RgbaImage> {
        let Some(font) = system_font() else {
            return Err(OsError::Other("未找到可用的系统字体".into()));
        };
        let cpu_pct = sample.cpu.usage.clamp(0.0, 100.0);
        let mem_pct = if sample.memory.total > 0 {
            (sample.memory.used as f32 / sample.memory.total as f32 * 100.0).min(100.0)
        } else {
            0.0
        };

        // 参考块：决定固定列宽/行高（各列取最宽可能内容）
        let v_ref = draw_text(font, "100%", 20, [255, 255, 255]);
        let l_ref = draw_text(font, "MEM", 13, [255, 255, 255]);
        let a_ref = draw_text(font, "↑", 15, [255, 255, 255]);
        let t_ref = draw_text(font, "999.9 M/s", 15, [255, 255, 255]);

        let col_w = v_ref.width().max(l_ref.width());
        let value_h = v_ref.height();
        let label_h = l_ref.height();
        let arrow_w = a_ref.width();
        let net_row_h = t_ref.height();
        let arrow_gap = 3 * SCALE / 2;
        let net_w = arrow_w + arrow_gap + t_ref.width();

        let gap = 8 * SCALE / 2; // 列间隙 8
        let pad = 4 * SCALE;
        let dv_w = SCALE; // 分隔线 1 逻辑像素
        let row_gap = 2 * SCALE / 2;

        // 启用列布局：相邻启用列之间插分隔线；总宽只随分区开关变化，不随采样值变化
        let enabled = [sections.cpu, sections.mem, sections.net];
        let col_widths = [col_w, col_w, net_w];
        let mut col_x = [0i64; 3];
        let mut divider_xs: Vec<i64> = Vec::new();
        let mut x = i64::from(pad);
        let mut first = true;
        for i in 0..3 {
            if !enabled[i] {
                continue;
            }
            if !first {
                divider_xs.push(x + i64::from(gap));
                x += i64::from(gap + dv_w + gap);
            }
            first = false;
            col_x[i] = x;
            x += i64::from(col_widths[i]);
        }
        let width = x + i64::from(pad);
        let height = i64::from(pad + value_h + row_gap + label_h + pad);
        let mut canvas = RgbaImage::new(width.max(1) as u32, height.max(1) as u32);
        let inner_h = height - 2 * i64::from(pad);

        // 实际内容块（宽度不会超过参考列宽，超出部分混合时自动裁剪）
        let v_cpu = draw_text(font, &format!("{cpu_pct:.0}%"), 20, value_color(cpu_pct));
        let v_mem = draw_text(font, &format!("{mem_pct:.0}%"), 20, value_color(mem_pct));
        let l_cpu = draw_text(font, "CPU", 13, [255, 255, 255]);
        let l_mem = draw_text(font, "MEM", 13, [255, 255, 255]);
        let a_up = draw_text(font, "↑", 15, COLOR_UP);
        let a_down = draw_text(font, "↓", 15, COLOR_DOWN);
        let t_up = draw_text(font, &rate_text(sample.network.up_rate as f64), 15, [255, 255, 255]);
        let t_down = draw_text(font, &rate_text(sample.network.down_rate as f64), 15, [255, 255, 255]);

        // 数值行在上、标签行在下，各自列内居中
        if sections.cpu {
            blend_stamp(&mut canvas, &v_cpu, col_x[0] + i64::from(col_w - v_cpu.width()) / 2, i64::from(pad), 1.0);
            blend_stamp(&mut canvas, &l_cpu, col_x[0] + i64::from(col_w - l_cpu.width()) / 2, i64::from(pad) + i64::from(value_h) + i64::from(row_gap), 0.45);
        }
        if sections.mem {
            blend_stamp(&mut canvas, &v_mem, col_x[1] + i64::from(col_w - v_mem.width()) / 2, i64::from(pad), 1.0);
            blend_stamp(&mut canvas, &l_mem, col_x[1] + i64::from(col_w - l_mem.width()) / 2, i64::from(pad) + i64::from(value_h) + i64::from(row_gap), 0.45);
        }
        if sections.net {
            // 网络两行整体垂直居中，左对齐固定 x（箭头彩色、数值白色）
            let net_h = i64::from(net_row_h) * 2 + i64::from(row_gap);
            let net_y = (inner_h - net_h) / 2 + i64::from(pad);
            blend_stamp(&mut canvas, &a_up, col_x[2], net_y, 1.0);
            blend_stamp(&mut canvas, &t_up, col_x[2] + i64::from(arrow_w) + i64::from(arrow_gap), net_y, 0.85);
            let down_y = net_y + i64::from(net_row_h) + i64::from(row_gap);
            blend_stamp(&mut canvas, &a_down, col_x[2], down_y, 1.0);
            blend_stamp(&mut canvas, &t_down, col_x[2] + i64::from(arrow_w) + i64::from(arrow_gap), down_y, 0.85);
        }

        // 竖向分隔线（白色 10%，贯穿内容区）
        for dx in divider_xs {
            for dy in i64::from(pad)..i64::from(pad) + inner_h {
                for px in 0..dv_w {
                    let cx = dx + i64::from(px);
                    if cx >= 0 && cx < i64::from(canvas.width()) && dy >= 0 && dy < i64::from(canvas.height()) {
                        canvas.put_pixel(cx as u32, dy as u32, image::Rgba([255, 255, 255, 26]));
                    }
                }
            }
        }
        Ok(canvas)
    }

    /// RGBA → PNG 字节（png crate 无损编码）。
    fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, img.width(), img.height());
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| OsError::Other(format!("菜单栏图片编码失败: {e}")))?;
            writer
                .write_image_data(img.as_raw())
                .map_err(|e| OsError::Other(format!("菜单栏图片写入失败: {e}")))?;
        }
        Ok(out)
    }

    /// 在系统菜单栏显示监控芯片（把采样绘制为图片设为状态项图标，只渲染启用分区）。
    /// 非主线程调用返回错误。
    pub fn set_chip(sample: &SystemSample, sections: &ChipSections) -> Result<()> {
        let mtm = main_thread()?;
        let chip = render_chip(sample, sections)?;
        let png = encode_png(&chip)?;
        let data = NSData::with_bytes(&png);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return Err(OsError::Other("菜单栏图片解码失败".into()));
        };
        // 源图为 2x retina 像素，按逻辑点减半显示
        image.setSize(CGSize::new(
            f64::from(chip.width()) / 2.0,
            f64::from(chip.height()) / 2.0,
        ));
        let mut cell = ITEM.0.borrow_mut();
        let item = match cell.as_ref() {
            Some(item) => item.clone(),
            None => {
                let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
                *cell = Some(item.clone());
                item
            }
        };
        drop(cell);
        if let Some(button) = item.button(mtm) {
            button.setTitle(&NSString::new());
            button.setImage(Some(&image));
        }
        Ok(())
    }

    /// 移除菜单栏状态项；未显示时为空操作。非主线程调用返回错误。
    pub fn hide() -> Result<()> {
        let _mtm = main_thread()?;
        if let Some(item) = ITEM.0.borrow_mut().take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&item);
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::monitor::{CpuSample, MemorySample, NetworkSample};

        fn sample() -> SystemSample {
            SystemSample {
                cpu: CpuSample {
                    usage: 26.0,
                    cores: vec![26.0; 8],
                },
                memory: MemorySample {
                    used: 8_000_000_000,
                    total: 16_000_000_000,
                },
                network: NetworkSample {
                    down_rate: 23_000,
                    up_rate: 39_400,
                    total_down: 0,
                    total_up: 0,
                },
                volumes: Vec::new(),
            }
        }

        #[test]
        fn rate_text_formats() {
            assert_eq!(rate_text(0.0), "0 K/s");
            assert_eq!(rate_text(39_400.0), "38.5 K/s");
            assert_eq!(rate_text(316_000.0), "309 K/s");
            assert_eq!(rate_text(1_300_000.0), "1.2 M/s");
        }

        #[test]
        fn chip_renders_with_sample() {
            if system_font().is_none() {
                return; // 无系统字体环境（极端 CI）跳过
            }
            let chip = render_chip(&sample(), &ChipSections::default()).expect("芯片应能渲染");
            assert!(chip.width() > 40, "芯片过窄: {}", chip.width());
            assert!(chip.height() > 20, "芯片过矮: {}", chip.height());
            // 像素内容非空且有白色系数值像素
            assert!(chip.pixels().any(|p| p[3] > 0));
            assert!(chip.pixels().any(|p| p[3] > 200 && p[0] > 240 && p[1] > 240));
        }

        #[test]
        fn chip_dimensions_stable_across_samples() {
            if system_font().is_none() {
                return;
            }
            // 数值位数/单位大幅变化的两次采样，画布尺寸必须完全一致（防止菜单栏左右晃动）
            let mut other = sample();
            other.cpu.usage = 8.0;
            other.memory.used = 1_000_000_000;
            other.network.up_rate = 900;
            other.network.down_rate = 1_500_000;
            let a = render_chip(&sample(), &ChipSections::default()).expect("芯片应能渲染");
            let b = render_chip(&other, &ChipSections::default()).expect("芯片应能渲染");
            assert_eq!(a.dimensions(), b.dimensions());
        }

        #[test]
        fn chip_sections_narrow_and_stable() {
            if system_font().is_none() {
                return;
            }
            let all = render_chip(&sample(), &ChipSections::default()).expect("芯片应能渲染");
            let cpu_only_secs = ChipSections {
                cpu: true,
                mem: false,
                net: false,
            };
            let cpu_only = render_chip(&sample(), &cpu_only_secs).expect("芯片应能渲染");
            assert!(cpu_only.width() < all.width(), "单列应窄于全量");
            // 同一分区配置下尺寸恒定
            let mut other = sample();
            other.cpu.usage = 5.0;
            other.network.down_rate = 2_000_000;
            let again = render_chip(&other, &cpu_only_secs).expect("芯片应能渲染");
            assert_eq!(cpu_only.dimensions(), again.dimensions());
        }
    }
}

#[cfg(target_os = "macos")]
pub use appkit::{hide, set_chip};
