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
use std::sync::{Mutex, OnceLock};

/// FFmpeg shared libs are not safe for concurrent use across threads in this crate.
static LIBAV_LOCK: Mutex<()> = Mutex::new(());

#[inline]
fn libav_guard() -> std::sync::MutexGuard<'static, ()> {
    LIBAV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ── FFmpeg ABI constants ──────────────────────────────────────────────────────
const AVMEDIA_TYPE_VIDEO: c_int = 0;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AV_PIX_FMT_RGBA: c_int = 26;
const AV_PIX_FMT_NONE: c_int = -1;
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
    avcodec_version:               unsafe extern "C" fn() -> std::os::raw::c_uint,
    avcodec_alloc_context3:        unsafe extern "C" fn(*const AVCodec) -> *mut AVCodecContext,
    avcodec_parameters_to_context: unsafe extern "C" fn(*mut AVCodecContext, *const ()) -> c_int,
    avcodec_open2:                 unsafe extern "C" fn(*mut AVCodecContext, *const AVCodec, *mut *mut ()) -> c_int,
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
    avcodec_parameters_copy:         unsafe extern "C" fn(*mut (), *const ()) -> c_int,
    avcodec_find_decoder:           unsafe extern "C" fn(c_int) -> *const AVCodec,
    avcodec_find_encoder_by_name:  unsafe extern "C" fn(*const c_char) -> *const AVCodec,
    avcodec_send_frame:            unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int,
    avcodec_receive_packet:        unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int,
    av_packet_rescale_ts:          unsafe extern "C" fn(*mut AVPacket, AVRational, AVRational),

    // avutil
    av_frame_get_buffer:           unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int,
    av_opt_set:                    unsafe extern "C" fn(*mut (), *const c_char, *const c_char, c_int) -> c_int,
    av_opt_set_int:                unsafe extern "C" fn(*mut (), *const c_char, i64, c_int) -> c_int,
    av_opt_set_q:                  unsafe extern "C" fn(*mut (), *const c_char, AVRational, c_int) -> c_int,
    av_opt_get_int:                unsafe extern "C" fn(*mut (), *const c_char, c_int, *mut i64) -> c_int,
    av_get_pix_fmt:                unsafe extern "C" fn(*const c_char) -> c_int,
    av_get_sample_fmt:             unsafe extern "C" fn(*const c_char) -> c_int,
    av_channel_layout_from_string: unsafe extern "C" fn(*mut (), *const c_char) -> c_int,
    av_channel_layout_copy:        unsafe extern "C" fn(*mut (), *const ()) -> c_int,
    av_dict_set:                   unsafe extern "C" fn(*mut *mut (), *const c_char, *const c_char, c_int) -> c_int,
    av_dict_free:                  unsafe extern "C" fn(*mut *mut ()),

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

    let avformat = open_lib!(["libavformat.so.61","libavformat.so.60","libavformat.so","libavformat.61.dylib","libavformat.60.dylib","avformat-61.dll","avformat-60.dll"]);
    let avcodec  = open_lib!(["libavcodec.so.61", "libavcodec.so.60", "libavcodec.so", "libavcodec.61.dylib", "libavcodec.60.dylib", "avcodec-61.dll", "avcodec-60.dll" ]);
    let avutil   = open_lib!(["libavutil.so.59",  "libavutil.so.58",  "libavutil.so",  "libavutil.59.dylib",  "libavutil.58.dylib",  "avutil-59.dll",  "avutil-58.dll"  ]);
    let swscale  = open_lib!(["libswscale.so.8",  "libswscale.so.7",  "libswscale.so", "libswscale.8.dylib",  "libswscale.7.dylib",  "swscale-8.dll",  "swscale-7.dll"  ]);

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

        avcodec_version:               sym!(avcodec,  unsafe extern "C" fn() -> std::os::raw::c_uint,                                                          b"avcodec_version\0"),
        avcodec_alloc_context3:        sym!(avcodec,  unsafe extern "C" fn(*const AVCodec)->*mut AVCodecContext,                                             b"avcodec_alloc_context3\0"),
        avcodec_parameters_to_context: sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*const ())->c_int,                                            b"avcodec_parameters_to_context\0"),
        avcodec_open2:                 sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext,*const AVCodec,*mut *mut ())->c_int,                          b"avcodec_open2\0"),
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
        avcodec_parameters_copy:         sym!(avcodec, unsafe extern "C" fn(*mut (), *const ()) -> c_int, b"avcodec_parameters_copy\0"),
        avcodec_find_decoder:           sym!(avcodec,  unsafe extern "C" fn(c_int) -> *const AVCodec, b"avcodec_find_decoder\0"),
        avcodec_find_encoder_by_name:  sym!(avcodec,  unsafe extern "C" fn(*const c_char) -> *const AVCodec, b"avcodec_find_encoder_by_name\0"),
        avcodec_send_frame:            sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int, b"avcodec_send_frame\0"),
        avcodec_receive_packet:        sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int, b"avcodec_receive_packet\0"),
        av_packet_rescale_ts:          sym!(avcodec,  unsafe extern "C" fn(*mut AVPacket, AVRational, AVRational), b"av_packet_rescale_ts\0"),

        av_frame_get_buffer:           sym!(avutil,   unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int, b"av_frame_get_buffer\0"),
        av_opt_set:                    sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char, *const c_char, c_int) -> c_int, b"av_opt_set\0"),
        av_opt_set_int:                sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char, i64, c_int) -> c_int, b"av_opt_set_int\0"),
        av_opt_set_q:                  sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char, AVRational, c_int) -> c_int, b"av_opt_set_q\0"),
        av_opt_get_int:                sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char, c_int, *mut i64) -> c_int, b"av_opt_get_int\0"),
        av_get_pix_fmt:                sym!(avutil,   unsafe extern "C" fn(*const c_char) -> c_int, b"av_get_pix_fmt\0"),
        av_get_sample_fmt:             sym!(avutil,   unsafe extern "C" fn(*const c_char) -> c_int, b"av_get_sample_fmt\0"),
        av_channel_layout_from_string: sym!(avutil,   unsafe extern "C" fn(*mut (), *const c_char) -> c_int, b"av_channel_layout_from_string\0"),
        av_channel_layout_copy:        sym!(avutil,   unsafe extern "C" fn(*mut (), *const ()) -> c_int, b"av_channel_layout_copy\0"),
        av_dict_set:                   sym!(avutil,   unsafe extern "C" fn(*mut *mut (), *const c_char, *const c_char, c_int) -> c_int, b"av_dict_set\0"),
        av_dict_free:                  sym!(avutil,   unsafe extern "C" fn(*mut *mut ()), b"av_dict_free\0"),

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
    unsafe { stream.add(24).cast::<i32>().read() }
}
unsafe fn stream_time_base_den(stream: *mut u8) -> i32 {
    unsafe { stream.add(28).cast::<i32>().read() }
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
unsafe fn frame_nb_samples(f: *mut AVFrame) -> i32 {
    unsafe { (f as *const u8).add(112).cast::<i32>().read() }
}
unsafe fn codecpar_codec_id(cp: *mut u8) -> i32 { unsafe { cp.add(4).cast::<i32>().read() } }

unsafe fn stream_set_time_base(stream: *mut u8, num: i32, den: i32, avformat_major: u32) {
    unsafe {
        let (no, doff) = if avformat_major >= 59 {
            (32, 36)
        } else {
            (24, 28)
        };
        stream.add(no).cast::<i32>().write(num);
        stream.add(doff).cast::<i32>().write(den);
    }
}

unsafe fn pkt_set_stream_index(pkt: *mut AVPacket, idx: i32) {
    unsafe { (pkt as *mut u8).add(36).cast::<i32>().write(idx); }
}
unsafe fn pkt_set_pts(pkt: *mut AVPacket, pts: i64) {
    unsafe { (pkt as *mut u8).add(8).cast::<i64>().write(pts); }
}
unsafe fn pkt_set_dts(pkt: *mut AVPacket, dts: i64) {
    unsafe { (pkt as *mut u8).add(16).cast::<i64>().write(dts); }
}
unsafe fn pkt_set_duration(pkt: *mut AVPacket, dur: i64) {
    unsafe { (pkt as *mut u8).add(24).cast::<i64>().write(dur); }
}

unsafe fn fmt_nb_streams(fmt: *mut AVFormatContext) -> u32 {
    unsafe { (fmt as *const u8).add(44).cast::<u32>().read() }
}

unsafe fn fmt_set_pb(fmt: *mut AVFormatContext, io: *mut AVIOContext, _avformat_major: u32) {
    unsafe {
        // AVFormatContext.pb @ 32 on LP64 (FFmpeg n6.1 / n7.1).
        (fmt as *mut u8).add(32).cast::<*mut AVIOContext>().write(io);
    }
}

const AV_OPT_SEARCH_CHILDREN: c_int = 2;

struct CodecCtxVideoLayout {
    width: usize,
    height: usize,
    pix_fmt: usize,
    gop_size: usize,
    bit_rate: usize,
    tb_num: usize,
    tb_den: usize,
}

struct CodecCtxAudioLayout {
    sample_rate: usize,
    channels: Option<usize>,
    sample_fmt: usize,
}

/// `AVChannelLayout` public ABI size (FFmpeg 5+).
const AV_SAMPLE_FMT_FLTP: i32 = 8;
const AV_CHANNEL_LAYOUT_SIZE: usize = 24;
const AV_CHANNEL_ORDER_NATIVE: i32 = 1;
const AV_CH_LAYOUT_STEREO: u64 = 3;
fn codec_ctx_ch_layout_offset(avcodec_major: u32) -> Option<usize> {
    // LP64 offsetof(AVCodecContext.ch_layout); major 60 validated on libavcodec 60.x.
    match avcodec_major {
        60 => Some(912),
        61..=62 => Some(368),
        _ => None,
    }
}

fn codec_ctx_audio_layout(avcodec_major: u32) -> CodecCtxAudioLayout {
    if avcodec_major >= 61 {
        CodecCtxAudioLayout {
            sample_rate: 344,
            channels: None,
            sample_fmt: 348,
        }
    } else {
        CodecCtxAudioLayout {
            sample_rate: 352,
            channels: Some(356),
            sample_fmt: 360,
        }
    }
}

unsafe fn codec_ctx_channels(cc: *mut (), avcodec_major: u32) -> usize {
    unsafe {
        if avcodec_major >= 61 {
            let ch = (cc as *const u8).add(368 + 4).cast::<i32>().read();
            if ch > 0 { ch as usize } else { 2 }
        } else if avcodec_major == 60 {
            let ch = (cc as *const u8).add(912 + 4).cast::<i32>().read();
            if ch > 0 { ch as usize } else { 2 }
        } else {
            let ch = (cc as *const u8).add(356).cast::<i32>().read();
            if ch > 0 { ch as usize } else { 2 }
        }
    }
}

fn frame_ch_layout_offset(_avutil_major: u32) -> usize {
    // offsetof(AVFrame, ch_layout) on LP64 FFmpeg n6.1 / n7.1.
    448
}

unsafe fn stereo_ch_layout_write_raw(dst: *mut u8) {
    unsafe {
        dst.cast::<i32>().write(AV_CHANNEL_ORDER_NATIVE);
        dst.add(4).cast::<i32>().write(2);
        dst.add(8).cast::<u64>().write(AV_CH_LAYOUT_STEREO);
        dst.add(16).cast::<*mut u8>().write(std::ptr::null_mut());
    }
}

unsafe fn stereo_ch_layout_apply(libs: &FfmpegLibs, dst: *mut u8) {
    unsafe {
        let stereo = CString::new("stereo").unwrap();
        let ret = (libs.av_channel_layout_from_string)(dst as *mut (), stereo.as_ptr());
        if ret < 0 {
            stereo_ch_layout_write_raw(dst);
        }
    }
}

unsafe fn stereo_layout_buf(libs: &FfmpegLibs) -> [u8; AV_CHANNEL_LAYOUT_SIZE] {
    let mut layout = [0u8; AV_CHANNEL_LAYOUT_SIZE];
    unsafe {
        stereo_ch_layout_apply(libs, layout.as_mut_ptr());
    }
    layout
}

unsafe fn codec_ctx_apply_stereo_ch_layout(libs: &FfmpegLibs, cc: *mut AVCodecContext, avcodec_major: u32) {
    unsafe {
        let Some(off) = codec_ctx_ch_layout_offset(avcodec_major) else {
            return;
        };
        let stereo = stereo_layout_buf(libs);
        let _ = (libs.av_channel_layout_copy)(
            (cc as *mut u8).add(off) as *mut (),
            stereo.as_ptr() as *const (),
        );
    }
}

unsafe fn frame_prepare_stereo_audio(
    libs: &FfmpegLibs,
    frame: *mut AVFrame,
    nb_samples: i32,
    sample_rate: i32,
    sample_fmt: i32,
) {
    unsafe {
        frame_set_nb_samples(frame, nb_samples);
        frame_set_format(frame, sample_fmt);
        frame_set_sample_rate(frame, sample_rate);
        let ch_off = frame_ch_layout_offset(0);
        stereo_ch_layout_apply(libs, (frame as *mut u8).add(ch_off));
        let frame_void = frame as *mut ();
        let _ = (libs.av_opt_set)(
            frame_void,
            CString::new("ch_layout").unwrap().as_ptr(),
            CString::new("stereo").unwrap().as_ptr(),
            AV_OPT_SEARCH_CHILDREN,
        );
    }
}

unsafe fn codec_ctx_apply_audio_encoder(
    libs: &FfmpegLibs,
    cc: *mut AVCodecContext,
    avcodec_major: u32,
    sample_rate: u32,
    sample_fmt: i32,
) {
    unsafe {
        let audio = codec_ctx_audio_layout(avcodec_major);
        let video = codec_ctx_video_layout(avcodec_major);
        let p = cc as *mut u8;
        let sr = sample_rate.max(1) as i32;
        let fmt = if sample_fmt >= 0 {
            sample_fmt
        } else {
            let name = CString::new("fltp").unwrap();
            (libs.av_get_sample_fmt)(name.as_ptr())
        };
        p.add(audio.sample_rate).cast::<i32>().write(sr);
        p.add(audio.sample_fmt).cast::<i32>().write(fmt);
        p.add(video.tb_num).cast::<i32>().write(1);
        p.add(video.tb_den).cast::<i32>().write(sr);
    }
}

fn codec_ctx_video_layout(avcodec_major: u32) -> CodecCtxVideoLayout {
    // Offsets from offsetof(AVCodecContext, …) on LP64 (FFmpeg n6.1.1 / n7.1 headers).
    if avcodec_major >= 61 {
        CodecCtxVideoLayout {
            bit_rate: 56,
            tb_num: 84,
            tb_den: 88,
            width: 116,
            height: 120,
            gop_size: 332,
            pix_fmt: 140,
        }
    } else {
        CodecCtxVideoLayout {
            bit_rate: 56,
            tb_num: 100,
            tb_den: 104,
            width: 116,
            height: 120,
            gop_size: 132,
            pix_fmt: 136,
        }
    }
}

unsafe fn codec_ctx_read_time_base(cc: *mut AVCodecContext, avcodec_major: u32) -> AVRational {
    unsafe {
        let layout = codec_ctx_video_layout(avcodec_major);
        let p = cc as *const u8;
        AVRational {
            num: p.add(layout.tb_num).cast::<i32>().read(),
            den: p.add(layout.tb_den).cast::<i32>().read().max(1),
        }
    }
}

unsafe fn stream_read_time_base(stream: *mut u8, avformat_major: u32) -> AVRational {
    unsafe {
        let (no, doff) = if avformat_major >= 59 {
            (32, 36)
        } else {
            (24, 28)
        };
        AVRational {
            num: stream.add(no).cast::<i32>().read(),
            den: stream.add(doff).cast::<i32>().read().max(1),
        }
    }
}

unsafe fn libav_opt_set(
    libs: &FfmpegLibs,
    obj: *mut (),
    key: &str,
    val: &str,
    flags: c_int,
) -> Result<(), String> {
    unsafe {
        let k = CString::new(key).map_err(|e| e.to_string())?;
        let v = CString::new(val).map_err(|e| e.to_string())?;
        let r = (libs.av_opt_set)(obj, k.as_ptr(), v.as_ptr(), flags);
        if r < 0 {
            Err(format!("av_opt_set({key}={val}) failed ({r})"))
        } else {
            Ok(())
        }
    }
}

unsafe fn libav_opt_set_int(
    libs: &FfmpegLibs,
    obj: *mut (),
    key: &str,
    val: i64,
    flags: c_int,
) -> Result<(), String> {
    unsafe {
        let k = CString::new(key).map_err(|e| e.to_string())?;
        let r = (libs.av_opt_set_int)(obj, k.as_ptr(), val, flags);
        if r < 0 {
            Err(format!("av_opt_set_int({key}={val}) failed ({r})"))
        } else {
            Ok(())
        }
    }
}

unsafe fn libav_opt_get_int(
    libs: &FfmpegLibs,
    obj: *mut (),
    key: &str,
    flags: c_int,
) -> Result<i64, String> {
    unsafe {
        let k = CString::new(key).map_err(|e| e.to_string())?;
        let mut out: i64 = 0;
        let r = (libs.av_opt_get_int)(obj, k.as_ptr(), flags, &mut out);
        if r < 0 {
            Err(format!("av_opt_get_int({key}) failed ({r})"))
        } else {
            Ok(out)
        }
    }
}

unsafe fn codec_ctx_read_dims(cc: *mut AVCodecContext, avcodec_major: u32) -> (i32, i32) {
    unsafe {
        let layout = codec_ctx_video_layout(avcodec_major);
        let p = cc as *const u8;
        (
            p.add(layout.width).cast::<i32>().read(),
            p.add(layout.height).cast::<i32>().read(),
        )
    }
}

unsafe fn encoder_yuv420p_pix_fmt(libs: &FfmpegLibs) -> Result<c_int, String> {
    unsafe {
        let name = CString::new("yuv420p").map_err(|e| e.to_string())?;
        let fmt = (libs.av_get_pix_fmt)(name.as_ptr());
        if fmt < 0 {
            return Err("av_get_pix_fmt(yuv420p) failed".into());
        }
        Ok(fmt)
    }
}

unsafe fn codec_ctx_set_pix_fmt(cc: *mut AVCodecContext, avcodec_major: u32, pix_fmt: c_int) {
    unsafe {
        let layout = codec_ctx_video_layout(avcodec_major);
        (cc as *mut u8)
            .add(layout.pix_fmt)
            .cast::<c_int>()
            .write(pix_fmt);
    }
}

unsafe fn codec_ctx_read_pix_fmt(cc: *mut AVCodecContext, avcodec_major: u32) -> c_int {
    unsafe {
        let layout = codec_ctx_video_layout(avcodec_major);
        (cc as *const u8)
            .add(layout.pix_fmt)
            .cast::<c_int>()
            .read()
    }
}

unsafe fn codec_ctx_pix_fmt_ok(cc: *mut AVCodecContext, avcodec_major: u32, pix_fmt: c_int) -> bool {
    unsafe { codec_ctx_read_pix_fmt(cc, avcodec_major) == pix_fmt }
}

/// Set direct AVCodecContext fields that are not reliably exposed as AVOptions on all builds.
unsafe fn codec_ctx_apply_video(
    cc: *mut AVCodecContext,
    avcodec_major: u32,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
    pix_fmt: c_int,
) {
    unsafe {
        let layout = codec_ctx_video_layout(avcodec_major);
        let p = cc as *mut u8;
        let fps_i = fps.max(1) as i32;
        p.add(layout.bit_rate).cast::<i64>().write((bitrate_kbps as i64) * 1000);
        p.add(layout.width).cast::<i32>().write(width as i32);
        p.add(layout.height).cast::<i32>().write(height as i32);
        p.add(layout.gop_size).cast::<i32>().write(fps_i);
        p.add(layout.pix_fmt).cast::<c_int>().write(pix_fmt);
    }
}

unsafe fn codec_ctx_dims_ok(cc: *mut AVCodecContext, avcodec_major: u32, width: u32, height: u32) -> bool {
    let (rw, rh) = unsafe { codec_ctx_read_dims(cc, avcodec_major) };
    rw == width as i32 && rh == height as i32 && rh > 0
}

unsafe fn configure_video_encoder(
    libs: &FfmpegLibs,
    cc: *mut AVCodecContext,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
) -> Result<c_int, String> {
    unsafe {
        if width == 0 || height == 0 || fps == 0 {
            return Err(format!(
                "Invalid encoder dimensions {width}x{height} @ {fps} fps"
            ));
        }
        let avcodec_major = (libs.avcodec_version)() >> 16;
        let fps_i = fps.max(1) as c_int;
        let pix_fmt = encoder_yuv420p_pix_fmt(libs)?;
        codec_ctx_apply_video(cc, avcodec_major, width, height, fps, bitrate_kbps, pix_fmt);
        if !codec_ctx_dims_ok(cc, avcodec_major, width, height) {
            return Err(format!(
                "Could not set encoder dimensions to {width}x{height} (libavcodec major {avcodec_major})"
            ));
        }
        if !codec_ctx_pix_fmt_ok(cc, avcodec_major, pix_fmt) {
            return Err(format!(
                "Could not set encoder pix_fmt to {pix_fmt} (libavcodec major {avcodec_major})"
            ));
        }

        let obj = cc as *mut ();
        let time_base = AVRational { num: 1, den: fps_i };
        let framerate = AVRational { num: fps_i, den: 1 };
        let tb_key = CString::new("time_base").unwrap();
        let fr_key = CString::new("framerate").unwrap();
        let ret_tb = (libs.av_opt_set_q)(obj, tb_key.as_ptr(), time_base, 0);
        if ret_tb < 0 {
            return Err(format!("av_opt_set_q(time_base) failed ({ret_tb})"));
        }
        let _ = (libs.av_opt_set_q)(obj, fr_key.as_ptr(), framerate, 0);

        Ok(pix_fmt)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

unsafe fn fmt_duration_secs(fmt: *mut AVFormatContext, avformat_major: u32) -> f64 {
    // AVFormatContext.duration is in AV_TIME_BASE (microseconds). Offset 40 is `pb` on FFmpeg 6.x x86_64.
    unsafe {
        let off = if avformat_major >= 59 { 72 } else { 56 };
        let dur_us = (fmt as *const u8).add(off).cast::<i64>().read();
        if dur_us <= 0 {
            return 0.0;
        }
        dur_us as f64 / 1_000_000.0
    }
}

/// Duration in seconds (libav first, then symphonia).
pub fn probe_media_duration_secs(path: &str) -> Option<f32> {
    if let Some(secs) = probe_media_duration_libav(path) {
        return Some(secs);
    }
    crate::audio_extract::probe_media_duration_symphonia(path)
}

fn probe_media_duration_libav(path: &str) -> Option<f32> {
    let libs = FFMPEG_LIBS.get_or_init(|| try_load_ffmpeg()).as_ref()?;
    let path_c = CString::new(path).ok()?;
    unsafe {
        let mut fmt: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_open_input)(&mut fmt, path_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut())
            < 0
        {
            return None;
        }
        if (libs.avformat_find_stream_info)(fmt, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut fmt);
            return None;
        }
        let avformat_major = (libs.avformat_version)() >> 16;

        let fmt_secs = fmt_duration_secs(fmt, avformat_major);
        if fmt_secs > 0.05 {
            (libs.avformat_close_input)(&mut fmt);
            return Some(fmt_secs as f32);
        }

        let mut best: f64 = 0.0;
        let nb = fmt_nb_streams(fmt);
        for i in 0..nb {
            let stream = fmt_stream(fmt, i);
            let dur_ts = stream_duration(stream, avformat_major);
            let tb_n = stream_tb_num(stream, avformat_major);
            let tb_d = stream_tb_den(stream, avformat_major).max(1);
            if dur_ts > 0 && tb_n > 0 {
                let secs = dur_ts as f64 * tb_n as f64 / tb_d as f64;
                if secs.is_finite() {
                    best = best.max(secs);
                }
            }
        }
        (libs.avformat_close_input)(&mut fmt);
        if best > 0.05 {
            return Some(best as f32);
        }
    }
    None
}

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
        let avcodec_major = (libs.avcodec_version)() >> 16;
        let audio_layout = codec_ctx_audio_layout(avcodec_major);
        let sr = (cc as *const u8).add(audio_layout.sample_rate).cast::<i32>().read();
        if sr > 0 {
            sample_rate = sr as u32;
        }
        let channels = codec_ctx_channels(cc as *mut (), avcodec_major);

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
                append_libav_audio_frame(&mut mono, frame.cast::<AVFrame>(), channels);
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
        let avcodec_major = (libs.avcodec_version)() >> 16;
        let audio_layout = codec_ctx_audio_layout(avcodec_major);
        let sr = (cc as *const u8).add(audio_layout.sample_rate).cast::<i32>().read();
        if sr > 0 {
            sample_rate = sr as u32;
        }
        let channels = codec_ctx_channels(cc as *mut (), avcodec_major);

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
                append_libav_audio_frame_stereo_i16(&mut interleaved, frame.cast::<AVFrame>(), channels);
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

const MP3_FRAME_SAMPLES: usize = 1152;
const AAC_FRAME_SAMPLES: usize = 1024;

/// Encode interleaved stereo i16 PCM to MP3 via libmp3lame (libav, no subprocess).
pub fn write_stereo_i16_as_mp3_libav(
    output: &std::path::Path,
    samples: &[i16],
    sample_rate: u32,
    bitrate_kbps: u32,
    mut on_progress: impl FnMut(f32),
) -> Result<(), String> {
    let _guard = libav_guard();
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
            let sr_i = sample_rate.max(1) as c_int;
            let time_base = AVRational { num: 1, den: sr_i };
            let tb_key = CString::new("time_base").unwrap();
            let _ = (libs.av_opt_set_q)(cc_void, tb_key.as_ptr(), time_base, 0);
            let _ = libav_opt_set(libs, cc_void, "time_base", &format!("1/{sr_i}"), 0);

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
                AV_OPT_SEARCH_CHILDREN,
            );
            let avcodec_major = (libs.avcodec_version)() >> 16;
            codec_ctx_apply_audio_encoder(libs, cc, avcodec_major, sample_rate, AV_SAMPLE_FMT_FLTP);
            codec_ctx_apply_stereo_ch_layout(libs, cc, avcodec_major);

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
        let stream_u8 = stream as *mut u8;
        let codecpar = stream_codecpar(stream_u8, avformat_major);
        if (libs.avcodec_parameters_from_context)(codecpar as *mut (), cc) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avcodec_parameters_from_context failed".into());
        }
        stream_set_time_base(stream_u8, 1, sample_rate.max(1) as i32, avformat_major);

        let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
        if (libs.avio_open)(&mut io_ctx, out_c.as_ptr(), 2) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avio_open failed".into());
        }
        fmt_set_pb(fmt_ctx, io_ctx, avformat_major);

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
            frame_prepare_stereo_audio(
                libs,
                frame,
                chunk as i32,
                sample_rate as i32,
                AV_SAMPLE_FMT_FLTP,
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

/// Encode mixed stereo PCM into an AAC-in-MP4 sidecar for remux.
pub fn write_stereo_i16_as_aac_mp4_libav(
    output: &std::path::Path,
    samples: &[i16],
    sample_rate: u32,
    bitrate_kbps: u32,
    mut on_progress: impl FnMut(f32),
) -> Result<(), String> {
    let _guard = libav_guard();
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
            return Err(format!("Could not allocate AAC muxer (code {})", ret));
        }

        let candidates = ["aac"];
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
            let sr_i = sample_rate.max(1) as c_int;
            let avcodec_major = (libs.avcodec_version)() >> 16;
            let time_base = AVRational { num: 1, den: sr_i };
            let tb_key = CString::new("time_base").unwrap();
            let ret_tb = (libs.av_opt_set_q)(cc_void, tb_key.as_ptr(), time_base, 0);
            let _ = libav_opt_set(libs, cc_void, "time_base", &format!("1/{sr_i}"), 0);
            let layout = codec_ctx_video_layout(avcodec_major);
            let p = cc as *mut u8;
            p.add(layout.tb_num).cast::<i32>().write(1);
            p.add(layout.tb_den).cast::<i32>().write(sr_i);
            if ret_tb < 0 {
                log::warn!("AAC av_opt_set_q(time_base) failed ({ret_tb}), using struct fallback");
            }

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
                AV_OPT_SEARCH_CHILDREN,
            );
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("channel_layout").unwrap().as_ptr(),
                CString::new("stereo").unwrap().as_ptr(),
                AV_OPT_SEARCH_CHILDREN,
            );
            let _ = (libs.av_opt_set)(cc_void, CString::new("b").unwrap().as_ptr(), br.as_ptr(), 0);
            let _ = (libs.av_opt_set)(
                cc_void,
                CString::new("sample_fmt").unwrap().as_ptr(),
                CString::new("fltp").unwrap().as_ptr(),
                AV_OPT_SEARCH_CHILDREN,
            );
            let _ = (libs.av_opt_set_int)(
                cc_void,
                CString::new("sample_fmt").unwrap().as_ptr(),
                AV_SAMPLE_FMT_FLTP as i64,
                AV_OPT_SEARCH_CHILDREN,
            );
            codec_ctx_apply_audio_encoder(libs, cc, avcodec_major, sample_rate, AV_SAMPLE_FMT_FLTP);
            codec_ctx_apply_stereo_ch_layout(libs, cc, avcodec_major);

            let ret = (libs.avcodec_open2)(cc, codec, std::ptr::null_mut());
            if ret >= 0 {
                opened_codec = codec;
                opened_cc = cc;
                break;
            }
            log::warn!("AAC encoder open failed ({ret})");
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        }

        if opened_codec.is_null() {
            (libs.avformat_free_context)(fmt_ctx);
            return Err("AAC encoder open failed (timebase or codec not available)".into());
        }

        let cc = opened_cc;
        let avcodec_major = (libs.avcodec_version)() >> 16;
        let stream = (libs.avformat_new_stream)(fmt_ctx, opened_codec);
        if stream.is_null() {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("Could not create AAC stream".into());
        }

        let avformat_major = (libs.avformat_version)() >> 16;
        let stream_u8 = stream as *mut u8;
        let codecpar = stream_codecpar(stream_u8, avformat_major);
        if (libs.avcodec_parameters_from_context)(codecpar as *mut (), cc) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avcodec_parameters_from_context failed".into());
        }
        stream_set_time_base(stream_u8, 1, sample_rate.max(1) as i32, avformat_major);

        let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
        if (libs.avio_open)(&mut io_ctx, out_c.as_ptr(), 2) < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err("avio_open failed".into());
        }
        fmt_set_pb(fmt_ctx, io_ctx, avformat_major);

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
        let mut enc_tb = codec_ctx_read_time_base(cc, avcodec_major);
        if enc_tb.num <= 0 || enc_tb.den <= 0 {
            enc_tb = AVRational {
                num: 1,
                den: sample_rate.max(1) as c_int,
            };
        }
        let mux_tb = stream_read_time_base(stream_u8, avformat_major);
        let mut packets_written: u64 = 0;

        while src_frame < total_frames {
            let chunk = (total_frames - src_frame).min(AAC_FRAME_SAMPLES);
            frame_prepare_stereo_audio(
                libs,
                frame,
                chunk as i32,
                sample_rate as i32,
                AV_SAMPLE_FMT_FLTP,
            );
            if (libs.av_frame_get_buffer)(frame, 0) < 0 {
                return Err("AAC av_frame_get_buffer failed".into());
            }

            let l_plane = frame_data(frame, 0) as *mut f32;
            let r_plane = frame_data(frame, 1) as *mut f32;
            if l_plane.is_null() || r_plane.is_null() {
                return Err("AAC frame planes are null".into());
            }
            for i in 0..chunk {
                let base = (src_frame + i) * 2;
                let l = samples[base] as f32 / i16::MAX as f32;
                let r = samples.get(base + 1).copied().unwrap_or(samples[base]) as f32 / i16::MAX as f32;
                *l_plane.add(i) = l.clamp(-1.0, 1.0);
                *r_plane.add(i) = r.clamp(-1.0, 1.0);
            }
            frame_set_pts(frame, pts);
            pts += chunk as i64;

            let send_ret = (libs.avcodec_send_frame)(cc, frame);
            if send_ret < 0 {
                return Err(format!("AAC avcodec_send_frame failed ({send_ret})"));
            }
            loop {
                (libs.av_packet_unref)(pkt);
                let ret = (libs.avcodec_receive_packet)(cc, pkt);
                if ret == AVERROR_EAGAIN || ret < -1000 {
                    break;
                }
                if ret < 0 {
                    break;
                }
                pkt_set_stream_index(pkt, 0);
                (libs.av_packet_rescale_ts)(pkt, enc_tb, mux_tb);
                let wr = (libs.av_interleaved_write_frame)(fmt_ctx, pkt);
                if wr < 0 {
                    return Err(format!("AAC av_interleaved_write_frame failed ({wr})"));
                }
                packets_written += 1;
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
            pkt_set_stream_index(pkt, 0);
            (libs.av_packet_rescale_ts)(pkt, enc_tb, mux_tb);
            let wr = (libs.av_interleaved_write_frame)(fmt_ctx, pkt);
            if wr < 0 {
                return Err(format!("AAC flush write failed ({wr})"));
            }
            packets_written += 1;
        }

        if packets_written == 0 {
            return Err("AAC encoder produced no packets".into());
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

/// Stream-copy video + audio into one container (no re-encode).
pub fn remux_video_and_audio_libav(
    video_path: &std::path::Path,
    audio_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), String> {
    let _guard = libav_guard();
    let libs = FFMPEG_LIBS
        .get_or_init(|| try_load_ffmpeg())
        .as_ref()
        .ok_or_else(|| "FFmpeg libraries not loaded".to_string())?;

    let video_c = CString::new(video_path.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    let audio_c = CString::new(audio_path.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    let out_c = CString::new(output_path.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;

    unsafe {
        let mut in_video: *mut AVFormatContext = std::ptr::null_mut();
        let mut in_audio: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_open_input)(&mut in_video, video_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut())
            < 0
        {
            return Err("Could not open temp video for remux".into());
        }
        if (libs.avformat_find_stream_info)(in_video, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not read temp video stream info".into());
        }
        if (libs.avformat_open_input)(&mut in_audio, audio_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut())
            < 0
        {
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not open temp audio for remux".into());
        }
        if (libs.avformat_find_stream_info)(in_audio, std::ptr::null_mut()) < 0 {
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not read temp audio stream info".into());
        }

        let mut v_codec: *const AVCodec = std::ptr::null();
        let v_si = (libs.av_find_best_stream)(
            in_video,
            AVMEDIA_TYPE_VIDEO,
            -1,
            -1,
            &mut v_codec,
            0,
        );
        let mut a_codec: *const AVCodec = std::ptr::null();
        let a_si = (libs.av_find_best_stream)(
            in_audio,
            AVMEDIA_TYPE_AUDIO,
            -1,
            -1,
            &mut a_codec,
            0,
        );
        if v_si < 0 || a_si < 0 {
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Remux inputs missing video or audio stream".into());
        }

        let mut out_fmt: *mut AVFormatContext = std::ptr::null_mut();
        if (libs.avformat_alloc_output_context2)(
            &mut out_fmt,
            std::ptr::null_mut(),
            std::ptr::null(),
            out_c.as_ptr(),
        ) < 0
            || out_fmt.is_null()
        {
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not allocate remux output".into());
        }

        let avformat_major = (libs.avformat_version)() >> 16;
        let in_v_stream = fmt_stream(in_video, v_si as u32);
        let in_a_stream = fmt_stream(in_audio, a_si as u32);
        let in_v_cp = stream_codecpar(in_v_stream, avformat_major);
        let in_a_cp = stream_codecpar(in_a_stream, avformat_major);

        let out_v_stream = (libs.avformat_new_stream)(out_fmt, std::ptr::null());
        let out_a_stream = (libs.avformat_new_stream)(out_fmt, std::ptr::null());
        if out_v_stream.is_null() || out_a_stream.is_null() {
            (libs.avformat_free_context)(out_fmt);
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not create remux output streams".into());
        }

        let out_v_u8 = out_v_stream as *mut u8;
        let out_a_u8 = out_a_stream as *mut u8;
        let out_v_cp = stream_codecpar(out_v_u8, avformat_major);
        let out_a_cp = stream_codecpar(out_a_u8, avformat_major);
        if (libs.avcodec_parameters_copy)(out_v_cp as *mut (), in_v_cp as *const ()) < 0
            || (libs.avcodec_parameters_copy)(out_a_cp as *mut (), in_a_cp as *const ()) < 0
        {
            (libs.avformat_free_context)(out_fmt);
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("avcodec_parameters_copy failed".into());
        }

        let in_v_tb = stream_read_time_base(in_v_stream, avformat_major);
        let in_a_tb = stream_read_time_base(in_a_stream, avformat_major);
        stream_set_time_base(out_v_u8, in_v_tb.num, in_v_tb.den, avformat_major);
        stream_set_time_base(out_a_u8, in_a_tb.num, in_a_tb.den, avformat_major);
        let out_v_tb = in_v_tb;
        let out_a_tb = in_a_tb;

        let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
        if (libs.avio_open)(&mut io_ctx, out_c.as_ptr(), 2) < 0 {
            (libs.avformat_free_context)(out_fmt);
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not open remux output file".into());
        }
        fmt_set_pb(out_fmt, io_ctx, avformat_major);
        if (libs.avformat_write_header)(out_fmt, std::ptr::null_mut()) < 0 {
            (libs.avio_closep)(&mut io_ctx);
            (libs.avformat_free_context)(out_fmt);
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not write remux header".into());
        }

        let pkt = (libs.av_packet_alloc)();
        if pkt.is_null() {
            (libs.avio_closep)(&mut io_ctx);
            (libs.avformat_free_context)(out_fmt);
            (libs.avformat_close_input)(&mut in_audio);
            (libs.avformat_close_input)(&mut in_video);
            return Err("Could not allocate remux packet".into());
        }

        while (libs.av_read_frame)(in_video, pkt) >= 0 {
            if pkt_stream_index(pkt) == v_si {
                pkt_set_stream_index(pkt, 0);
                (libs.av_packet_rescale_ts)(pkt, in_v_tb, out_v_tb);
                let ret = (libs.av_interleaved_write_frame)(out_fmt, pkt);
                if ret < 0 {
                    (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
                    (libs.avio_closep)(&mut io_ctx);
                    (libs.avformat_free_context)(out_fmt);
                    (libs.avformat_close_input)(&mut in_audio);
                    (libs.avformat_close_input)(&mut in_video);
                    return Err(format!("remux video packet failed ({ret})"));
                }
            }
            (libs.av_packet_unref)(pkt);
        }
        while (libs.av_read_frame)(in_audio, pkt) >= 0 {
            if pkt_stream_index(pkt) == a_si {
                pkt_set_stream_index(pkt, 1);
                (libs.av_packet_rescale_ts)(pkt, in_a_tb, out_a_tb);
                let ret = (libs.av_interleaved_write_frame)(out_fmt, pkt);
                if ret < 0 {
                    (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
                    (libs.avio_closep)(&mut io_ctx);
                    (libs.avformat_free_context)(out_fmt);
                    (libs.avformat_close_input)(&mut in_audio);
                    (libs.avformat_close_input)(&mut in_video);
                    return Err(format!("remux audio packet failed ({ret})"));
                }
            }
            (libs.av_packet_unref)(pkt);
        }

        (libs.av_write_trailer)(out_fmt);
        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.avio_closep)(&mut io_ctx);
        (libs.avformat_free_context)(out_fmt);
        (libs.avformat_close_input)(&mut in_audio);
        (libs.avformat_close_input)(&mut in_video);
    }

    Ok(())
}

unsafe fn frame_set_nb_samples(f: *mut AVFrame, n: i32) {
    unsafe { (f as *mut u8).add(112).cast::<i32>().write(n); }
}

unsafe fn frame_set_sample_rate(f: *mut AVFrame, rate: i32) {
    unsafe { (f as *mut u8).add(208).cast::<i32>().write(rate); }
}
unsafe fn frame_set_format(f: *mut AVFrame, fmt: i32) {
    unsafe { (f as *mut u8).add(116).cast::<i32>().write(fmt); }
}
unsafe fn frame_set_pts(f: *mut AVFrame, pts: i64) {
    // AVFrame.pts @ 136 on LP64 (FFmpeg n6.x / n7.x). Do not write @ 32 (data[4] pointer).
    unsafe {
        (f as *mut u8).add(136).cast::<i64>().write(pts);
    }
}

unsafe fn stream_duration(stream: *mut u8, avformat_major: u32) -> i64 {
    unsafe {
        let off = if avformat_major >= 59 { 48 } else { 32 };
        (stream.add(off).cast::<i64>().read()).max(0)
    }
}

unsafe fn append_libav_audio_frame_stereo_i16(out: &mut Vec<i16>, frame: *mut AVFrame, channels: usize) {
    unsafe {
        let n = frame_nb_samples(frame).max(0) as usize;
        if n == 0 {
            return;
        }
        let fmt = frame_format(frame);

        if fmt == 1 { // S16 (packed)
            let ptr = frame_data(frame, 0) as *const i16;
            for i in 0..n {
                let l = *ptr.add(i * channels);
                let r = if channels > 1 { *ptr.add(i * channels + 1) } else { l };
                out.push(l);
                out.push(r);
            }
        } else if fmt == 6 { // S16P (planar)
            let l_ptr = frame_data(frame, 0) as *const i16;
            let r_ptr = if channels > 1 { frame_data(frame, 1) as *const i16 } else { l_ptr };
            for i in 0..n {
                out.push(*l_ptr.add(i));
                out.push(*r_ptr.add(i));
            }
        } else if fmt == 3 { // FLT (packed float)
            let ptr = frame_data(frame, 0) as *const f32;
            for i in 0..n {
                let l = *ptr.add(i * channels);
                let r = if channels > 1 { *ptr.add(i * channels + 1) } else { l };
                out.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                out.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            }
        } else if fmt == 8 { // FLTP (planar float)
            let l_ptr = frame_data(frame, 0) as *const f32;
            let r_ptr = if channels > 1 { frame_data(frame, 1) as *const f32 } else { l_ptr };
            for i in 0..n {
                let l = *l_ptr.add(i);
                let r = *r_ptr.add(i);
                out.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                out.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            }
        } else if fmt == 2 { // S32 (packed)
            let ptr = frame_data(frame, 0) as *const i32;
            for i in 0..n {
                let l = *ptr.add(i * channels);
                let r = if channels > 1 { *ptr.add(i * channels + 1) } else { l };
                out.push((l as f64 / i32::MAX as f64 * i16::MAX as f64) as i16);
                out.push((r as f64 / i32::MAX as f64 * i16::MAX as f64) as i16);
            }
        } else if fmt == 7 { // S32P (planar)
            let l_ptr = frame_data(frame, 0) as *const i32;
            let r_ptr = if channels > 1 { frame_data(frame, 1) as *const i32 } else { l_ptr };
            for i in 0..n {
                let l = *l_ptr.add(i);
                let r = *r_ptr.add(i);
                out.push((l as f64 / i32::MAX as f64 * i16::MAX as f64) as i16);
                out.push((r as f64 / i32::MAX as f64 * i16::MAX as f64) as i16);
            }
        } else {
            // Fallback: assume planar float/s16
            let l_ptr = frame_data(frame, 0) as *const i16;
            let r_ptr = if channels > 1 { frame_data(frame, 1) as *const i16 } else { l_ptr };
            for i in 0..n {
                out.push(*l_ptr.add(i));
                out.push(*r_ptr.add(i));
            }
        }
    }
}
unsafe fn append_libav_audio_frame(out: &mut Vec<f32>, frame: *mut AVFrame, channels: usize) {
    unsafe {
        let n = frame_nb_samples(frame).max(0) as usize;
        if n == 0 {
            return;
        }
        let fmt = frame_format(frame);

        if fmt == 1 { // S16 (packed)
            let ptr = frame_data(frame, 0) as *const i16;
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    sum += *ptr.add(i * channels + p) as f32 / i16::MAX as f32;
                }
                out.push(sum / channels as f32);
            }
        } else if fmt == 6 { // S16P (planar)
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    let ptr = frame_data(frame, p) as *const i16;
                    sum += *ptr.add(i) as f32 / i16::MAX as f32;
                }
                out.push(sum / channels as f32);
            }
        } else if fmt == 3 { // FLT (packed float)
            let ptr = frame_data(frame, 0) as *const f32;
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    sum += *ptr.add(i * channels + p);
                }
                out.push(sum / channels as f32);
            }
        } else if fmt == 8 { // FLTP (planar float)
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    let ptr = frame_data(frame, p) as *const f32;
                    sum += *ptr.add(i);
                }
                out.push(sum / channels as f32);
            }
        } else if fmt == 2 { // S32 (packed)
            let ptr = frame_data(frame, 0) as *const i32;
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    sum += *ptr.add(i * channels + p) as f32 / i32::MAX as f32;
                }
                out.push(sum / channels as f32);
            }
        } else if fmt == 7 { // S32P (planar)
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    let ptr = frame_data(frame, p) as *const i32;
                    sum += *ptr.add(i) as f32 / i32::MAX as f32;
                }
                out.push(sum / channels as f32);
            }
        } else {
            // Fallback: assume planar float/s16
            for i in 0..n {
                let mut sum = 0.0f32;
                for p in 0..channels {
                    let ptr = frame_data(frame, p) as *const i16;
                    sum += *ptr.add(i) as f32 / i16::MAX as f32;
                }
                out.push(sum / channels as f32);
            }
        }
    }
}
// ── libav backend ─────────────────────────────────────────────────────────────
fn decode_libav(libs: &FfmpegLibs, path: &str, source_frame: usize, fps: f32) -> Option<(u32, u32, Vec<u8>)> {
    let _guard = libav_guard();
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
        let _guard = libav_guard();
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
        let _guard = libav_guard();
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
        let _guard = libav_guard();
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
    fps: u32,
    stream_index: i32,
    avformat_major: u32,
    avcodec_major: u32,
    enc_pix_fmt: c_int,
    enc_time_base: AVRational,
    stream_time_base: AVRational,
    released: bool,
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
        let _guard = libav_guard();
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
            let mut enc_pix_fmt: c_int = AV_PIX_FMT_NONE;

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

                let configured_pix_fmt = match configure_video_encoder(libs, cc, width, height, fps, bitrate_kbps) {
                    Ok(fmt) => fmt,
                    Err(e) => {
                        log::warn!(
                            "Encoder configure failed for candidate {}: {}",
                            candidate,
                            e
                        );
                        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                        continue;
                    }
                };
                let threads_str = CString::new(encoder_threads.to_string()).unwrap();
                let preset_name = if encoder_threads <= 1 {
                    "veryfast"
                } else {
                    "medium"
                };
                let preset_c = CString::new(preset_name).unwrap();
                let threads_key = CString::new("threads").unwrap();
                let preset_key = CString::new("preset").unwrap();

                let mut opts: *mut () = std::ptr::null_mut();
                let _ = (libs.av_dict_set)(&mut opts, preset_key.as_ptr(), preset_c.as_ptr(), 0);
                let _ = (libs.av_dict_set)(&mut opts, threads_key.as_ptr(), threads_str.as_ptr(), 0);

                let ret = (libs.avcodec_open2)(cc, codec, &mut opts);
                (libs.av_dict_free)(&mut opts);
                if ret >= 0 {
                    let avcodec_major = (libs.avcodec_version)() >> 16;
                    let opened_pix = codec_ctx_read_pix_fmt(cc, avcodec_major);
                    if opened_pix < 0 {
                        log::warn!(
                            "Encoder {} opened with invalid pix_fmt ({opened_pix}); trying next candidate",
                            candidate
                        );
                        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                        continue;
                    }
                    opened_codec = codec;
                    opened_cc = cc;
                    enc_pix_fmt = opened_pix;
                    log::info!(
                        "Opened video encoder {} (pix_fmt={opened_pix}, libavcodec major {avcodec_major})",
                        candidate
                    );
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

            let avformat_major = (libs.avformat_version)() >> 16;
            let stream_u8 = stream as *mut u8;
            let codecpar = stream_codecpar(stream_u8, avformat_major);
            if codecpar.is_null() {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Stream codecpar is null".to_string());
            }

            let ret = (libs.avcodec_parameters_from_context)(codecpar as *mut (), cc);
            if ret < 0 {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not copy codec parameters".to_string());
            }

            let stream_index = (fmt_nb_streams(fmt_ctx).saturating_sub(1)) as i32;
            let avcodec_major = (libs.avcodec_version)() >> 16;
            let stream_time_base = AVRational {
                num: 1,
                den: fps.max(1) as c_int,
            };
            let stream_u8 = stream as *mut u8;
            stream_set_time_base(
                stream_u8,
                stream_time_base.num,
                stream_time_base.den,
                avformat_major,
            );

            // Open output file
            let mut io_ctx: *mut AVIOContext = std::ptr::null_mut();
            let ret = (libs.avio_open)(&mut io_ctx, output_c.as_ptr(), 2 /*AVIO_FLAG_WRITE*/);
            if ret < 0 || io_ctx.is_null() {
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err(format!("Could not open output file '{}' (code {})", output_path, ret));
            }
            fmt_set_pb(fmt_ctx, io_ctx, avformat_major);

            // Write header
            let ret = (libs.avformat_write_header)(fmt_ctx, std::ptr::null_mut());
            if ret < 0 {
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err(format!("Could not write format header (code {})", ret));
            }

            let mut enc_time_base = codec_ctx_read_time_base(cc, avcodec_major);
            if enc_time_base.num <= 0 || enc_time_base.den <= 0 {
                enc_time_base = stream_time_base;
            }
            let mut mux_stream_time_base = stream_read_time_base(stream_u8, avformat_major);
            if mux_stream_time_base.num <= 0 || mux_stream_time_base.den <= 0 {
                mux_stream_time_base = stream_time_base;
            }

            // Prepare frame for conversion (YUV420P)
            let frame = (libs.av_frame_alloc)();
            if frame.is_null() {
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not allocate frame".to_string());
            }
            (frame as *mut u8).add(116).cast::<c_int>().write(enc_pix_fmt);
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

            let pkt = (libs.av_packet_alloc)();
            if pkt.is_null() {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("Could not allocate packet".to_string());
            }

            // sws context for RGBA -> YUV420P
            let sws = (libs.sws_getContext)(
                width as c_int, height as c_int, AV_PIX_FMT_RGBA,
                width as c_int, height as c_int, enc_pix_fmt,
                SWS_BILINEAR, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(),
            );
            if sws.is_null() {
                (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
                (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
                (libs.avio_closep)(&mut io_ctx);
                (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
                (libs.avformat_free_context)(fmt_ctx);
                return Err("sws_getContext failed".into());
            }

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
                fps,
                stream_index,
                avformat_major,
                avcodec_major,
                enc_pix_fmt,
                enc_time_base,
                stream_time_base: mux_stream_time_base,
                released: false,
            })
        }
    }

    unsafe fn mux_encoded_packet(&mut self, libs: &FfmpegLibs) -> Result<(), String> {
        unsafe {
            (libs.av_packet_rescale_ts)(self.pkt, self.enc_time_base, self.stream_time_base);
            pkt_set_stream_index(self.pkt, self.stream_index);
            let ret = (libs.av_interleaved_write_frame)(self.fmt_ctx, self.pkt);
            if ret < 0 {
                return Err(format!("av_interleaved_write_frame failed with code {}", ret));
            }
        }
        Ok(())
    }

    pub fn write_frame(&mut self, rgba_data: &[u8]) -> Result<(), String> {
        let _guard = libav_guard();
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

            let pts = self.pts;
            frame_set_pts(self.frame, pts);
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
                self.mux_encoded_packet(libs)?;
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        let _guard = libav_guard();
        let libs = FFMPEG_LIBS.get()
            .and_then(|opt| opt.as_ref())
            .ok_or_else(|| "FFmpeg libs not loaded".to_string())?;

        unsafe {
            if !self.released && !self.cc.is_null() {
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
                        self.mux_encoded_packet(libs)?;
                    }
                }
            }
            if !self.released && !self.fmt_ctx.is_null() {
                (libs.av_write_trailer)(self.fmt_ctx);
            }
        }
        self.release_resources();
        Ok(())
    }

    fn release_resources(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let Some(Some(libs)) = FFMPEG_LIBS.get().map(|opt| opt.as_ref()) else {
            return;
        };
        unsafe {
            if !self.sws.is_null() {
                (libs.sws_freeContext)(self.sws);
                self.sws = std::ptr::null_mut();
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
                self.fmt_ctx = std::ptr::null_mut();
            }
        }
    }
}

impl Drop for LibavEncoder {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let _guard = libav_guard();
        self.release_resources();
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

#[cfg(test)]
mod libav_encoder_tests {
    use super::*;

    fn require_libav() -> bool {
        if !is_libav_available() {
            panic!(
                "FFmpeg shared libraries failed to load (check libavcodec/libavformat/libavutil)"
            );
        }
        true
    }

    #[test]
    fn libx264_encoder_opens_and_finishes_without_frames() {
        if !require_libav() {
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "vadadee_libx264_open_{}.mp4",
            std::process::id()
        ));
        let path_str = path.to_str().expect("temp path utf-8");
        let enc = LibavEncoder::new(path_str, 64, 64, 12, 800, "libx264", 1)
            .expect("LibavEncoder::new (libx264)");
        enc.finish().expect("finish without frames");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn libx264_encoder_smoke_writes_mp4() {
        if !require_libav() {
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "vadadee_libx264_smoke_{}.mp4",
            std::process::id()
        ));
        let path_str = path.to_str().expect("temp path utf-8");
        let width = 64u32;
        let height = 64u32;
        let fps = 12u32;
        let frame_bytes = (width as usize) * (height as usize) * 4;
        let rgba = vec![128u8; frame_bytes];

        let mut enc = LibavEncoder::new(path_str, width, height, fps, 800, "libx264", 1)
            .expect("LibavEncoder::new (libx264)");

        for _ in 0..fps {
            enc.write_frame(&rgba)
                .expect("write_frame should not crash or error");
        }
        enc.finish().expect("finish");

        let len = std::fs::metadata(&path)
            .expect("output mp4 exists")
            .len();
        assert!(len > 200, "encoded mp4 too small ({len} bytes)");
        let probed = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
                path_str,
            ])
            .output()
            .expect("ffprobe");
        let dur_s: f64 = String::from_utf8_lossy(&probed.stdout)
            .trim()
            .parse()
            .expect("ffprobe duration");
        assert!(
            dur_s >= 0.9,
            "expected ~1s video from {fps} frames at {fps} fps, ffprobe duration={dur_s}s"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn configure_video_encoder_sets_yuv420p_pix_fmt() {
        if !require_libav() {
            return;
        }
        let _guard = libav_guard();
        let libs = FFMPEG_LIBS
            .get_or_init(|| try_load_ffmpeg())
            .as_ref()
            .expect("libs");
        unsafe {
            let name = CString::new("libx264").unwrap();
            let codec = (libs.avcodec_find_encoder_by_name)(name.as_ptr());
            assert!(!codec.is_null(), "libx264 encoder not found");
            let cc = (libs.avcodec_alloc_context3)(codec);
            assert!(!cc.is_null());
            let pix = configure_video_encoder(libs, cc, 320, 180, 24, 1500)
                .expect("configure_video_encoder");
            assert!(pix >= 0, "pix_fmt must be valid AVPixelFormat");
            let major = (libs.avcodec_version)() >> 16;
            assert!(
                codec_ctx_pix_fmt_ok(cc, major, pix),
                "pix_fmt should be yuv420p in AVCodecContext"
            );
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        }
    }

    #[test]
    fn aac_sidecar_encoder_opens() {
        if !require_libav() {
            return;
        }
        let path = std::env::temp_dir().join(format!("vadadee_aac_{}.m4a", std::process::id()));
        let sr = 44_100u32;
        let frames = sr as usize;
        let mut pcm = vec![0i16; frames * 2];
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.25;
            let s = (v * i16::MAX as f32) as i16;
            pcm[i * 2] = s;
            pcm[i * 2 + 1] = s;
        }
        write_stereo_i16_as_aac_mp4_libav(&path, &pcm, sr, 128, |_| {})
            .expect("AAC sidecar encode");
        let probed = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
                path.to_str().unwrap(),
            ])
            .output()
            .expect("ffprobe");
        let codec = String::from_utf8_lossy(&probed.stdout).trim().to_string();
        assert_eq!(codec, "aac", "ffprobe codec_name");
        let wav = std::env::temp_dir().join(format!("vadadee_aac_{}.wav", std::process::id()));
        let decode = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-i",
                path.to_str().unwrap(),
                "-f",
                "wav",
                wav.to_str().unwrap(),
            ])
            .status()
            .expect("ffmpeg decode");
        assert!(decode.success(), "ffmpeg should decode AAC sidecar");
        let bytes = std::fs::read(&wav).expect("read wav");
        let mut max_amp = 0i16;
        for chunk in bytes[44..].chunks_exact(2) {
            let s = i16::from_le_bytes([chunk[0], chunk[1]]);
            max_amp = max_amp.max(s.abs());
        }
        assert!(max_amp > 500, "AAC sidecar should contain audible PCM (max={max_amp})");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(wav);
    }
}
