//! FFmpeg segmented H.264/PulseAudio encoding, concatenation & crisp GIF export.

use crate::linux_record::SharedRecState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tomoyo_core::logs_dir;

pub(crate) struct SegmentCfg<'a> {
    pub w: i32,
    pub h: i32,
    pub fps: u8,
    pub pixel_fmt: &'a str,
    pub audio_srcs: &'a [String],
    pub x11_origin: Option<(i32, i32)>,
    pub cursor: bool,
}

pub(crate) fn spawn_ffmpeg_segment(cfg: &SegmentCfg<'_>, out_path: &Path) -> Option<Child> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y"]);
    if let Some((ox, oy)) = cfg.x11_origin {
        let disp = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        cmd.args([
            "-f", "x11grab", "-draw_mouse", if cfg.cursor { "1" } else { "0" },
            "-framerate", &cfg.fps.to_string(),
            "-video_size", &format!("{}x{}", cfg.w, cfg.h),
            "-i", &format!("{disp}.0+{ox},{oy}"),
        ]);
    } else {
        cmd.args([
            "-f", "rawvideo", "-pixel_format", cfg.pixel_fmt,
            "-video_size", &format!("{}x{}", cfg.w, cfg.h),
            "-framerate", &cfg.fps.to_string(),
            "-i", "pipe:0",
        ]);
    }
    for asrc in cfg.audio_srcs {
        cmd.args(["-thread_queue_size", "1024", "-f", "pulse", "-i", asrc]);
    }
    if cfg.audio_srcs.len() >= 2 {
        cmd.args([
            "-filter_complex", "[1:a][2:a]amix=inputs=2:duration=longest:dropout_transition=0[aout]",
            "-map", "0:v", "-map", "[aout]",
        ]);
    }
    cmd.args([
        "-c:v", "libx264", "-preset", "veryfast", "-tune", "zerolatency",
        "-crf", "20", "-pix_fmt", "yuv420p", "-g", &cfg.fps.max(1).to_string(),
    ]);
    if !cfg.audio_srcs.is_empty() {
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-shortest"]);
    }
    if out_path.extension().and_then(|e| e.to_str()) == Some("mkv") {
        cmd.args(["-f", "matroska", "-flush_packets", "1"]);
    } else {
        cmd.args(["-movflags", "+faststart"]);
    }
    let stderr_io = std::fs::File::create(logs_dir().join("ffmpeg_rec.log"))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    cmd.arg(out_path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(stderr_io);
    cmd.spawn().ok()
}

pub(crate) fn close_ffmpeg(enc: Option<Child>, send_q: bool) {
    let Some(mut c) = enc else { return; };
    if send_q {
        if let Some(ref mut sin) = c.stdin { let _ = sin.write_all(b"q\n"); }
    } else {
        drop(c.stdin.take());
    }
    let _ = c.wait();
}

pub(crate) fn update_bytes_stat(segments: &Arc<Mutex<Vec<PathBuf>>>, shared: &Arc<SharedRecState>) {
    if let Ok(g) = segments.lock() {
        let tot: u64 = g.iter().filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len())).sum();
        shared.bytes.store(tot, Ordering::Relaxed);
    }
}

pub fn concat_and_export_recording(
    segments: &[PathBuf], tmp_dir: &Path, final_path: &Path, is_gif: bool, fps: u8,
) -> Result<u64, String> {
    let valid: Vec<&PathBuf> = segments.iter().filter(|p| p.is_file() && std::fs::metadata(p).is_ok_and(|m| m.len() > 256)).collect();
    if valid.is_empty() { return Err("未捕获到有效视频帧".into()); }
    let merged = if valid.len() == 1 {
        valid[0].clone()
    } else {
        let list = tmp_dir.join("concat.txt");
        let body: String = valid.iter().map(|p| format!("file '{}'\n", p.display())).collect();
        std::fs::write(&list, body).map_err(|e| format!("concat list: {e}"))?;
        let m = tmp_dir.join("merged.mkv");
        let st = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "concat", "-safe", "0"]).arg("-i").arg(&list)
            .args(["-c", "copy", "-f", "matroska"]).arg(&m)
            .status().map_err(|e| format!("ffmpeg concat: {e}"))?;
        if !st.success() { return Err("拼接录屏片段失败".into()); }
        m
    };
    if is_gif {
        let gf = fps.clamp(8, 20);
        let st = Command::new("ffmpeg")
            .args(["-v", "error", "-y"]).arg("-i").arg(&merged)
            .args(["-vf", &format!("fps={gf},scale='min(1280,iw)':-1:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3")])
            .arg(final_path).status().map_err(|e| format!("ffmpeg gif: {e}"))?;
        if !st.success() { return Err("GIF 调色板转换失败".into()); }
    } else if merged.extension().and_then(|e| e.to_str()) == Some("mkv") {
        let st = Command::new("ffmpeg")
            .args(["-v", "error", "-y"]).arg("-i").arg(&merged)
            .args(["-c", "copy", "-movflags", "+faststart"]).arg(final_path)
            .status().map_err(|e| format!("ffmpeg mp4: {e}"))?;
        if !st.success() { return Err("MP4 封装失败".into()); }
    } else if std::fs::rename(&merged, final_path).is_err() {
        std::fs::copy(&merged, final_path).map_err(|e| format!("save mp4: {e}"))?;
    }
    Ok(std::fs::metadata(final_path).map(|m| m.len()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux_record::resolve_portal_crop;
    use crate::screen::ScreenRect;

    #[test]
    fn encodes_raw_segments_concats_and_exports_crisp_gif() {
        if !Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tomoyo_rec_unit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = SegmentCfg {
            w: 32, h: 24, fps: 15, pixel_fmt: "bgr0", audio_srcs: &[], x11_origin: None, cursor: false,
        };
        let frame = vec![0x7Fu8; 32 * 24 * 4];
        let mut segs = Vec::new();
        for idx in 0..2 {
            let p = dir.join(format!("seg_{idx}.mkv"));
            let mut child = spawn_ffmpeg_segment(&cfg, &p).expect("spawn ffmpeg segment");
            {
                let sin = child.stdin.as_mut().unwrap();
                for _ in 0..4 { sin.write_all(&frame).unwrap(); }
            }
            drop(child.stdin.take());
            assert!(child.wait().unwrap().success());
            segs.push(p);
        }
        let mp4_out = dir.join("out.mp4");
        let mp4_sz = concat_and_export_recording(&segs, &dir, &mp4_out, false, 15).expect("concat mp4");
        assert!(mp4_sz > 256);

        let gif_out = dir.join("out.gif");
        let gif_sz = concat_and_export_recording(&[mp4_out], &dir, &gif_out, true, 15).expect("export gif");
        assert!(gif_sz > 64);
        assert_eq!(&std::fs::read(&gif_out).unwrap()[..6], b"GIF89a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_mismatched_portal_region_without_double_crop() {
        let mons = vec![ScreenRect { x: 0, y: 0, w: 2560, h: 1440 }];
        let sel = ScreenRect { x: 1480, y: 380, w: 400, h: 300 };
        let (crop, border, save) = resolve_portal_crop(sel, 2560, 1440, 2560, 1440, &mons);
        assert!(save && border == Some(sel) && crop == sel);

        let (sub_crop, sub_border, sub_save) = resolve_portal_crop(sel, 2560, 1440, 458, 212, &mons);
        assert!(!sub_save && sub_border.is_none());
        assert_eq!(sub_crop, ScreenRect { x: 0, y: 0, w: 458, h: 212 });
    }
}
