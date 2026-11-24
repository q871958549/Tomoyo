//! Native Windows WASAPI loopback (system audio) & microphone capture for MP4 recording.
//!
//! Normalizes captured audio to 16-bit interleaved stereo PCM at 44100 Hz or 48000 Hz so
//! Windows Media Foundation's built-in AAC encoder accepts it directly without extra codecs.

#![cfg(windows)]

use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

const WAVE_FORMAT_PCM_TAG: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT_TAG: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE_TAG: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SrcFmt {
    Float32,
    Pcm16,
    Pcm32,
}

pub struct WinAudioCapture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    src_channels: usize,
    src_rate: u32,
    src_fmt: SrcFmt,
    out_rate: u32,
    com_inited: bool,
}

impl WinAudioCapture {
    /// Opens default Windows system audio (`loopback = true`) or default microphone (`false`).
    pub fn start(loopback: bool) -> Result<Self, String> {
        unsafe {
            let com_inited = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let res = Self::start_inner(loopback, com_inited);
            if res.is_err() && com_inited {
                CoUninitialize();
            }
            res
        }
    }

    unsafe fn start_inner(loopback: bool, com_inited: bool) -> Result<Self, String> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|e| format!("WASAPI enumerator: {e}"))?;
        let flow = if loopback { eRender } else { eCapture };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) }
            .map_err(|e| format!("未找到默认音频设备: {e}"))?;
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|e| format!("WASAPI activate: {e}"))?;
        let mix_ptr = unsafe { client.GetMixFormat() }
            .map_err(|e| format!("WASAPI GetMixFormat: {e}"))?;
        if mix_ptr.is_null() {
            return Err("WASAPI mix format null".into());
        }
        let wf = unsafe { std::ptr::read_unaligned(mix_ptr) };
        let src_channels = wf.nChannels.max(1) as usize;
        let src_rate = wf.nSamplesPerSec.max(8000);
        let src_fmt = detect_src_format(mix_ptr, &wf);
        let flags = if loopback { AUDCLNT_STREAMFLAGS_LOOPBACK } else { 0 };
        let buf_hns = 2_000_000i64; // 200 ms shared buffer
        let init_res = unsafe {
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, buf_hns, 0, mix_ptr, None)
        };
        unsafe { CoTaskMemFree(Some(mix_ptr.cast())) };
        init_res.map_err(|e| format!("WASAPI init: {e}"))?;

        let capture: IAudioCaptureClient = unsafe { client.GetService() }
            .map_err(|e| format!("WASAPI capture service: {e}"))?;
        unsafe { client.Start() }.map_err(|e| format!("WASAPI start: {e}"))?;

        let out_rate = if src_rate == 44_100 { 44_100 } else { 48_000 };
        Ok(Self {
            client,
            capture,
            src_channels,
            src_rate,
            src_fmt,
            out_rate,
            com_inited,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.out_rate
    }

    /// Drains all available captured audio packets into `out` as stereo 16-bit PCM.
    /// When `discard` is true (e.g. while paused), packets are dropped to keep buffer fresh.
    pub fn drain_pcm16(&mut self, out: &mut Vec<i16>, discard: bool) {
        unsafe {
            while let Ok(pkt_frames) = self.capture.GetNextPacketSize() {
                if pkt_frames == 0 {
                    break;
                }
                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut num_frames = 0u32;
                let mut flags = 0u32;
                if self
                    .capture
                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                    .is_err()
                {
                    break;
                }
                if !discard && num_frames > 0 {
                    let silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0
                        || data_ptr.is_null();
                    if silent {
                        let out_frames = scaled_frame_count(num_frames as usize, self.src_rate, self.out_rate);
                        out.resize(out.len() + out_frames * 2, 0);
                    } else {
                        convert_packet_to_stereo_i16(
                            data_ptr,
                            num_frames as usize,
                            self.src_channels,
                            self.src_fmt,
                            self.src_rate,
                            self.out_rate,
                            out,
                        );
                    }
                }
                let _ = self.capture.ReleaseBuffer(num_frames);
            }
        }
    }
}

impl Drop for WinAudioCapture {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            if self.com_inited {
                CoUninitialize();
            }
        }
    }
}

fn detect_src_format(ptr: *const WAVEFORMATEX, wf: &WAVEFORMATEX) -> SrcFmt {
    if wf.wFormatTag == WAVE_FORMAT_IEEE_FLOAT_TAG {
        return SrcFmt::Float32;
    }
    if wf.wFormatTag == WAVE_FORMAT_PCM_TAG {
        return if wf.wBitsPerSample >= 32 { SrcFmt::Pcm32 } else { SrcFmt::Pcm16 };
    }
    if wf.wFormatTag == WAVE_FORMAT_EXTENSIBLE_TAG && wf.cbSize >= 22 {
        let ext = unsafe { std::ptr::read_unaligned(ptr.cast::<WAVEFORMATEXTENSIBLE>()) };
        let sub = ext.SubFormat;
        if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            return SrcFmt::Float32;
        }
        return if wf.wBitsPerSample >= 32 { SrcFmt::Pcm32 } else { SrcFmt::Pcm16 };
    }
    SrcFmt::Float32
}

fn scaled_frame_count(in_frames: usize, src_rate: u32, out_rate: u32) -> usize {
    if src_rate == out_rate {
        in_frames
    } else {
        ((in_frames as u64 * out_rate as u64) / src_rate.max(1) as u64).max(1) as usize
    }
}

#[inline]
fn f32_to_i16(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * 32767.0) as i16
}

unsafe fn read_stereo_sample(
    base: *const u8,
    frame_idx: usize,
    channels: usize,
    fmt: SrcFmt,
) -> (i16, i16) {
    match fmt {
        SrcFmt::Float32 => {
            let off = frame_idx * channels * 4;
            let l = unsafe { std::ptr::read_unaligned(base.add(off).cast::<f32>()) };
            let r = if channels >= 2 {
                unsafe { std::ptr::read_unaligned(base.add(off + 4).cast::<f32>()) }
            } else {
                l
            };
            (f32_to_i16(l), f32_to_i16(r))
        }
        SrcFmt::Pcm16 => {
            let off = frame_idx * channels * 2;
            let l = unsafe { std::ptr::read_unaligned(base.add(off).cast::<i16>()) };
            let r = if channels >= 2 {
                unsafe { std::ptr::read_unaligned(base.add(off + 2).cast::<i16>()) }
            } else {
                l
            };
            (l, r)
        }
        SrcFmt::Pcm32 => {
            let off = frame_idx * channels * 4;
            let l = (unsafe { std::ptr::read_unaligned(base.add(off).cast::<i32>()) } >> 16) as i16;
            let r = if channels >= 2 {
                (unsafe { std::ptr::read_unaligned(base.add(off + 4).cast::<i32>()) } >> 16) as i16
            } else {
                l
            };
            (l, r)
        }
    }
}

unsafe fn convert_packet_to_stereo_i16(
    data: *const u8,
    in_frames: usize,
    channels: usize,
    fmt: SrcFmt,
    src_rate: u32,
    out_rate: u32,
    out: &mut Vec<i16>,
) {
    if src_rate == out_rate {
        out.reserve(in_frames * 2);
        for f in 0..in_frames {
            let (l, r) = unsafe { read_stereo_sample(data, f, channels, fmt) };
            out.push(l);
            out.push(r);
        }
        return;
    }
    let out_frames = scaled_frame_count(in_frames, src_rate, out_rate);
    out.reserve(out_frames * 2);
    let ratio = src_rate as f64 / out_rate as f64;
    let max_i = in_frames.saturating_sub(1);
    for i in 0..out_frames {
        let pos = i as f64 * ratio;
        let idx = (pos as usize).min(max_i);
        let (l, r) = unsafe { read_stereo_sample(data, idx, channels, fmt) };
        out.push(l);
        out.push(r);
    }
}
