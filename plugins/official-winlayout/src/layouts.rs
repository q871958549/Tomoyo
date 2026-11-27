//! Cross-platform window tiling preset definitions and multi-monitor rect math.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutPreset {
    pub id: &'static str,
    pub badge: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub aliases: &'static str,
}

pub const PRESETS: &[LayoutPreset] = &[
    LayoutPreset {
        id: "left",
        badge: "HALF L",
        title: "左半屏 (Left Half)",
        subtitle: "将当前窗口贴靠到工作区左半边 1/2 区域",
        aliases: "left zuo 左 半屏 1/2 split",
    },
    LayoutPreset {
        id: "right",
        badge: "HALF R",
        title: "右半屏 (Right Half)",
        subtitle: "将当前窗口贴靠到工作区右半边 1/2 区域",
        aliases: "right you 右 半屏 1/2 split",
    },
    LayoutPreset {
        id: "center",
        badge: "FOCUS",
        title: "居中专注 (Center Focus)",
        subtitle: "按设定比例在当前屏幕工作区正中央对齐窗口",
        aliases: "center focus jz 居中 专注 middle",
    },
    LayoutPreset {
        id: "max",
        badge: "MAX",
        title: "最大化 / 还原切换 (Maximize)",
        subtitle: "切换当前窗口全工作区最大化或恢复原尺寸",
        aliases: "max full zdh 最大化 全屏 restore",
    },
    LayoutPreset {
        id: "top",
        badge: "TOP",
        title: "上半屏 (Top Half)",
        subtitle: "将当前窗口贴靠到工作区上半部分",
        aliases: "top up shang 上 半屏",
    },
    LayoutPreset {
        id: "bottom",
        badge: "BOTTOM",
        title: "下半屏 (Bottom Half)",
        subtitle: "将当前窗口贴靠到工作区下半部分",
        aliases: "bottom down xia 下 半屏",
    },
    LayoutPreset {
        id: "tl",
        badge: "Q1 TL",
        title: "左上四分之一 (Top-Left Quarter)",
        subtitle: "四象限网格左上角",
        aliases: "tl top-left zuoshang 左上 四分",
    },
    LayoutPreset {
        id: "tr",
        badge: "Q2 TR",
        title: "右上四分之一 (Top-Right Quarter)",
        subtitle: "四象限网格右上角",
        aliases: "tr top-right youshang 右上 四分",
    },
    LayoutPreset {
        id: "bl",
        badge: "Q3 BL",
        title: "左下四分之一 (Bottom-Left Quarter)",
        subtitle: "四象限网格左下角",
        aliases: "bl bottom-left zuoxia 左下 四分",
    },
    LayoutPreset {
        id: "br",
        badge: "Q4 BR",
        title: "右下四分之一 (Bottom-Right Quarter)",
        subtitle: "四象限网格右下角",
        aliases: "br bottom-right youxia 右下 四分",
    },
    LayoutPreset {
        id: "third-l",
        badge: "1/3 L",
        title: "三分屏 · 左侧 1/3",
        subtitle: "超宽屏 / 大屏三列网格最左侧",
        aliases: "third-l 1/3 three sanfen 三分 左",
    },
    LayoutPreset {
        id: "third-c",
        badge: "1/3 C",
        title: "三分屏 · 中间 1/3",
        subtitle: "超宽屏 / 大屏三列网格正中间",
        aliases: "third-c 2/3 mid sanfen 三分 中",
    },
    LayoutPreset {
        id: "third-r",
        badge: "1/3 R",
        title: "三分屏 · 右侧 1/3",
        subtitle: "超宽屏 / 大屏三列网格最右侧",
        aliases: "third-r 3/3 sanfen 三分 右",
    },
    LayoutPreset {
        id: "two-thirds-l",
        badge: "2/3 L",
        title: "主从分屏 · 左侧 2/3",
        subtitle: "左侧主编辑区占 2/3 宽度",
        aliases: "two-thirds-l 2/3 zhuceng 三分之二 主屏 左",
    },
    LayoutPreset {
        id: "two-thirds-r",
        badge: "2/3 R",
        title: "主从分屏 · 右侧 2/3",
        subtitle: "右侧主编辑区占 2/3 宽度",
        aliases: "two-thirds-r 2/3 sanfen 三分之二 右",
    },
    LayoutPreset {
        id: "always-top",
        badge: "PIN",
        title: "置顶切换 (Toggle Always on Top)",
        subtitle: "切换当前前台窗口是否保持在最顶层",
        aliases: "pin top always-top zhiding 置顶 悬停",
    },
    LayoutPreset {
        id: "minimize-others",
        badge: "CLEAN",
        title: "最小化其余窗口 (Focus Window)",
        subtitle: "保留当前前台窗口并将桌面其他窗口最小化",
        aliases: "minimize clean focus-only 最小化 其他 清理桌面",
    },
];

pub fn compute_rect(preset_id: &str, area: WorkArea, gap_px: i32, center_pct: i32) -> Option<WindowRect> {
    let gap = gap_px.clamp(0, 48);
    let inner_w = (area.w - gap * 2).max(240);
    let inner_h = (area.h - gap * 2).max(180);
    let x0 = area.x + gap;
    let y0 = area.y + gap;

    let half_w = (inner_w - gap) / 2;
    let half_h = (inner_h - gap) / 2;
    let third_w = (inner_w - gap * 2) / 3;

    let rect = match preset_id {
        "left" => WindowRect { x: x0, y: y0, w: half_w, h: inner_h },
        "right" => WindowRect { x: x0 + half_w + gap, y: y0, w: inner_w - half_w - gap, h: inner_h },
        "top" => WindowRect { x: x0, y: y0, w: inner_w, h: half_h },
        "bottom" => WindowRect { x: x0, y: y0 + half_h + gap, w: inner_w, h: inner_h - half_h - gap },
        "tl" => WindowRect { x: x0, y: y0, w: half_w, h: half_h },
        "tr" => WindowRect { x: x0 + half_w + gap, y: y0, w: inner_w - half_w - gap, h: half_h },
        "bl" => WindowRect { x: x0, y: y0 + half_h + gap, w: half_w, h: inner_h - half_h - gap },
        "br" => WindowRect {
            x: x0 + half_w + gap,
            y: y0 + half_h + gap,
            w: inner_w - half_w - gap,
            h: inner_h - half_h - gap,
        },
        "third-l" => WindowRect { x: x0, y: y0, w: third_w, h: inner_h },
        "third-c" => WindowRect { x: x0 + third_w + gap, y: y0, w: third_w, h: inner_h },
        "third-r" => WindowRect {
            x: x0 + (third_w + gap) * 2,
            y: y0,
            w: inner_w - (third_w + gap) * 2,
            h: inner_h,
        },
        "two-thirds-l" => {
            let w = third_w * 2 + gap;
            WindowRect { x: x0, y: y0, w, h: inner_h }
        }
        "two-thirds-r" => {
            let left_w = third_w + gap;
            WindowRect { x: x0 + left_w, y: y0, w: inner_w - left_w, h: inner_h }
        }
        "center" => {
            let pct = center_pct.clamp(50, 95);
            let w = (inner_w * pct) / 100;
            let h = (inner_h * pct) / 100;
            WindowRect {
                x: x0 + (inner_w - w) / 2,
                y: y0 + (inner_h - h) / 2,
                w,
                h,
            }
        }
        _ => return None,
    };
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_multi_monitor_rects_with_gap_and_center_ratio() {
        let mon2 = WorkArea { x: 1920, y: 40, w: 1920, h: 1040 };
        let l = compute_rect("left", mon2, 10, 72).unwrap();
        let r = compute_rect("right", mon2, 10, 72).unwrap();
        assert_eq!(l, WindowRect { x: 1930, y: 50, w: 945, h: 1020 });
        assert_eq!(r.x, 1930 + 945 + 10);
        assert_eq!(l.w + 10 + r.w, 1900);

        let c = compute_rect("center", WorkArea { x: 0, y: 0, w: 1000, h: 1000 }, 0, 80).unwrap();
        assert_eq!(c, WindowRect { x: 100, y: 100, w: 800, h: 800 });

        let t3 = compute_rect("third-r", WorkArea { x: 0, y: 0, w: 1200, h: 800 }, 0, 72).unwrap();
        assert_eq!(t3, WindowRect { x: 800, y: 0, w: 400, h: 800 });
        assert!(compute_rect("max", mon2, 0, 72).is_none());
    }
}
