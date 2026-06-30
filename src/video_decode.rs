/// Cross-platform FFmpeg video frame decoder.
///
/// Strategy: **dynamic libav** only — dlopen libavformat, libavcodec, libavutil,
/// libswscale. No `ffmpeg` / `ffprobe` subprocesses.
///
/// Public entry points:
///   - [`decode_frame`]      – decode a single frame, picks best backend
///   - [`is_libav_available`] – returns true if libav was successfully loaded

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

// ── FFmpeg ABI constants ──────────────────────────────────────────────────────
const AVMEDIA_TYPE_VIDEO: c_int = 0;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AV_PIX_FMT_RGBA: c_int = 26;
const AVERROR_EAGAIN: c_int = -11;
const SWS_BILINEAR: c_int = 2;

// ── Opaque C types ────────────────────────────────────────────────────────────
#[repr(C)] struct AVFormatContext { _p: [u8; 0] }
#[repr(C)] struct AVCodecContext  { _p: [u8; 0] }
#[repr(C)] struct AVCodec         { _p: [u8; 0] }
#[repr(C)] struct AVPacket        { _p: [u8; 0] }
#[repr(C)] struct AVFrame         { _p: [u8; 0] }
#[repr(C)] struct SwsContext      { _p: [u8; 0] }
#[repr(C)] struct AVIOContext     { _p: [u8; 0] }

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AVRational {
    pub num: c_int,
    pub den: c_int,
}

// ── Dynamically loaded function table ────────────────────────────────────────
struct FfmpegLibs {
    _avformat: libloading::Library,
    _avcodec:  libloading::Library,
    _avutil:   libloading::Library,
    _swscale:  libloading::Library,

    // avformat
    avformat_version:             unsafe extern "C" fn() -> std::os::raw::c_uint,
    avformat_open_input:          unsafe extern "C" fn(*mut *mut AVFormatContext, *const c_char, *mut (), *mut ()) -> c_int,
    avformat_find_stream_info:    unsafe extern "C" fn(*mut AVFormatContext, *mut ()) -> c_int,
    avformat_close_input:         unsafe extern "C" fn(*mut *mut AVFormatContext),
    av_find_best_stream:          unsafe extern "C" fn(*mut AVFormatContext, c_int, c_int, c_int, *mut *const AVCodec, c_int) -> c_int,
    av_seek_frame:                unsafe extern "C" fn(*mut AVFormatContext, c_int, i64, c_int) -> c_int,
    av_read_frame:                unsafe extern "C" fn(*mut AVFormatContext, *mut AVPacket) -> c_int,

    avformat_alloc_output_context2: unsafe extern "C" fn(*mut *mut AVFormatContext, *mut (), *const c_char, *const c_char) -> c_int,
    avformat_new_stream:            unsafe extern "C" fn(*mut AVFormatContext, *const AVCodec) -> *mut (),
    avio_open:                      unsafe extern "C" fn(*mut *mut AVIOContext, *const c_char, c_int) -> c_int,
    avformat_write_header:          unsafe extern "C" fn(*mut AVFormatContext, *mut *mut ()) -> c_int,
    av_interleaved_write_frame:     unsafe extern "C" fn(*mut AVFormatContext, *mut AVPacket) -> c_int,
    av_write_trailer:               unsafe extern "C" fn(*mut AVFormatContext) -> c_int,
    avio_closep:                    unsafe extern "C" fn(*mut *mut AVIOContext) -> c_int,
    avformat_free_context:          unsafe extern "C" fn(*mut AVFormatContext),

    // avcodec
    avcodec_alloc_context3:        unsafe extern "C" fn(*const AVCodec) -> *mut AVCodecContext,
    avcodec_parameters_to_context: unsafe extern "C" fn(*mut AVCodecContext, *const ()) -> c_int,
    avcodec_open2:                 unsafe extern "C" fn(*mut AVCodecContext, *const AVCodec, *mut ()) -> c_int,
    avcodec_free_context:          unsafe extern "C" fn(*mut *mut AVCodecContext),
    avcodec_send_packet:           unsafe extern "C" fn(*mut AVCodecContext, *const AVPacket) -> c_int,
    avcodec_receive_frame:         unsafe extern "C" fn(*mut AVCodecContext, *mut AVFrame) -> c_int,
    avcodec_flush_buffers:         unsafe extern "C" fn(*mut AVCodecContext),
    av_packet_alloc:               unsafe extern "C" fn() -> *mut AVPacket,
    av_packet_free:                unsafe extern "C" fn(*mut *mut AVPacket),
    av_packet_unref:               unsafe extern "C" fn(*mut AVPacket),
    av_frame_alloc:                unsafe extern "C" fn() -> *mut AVFrame,
    av_frame_unref:                unsafe extern "C" fn(*mut AVFrame),
    av_frame_free:                 unsafe extern "C" fn(*mut *mut AVFrame),

    avcodec_parameters_from_context: unsafe extern "C" fn(*mut (), *const AVCodecContext) -> c_int,
    avcodec_find_decoder:           unsafe extern "C" fn(c_int) -> *const AVCodec,
    avcodec_find_encoder_by_name:  unsafe extern "C" fn(*const c_char) -> *const AVCodec,
    avcodec_send_frame:            unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int,
    avcodec_receive_packet:        unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int,

    // avutil
    av_frame_get_buffer:           unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int,
    av_opt_set:                    unsafe extern "C" fn(*mut (), *const c_char, *const c_char, c_int) -> c_int,

    // swscale
    sws_getContext: unsafe extern "C" fn(c_int,c_int,c_int,c_int,c_int,c_int,c_int,*mut (),*mut (),*const f64) -> *mut SwsContext,
    sws_scale:      unsafe extern "C" fn(*mut SwsContext,*const *const u8,*const c_int,c_int,c_int,*const *mut u8,*const c_int) -> c_int,
    sws_freeContext: unsafe extern "C" fn(*mut SwsContext),
}

unsafe impl Send for FfmpegLibs {}
unsafe impl Sync for FfmpegLibs {}

static FFMPEG_LIBS: OnceLock<Option<FfmpegLibs>> = OnceLock::new();

// ── Library loader ────────────────────────────────────────────────────────────
fn try_load_ffmpeg() -> Option<FfmpegLibs> {
    macro_rules! open_lib {
        ($names:expr) => {{
            let mut found = None;
            for name in $names {
                if let Ok(lib) = unsafe { libloading::Library::new(name) } {
                    found = Some(lib);
                    break;
                }
            }
            found?
        }};
    }
    macro_rules! sym {
        ($lib:expr, $ty:ty, $name:literal) => {
            unsafe { *$lib.get::<$ty>($name).ok()? }
        };
    }

    let avformat = open_lib!(["libavformat.so.60","libavformat.so","libavformat.60.dylib","avformat-60.dll"]);
    let avcodec  = open_lib!(["libavcodec.so.60", "libavcodec.so", "libavcodec.60.dylib", "avcodec-60.dll" ]);
    let avutil   = open_lib!(["libavutil.so.58",  "libavutil.so",  "libavutil.58.dylib",  "avutil-58.dll"  ]);
    let swscale  = open_lib!(["libswscale.so.7",  "libswscale.so", "libswscale.7.dylib",  "swscale-7.dll"  ]);

    Some(FfmpegLibs {
        avformat_version:              sym!(avformat, unsafe extern "C" fn() -> std::os::raw::c_uint,                                                         b"avformat_version\0"),
        avformat_open_input:           sym!(avformat, unsafe extern "C" fn(*mut *mut AVFormatContext,*const c_char,*mut (),*mut ())->c_int,                  b"avformat_open_input\0"),
        avformat_find_stream_info:     sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext,*mut ())->c_int,                                              b"avformat_find_stream_info\0"),
        avformat_close_input:          sym!(avformat, unsafe extern "C" fn(*mut *mut AVFormatContext),                                                        b"avformat_close_input\0"),
        av_find_best_stream:           sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext,c_int,c_int,c_int,*mut *const AVCodec,c_int)->c_int,          b"av_find_best_stream\0"),
        av_seek_frame:                 sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext,c_int,i64,c_int)->c_int,                                      b"av_seek_frame\0"),
        av_read_frame:                 sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext,*mut AVPacket)->c_int,                                         b"av_read_frame\0"),

        avformat_alloc_output_context2: sym!(avformat, unsafe extern "C" fn(*mut *mut AVFormatContext, *mut (), *const c_char, *const c_char) -> c_int, b"avformat_alloc_output_context2\0"),
        avformat_new_stream:            sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext, *const AVCodec) -> *mut (), b"avformat_new_stream\0"),
        avio_open:                      sym!(avformat, unsafe extern "C" fn(*mut *mut AVIOContext, *const c_char, c_int) -> c_int, b"avio_open\0"),
        avformat_write_header:          sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext, *mut *mut ()) -> c_int, b"avformat_write_header\0"),
        av_interleaved_write_frame:     sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext, *mut AVPacket) -> c_int, b"av_interleaved_write_frame\0"),
        av_write_trailer:               sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext) -> c_int, b"av_write_trailer\0"),
        avio_closep:                    sym!(avformat, unsafe extern "C" fn(*mut *mut AVIOContext) -> c_int, b"avio_closep\0"),
        avformat_free_context:          sym!(avformat, unsafe extern "C" fn(*mut AVFormatContext), b"avformat_free_context\0"),

        avcodec_alloc_context3:        sym!(avcodec,  unsafe extern "C" fn(*const AVCodec)->*mut AVCodecContext,                                             b"avcodec_alloc_context3\0"),
        avcodec_parameters_to_context: sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*const ())->c_int,                                            b"avcodec_parameters_to_context\0"),
        avcodec_open2:                 sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*const AVCodec,*mut ())->c_int,                                b"avcodec_open2\0"),
        avcodec_free_context:          sym!(avcodec,  unsafe extern "C" fn(*mut *mut AVCodecContext),                                                         b"avcodec_free_context\0"),
        avcodec_send_packet:           sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*const AVPacket)->c_int,                                       b"avcodec_send_packet\0"),
        avcodec_receive_frame:         sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*mut AVFrame)->c_int,                                          b"avcodec_receive_frame\0"),
        avcodec_flush_buffers:         sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext),                                                              b"avcodec_flush_buffers\0"),
        av_packet_alloc:               sym!(avcodec,  unsafe extern "C" fn()->*mut AVPacket,                                                                  b"av_packet_alloc\0"),
        av_packet_free:                sym!(avcodec,  unsafe extern "C" fn(*mut *mut AVPacket),                                                               b"av_packet_free\0"),
        av_packet_unref:               sym!(avcodec,  unsafe extern "C" fn(*mut AVPacket),                                                                    b"av_packet_unref\0"),
        av_frame_alloc:                sym!(avcodec,  unsafe extern "C" fn()->*mut AVFrame,                                                                   b"av_frame_alloc\0"),
        av_frame_unref:                sym!(avcodec,  unsafe extern "C" fn(*mut AVFrame),                                                                    b"av_frame_unref\0"),
        av_frame_free:                 sym!(avcodec,  unsafe extern "C" fn(*mut *mut AVFrame),                                                                b"av_frame_free\0"),

        avcodec_parameters_from_context: sym!(avcodec, unsafe extern "C" fn(*mut (), *const AVCodecContext) -> c_int, b"avcodec_parameters_from_context\0"),
        avcodec_find_decoder:           sym!(avcodec,  unsafe extern "C" fn(c_int) -> *const AVCodec, b"avcodec_find_decoder\0"),
        avcodec_find_encoder_by_name:  sym!(avcodec,  unsafe extern "C" fn(*const c_char) -> *const AVCodec, b"avcodec_find_encoder_by_name\0"),
        avcodec_send_frame:            sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int, b"avcodec_send_frame\0"),
        avcodec_receive_packet:        sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int, b"avcodec_receive_packet\0"),

        av_frame_get_buffer:           sym!(avutil,   unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int, b"av_frame_get_buffer\0"),
        av_opt_set:                    sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char, *const c_char, c_int) -> c_int, b"av_opt_set\0"),

        sws_getContext: sym!(swscale, unsafe extern "C" fn(c_int,c_int,c_int,c_int,c_int,c_int,c_int,*mut (),*mut (),*const f64)->*mut SwsContext, b"sws_getContext\0"),
        sws_scale:      sym!(swscale, unsafe extern "C" fn(*mut SwsContext,*const *const u8,*const c_int,c_int,c_int,*const *mut u8,*const c_int)->c_int, b"sws_scale\0"),
        sws_freeContext: sym!(swscale, unsafe extern "C" fn(*mut SwsContext), b"sws_freeContext\0"),

        _avformat: avformat,
        _avcodec:  avcodec,
        _avutil:   avutil,
        _swscale:  swscale,
    })
}

// ── ABI offset helpers (FFmpeg 6.x / n6.x, 64-bit) ───────────────────────────
// AVFormatContext: nb_streams@44, streams@48
// AVStream:        codecpar@8, time_base(num@24, den@28)
// AVCodecParameters: width@56, height@60
// AVPacket:        stream_index@36
// AVFrame:         data[0]@0, linesize[0]@64, width@104, height@108, format@116

unsafe fn stream_time_base_num(stream: *mut u8) -> i32 {
    stream.add(24).cast::<i32>().read()
}
unsafe fn stream_time_base_den(stream: *mut u8) -> i32 {
    stream.add(28).cast::<i32>().read()
}

unsafe fn fmt_stream(c: *mut AVFormatContext, i: u32) -> *mut u8 {
    unsafe {
        let ptr = (c as *const u8).add(48).cast::<*mut *mut u8>().read();
        ptr.add(i as usize).read()
    }
}
unsafe fn stream_codecpar(s: *mut u8, avformat_major: u32) -> *mut u8 {
    unsafe {
        let offset = if avformat_major >= 59 { 16 } else { 8 };
        s.add(offset).cast::<*mut u8>().read()
    }
}
unsafe fn codecpar_width(cp: *mut u8)  -> i32 { unsafe { cp.add(56).cast::<i32>().read() } }
unsafe fn codecpar_height(cp: *mut u8) -> i32 { unsafe { cp.add(60).cast::<i32>().read() } }
unsafe fn stream_tb_num(s: *mut u8, avformat_major: u32) -> i32 {
    unsafe {
        let offset = if avformat_major >= 59 { 32 } else { 24 };
        s.add(offset).cast::<i32>().read()
    }
}
unsafe fn stream_tb_den(s: *mut u8, avformat_major: u32) -> i32 {
    unsafe {
        let offset = if avformat_major >= 59 { 36 } else { 28 };
        s.add(offset).cast::<i32>().read()
    }
}
unsafe fn pkt_stream_index(p: *const AVPacket) -> i32 {
    unsafe { (p as *const u8).add(36).cast::<i32>().read() }
}
unsafe fn frame_data(f: *mut AVFrame, plane: usize) -> *mut u8 {
    unsafe { (f as *mut u8).add(plane * 8).cast::<*mut u8>().read() }
}
unsafe fn frame_linesize(f: *mut AVFrame, plane: usize) -> c_int {
    unsafe { (f as *const u8).add(64 + plane * 4).cast::<c_int>().read() }
}
unsafe fn frame_width(f: *mut AVFrame)  -> i32 { unsafe { (f as *const u8).add(104).cast::<i32>().read() } }
unsafe fn frame_height(f: *mut AVFrame) -> i32 { unsafe { (f as *const u8).add(108).cast::<i32>().read() } }
unsafe fn frame_format(f: *mut AVFrame) -> i32 { unsafe { (f as *const u8).add(116).cast::<i32>().read() } }
unsafe fn frame_nb_samples(f: *mut AVFrame) -> i32 { unsafe { (f as *const u8).add(88).cast::<i32>().read() } }
unsafe fn codecpar_codec_id(cp: *mut u8) -> i32 { unsafe { cp.add(4).cast::<i32>().read() } }

// ── Public API ────────────────────────────────────────────────────────────────

/// Decode one video frame from `video_path` at `source_frame` index.
/// Returns `(width, height, rgba_bytes)`.
pub fn decode_frame(video_path: &str, source_frame: usize, fps: f32) -> Option<(u32, u32, Vec<u8>)> {
    let libs = FFMPEG_LIBS.get_or_init(|| {
        match try_load_ffmpeg() {
            Some(l) => { log::info!("[video] libav backend active"); Some(l) }
            None    => { log::warn!("[video] libav not found, using process backend"); None }
        }
    });
    match libs {
        Some(libs) => decode_libav(libs, video_path, source_frame, fps),
        None => {
            log::warn!("[video] libav not loaded — cannot decode (subprocess ffmpeg disabled)");
            None
        }
    }
}

/// Returns `true` if FFmpeg shared libraries loaded successfully.
pub fn is_libav_available() -> bool {
    FFMPEG_LIBS.get_or_init(|| try_load_ffmpeg()).is_some()
}

/// Demux + decode full audio track to mono f32 samples via libav.
pub fn decode_audio_to_mono_f32_libav(input: &str) -> Result<(Vec<f32>, u32), String> {
    let libs = FFMPEG_LIBS
        .get_or_init(|| try_load_ffmpeg())
        .as_ref()
        .ok_or_else(|| "FFmpeg libraries not loaded".to_string())?;

    let path_c = CString::new(input).map_err(|e| e.to_string())?;
    let mut mono: Vec<f32> = Vec::new();
    let mut sample_rate: u32 = 44_100;

    unsafe {
        let mut fmt: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_open_input)(&mut fmt, path_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut())
            < 0
        {
            return Err("avformat_open_input failed".into());
        }
        if (libs.avformat_find_stream_info)(fmt, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut fmt);
            return Err("avformat_find_stream_info failed".into());
        }

        let mut codec_ptr: *const AVCodec = std::ptr::null();
        let si = (libs.av_find_best_stream)(fmt, AVMEDIA_TYPE_AUDIO, -1, -1, &mut codec_ptr, 0);
        if si < 0 {
            (libs.avformat_close_input)(&mut fmt);
            return Err("No audio stream".into());
        }

        let avformat_major = (libs.avformat_version)() >> 16;
        let stream = fmt_stream(fmt, si as u32);
        let cp = stream_codecpar(stream, avformat_major);
        let codec_id = codecpar_codec_id(cp);
        if codec_ptr.is_null() {
            codec_ptr = (libs.avcodec_find_decoder)(codec_id);
        }
        if codec_ptr.is_null() {
            (libs.avformat_close_input)(&mut fmt);
            return Err("No audio decoder".into());
        }

        let cc = (libs.avcodec_alloc_context3)(codec_ptr);
        if cc.is_null() {
            (libs.avformat_close_input)(&mut fmt);
            return Err("avcodec_alloc_context3 failed".into());
        }
        if (libs.avcodec_parameters_to_context)(cc, cp as *const ()) < 0
            || (libs.avcodec_open2)(cc, codec_ptr, std::ptr::null_mut()) < 0
        {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt);
            return Err("Could not open audio decoder".into());
        }
        let sr = (cc as *const u8).add(304).cast::<i32>().read();
        if sr > 0 {
            sample_rate = sr as u32;
        }

        let pkt = (libs.av_packet_alloc)();
        let frame = (libs.av_frame_alloc)();
        if pkt.is_null() || frame.is_null() {
            if !pkt.is_null() {
                (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
            }
            if !frame.is_null() {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
            }
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt);
            return Err("alloc packet/frame failed".into());
        }

        while (libs.av_read_frame)(fmt, pkt.cast::<AVPacket>()) >= 0 {
            if pkt_stream_index(pkt.cast::<AVPacket>()) != si {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            if (libs.avcodec_send_packet)(cc.cast::<AVCodecContext>(), pkt.cast::<AVPacket>()) < 0 {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            (libs.av_packet_unref)(pkt.cast::<AVPacket>());

            loop {
                let r = (libs.avcodec_receive_frame)(cc.cast::<AVCodecContext>(), frame.cast::<AVFrame>());
                if r == AVERROR_EAGAIN || r < -1000 {
                    break;
                }
                if r < 0 {
                    break;
                }
                append_libav_audio_frame(&mut mono, frame.cast::<AVFrame>());
            }
        }

        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        (libs.avformat_close_input)(&mut fmt);
    }

    if mono.is_empty() {
        return Err("No audio samples decoded".into());
    }
    Ok((mono, sample_rate))
}

/// Demux + decode full audio track to interleaved stereo i16 via libav.
pub fn decode_audio_to_stereo_i16_libav(
    input: &str,
    mut on_progress: impl FnMut(f32),
) -> Result<(Vec<i16>, u32), String> {
    let libs = FFMPEG_LIBS
        .get_or_init(|| try_load_ffmpeg())
        .as_ref()
        .ok_or_else(|| "FFmpeg libraries not loaded".to_string())?;

    let path_c = CString::new(input).map_err(|e| e.to_string())?;
    let mut interleaved: Vec<i16> = Vec::new();
    let mut sample_rate: u32 = 44_100;
    let mut duration_ts: i64 = 0;
    let mut stream_tb_num: i32 = 1;
    let mut stream_tb_den: i32 = 1;
    let mut last_pts: i64 = 0;

    unsafe {
        let mut fmt: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_open_input)(&mut fmt, path_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut())
            < 0
        {
            return Err("avformat_open_input failed".into());
        }
        if (libs.avformat_find_stream_info)(fmt, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut fmt);
            return Err("avformat_find_stream_info failed".into());
        }

        let mut codec_ptr: *const AVCodec = std::ptr::null();
        let si = (libs.av_find_best_stream)(fmt, AVMEDIA_TYPE_AUDIO, -1, -1, &mut codec_ptr, 0);
        if si < 0 {
            (libs.avformat_close_input)(&mut fmt);
            return Err("No audio stream".into());
        }

        let avformat_major = (libs.avformat_version)() >> 16;
        let stream = fmt_stream(fmt, si as u32);
        stream_tb_num = stream_time_base_num(stream);
        stream_tb_den = stream_time_base_den(stream).max(1);
        duration_ts = stream_duration(stream, avformat_major);

        let cp = stream_codecpar(stream, avformat_major);
        let codec_id = codecpar_codec_id(cp);
        if codec_ptr.is_null() {
            codec_ptr = (libs.avcodec_find_decoder)(codec_id);
        }
        if codec_ptr.is_null() {
            (libs.avformat_close_input)(&mut fmt);
            return Err("No audio decoder".into());
        }

        let cc = (libs.avcodec_alloc_context3)(codec_ptr);
        if cc.is_null() {
            (libs.avformat_close_input)(&mut fmt);
            return Err("avcodec_alloc_context3 failed".into());
        }
        if (libs.avcodec_parameters_to_context)(cc, cp as *const ()) < 0
            || (libs.avcodec_open2)(cc, codec_ptr, std::ptr::null_mut()) < 0
        {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt);
            return Err("Could not open audio decoder".into());
        }
        let sr = (cc as *const u8).add(304).cast::<i32>().read();
        if sr > 0 {
            sample_rate = sr as u32;
        }

        let pkt = (libs.av_packet_alloc)();
        let frame = (libs.av_frame_alloc)();
        if pkt.is_null() || frame.is_null() {
            if !pkt.is_null() {
                (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
            }
            if !frame.is_null() {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
            }
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt);
            return Err("alloc packet/frame failed".into());
        }

        while (libs.av_read_frame)(fmt, pkt.cast::<AVPacket>()) >= 0 {
            if pkt_stream_index(pkt.cast::<AVPacket>()) != si {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            if (libs.avcodec_send_packet)(cc.cast::<AVCodecContext>(), pkt.cast::<AVPacket>()) < 0 {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            (libs.av_packet_unref)(pkt.cast::<AVPacket>());

            loop {
                let r = (libs.avcodec_receive_frame)(cc.cast::<AVCodecContext>(), frame.cast::<AVFrame>());
                if r == AVERROR_EAGAIN || r < -1000 {
                    break;
                }
                if r < 0 {
                    break;
                }
                append_libav_audio_frame_stereo_i16(&mut interleaved, frame.cast::<AVFrame>());
                last_pts = (frame as *const u8).add(32).cast::<i64>().read();
                if duration_ts > 0 {
                    let p = (last_pts as f64 / duration_ts as f64).clamp(0.0, 1.0) as f32;
                    on_progress(p);
                }
            }
        }

        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        (libs.avformat_close_input)(&mut fmt);
    }

    let _ = (stream_tb_num, stream_tb_den);
    if interleaved.is_empty() {
        return Err("No audio samples decoded".into());
    }
    on_progress(1.0);
    Ok((interleaved, sample_rate))
}

const AV_SAMPLE_FMT_FLTP: i32 = 8;
const MP3_FRAME_SAMPLES: usize = 1152;

/// Encode interleaved stereo i16 PCM to MP3 via libmp3lame (libav, no subprocess).
pub fn write_stereo_i16_as_mp3_libav(
    output: &std::path::Path,
    samples: &[i16],
    sample_rate: u32,
    bitrate_kbps: u32,
    mut on_progress: impl FnMut(f32),
) -> Result<(), String> {
    if samples.len() < 2 {
        return Err("Not enough audio samples".into());
    }
    let libs = FFMPEG_LIBS
        .get_or_init(|| try_load_ffmpeg())
        .as_ref()
        .ok_or_else(|| "FFmpeg libraries not loaded".to_string())?;

    let out_path = output.to_str().ok_or("bad output path")?;
    let out_c = CString::new(out_path).map_err(|e| e.to_string())?;

    unsafe {
        let mut fmt_ctx: *mut AVFormatContext = std::ptr::null_mut();
        let ret = (libs.avformat_alloc_output_context2)(
            &mut fmt_ctx,
            std::ptr::null_mut(),
            std::ptr::null(),
            out_c.as_ptr(),
        );
        if ret < 0 || fmt_ctx.is_null() {
            return Err(format!("Could not allocate MP3 muxer (code {})", ret));
        }

        let candidates = ["libmp3lame", "mp3"];
        let mut opened_codec = std::ptr::null();
        let mut opened_cc = std::ptr::null_mut();

        for &candidate in &candidates {
            let candidate_c = CString::new(candidate).unwrap();
            let codec = (libs.avcodec_find_encoder_by_name)(candidate_c.as_ptr());
            if codec.is_null() {
                continue;
            }
            let cc = (libs.avcodec_alloc_context3)(codec);
            if cc.is_null() {
                continue;
            }
            let cc_void = cc as *mut ();
            let sr = CString::new(sample_rate.to_string()).unwrap();
            let ch = CString::new("2").unwrap();
            let br = CString::new((bitrate_kbps * 1000).to_string()).unwrap();
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("sample_rate").unwrap().as_ptr(),
                sr.as_ptr(),
                0,
            );
            let _ = (libs.av_opt_set)(cc_void, CString::new("channels").unwrap().as_ptr(), ch.as_ptr(), 0);
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("ch_layout").unwrap().as_ptr(),
                CString::new("stereo").unwrap().as_ptr(),
                0,
            );
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("channel_layout").unwrap().as_ptr(),
                CString::new("stereo").unwrap().as_ptr(),
                0,
            );
            let _ = (libs.av_opt_set)(cc_void, CString::new("b").unwrap().as_ptr(), br.as_ptr(), 0);
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("sample_fmt").unwrap().as_ptr(),
                CString::new("fltp").unwrap().as_ptr(),
                0,
            );

            let ret = (libs.avcodec_open2)(cc, codec, std::ptr::null_mut());
            if ret >= 0 {
                opened_codec = codec;
                opened_cc = cc;
                break;
            }
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        }

        if opened_codec.is_null() {
            (libs.avformat_free_context)(fmt_ctx);
            return Err("libmp3lame encoder not available".into());
        }

        let cc = opened_cc;
        let stream = (libs.avformat_new_stream)(fmt_ctx, opened_codec);
        if stream.is_null() {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("Could not create audio stream".into());
        }

        let avformat_major = (libs.avformat_version)() >> 16;
        let stream_offset = if avformat_major >= 59 { 16 } else { 8 };
        let codecpar_ptr = (stream as *mut u8).add(stream_offset) as *mut *mut ();
        let codecpar = codecpar_ptr.read();
        if (libs.avcodec_parameters_from_context)(codecpar, cc) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avcodec_parameters_from_context failed".into());
        }

        let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
        if (libs.avio_open)(&mut io_ctx, out_c.as_ptr(), 2) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avio_open failed".into());
        }
        let fmt_io_ptr = (fmt_ctx as *mut u8).add(40) as *mut *mut AVIOContext;
        fmt_io_ptr.write(io_ctx);

        if (libs.avformat_write_header)(fmt_ctx, std::ptr::null_mut()) < 0 {
            (libs.avio_closep)(&mut io_ctx);
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avformat_write_header failed".into());
        }

        let frame = (libs.av_frame_alloc)();
        let pkt = (libs.av_packet_alloc)();
        if frame.is_null() || pkt.is_null() {
            if !frame.is_null() {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
            }
            if !pkt.is_null() {
                (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
            }
            (libs.avio_closep)(&mut io_ctx);
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("alloc frame/packet failed".into());
        }

        let total_frames = samples.len() / 2;
        let mut src_frame = 0usize;
        let mut pts: i64 = 0;

        while src_frame < total_frames {
            let chunk = (total_frames - src_frame).min(MP3_FRAME_SAMPLES);
            frame_set_nb_samples(frame, chunk as i32);
            frame_set_format(frame, AV_SAMPLE_FMT_FLTP);
            let frame_void = frame as *mut ();
            let _ = (libs.av_opt_set)(
                frame_void,
                CString::new("ch_layout").unwrap().as_ptr(),
                CString::new("stereo").unwrap().as_ptr(),
                0,
            );
            if (libs.av_frame_get_buffer)(frame, 0) < 0 {
                break;
            }

            let l_plane = frame_data(frame, 0) as *mut f32;
            let r_plane = frame_data(frame, 1) as *mut f32;
            for i in 0..chunk {
                let base = (src_frame + i) * 2;
                let l = samples[base] as f32 / i16::MAX as f32;
                let r = samples.get(base + 1).copied().unwrap_or(samples[base]) as f32 / i16::MAX as f32;
                *l_plane.add(i) = l.clamp(-1.0, 1.0);
                *r_plane.add(i) = r.clamp(-1.0, 1.0);
            }
            frame_set_pts(frame, pts);
            pts += chunk as i64;

            if (libs.avcodec_send_frame)(cc, frame) >= 0 {
                loop {
                    (libs.av_packet_unref)(pkt);
                    let ret = (libs.avcodec_receive_packet)(cc, pkt);
                    if ret == AVERROR_EAGAIN || ret < -1000 {
                        break;
                    }
                    if ret < 0 {
                        break;
                    }
                    (libs.av_interleaved_write_frame)(fmt_ctx, pkt);
                }
            }

            src_frame += chunk;
            on_progress((src_frame as f32 / total_frames as f32).clamp(0.0, 1.0));
            (libs.av_frame_unref)(frame);
        }

        let _ = (libs.avcodec_send_frame)(cc, std::ptr::null());
        loop {
            (libs.av_packet_unref)(pkt);
            let ret = (libs.avcodec_receive_packet)(cc, pkt);
            if ret == AVERROR_EAGAIN || ret < -1000 {
                break;
            }
            if ret < 0 {
                break;
            }
            (libs.av_interleaved_write_frame)(fmt_ctx, pkt);
        }

        (libs.av_write_trailer)(fmt_ctx);
        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        (libs.avio_closep)(&mut io_ctx);
        (libs.avformat_free_context)(fmt_ctx);
    }

    on_progress(1.0);
    Ok(())
}

unsafe fn frame_set_nb_samples(f: *mut AVFrame, n: i32) {
    (f as *mut u8).add(88).cast::<i32>().write(n);
}
unsafe fn frame_set_format(f: *mut AVFrame, fmt: i32) {
    (f as *mut u8).add(116).cast::<i32>().write(fmt);
}
unsafe fn frame_set_pts(f: *mut AVFrame, pts: i64) {
    (f as *mut u8).add(32).cast::<i64>().write(pts);
}

unsafe fn stream_duration(stream: *mut u8, avformat_major: u32) -> i64 {
    let off = if avformat_major >= 59 { 40 } else { 32 };
    (stream.add(off).cast::<i64>().read()).max(0)
}

unsafe fn append_libav_audio_frame_stereo_i16(out: &mut Vec<i16>, frame: *mut AVFrame) {
    let n = frame_nb_samples(frame).max(0) as usize;
    if n == 0 {
        return;
    }
    let fmt = frame_format(frame);
    let mut planes = 0usize;
    for p in 0..8 {
        if frame_data(frame, p).is_null() {
            break;
        }
        planes += 1;
    }
    if planes == 0 {
        planes = 1;
    }
    if fmt == 3 {
        for i in 0..n {
            let l = *(frame_data(frame, 0) as *const f32).add(i);
            let r = if planes > 1 {
                *(frame_data(frame, 1) as *const f32).add(i)
            } else {
                l
            };
            out.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            out.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
    } else if fmt == 1 {
        let ptr = frame_data(frame, 0) as *const i16;
        let total = n * planes;
        for i in 0..n {
            let l = *ptr.add(i * planes);
            let r = if planes > 1 {
                *ptr.add(i * planes + 1)
            } else {
                l
            };
            out.push(l);
            out.push(r);
        }
        let _ = total;
    } else if fmt == 7 {
        for i in 0..n {
            let l = *(frame_data(frame, 0) as *const i16).add(i);
            let r = if planes > 1 {
                *(frame_data(frame, 1) as *const i16).add(i)
            } else {
                l
            };
            out.push(l);
            out.push(r);
        }
    }
}

unsafe fn append_libav_audio_frame(out: &mut Vec<f32>, frame: *mut AVFrame) {
    let n = frame_nb_samples(frame).max(0) as usize;
    if n == 0 {
        return;
    }
    let fmt = frame_format(frame);
    let mut planes = 0usize;
    for p in 0..8 {
        if frame_data(frame, p).is_null() {
            break;
        }
        planes += 1;
    }
    if planes == 0 {
        planes = 1;
    }
    // AV_SAMPLE_FMT_FLTP = 3, S16P = 7, S16 = 1
    if fmt == 3 {
        for i in 0..n {
            let mut sum = 0.0f32;
            for p in 0..planes {
                let ptr = frame_data(frame, p) as *const f32;
                sum += *ptr.add(i);
            }
            out.push(sum / planes as f32);
        }
    } else if fmt == 1 {
        let ptr = frame_data(frame, 0) as *const i16;
        let total = n * planes;
        for i in 0..total {
            let s = *ptr.add(i) as f32 / i16::MAX as f32;
            if i % planes == 0 {
                out.push(s);
            } else {
                let last = out.len() - 1;
                out[last] = (out[last] + s) / 2.0;
            }
        }
    } else if fmt == 7 {
        for i in 0..n {
            let mut sum = 0.0f32;
            for p in 0..planes {
                let ptr = frame_data(frame, p) as *const i16;
                sum += *ptr.add(i) as f32 / i16::MAX as f32;
            }
            out.push(sum / planes as f32);
        }
    }
}

// ── libav backend ─────────────────────────────────────────────────────────────
fn decode_libav(libs: &FfmpegLibs, path: &str, source_frame: usize, fps: f32) -> Option<(u32, u32, Vec<u8>)> {
    let path_c = CString::new(path).ok()?;
    let time_sec = source_frame as f64 / fps as f64;

    unsafe {
        // Open container
        let mut fmt: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_open_input)(&mut fmt, path_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return None; }
        if (libs.avformat_find_stream_info)(fmt, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut fmt); return None;
        }

        // Best video stream
        let mut codec_ptr: *const AVCodec = std::ptr::null();
        let si = (libs.av_find_best_stream)(fmt, AVMEDIA_TYPE_VIDEO, -1, -1, &mut codec_ptr, 0);
        if si < 0 || codec_ptr.is_null() { (libs.avformat_close_input)(&mut fmt); return None; }

        let avformat_major = (libs.avformat_version)() >> 16;

        let stream = fmt_stream(fmt, si as u32);
        let cp = stream_codecpar(stream, avformat_major);
        let w = codecpar_width(cp);
        let h = codecpar_height(cp);
        if w <= 0 || h <= 0 { (libs.avformat_close_input)(&mut fmt); return None; }

        // Codec context
        let cc = (libs.avcodec_alloc_context3)(codec_ptr);
        if cc.is_null() { (libs.avformat_close_input)(&mut fmt); return None; }
        if (libs.avcodec_parameters_to_context)(cc, cp as *const ()) < 0
            || (libs.avcodec_open2)(cc, codec_ptr, std::ptr::null_mut()) < 0
        {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt); return None;
        }

        // Seek
        let tb_n = stream_tb_num(stream, avformat_major);
        let tb_d = stream_tb_den(stream, avformat_major);
        let ts = if tb_n > 0 && tb_d > 0 {
            (time_sec * tb_d as f64 / tb_n as f64) as i64
        } else {
            (time_sec * 1_000_000.0) as i64
        };
        let _ = (libs.av_seek_frame)(fmt, si, ts, 1 /*AVSEEK_FLAG_BACKWARD*/);
        (libs.avcodec_flush_buffers)(cc.cast::<AVCodecContext>());

        let pkt   = (libs.av_packet_alloc)();
        let frame = (libs.av_frame_alloc)();
        if pkt.is_null() || frame.is_null() {
            if !pkt.is_null()   { (libs.av_packet_free)(&mut pkt.cast::<AVPacket>()); }
            if !frame.is_null() { (libs.av_frame_free)(&mut frame.cast::<AVFrame>()); }
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_close_input)(&mut fmt); return None;
        }

        let mut result: Option<(u32, u32, Vec<u8>)> = None;
        let skip_to = source_frame.saturating_sub(4);
        let mut decoded = 0usize;

        'read: loop {
            if (libs.av_read_frame)(fmt, pkt.cast::<AVPacket>()) < 0 { break; }
            if pkt_stream_index(pkt.cast::<AVPacket>()) != si {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            if (libs.avcodec_send_packet)(cc.cast::<AVCodecContext>(), pkt.cast::<AVPacket>()) < 0 {
                (libs.av_packet_unref)(pkt.cast::<AVPacket>());
                continue;
            }
            (libs.av_packet_unref)(pkt.cast::<AVPacket>());

            loop {
                let r = (libs.avcodec_receive_frame)(cc.cast::<AVCodecContext>(), frame.cast::<AVFrame>());
                if r == AVERROR_EAGAIN || r < -1000 { break; }
                if r < 0 { break 'read; }

                if decoded < skip_to { decoded += 1; continue; }

                let fw = frame_width(frame.cast::<AVFrame>());
                let fh = frame_height(frame.cast::<AVFrame>());
                let fmt_id = frame_format(frame.cast::<AVFrame>());
                if fw <= 0 || fh <= 0 { decoded += 1; continue; }

                let stride = fw * 4;
                let mut rgba = vec![0u8; (stride * fh) as usize];

                let sws = (libs.sws_getContext)(
                    fw, fh, fmt_id,
                    fw, fh, AV_PIX_FMT_RGBA,
                    SWS_BILINEAR, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(),
                );
                if sws.is_null() { decoded += 1; continue; }

                let src_data: [*const u8; 8] = [
                    frame_data(frame.cast::<AVFrame>(), 0),
                    frame_data(frame.cast::<AVFrame>(), 1),
                    frame_data(frame.cast::<AVFrame>(), 2),
                    frame_data(frame.cast::<AVFrame>(), 3),
                    std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(),
                ];
                let src_ls: [c_int; 8] = [
                    frame_linesize(frame.cast::<AVFrame>(), 0),
                    frame_linesize(frame.cast::<AVFrame>(), 1),
                    frame_linesize(frame.cast::<AVFrame>(), 2),
                    frame_linesize(frame.cast::<AVFrame>(), 3),
                    0, 0, 0, 0,
                ];
                let dst_ptr = rgba.as_mut_ptr();
                let dst_data: [*mut u8; 8] = [dst_ptr, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
                                               std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()];
                let dst_ls: [c_int; 8] = [stride, 0, 0, 0, 0, 0, 0, 0];

                (libs.sws_scale)(sws, src_data.as_ptr(), src_ls.as_ptr(), 0, fh,
                                  dst_data.as_ptr() as *const *mut u8, dst_ls.as_ptr());
                (libs.sws_freeContext)(sws);

                result = Some((fw as u32, fh as u32, rgba));
                break 'read;
            }
            decoded += 1;
        }

        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        (libs.avformat_close_input)(&mut fmt);
        result
    }
}

// ── Streaming API ─────────────────────────────────────────────────────────────
pub struct VideoStream {
    libs: &'static FfmpegLibs,
    fmt: *mut AVFormatContext,
    cc: *mut AVCodecContext,
    pkt: *mut AVPacket,
    frame: *mut AVFrame,
    stream_idx: i32,
    avformat_major: u32,
    pub width: u32,
    pub height: u32,
    current_frame: Option<usize>,
}

unsafe impl Send for VideoStream {}
unsafe impl Sync for VideoStream {}

impl VideoStream {
    pub fn open(path: &str) -> Option<Self> {
        let libs = FFMPEG_LIBS.get()?.as_ref()?;
        let path_c = CString::new(path).ok()?;
        
        unsafe {
            let mut fmt: *mut AVFormatContext = std::ptr::null_mut();
            if (libs.avformat_open_input)(&mut fmt, path_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return None; }
            if (libs.avformat_find_stream_info)(fmt, std::ptr::null_mut()) < 0 {
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }

            let mut codec_ptr: *const AVCodec = std::ptr::null();
            let si = (libs.av_find_best_stream)(fmt, AVMEDIA_TYPE_VIDEO, -1, -1, &mut codec_ptr, 0);
            if si < 0 || codec_ptr.is_null() {
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }

            let avformat_major = (libs.avformat_version)() >> 16;
            let stream = fmt_stream(fmt, si as u32);
            let cp = stream_codecpar(stream, avformat_major);
            let w = codecpar_width(cp);
            let h = codecpar_height(cp);
            if w <= 0 || h <= 0 {
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }

            let cc = (libs.avcodec_alloc_context3)(codec_ptr);
            if cc.is_null() {
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }
            if (libs.avcodec_parameters_to_context)(cc, cp as *const ()) < 0
                || (libs.avcodec_open2)(cc, codec_ptr, std::ptr::null_mut()) < 0
            {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }

            let pkt = (libs.av_packet_alloc)();
            let frame = (libs.av_frame_alloc)();
            if pkt.is_null() || frame.is_null() {
                if !pkt.is_null() { (libs.av_packet_free)(&mut pkt.cast::<AVPacket>()); }
                if !frame.is_null() { (libs.av_frame_free)(&mut frame.cast::<AVFrame>()); }
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_close_input)(&mut fmt);
                return None;
            }

            Some(Self {
                libs,
                fmt,
                cc,
                pkt,
                frame,
                stream_idx: si,
                avformat_major,
                width: w as u32,
                height: h as u32,
                current_frame: None,
            })
        }
    }

    pub fn get_frame(&mut self, frame_idx: usize, fps: f32) -> Option<(u32, u32, Vec<u8>)> {
        let time_sec = frame_idx as f64 / fps as f64;
        
        unsafe {
            // Optimize seeking: only seek if target frame is backward OR too far forward (> 30 frames)
            let seek_needed = match self.current_frame {
                Some(curr) => frame_idx < curr || frame_idx > curr + 30,
                None => true,
            };
            
            if seek_needed {
                let stream = fmt_stream(self.fmt, self.stream_idx as u32);
                let tb_n = stream_tb_num(stream, self.avformat_major);
                let tb_d = stream_tb_den(stream, self.avformat_major);
                let ts = if tb_n > 0 && tb_d > 0 {
                    (time_sec * tb_d as f64 / tb_n as f64) as i64
                } else {
                    (time_sec * 1_000_000.0) as i64
                };
                let _ = (self.libs.av_seek_frame)(self.fmt, self.stream_idx, ts, 1 /*AVSEEK_FLAG_BACKWARD*/);
                (self.libs.avcodec_flush_buffers)(self.cc.cast::<AVCodecContext>());
                self.current_frame = None;
            }
            
            let mut decoded = self.current_frame;
            
            'read: loop {
                if (self.libs.av_read_frame)(self.fmt, self.pkt.cast::<AVPacket>()) < 0 { break; }
                if pkt_stream_index(self.pkt.cast::<AVPacket>()) != self.stream_idx {
                    (self.libs.av_packet_unref)(self.pkt.cast::<AVPacket>());
                    continue;
                }
                if (self.libs.avcodec_send_packet)(self.cc.cast::<AVCodecContext>(), self.pkt.cast::<AVPacket>()) < 0 {
                    (self.libs.av_packet_unref)(self.pkt.cast::<AVPacket>());
                    continue;
                }
                (self.libs.av_packet_unref)(self.pkt.cast::<AVPacket>());
                
                loop {
                    let r = (self.libs.avcodec_receive_frame)(self.cc.cast::<AVCodecContext>(), self.frame.cast::<AVFrame>());
                    if r == AVERROR_EAGAIN || r < -1000 { break; }
                    if r < 0 { break 'read; }
                    
                    // Retrieve exact presentation timestamp (pts) at offset 136 of AVFrame to compute true frame index
                    let pts = (self.frame as *const u8).add(136).cast::<i64>().read();
                    let stream = fmt_stream(self.fmt, self.stream_idx as u32);
                    let tb_n = stream_tb_num(stream, self.avformat_major) as f64;
                    let tb_d = stream_tb_den(stream, self.avformat_major) as f64;
                    
                    let frame_idx_decoded = if pts != i64::MIN && tb_d > 0.0 {
                        let sec = pts as f64 * tb_n / tb_d;
                        (sec * fps as f64).round() as usize
                    } else {
                        decoded.map(|d| d + 1).unwrap_or(0)
                    };
                    
                    decoded = Some(frame_idx_decoded);
                    
                    if frame_idx_decoded < frame_idx {
                        continue;
                    }
                    
                    let fw = frame_width(self.frame.cast::<AVFrame>());
                    let fh = frame_height(self.frame.cast::<AVFrame>());
                    let fmt_id = frame_format(self.frame.cast::<AVFrame>());
                    if fw <= 0 || fh <= 0 { continue; }
                    
                    let stride = fw * 4;
                    let mut rgba = vec![0u8; (stride * fh) as usize];
                    
                    let sws = (self.libs.sws_getContext)(
                        fw, fh, fmt_id,
                        fw, fh, AV_PIX_FMT_RGBA,
                        SWS_BILINEAR, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(),
                    );
                    if sws.is_null() { continue; }
                    
                    let src_data: [*const u8; 8] = [
                        frame_data(self.frame.cast::<AVFrame>(), 0),
                        frame_data(self.frame.cast::<AVFrame>(), 1),
                        frame_data(self.frame.cast::<AVFrame>(), 2),
                        frame_data(self.frame.cast::<AVFrame>(), 3),
                        std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(),
                    ];
                    let src_ls: [c_int; 8] = [
                        frame_linesize(self.frame.cast::<AVFrame>(), 0),
                        frame_linesize(self.frame.cast::<AVFrame>(), 1),
                        frame_linesize(self.frame.cast::<AVFrame>(), 2),
                        frame_linesize(self.frame.cast::<AVFrame>(), 3),
                        0, 0, 0, 0,
                    ];
                    let dst_ptr = rgba.as_mut_ptr();
                    let dst_data: [*mut u8; 8] = [dst_ptr, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
                                                   std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()];
                    let dst_ls: [c_int; 8] = [stride, 0, 0, 0, 0, 0, 0, 0];
                    
                    (self.libs.sws_scale)(sws, src_data.as_ptr(), src_ls.as_ptr(), 0, fh,
                                      dst_data.as_ptr() as *const *mut u8, dst_ls.as_ptr());
                    (self.libs.sws_freeContext)(sws);
                    
                    self.current_frame = Some(frame_idx_decoded);
                    return Some((fw as u32, fh as u32, rgba));
                }
            }
            None
        }
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        unsafe {
            if !self.pkt.is_null() { (self.libs.av_packet_free)(&mut self.pkt.cast::<AVPacket>()); }
            if !self.frame.is_null() { (self.libs.av_frame_free)(&mut self.frame.cast::<AVFrame>()); }
            if !self.cc.is_null() { (self.libs.avcodec_free_context)(&mut self.cc.cast::<AVCodecContext>()); }
            if !self.fmt.is_null() { (self.libs.avformat_close_input)(&mut self.fmt); }
        }
    }
}

pub struct LibavEncoder {
    fmt_ctx: *mut AVFormatContext,
    cc: *mut AVCodecContext,
    frame: *mut AVFrame,
    pkt: *mut AVPacket,
    sws: *mut SwsContext,
    io_ctx: *mut AVIOContext,
    pts: i64,
    width: u32,
    height: u32,
    avformat_major: u32,
    finished: bool,
}

unsafe impl Send for LibavEncoder {}
unsafe impl Sync for LibavEncoder {}

impl LibavEncoder {
    pub fn new(
        output_path: &str,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        vcodec_name: &str,
        // Encoder threads: `1` = power saving, `0` = libav auto (all cores).
        encoder_threads: u32,
    ) -> Result<Self, String> {
        let libs = FFMPEG_LIBS.get_or_init(|| try_load_ffmpeg()).as_ref()
            .ok_or_else(|| "FFmpeg shared libraries not loaded".to_string())?;

        unsafe {
            let output_c = CString::new(output_path).unwrap();
            let mut fmt_ctx: *mut AVFormatContext = std::ptr::null_mut();
            
            let ret = (libs.avformat_alloc_output_context2)(
                &mut fmt_ctx,
                std::ptr::null_mut(),
                std::ptr::null(),
                output_c.as_ptr(),
            );
            if ret < 0 || fmt_ctx.is_null() {
                return Err(format!("Could not allocate output format context (code {})", ret));
            }

            // Software encoders only (no subprocess); skip NVENC/VAAPI to avoid failed init delays.
            let candidates = [vcodec_name, "libx264", "h264"];
            let mut opened_codec = std::ptr::null();
            let mut opened_cc = std::ptr::null_mut();

            for &candidate in &candidates {
                let candidate_c = CString::new(candidate).unwrap();
                let codec = (libs.avcodec_find_encoder_by_name)(candidate_c.as_ptr());
                if codec.is_null() {
                    continue;
                }

                let cc = (libs.avcodec_alloc_context3)(codec);
                if cc.is_null() {
                    continue;
                }

                // Configure codec context fields using ABI-safe av_opt_set
                let cc_void = cc as *mut ();
                let width_str = CString::new(width.to_string()).unwrap();
                let height_str = CString::new(height.to_string()).unwrap();
                let fps_str = CString::new(format!("1/{}", fps)).unwrap();
                let bitrate_str = CString::new((bitrate_kbps * 1000).to_string()).unwrap();
                
                let size_str = CString::new(format!("{}x{}", width, height)).unwrap();
                let r1 = (libs.av_opt_set)(cc_void, CString::new("video_size").unwrap().as_ptr(), size_str.as_ptr(), 0);
                let r2 = (libs.av_opt_set)(cc_void, CString::new("pixel_format").unwrap().as_ptr(), CString::new("yuv420p").unwrap().as_ptr(), 0);
                let r3 = (libs.av_opt_set)(cc_void, CString::new("b").unwrap().as_ptr(), bitrate_str.as_ptr(), 0);
                let r4 = (libs.av_opt_set)(cc_void, CString::new("g").unwrap().as_ptr(), CString::new("12").unwrap().as_ptr(), 0);
                let r5 = (libs.av_opt_set)(cc_void, CString::new("time_base").unwrap().as_ptr(), fps_str.as_ptr(), 0);
                let threads_str = CString::new(encoder_threads.to_string()).unwrap();
                let preset = if encoder_threads <= 1 {
                    CString::new("veryfast").unwrap()
                } else {
                    CString::new("medium").unwrap()
                };
                let _ = (libs.av_opt_set)(
                    cc_void,
                    CString::new("threads").unwrap().as_ptr(),
                    threads_str.as_ptr(),
                    0,
                );
                let _ = (libs.av_opt_set)(
                    cc_void,
                    CString::new("preset").unwrap().as_ptr(),
                    preset.as_ptr(),
                    0,
                );
                let _ = (r1, r2, r3, r4, r5);

                // Open codec
                let ret = (libs.avcodec_open2)(cc, codec, std::ptr::null_mut());
                if ret >= 0 {
                    opened_codec = codec;
                    opened_cc = cc;
                    log::info!("Successfully opened video encoder candidate: {}", candidate);
                    break;
                } else {
                    log::warn!("Failed to open video encoder candidate {}: error {}", candidate, ret);
                    (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                }
            }

            if opened_codec.is_null() {
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Failed to open any video encoder candidate".to_string());
            }

            let codec = opened_codec;
            let cc = opened_cc;

            // Add stream
            let stream = (libs.avformat_new_stream)(fmt_ctx, codec);
            if stream.is_null() {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not create stream".to_string());
            }

            // Copy codec context parameters to stream's codecpar
            let avformat_major = (libs.avformat_version)() >> 16;
            let stream_offset = if avformat_major >= 59 { 16 } else { 8 };
            let codecpar_ptr = (stream as *mut u8).add(stream_offset) as *mut *mut ();
            let codecpar = codecpar_ptr.read();

            let ret = (libs.avcodec_parameters_from_context)(codecpar, cc);
            if ret < 0 {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not copy codec parameters".to_string());
            }

            // Open output file
            let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
            let ret = (libs.avio_open)(&mut io_ctx, output_c.as_ptr(), 2 /*AVIO_FLAG_WRITE*/);
            if ret < 0 || io_ctx.is_null() {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err(format!("Could not open output file '{}' (code {})", output_path, ret));
            }
            (fmt_ctx as *mut u8).add(32).cast::<*mut AVIOContext>().write(io_ctx);

            // Write header
            let ret = (libs.avformat_write_header)(fmt_ctx, std::ptr::null_mut());
            if ret < 0 {
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err(format!("Could not write format header (code {})", ret));
            }

            // Prepare frame for conversion (YUV420P)
            let frame = (libs.av_frame_alloc)();
            if frame.is_null() {
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not allocate frame".to_string());
            }
            (frame as *mut u8).add(116).cast::<c_int>().write(0); // format = YUV420P
            (frame as *mut u8).add(104).cast::<c_int>().write(width as c_int);
            (frame as *mut u8).add(108).cast::<c_int>().write(height as c_int);

            let ret = (libs.av_frame_get_buffer)(frame, 0);
            if ret < 0 {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err(format!("Could not allocate frame data (code {})", ret));
            }

            // Prepare packet
            let pkt = (libs.av_packet_alloc)();

            // sws context for RGBA -> YUV420P
            let sws = (libs.sws_getContext)(
                width as c_int, height as c_int, AV_PIX_FMT_RGBA,
                width as c_int, height as c_int, 0, // AV_PIX_FMT_YUV420P
                SWS_BILINEAR, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(),
            );

            Ok(Self {
                fmt_ctx,
                cc,
                frame,
                pkt,
                sws,
                io_ctx,
                pts: 0,
                width,
                height,
                avformat_major,
                finished: false,
            })
        }
    }

    pub fn write_frame(&mut self, rgba_data: &[u8]) -> Result<(), String> {
        let libs = FFMPEG_LIBS.get()
            .and_then(|opt| opt.as_ref())
            .ok_or_else(|| "FFmpeg libs not loaded".to_string())?;

        unsafe {
            let src_ptr = rgba_data.as_ptr();
            let src_data: [*const u8; 8] = [
                src_ptr, std::ptr::null(), std::ptr::null(), std::ptr::null(),
                std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null()
            ];
            let src_ls: [c_int; 8] = [
                (self.width * 4) as c_int, 0, 0, 0, 0, 0, 0, 0
            ];

            let dst_data: [*mut u8; 8] = [
                frame_data(self.frame, 0),
                frame_data(self.frame, 1),
                frame_data(self.frame, 2),
                frame_data(self.frame, 3),
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()
            ];
            let dst_ls: [c_int; 8] = [
                frame_linesize(self.frame, 0),
                frame_linesize(self.frame, 1),
                frame_linesize(self.frame, 2),
                frame_linesize(self.frame, 3),
                0, 0, 0, 0
            ];

            (libs.sws_scale)(
                self.sws,
                src_data.as_ptr(),
                src_ls.as_ptr(),
                0,
                self.height as c_int,
                dst_data.as_ptr() as *const *mut u8,
                dst_ls.as_ptr(),
            );

            (self.frame as *mut u8).add(136).cast::<i64>().write(self.pts);
            self.pts += 1;

            let ret = (libs.avcodec_send_frame)(self.cc, self.frame);
            if ret < 0 {
                return Err(format!("avcodec_send_frame failed with code {}", ret));
            }

            loop {
                (libs.av_packet_unref)(self.pkt);
                let ret = (libs.avcodec_receive_packet)(self.cc, self.pkt);
                if ret == AVERROR_EAGAIN || ret < -1000 {
                    break;
                }
                if ret < 0 {
                    return Err(format!("avcodec_receive_packet failed with code {}", ret));
                }
                (libs.av_interleaved_write_frame)(self.fmt_ctx, self.pkt);
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        let libs = FFMPEG_LIBS.get()
            .and_then(|opt| opt.as_ref())
            .ok_or_else(|| "FFmpeg libs not loaded".to_string())?;

        unsafe {
            // Flush encoder
            let ret = (libs.avcodec_send_frame)(self.cc, std::ptr::null());
            if ret >= 0 {
                loop {
                    (libs.av_packet_unref)(self.pkt);
                    let ret = (libs.avcodec_receive_packet)(self.cc, self.pkt);
                    if ret == AVERROR_EAGAIN || ret < -1000 {
                        break;
                    }
                    if ret < 0 {
                        break;
                    }
                    (libs.av_interleaved_write_frame)(self.fmt_ctx, self.pkt);
                }
            }

            (libs.av_write_trailer)(self.fmt_ctx);
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for LibavEncoder {
    fn drop(&mut self) {
        if let Some(Some(libs)) = FFMPEG_LIBS.get().map(|opt| opt.as_ref()) {
            unsafe {
                if !self.sws.is_null() {
                    (libs.sws_freeContext)(self.sws);
                }
                if !self.pkt.is_null() {
                    (libs.av_packet_free)(&mut self.pkt.cast::<AVPacket>());
                }
                if !self.frame.is_null() {
                    (libs.av_frame_free)(&mut self.frame.cast::<AVFrame>());
                }
                if !self.io_ctx.is_null() {
                    (libs.avio_closep)(&mut self.io_ctx);
                }
                if !self.cc.is_null() {
                    (libs.avcodec_free_context)(&mut self.cc.cast::<AVCodecContext>());
                }
                if !self.fmt_ctx.is_null() {
                    (libs.avformat_free_context)(self.fmt_ctx);
                }
            }
        }
    }
}

impl std::fmt::Debug for LibavEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibavEncoder")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pts", &self.pts)
            .finish()
    }
}
