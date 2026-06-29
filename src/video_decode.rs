/// Cross-platform FFmpeg video frame decoder.
///
/// Strategy (in priority order):
/// 1. **Dynamic libav** – at first call, attempt to dlopen the FFmpeg shared
///    libraries (libavformat, libavcodec, libavutil, libswscale).
///    If they are present we decode frames in-process: zero subprocess spawning.
/// 2. **Process fallback** – if the shared libraries are not available,
///    we spawn an `ffmpeg` child process as before.
///
/// Public entry points:
///   - [`decode_frame`]      – decode a single frame, picks best backend
///   - [`is_libav_available`] – returns true if libav was successfully loaded

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

// ── FFmpeg ABI constants ──────────────────────────────────────────────────────
const AVMEDIA_TYPE_VIDEO: c_int = 0;
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
    av_frame_free:                 unsafe extern "C" fn(*mut *mut AVFrame),

    avcodec_parameters_from_context: unsafe extern "C" fn(*mut (), *const AVCodecContext) -> c_int,
    avcodec_find_encoder_by_name:  unsafe extern "C" fn(*const c_char) -> *const AVCodec,
    avcodec_send_frame:            unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int,
    avcodec_receive_packet:        unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int,

    // avutil
    av_frame_get_buffer:           unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int,

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
        av_frame_free:                 sym!(avcodec,  unsafe extern "C" fn(*mut *mut AVFrame),                                                                b"av_frame_free\0"),

        avcodec_parameters_from_context: sym!(avcodec, unsafe extern "C" fn(*mut (), *const AVCodecContext) -> c_int, b"avcodec_parameters_from_context\0"),
        avcodec_find_encoder_by_name:  sym!(avcodec,  unsafe extern "C" fn(*const c_char) -> *const AVCodec, b"avcodec_find_encoder_by_name\0"),
        avcodec_send_frame:            sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *const AVFrame) -> c_int, b"avcodec_send_frame\0"),
        avcodec_receive_packet:        sym!(avcodec,  unsafe extern "C" fn(*mut AVCodecContext, *mut AVPacket) -> c_int, b"avcodec_receive_packet\0"),

        av_frame_get_buffer:           sym!(avutil,   unsafe extern "C" fn(*mut AVFrame, c_int) -> c_int, b"av_frame_get_buffer\0"),

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
        None       => decode_process(video_path, source_frame, fps),
    }
}

/// Returns `true` if FFmpeg shared libraries loaded successfully.
pub fn is_libav_available() -> bool {
    FFMPEG_LIBS.get_or_init(|| try_load_ffmpeg()).is_some()
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

// ── Process fallback ──────────────────────────────────────────────────────────
fn decode_process(video_path: &str, source_frame: usize, fps: f32) -> Option<(u32, u32, Vec<u8>)> {
    use std::process::Command;
    let time_sec = source_frame as f32 / fps;

    // Probe dimensions
    let probe = Command::new("ffprobe")
        .args(["-v","error","-select_streams","v:0",
               "-show_entries","stream=width,height",
               "-of","csv=p=0", video_path])
        .output().ok()?;
    let s = String::from_utf8_lossy(&probe.stdout);
    let mut d = s.trim().split(',');
    let w: u32 = d.next()?.trim().parse().ok()?;
    let h: u32 = d.next()?.trim().parse().ok()?;
    if w == 0 || h == 0 { return None; }

    // Raw RGBA
    let expected = (w * h * 4) as usize;
    let out = Command::new("ffmpeg")
        .args(["-y","-noautorotate","-ss",&format!("{:.3}",time_sec),
               "-i",video_path,"-vframes","1",
               "-f","rawvideo","-pix_fmt","rgba","pipe:1"])
        .output().ok()?;
    if out.status.success() && out.stdout.len() == expected {
        return Some((w, h, out.stdout));
    }

    // PNG fallback
    let png = Command::new("ffmpeg")
        .args(["-y","-noautorotate","-ss",&format!("{:.3}",time_sec),
               "-i",video_path,"-vframes","1",
               "-f","image2pipe","-vcodec","png","-"])
        .output().ok()?;
    if png.status.success() && !png.stdout.is_empty() {
        let img = image::load_from_memory(&png.stdout).ok()?;
        let rgba = img.to_rgba8();
        let (iw, ih) = rgba.dimensions();
        return Some((iw, ih, rgba.into_raw()));
    }
    None
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

pub fn encode_video_libav(
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    fps: u32,
    bitrate_kbps: u32,
    vcodec_name: &str,
) -> Result<(), String> {
    let libs = FFMPEG_LIBS.get_or_init(|| try_load_ffmpeg()).as_ref()
        .ok_or_else(|| "FFmpeg shared libraries not loaded".to_string())?;

    // Collect and sort PNG files from frames_dir
    let mut entries = std::fs::read_dir(frames_dir)
        .map_err(|e| format!("Failed to read frames dir: {}", e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "png"))
        .collect::<Vec<_>>();

    entries.sort_by_key(|e| e.file_name());
    if entries.is_empty() {
        return Err("No frame PNGs found in directory".to_string());
    }

    // Load first image to get dimensions
    let first_img = image::open(&entries[0].path())
        .map_err(|e| format!("Failed to open first frame: {}", e))?;
    let (width, height) = (first_img.width(), first_img.height());

    unsafe {
        let output_c = CString::new(output.to_str().ok_or("Invalid output path")?).unwrap();
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

        // Find encoder
        let encoder_c = CString::new(vcodec_name).unwrap();
        let mut codec = (libs.avcodec_find_encoder_by_name)(encoder_c.as_ptr());
        if codec.is_null() {
            // Fallbacks
            let fallback = if vcodec_name.contains("vp9") {
                "libvpx"
            } else if vcodec_name.contains("prores") {
                "prores"
            } else {
                "h264"
            };
            let fallback_c = CString::new(fallback).unwrap();
            codec = (libs.avcodec_find_encoder_by_name)(fallback_c.as_ptr());
        }
        if codec.is_null() {
            (libs.avformat_free_context)(fmt_ctx);
            return Err(format!("Codec '{}' not found", vcodec_name));
        }

        // Add stream
        let stream = (libs.avformat_new_stream)(fmt_ctx, codec);
        if stream.is_null() {
            (libs.avformat_free_context)(fmt_ctx);
            return Err("Could not create stream".to_string());
        }

        // Allocate codec context
        let cc = (libs.avcodec_alloc_context3)(codec);
        if cc.is_null() {
            (libs.avformat_free_context)(fmt_ctx);
            return Err("Could not allocate codec context".to_string());
        }

        // Configure codec context fields using offsets
        let cc_u8 = cc as *mut u8;
        cc_u8.add(92).cast::<c_int>().write(width as c_int); // width
        cc_u8.add(96).cast::<c_int>().write(height as c_int); // height
        cc_u8.add(80).cast::<c_int>().write(1); // time_base.num
        cc_u8.add(84).cast::<c_int>().write(fps as c_int); // time_base.den
        cc_u8.add(112).cast::<c_int>().write(0); // pix_fmt = AV_PIX_FMT_YUV420P
        cc_u8.add(40).cast::<i64>().write((bitrate_kbps * 1000) as i64); // bit_rate
        cc_u8.add(108).cast::<c_int>().write(12); // gop_size

        // Open codec
        let ret = (libs.avcodec_open2)(cc, codec, std::ptr::null_mut());
        if ret < 0 {
            (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
            (libs.avformat_free_context)(fmt_ctx);
            return Err(format!("Could not open codec (code {})", ret));
        }

        // Copy codec context parameters to stream's codecpar
        let avformat_major = (libs.avformat_version)() >> 16;
        let stream_offset = if avformat_major >= 59 { 16 } else { 8 };
        let codecpar = (stream as *mut u8).add(stream_offset).cast::<*mut ()>().read();
        
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
            return Err(format!("Could not open output file '{}' (code {})", output.display(), ret));
        }
        (fmt_ctx as *mut u8).add(16).cast::<*mut AVIOContext>().write(io_ctx);

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

        let mut pts = 0i64;
        for entry in &entries {
            if let Ok(img) = image::open(&entry.path()) {
                let rgba = img.to_rgba8();
                let src_ptr = rgba.as_ptr();
                let src_data: [*const u8; 8] = [
                    src_ptr, std::ptr::null(), std::ptr::null(), std::ptr::null(),
                    std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null()
                ];
                let src_ls: [c_int; 8] = [
                    (width * 4) as c_int, 0, 0, 0, 0, 0, 0, 0
                ];

                let dst_data: [*mut u8; 8] = [
                    frame_data(frame, 0),
                    frame_data(frame, 1),
                    frame_data(frame, 2),
                    frame_data(frame, 3),
                    std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()
                ];
                let dst_ls: [c_int; 8] = [
                    frame_linesize(frame, 0),
                    frame_linesize(frame, 1),
                    frame_linesize(frame, 2),
                    frame_linesize(frame, 3),
                    0, 0, 0, 0
                ];

                (libs.sws_scale)(
                    sws,
                    src_data.as_ptr(),
                    src_ls.as_ptr(),
                    0,
                    height as c_int,
                    dst_data.as_ptr() as *const *mut u8,
                    dst_ls.as_ptr(),
                );

                (frame as *mut u8).add(136).cast::<i64>().write(pts);
                pts += 1;

                let ret = (libs.avcodec_send_frame)(cc, frame);
                if ret >= 0 {
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
            }
        }

        // Flush encoder
        let ret = (libs.avcodec_send_frame)(cc, std::ptr::null());
        if ret >= 0 {
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

        (libs.av_write_trailer)(fmt_ctx);

        (libs.sws_freeContext)(sws);
        (libs.av_packet_free)(&mut pkt.cast::<AVPacket>());
        (libs.av_frame_free)(&mut frame.cast::<AVFrame>());
        (libs.avio_closep)(&mut io_ctx);
        (libs.avcodec_free_context)(&mut cc.cast::<AVCodecContext>());
        (libs.avformat_free_context)(fmt_ctx);
    }

    Ok(())
}
