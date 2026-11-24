//! Native Windows Media Foundation (`IMFSinkWriter`) H.264 + AAC MP4 encoder.
//!
//! Zero external FFmpeg dependency: leverages Windows' built-in Media Foundation transforms
//! (with hardware acceleration and automatic software fallback) to keep `tomoyo-capture.exe` tiny.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFShutdown, MFStartup,
    MFVideoFormat_H264, MFVideoFormat_RGB32, MFVideoInterlace_Progressive, MFSTARTUP_NOSOCKET,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

#[inline]
fn pack_u64(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

pub struct MfMp4Writer {
    path: PathBuf,
    width: u32,
    height: u32,
    writer: Option<IMFSinkWriter>,
    video_stream: u32,
    audio_stream: Option<u32>,
    mf_started: bool,
    com_inited: bool,
}

impl MfMp4Writer {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: u8,
        audio_sample_rate: Option<u32>,
    ) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let w = (width & !1).max(2);
        let h = (height & !1).max(2);
        let fps = (fps as u32).clamp(10, 60);

        unsafe {
            let com_inited = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                if com_inited {
                    CoUninitialize();
                }
                return Err(format!("Media Foundation 启动失败: {e}"));
            }

            // Try hardware transforms first, then fall back cleanly to software MFT.
            let attempt = Self::open_writer(path, w, h, fps, audio_sample_rate, true)
                .or_else(|_| Self::open_writer(path, w, h, fps, audio_sample_rate, false));
            match attempt {
                Ok((writer, video_stream, audio_stream)) => Ok(Self {
                    path: path.to_path_buf(),
                    width: w,
                    height: h,
                    writer: Some(writer),
                    video_stream,
                    audio_stream,
                    mf_started: true,
                    com_inited,
                }),
                Err(e) => {
                    let _ = MFShutdown();
                    if com_inited {
                        CoUninitialize();
                    }
                    Err(e)
                }
            }
        }
    }

    unsafe fn open_writer(
        path: &Path,
        w: u32,
        h: u32,
        fps: u32,
        audio_sample_rate: Option<u32>,
        hw_accel: bool,
    ) -> Result<(IMFSinkWriter, u32, Option<u32>), String> {
        let mut attrs: Option<IMFAttributes> = None;
        unsafe { MFCreateAttributes(&mut attrs, 2) }.map_err(|e| format!("MF attrs: {e}"))?;
        if let Some(a) = attrs.as_ref() {
            let _ = unsafe { a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, u32::from(hw_accel)) };
        }
        let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let writer = unsafe { MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, attrs.as_ref()) }
            .map_err(|e| format!("创建 MP4 SinkWriter 失败: {e}"))?;

        let out_v: IMFMediaType = unsafe { MFCreateMediaType() }.map_err(|e| format!("out_v: {e}"))?;
        let bitrate = ((w as u64 * h as u64 * fps as u64) / 5).clamp(2_000_000, 18_000_000) as u32;
        unsafe {
            out_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| format!("{e}"))?;
            out_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(|e| format!("{e}"))?;
            out_v.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).map_err(|e| format!("{e}"))?;
            out_v.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(|e| format!("{e}"))?;
            out_v.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(w, h)).map_err(|e| format!("{e}"))?;
            out_v.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(fps, 1)).map_err(|e| format!("{e}"))?;
            out_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1)).map_err(|e| format!("{e}"))?;
        }
        let video_stream = unsafe { writer.AddStream(&out_v) }.map_err(|e| format!("H.264 stream: {e}"))?;

        let in_v: IMFMediaType = unsafe { MFCreateMediaType() }.map_err(|e| format!("in_v: {e}"))?;
        unsafe {
            in_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| format!("{e}"))?;
            in_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(|e| format!("{e}"))?;
            in_v.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(|e| format!("{e}"))?;
            in_v.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(w, h)).map_err(|e| format!("{e}"))?;
            in_v.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(fps, 1)).map_err(|e| format!("{e}"))?;
            in_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1)).map_err(|e| format!("{e}"))?;
            // Standard Windows RGB32 bottom-up layout is universally respected by HW & SW MFTs.
            in_v.SetUINT32(&MF_MT_DEFAULT_STRIDE, (-(w as i32 * 4)) as u32).map_err(|e| format!("{e}"))?;
        }
        unsafe { writer.SetInputMediaType(video_stream, &in_v, None) }
            .map_err(|e| format!("系统缺少 H.264 媒体编码器（可改选 GIF 或安装 Windows 媒体功能包）: {e}"))?;

        let audio_stream = if let Some(rate) = audio_sample_rate {
            unsafe { Self::try_add_aac_stream(&writer, rate) }.ok()
        } else {
            None
        };

        unsafe { writer.BeginWriting() }.map_err(|e| format!("BeginWriting: {e}"))?;
        Ok((writer, video_stream, audio_stream))
    }

    unsafe fn try_add_aac_stream(writer: &IMFSinkWriter, rate: u32) -> Result<u32, String> {
        let out_a: IMFMediaType = unsafe { MFCreateMediaType() }.map_err(|e| format!("{e}"))?;
        unsafe {
            out_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(|e| format!("{e}"))?;
            out_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(|e| format!("{e}"))?;
            out_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(|e| format!("{e}"))?;
            out_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate).map_err(|e| format!("{e}"))?;
            out_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(|e| format!("{e}"))?;
            out_a.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 16_000).map_err(|e| format!("{e}"))?;
        }
        let idx = unsafe { writer.AddStream(&out_a) }.map_err(|e| format!("{e}"))?;

        let in_a: IMFMediaType = unsafe { MFCreateMediaType() }.map_err(|e| format!("{e}"))?;
        unsafe {
            in_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(|e| format!("{e}"))?;
            in_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(|e| format!("{e}"))?;
            in_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(|e| format!("{e}"))?;
            in_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate).map_err(|e| format!("{e}"))?;
            in_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(|e| format!("{e}"))?;
            in_a.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4).map_err(|e| format!("{e}"))?;
            in_a.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 4).map_err(|e| format!("{e}"))?;
        }
        unsafe { writer.SetInputMediaType(idx, &in_a, None) }.map_err(|e| format!("{e}"))?;
        Ok(idx)
    }

    pub fn has_audio(&self) -> bool {
        self.audio_stream.is_some()
    }

    /// Encodes a top-down BGRA frame (flips rows to bottom-up RGB32 for guaranteed MFT parity).
    pub fn write_video_frame(
        &mut self,
        top_down_bgra: &[u8],
        src_stride: usize,
        timestamp_hns: i64,
        duration_hns: i64,
    ) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err("SinkWriter already finalized".into());
        };
        let row_bytes = (self.width as usize) * 4;
        let frame_bytes = row_bytes * (self.height as usize);
        if top_down_bgra.len() < src_stride * (self.height as usize) || src_stride < row_bytes {
            return Err("video frame size mismatch".into());
        }
        unsafe {
            let buf = MFCreateMemoryBuffer(frame_bytes as u32).map_err(|e| format!("{e}"))?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None).map_err(|e| format!("{e}"))?;
            let dst = std::slice::from_raw_parts_mut(ptr, frame_bytes);
            let h = self.height as usize;
            for y in 0..h {
                let src_off = (h - 1 - y) * src_stride;
                let dst_off = y * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&top_down_bgra[src_off..src_off + row_bytes]);
            }
            let _ = buf.Unlock();
            buf.SetCurrentLength(frame_bytes as u32).map_err(|e| format!("{e}"))?;

            let sample = MFCreateSample().map_err(|e| format!("{e}"))?;
            sample.AddBuffer(&buf).map_err(|e| format!("{e}"))?;
            sample.SetSampleTime(timestamp_hns).map_err(|e| format!("{e}"))?;
            sample.SetSampleDuration(duration_hns.max(1)).map_err(|e| format!("{e}"))?;
            writer.WriteSample(self.video_stream, &sample).map_err(|e| format!("WriteSample(video): {e}"))?;
        }
        Ok(())
    }

    /// Writes interleaved stereo 16-bit PCM samples to the optional AAC track.
    pub fn write_audio_pcm16(
        &mut self,
        samples: &[i16],
        timestamp_hns: i64,
        sample_rate: u32,
    ) -> Result<(), String> {
        let (Some(writer), Some(stream)) = (self.writer.as_ref(), self.audio_stream) else {
            return Ok(());
        };
        let frames = samples.len() / 2;
        if frames == 0 {
            return Ok(());
        }
        let byte_len = frames * 4;
        let dur_hns = ((frames as i64) * 10_000_000) / (sample_rate.max(1) as i64);
        unsafe {
            let buf = MFCreateMemoryBuffer(byte_len as u32).map_err(|e| format!("{e}"))?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            buf.Lock(&mut ptr, None, None).map_err(|e| format!("{e}"))?;
            std::ptr::copy_nonoverlapping(samples.as_ptr().cast::<u8>(), ptr, byte_len);
            let _ = buf.Unlock();
            buf.SetCurrentLength(byte_len as u32).map_err(|e| format!("{e}"))?;

            let sample = MFCreateSample().map_err(|e| format!("{e}"))?;
            sample.AddBuffer(&buf).map_err(|e| format!("{e}"))?;
            sample.SetSampleTime(timestamp_hns).map_err(|e| format!("{e}"))?;
            sample.SetSampleDuration(dur_hns.max(1)).map_err(|e| format!("{e}"))?;
            writer.WriteSample(stream, &sample).map_err(|e| format!("WriteSample(audio): {e}"))?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64, String> {
        if let Some(writer) = self.writer.take() {
            unsafe { writer.Finalize() }.map_err(|e| format!("封装 MP4 失败: {e}"))?;
            drop(writer);
        }
        let sz = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if sz < 256 {
            return Err("生成的 MP4 文件为空".into());
        }
        Ok(sz)
    }
}

impl Drop for MfMp4Writer {
    fn drop(&mut self) {
        self.writer.take();
        unsafe {
            if self.mf_started {
                let _ = MFShutdown();
            }
            if self.com_inited {
                CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::MediaFoundation::{
        MFCreateSourceReaderFromURL, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    };

    #[test]
    fn encodes_real_h264_mp4_with_correct_vertical_orientation() {
        let _gui_guard = crate::PROCESS_GUI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = std::env::temp_dir().join(format!("tomoyo_mf_test_{}.mp4", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (w, h, fps) = (64u32, 48u32, 15u8);
        let mut frame = vec![0u8; (w * h * 4) as usize];
        // Top half pure red (BGRA: 0,0,240,255), bottom half pure blue (BGRA: 240,0,0,255)
        for y in 0..h as usize {
            let px = if y < (h as usize / 2) { [10u8, 10, 235, 255] } else { [235u8, 10, 10, 255] };
            for x in 0..w as usize {
                let off = (y * w as usize + x) * 4;
                frame[off..off + 4].copy_from_slice(&px);
            }
        }
        let mut writer = match MfMp4Writer::create(&path, w, h, fps, None) {
            Ok(wr) => wr,
            Err(e) if std::env::var("CI").is_ok() => {
                eprintln!("[skip on headless Windows Server CI without H.264 MFT]: {e}");
                return;
            }
            Err(e) => panic!("create MF MP4 writer failed: {e}"),
        };
        let dur = 10_000_000i64 / fps as i64;
        for i in 0..8i64 {
            writer.write_video_frame(&frame, (w * 4) as usize, i * dur, dur).expect("write video frame");
        }
        let sz = writer.finish().expect("finalize MP4");
        assert!(sz > 512, "MP4 too small: {sz}");
        let raw = std::fs::read(&path).expect("read MP4");
        assert!(raw.windows(4).any(|s| s == b"ftyp"), "missing MP4 ftyp atom");

        // Verify orientation via SourceReader decoding RGB32
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
            let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
            if let Ok(reader) = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), None) {
                let mt = MFCreateMediaType().unwrap();
                let _ = mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
                let _ = mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
                let _ = reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &mt);
                let mut flags = 0u32;
                let mut sample = None;
                if reader.ReadSample(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0, None, Some(&mut flags), None, Some(&mut sample)).is_ok()
                    && let Some(s) = sample
                    && let Ok(buf) = s.ConvertToContiguousBuffer()
                {
                    let mut p = std::ptr::null_mut();
                    let mut len = 0u32;
                    if buf.Lock(&mut p, None, Some(&mut len)).is_ok() && (len as usize) >= (w * h * 4) as usize {
                        let sl = std::slice::from_raw_parts(p, len as usize);
                        // Sample center of top quarter (y=8, x=32) vs bottom quarter (y=40, x=32)
                        let top_off = (8 * w as usize + 32) * 4;
                        let bot_off = (40 * w as usize + 32) * 4;
                        let (tb, tr) = (sl[top_off], sl[top_off + 2]);
                        let (bb, br) = (sl[bot_off], sl[bot_off + 2]);
                        assert!(tr > tb + 80, "expected top half red, got B={tb} R={tr}");
                        assert!(bb > br + 80, "expected bottom half blue, got B={bb} R={br}");
                        let _ = buf.Unlock();
                    }
                }
            }
            let _ = MFShutdown();
            if com { CoUninitialize(); }
        }
        let _ = std::fs::remove_file(&path);
    }
}
