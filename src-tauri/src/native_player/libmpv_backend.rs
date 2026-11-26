use libloading::Library;
use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::{
    font_size_to_mpv_sub_scale, scale_factor_to_mpv_zoom, MediaTrack, NativePlayerState,
};
use serde_json::Value;
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Official libmpv C API constants (mpv/client.h)
const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_LOG_MESSAGE: c_int = 2;
const MPV_EVENT_START_FILE: c_int = 6;
const MPV_EVENT_END_FILE: c_int = 7;
const MPV_EVENT_FILE_LOADED: c_int = 8;
const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

const MPV_END_FILE_REASON_EOF: c_int = 0;
const MPV_END_FILE_REASON_ERROR: c_int = 4;

#[repr(C)]
struct MpvEventLogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

struct LibMpvSymbols {
    _lib: Library,
    mpv_client_api_version: unsafe extern "C" fn() -> u64,
    mpv_create: unsafe extern "C" fn() -> *mut c_void,
    mpv_initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    mpv_terminate_destroy: unsafe extern "C" fn(*mut c_void),
    mpv_set_option_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    mpv_set_option: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int,
    mpv_set_property_string:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    mpv_set_property: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int,
    mpv_get_property_string: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
    mpv_get_property: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int,
    mpv_command: unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int,
    mpv_observe_property: unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int) -> c_int,
    mpv_request_log_messages: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    mpv_wait_event: unsafe extern "C" fn(*mut c_void, c_double) -> *const MpvEvent,
    mpv_wakeup: unsafe extern "C" fn(*mut c_void),
    mpv_free: unsafe extern "C" fn(*mut c_void),
    mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
}

impl LibMpvSymbols {
    fn load(dll_path: &Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(dll_path).map_err(|e| {
                format!(
                    "Failed to load libmpv dynamic library at '{}': {e}",
                    dll_path.display()
                )
            })?;

            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let s: libloading::Symbol<$ty> = lib
                        .get($name)
                        .map_err(|e| format!("Missing libmpv symbol {:?}: {e}", $name))?;
                    *s
                }};
            }

            let mpv_client_api_version = sym!(b"mpv_client_api_version\0", unsafe extern "C" fn() -> u64);
            let mpv_create = sym!(b"mpv_create\0", unsafe extern "C" fn() -> *mut c_void);
            let mpv_initialize = sym!(b"mpv_initialize\0", unsafe extern "C" fn(*mut c_void) -> c_int);
            let mpv_terminate_destroy = sym!(b"mpv_terminate_destroy\0", unsafe extern "C" fn(*mut c_void));
            let mpv_set_option_string = sym!(
                b"mpv_set_option_string\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int
            );
            let mpv_set_option = sym!(
                b"mpv_set_option\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int
            );
            let mpv_set_property_string = sym!(
                b"mpv_set_property_string\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int
            );
            let mpv_set_property = sym!(
                b"mpv_set_property\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int
            );
            let mpv_get_property_string = sym!(
                b"mpv_get_property_string\0",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char
            );
            let mpv_get_property = sym!(
                b"mpv_get_property\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int
            );
            let mpv_command = sym!(
                b"mpv_command\0",
                unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int
            );
            let mpv_observe_property = sym!(
                b"mpv_observe_property\0",
                unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int) -> c_int
            );
            let mpv_request_log_messages = sym!(
                b"mpv_request_log_messages\0",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int
            );
            let mpv_wait_event = sym!(
                b"mpv_wait_event\0",
                unsafe extern "C" fn(*mut c_void, c_double) -> *const MpvEvent
            );
            let mpv_wakeup = sym!(b"mpv_wakeup\0", unsafe extern "C" fn(*mut c_void));
            let mpv_free = sym!(b"mpv_free\0", unsafe extern "C" fn(*mut c_void));
            let mpv_error_string = sym!(
                b"mpv_error_string\0",
                unsafe extern "C" fn(c_int) -> *const c_char
            );

            Ok(Self {
                _lib: lib,
                mpv_client_api_version,
                mpv_create,
                mpv_initialize,
                mpv_terminate_destroy,
                mpv_set_option_string,
                mpv_set_option,
                mpv_set_property_string,
                mpv_set_property,
                mpv_get_property_string,
                mpv_get_property,
                mpv_command,
                mpv_observe_property,
                mpv_request_log_messages,
                mpv_wait_event,
                mpv_wakeup,
                mpv_free,
                mpv_error_string,
            })
        }
    }

    fn error_string(&self, code: c_int) -> String {
        unsafe {
            let ptr = (self.mpv_error_string)(code);
            if ptr.is_null() {
                format!("mpv error {code}")
            } else {
                CStr::from_ptr(ptr).to_string_lossy().to_string()
            }
        }
    }

    fn set_opt_str(&self, ctx: *mut c_void, name: &str, val: &str) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let cval = CString::new(val).map_err(|e| e.to_string())?;
        let rc = unsafe { (self.mpv_set_option_string)(ctx, cname.as_ptr(), cval.as_ptr()) };
        if rc < 0 {
            Err(format!("mpv_set_option_string({name}={val}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn set_opt_i64(&self, ctx: *mut c_void, name: &str, mut val: i64) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let rc = unsafe {
            (self.mpv_set_option)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        };
        if rc < 0 {
            Err(format!("mpv_set_option({name}={val}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn set_prop_str(&self, ctx: *mut c_void, name: &str, val: &str) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let cval = CString::new(val).map_err(|e| e.to_string())?;
        let rc = unsafe { (self.mpv_set_property_string)(ctx, cname.as_ptr(), cval.as_ptr()) };
        if rc < 0 {
            Err(format!("mpv_set_property_string({name}={val}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn set_prop_flag(&self, ctx: *mut c_void, name: &str, flag: bool) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let mut v: c_int = if flag { 1 } else { 0 };
        let rc = unsafe {
            (self.mpv_set_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut v as *mut c_int as *mut c_void,
            )
        };
        if rc < 0 {
            Err(format!("mpv_set_property({name}={flag}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn set_prop_i64(&self, ctx: *mut c_void, name: &str, mut val: i64) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let rc = unsafe {
            (self.mpv_set_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_INT64,
                &mut val as *mut i64 as *mut c_void,
            )
        };
        if rc < 0 {
            Err(format!("mpv_set_property({name}={val}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn set_prop_f64(&self, ctx: *mut c_void, name: &str, mut val: f64) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let rc = unsafe {
            (self.mpv_set_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut val as *mut f64 as *mut c_void,
            )
        };
        if rc < 0 {
            Err(format!("mpv_set_property({name}={val}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn get_prop_str(&self, ctx: *mut c_void, name: &str) -> Option<String> {
        let cname = CString::new(name).ok()?;
        unsafe {
            let raw = (self.mpv_get_property_string)(ctx, cname.as_ptr());
            if raw.is_null() {
                return None;
            }
            let out = CStr::from_ptr(raw).to_string_lossy().to_string();
            (self.mpv_free)(raw as *mut c_void);
            Some(out)
        }
    }

    fn get_prop_f64(&self, ctx: *mut c_void, name: &str) -> Option<f64> {
        let cname = CString::new(name).ok()?;
        let mut out: f64 = 0.0;
        let rc = unsafe {
            (self.mpv_get_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut out as *mut f64 as *mut c_void,
            )
        };
        if rc >= 0 { Some(out) } else { None }
    }

    fn get_prop_i64(&self, ctx: *mut c_void, name: &str) -> Option<i64> {
        let cname = CString::new(name).ok()?;
        let mut out: i64 = 0;
        let rc = unsafe {
            (self.mpv_get_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_INT64,
                &mut out as *mut i64 as *mut c_void,
            )
        };
        if rc >= 0 { Some(out) } else { None }
    }

    fn get_prop_flag(&self, ctx: *mut c_void, name: &str) -> Option<bool> {
        let cname = CString::new(name).ok()?;
        let mut out: c_int = 0;
        let rc = unsafe {
            (self.mpv_get_property)(
                ctx,
                cname.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut out as *mut c_int as *mut c_void,
            )
        };
        if rc >= 0 { Some(out != 0) } else { None }
    }

    fn command(&self, ctx: *mut c_void, args: &[&str]) -> Result<(), String> {
        let cargs: Vec<CString> = args
            .iter()
            .map(|a| CString::new(*a).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ptrs: Vec<*const c_char> = cargs.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let rc = unsafe { (self.mpv_command)(ctx, ptrs.as_ptr()) };
        if rc < 0 {
            Err(format!("mpv_command({args:?}): {}", self.error_string(rc)))
        } else {
            Ok(())
        }
    }

    fn observe(&self, ctx: *mut c_void, id: u64, name: &str, format: c_int) {
        if let Ok(cname) = CString::new(name) {
            unsafe {
                let _ = (self.mpv_observe_property)(ctx, id, cname.as_ptr(), format);
            }
        }
    }
}

pub fn probe_libmpv_version(dll_path: &Path) -> Option<String> {
    let syms = LibMpvSymbols::load(dll_path).ok()?;
    let ver = unsafe { (syms.mpv_client_api_version)() };
    let major = ver >> 16;
    let minor = ver & 0xFFFF;
    Some(format!("libmpv API {major}.{minor}"))
}

pub enum MpvCmd {
    Load {
        target: String,
        start_secs: Option<f64>,
        subtitle_file: Option<String>,
        prefs: PlayerPreferences,
    },
    Stop,
    SetPaused(bool),
    TogglePause,
    SeekAbsolute(f64),
    SeekRelative(f64),
    SetVolume(f64),
    SetMuted(bool),
    SetSpeed(f64),
    SetAudioTrack(i64),
    SetSubtitleTrack(i64), // 0 = disable subtitles
    AddSubtitleFile(String),
    SetSubtitleDelayMs(i64),
    SetSubtitlePos(u8),
    SetSubtitleFontSize(u8),
    SetPreferAssStyling(bool),
    SetAspectRatio(String),
    SetVideoScale(f64),
    SetHardwareDecoding(bool),
    Shutdown,
}

#[derive(Clone, Copy)]
struct WakeupHandle {
    ctx: usize,
    wakeup_fn: unsafe extern "C" fn(*mut c_void),
}

impl WakeupHandle {
    fn wake(&self) {
        if self.ctx != 0 {
            unsafe { (self.wakeup_fn)(self.ctx as *mut c_void) };
        }
    }
}

pub struct EmbeddedLibMpv {
    tx: Sender<MpvCmd>,
    wakeup: Arc<Mutex<Option<WakeupHandle>>>,
    state: Arc<Mutex<NativePlayerState>>,
    logs: Arc<Mutex<Vec<String>>>,
    join: Option<JoinHandle<()>>,
}

impl EmbeddedLibMpv {
    pub fn spawn(
        dll_path: PathBuf,
        wid_hwnd: isize,
        headless: bool,
        on_state: Arc<dyn Fn(NativePlayerState) + Send + Sync>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<MpvCmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(NativePlayerState {
            engine: "embedded-libmpv".to_string(),
            ..NativePlayerState::default()
        }));
        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let wakeup: Arc<Mutex<Option<WakeupHandle>>> = Arc::new(Mutex::new(None));

        let state_thread = Arc::clone(&state);
        let logs_thread = Arc::clone(&logs);
        let wakeup_thread = Arc::clone(&wakeup);

        let join = thread::Builder::new()
            .name("seasonfetch-libmpv".to_string())
            .spawn(move || {
                let syms = match LibMpvSymbols::load(&dll_path) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };

                let ctx = unsafe { (syms.mpv_create)() };
                if ctx.is_null() {
                    let _ = ready_tx.send(Err("mpv_create returned null".to_string()));
                    return;
                }

                // Configure libmpv before mpv_initialize (Stremio v5 Windows GPU pipeline)
                let _ = syms.set_opt_str(ctx, "idle", "yes");
                let _ = syms.set_opt_str(ctx, "keep-open", "yes");
                let _ = syms.set_opt_str(ctx, "osc", "no");
                let _ = syms.set_opt_str(ctx, "osd-level", "0");
                let _ = syms.set_opt_str(ctx, "input-default-bindings", "no");
                let _ = syms.set_opt_str(ctx, "input-vo-keyboard", "no");
                let _ = syms.set_opt_str(ctx, "audio-client-name", "SeasonFetch");
                let _ = syms.set_opt_str(ctx, "sub-ass", "yes");
                let _ = syms.set_opt_str(ctx, "sub-font-provider", "auto");
                let _ = syms.set_opt_str(ctx, "hwdec", "auto-safe");
                let _ = syms.set_opt_str(ctx, "cache", "yes");
                let _ = syms.set_opt_str(ctx, "network-timeout", "60");
                let _ = syms.set_opt_str(ctx, "demuxer-max-bytes", "150MiB");
                let _ = syms.set_opt_str(ctx, "demuxer-readahead-secs", "20");
                let _ = syms.set_opt_str(
                    ctx,
                    "demuxer-lavf-o",
                    "reconnect=1,reconnect_on_network_error=1,reconnect_delay_max=15",
                );

                if headless || wid_hwnd == 0 {
                    let _ = syms.set_opt_str(ctx, "vo", "null");
                    let _ = syms.set_opt_str(ctx, "ao", "null");
                } else {
                    let _ = syms.set_opt_i64(ctx, "wid", wid_hwnd as i64);
                    // Stremio v5 Windows VO chain: gpu-next (libplacebo D3D11 + HDR) -> gpu -> fallback
                    let _ = syms.set_opt_str(ctx, "vo", "gpu-next,gpu,");
                    let _ = syms.set_opt_str(ctx, "gpu-context", "d3d11");
                    let _ = syms.set_opt_str(ctx, "d3d11-output-format", "auto");
                    let _ = syms.set_opt_str(ctx, "d3d11-output-csp", "auto");
                    let _ = syms.set_opt_str(ctx, "force-window", "no");
                }

                let init_rc = unsafe { (syms.mpv_initialize)(ctx) };
                if init_rc < 0 {
                    let err = format!("mpv_initialize failed: {}", syms.error_string(init_rc));
                    unsafe { (syms.mpv_terminate_destroy)(ctx) };
                    let _ = ready_tx.send(Err(err));
                    return;
                }

                // Request info-level libmpv logs for VO/D3D11/hwdec/codec diagnostics
                if let Ok(lvl) = CString::new("v") {
                    unsafe { (syms.mpv_request_log_messages)(ctx, lvl.as_ptr()) };
                }

                if let Ok(mut w) = wakeup_thread.lock() {
                    *w = Some(WakeupHandle {
                        ctx: ctx as usize,
                        wakeup_fn: syms.mpv_wakeup,
                    });
                }

                // Observe core playback, hardware-decode, codec, and track properties
                syms.observe(ctx, 1, "pause", MPV_FORMAT_FLAG);
                syms.observe(ctx, 2, "time-pos", MPV_FORMAT_DOUBLE);
                syms.observe(ctx, 3, "duration", MPV_FORMAT_DOUBLE);
                syms.observe(ctx, 4, "paused-for-cache", MPV_FORMAT_FLAG);
                syms.observe(ctx, 5, "eof-reached", MPV_FORMAT_FLAG);
                syms.observe(ctx, 6, "volume", MPV_FORMAT_DOUBLE);
                syms.observe(ctx, 7, "mute", MPV_FORMAT_FLAG);
                syms.observe(ctx, 8, "speed", MPV_FORMAT_DOUBLE);
                syms.observe(ctx, 9, "hwdec-current", MPV_FORMAT_STRING);
                syms.observe(ctx, 10, "video-codec", MPV_FORMAT_STRING);
                syms.observe(ctx, 11, "audio-codec-name", MPV_FORMAT_STRING);
                syms.observe(ctx, 12, "video-params/w", MPV_FORMAT_INT64);
                syms.observe(ctx, 13, "video-params/h", MPV_FORMAT_INT64);
                syms.observe(ctx, 14, "vo-configured", MPV_FORMAT_FLAG);
                syms.observe(ctx, 15, "track-list/count", MPV_FORMAT_INT64);
                syms.observe(ctx, 16, "aid", MPV_FORMAT_INT64);
                syms.observe(ctx, 17, "sid", MPV_FORMAT_INT64);
                syms.observe(ctx, 18, "sub-delay", MPV_FORMAT_DOUBLE);
                syms.observe(ctx, 19, "current-vo", MPV_FORMAT_STRING);
                syms.observe(ctx, 20, "estimated-vf-fps", MPV_FORMAT_DOUBLE);

                let _ = ready_tx.send(Ok(()));

                let mut last_emit = Instant::now();
                let mut dirty_time = false;
                let mut pending_start_secs: Option<f64> = None;
                let mut pending_sub_file: Option<String> = None;
                let mut running = true;

                while running {
                    // Drain all queued commands immediately
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            MpvCmd::Load {
                                target,
                                start_secs,
                                subtitle_file,
                                prefs,
                            } => {
                                let hw = if prefs.hardware_decoding {
                                    "auto-safe"
                                } else {
                                    "no"
                                };
                                let _ = syms.set_prop_str(ctx, "hwdec", hw);
                                let _ = syms.set_prop_f64(
                                    ctx,
                                    "volume",
                                    prefs.default_volume.clamp(0.0, 1.0) * 100.0,
                                );
                                let _ = syms.set_prop_f64(
                                    ctx,
                                    "speed",
                                    prefs.playback_speed.clamp(0.25, 4.0),
                                );
                                let _ = syms.set_prop_f64(
                                    ctx,
                                    "sub-delay",
                                    prefs.subtitle_delay_ms as f64 / 1000.0,
                                );
                                let _ = syms.set_prop_i64(
                                    ctx,
                                    "sub-pos",
                                    prefs.subtitle_vertical_pos.clamp(5, 98) as i64,
                                );
                                let sub_scale = font_size_to_mpv_sub_scale(prefs.subtitle_font_size);
                                let _ = syms.set_prop_f64(ctx, "sub-scale", sub_scale);
                                let _ = syms.set_prop_str(
                                    ctx,
                                    "sub-ass-override",
                                    if prefs.prefer_ass_styling { "no" } else { "force" },
                                );
                                apply_aspect(&syms, ctx, &prefs.aspect_ratio);
                                let _ = syms.set_prop_f64(
                                    ctx,
                                    "video-zoom",
                                    scale_factor_to_mpv_zoom(prefs.video_scale),
                                );
                                if prefs.preferred_audio_lang != "any" {
                                    let _ = syms.set_prop_str(ctx, "alang", &prefs.preferred_audio_lang);
                                }
                                let _ = syms.set_prop_flag(ctx, "pause", false);

                                pending_start_secs = start_secs.filter(|s| *s > 2.0);
                                pending_sub_file =
                                    subtitle_file.filter(|s| !s.trim().is_empty());

                                if let Ok(mut st) = state_thread.lock() {
                                    st.active = true;
                                    st.buffering = true;
                                    st.eof_reached = false;
                                    st.error = None;
                                    st.hwdec_mode = hw.to_string();
                                    st.sub_delay_ms = prefs.subtitle_delay_ms;
                                    st.sub_pos = prefs.subtitle_vertical_pos;
                                    st.sub_scale = sub_scale;
                                    st.prefer_ass_styling = prefs.prefer_ass_styling;
                                    st.aspect_ratio = prefs.aspect_ratio.clone();
                                    st.video_zoom = prefs.video_scale;
                                    st.audio_tracks.clear();
                                    st.subtitle_tracks.clear();
                                    on_state(st.clone());
                                }

                                if let Err(err) = syms.command(ctx, &["loadfile", &target, "replace"]) {
                                    if let Ok(mut st) = state_thread.lock() {
                                        st.buffering = false;
                                        st.error = Some(err);
                                        on_state(st.clone());
                                    }
                                }
                            }
                            MpvCmd::Stop => {
                                let _ = syms.command(ctx, &["stop"]);
                                pending_start_secs = None;
                                pending_sub_file = None;
                                if let Ok(mut st) = state_thread.lock() {
                                    st.active = false;
                                    st.playing = false;
                                    st.paused = false;
                                    st.buffering = false;
                                    st.position_secs = 0.0;
                                    st.duration_secs = 0.0;
                                    st.eof_reached = false;
                                    st.error = None;
                                    st.audio_tracks.clear();
                                    st.subtitle_tracks.clear();
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetPaused(p) => {
                                let _ = syms.set_prop_flag(ctx, "pause", p);
                            }
                            MpvCmd::TogglePause => {
                                let _ = syms.command(ctx, &["cycle", "pause"]);
                            }
                            MpvCmd::SeekAbsolute(secs) => {
                                let val = format!("{:.3}", secs.max(0.0));
                                let _ = syms.command(ctx, &["seek", &val, "absolute"]);
                            }
                            MpvCmd::SeekRelative(delta) => {
                                let val = format!("{delta:.3}");
                                let _ = syms.command(ctx, &["seek", &val, "relative"]);
                            }
                            MpvCmd::SetVolume(vol) => {
                                let clamped = vol.clamp(0.0, 1.0);
                                let _ = syms.set_prop_f64(ctx, "volume", clamped * 100.0);
                                let _ = syms.set_prop_flag(ctx, "mute", false);
                            }
                            MpvCmd::SetMuted(muted) => {
                                let _ = syms.set_prop_flag(ctx, "mute", muted);
                            }
                            MpvCmd::SetSpeed(speed) => {
                                let _ = syms.set_prop_f64(ctx, "speed", speed.clamp(0.25, 4.0));
                            }
                            MpvCmd::SetAudioTrack(id) => {
                                if id <= 0 {
                                    let _ = syms.set_prop_str(ctx, "aid", "no");
                                } else {
                                    let _ = syms.set_prop_i64(ctx, "aid", id);
                                }
                                refresh_tracks_and_emit(&syms, ctx, &state_thread, &on_state);
                            }
                            MpvCmd::SetSubtitleTrack(id) => {
                                if id <= 0 {
                                    let _ = syms.set_prop_str(ctx, "sid", "no");
                                } else {
                                    let _ = syms.set_prop_i64(ctx, "sid", id);
                                }
                                refresh_tracks_and_emit(&syms, ctx, &state_thread, &on_state);
                            }
                            MpvCmd::AddSubtitleFile(path) => {
                                if let Err(err) = syms.command(ctx, &["sub-add", &path, "select"]) {
                                    if let Ok(mut st) = state_thread.lock() {
                                        st.error = Some(format!("Failed to load subtitle: {err}"));
                                        on_state(st.clone());
                                    }
                                } else {
                                    refresh_tracks_and_emit(&syms, ctx, &state_thread, &on_state);
                                }
                            }
                            MpvCmd::SetSubtitleDelayMs(ms) => {
                                let clamped = ms.clamp(-30_000, 30_000);
                                let _ = syms.set_prop_f64(ctx, "sub-delay", clamped as f64 / 1000.0);
                                if let Ok(mut st) = state_thread.lock() {
                                    st.sub_delay_ms = clamped;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetSubtitlePos(pos) => {
                                let clamped = pos.clamp(5, 98);
                                let _ = syms.set_prop_i64(ctx, "sub-pos", clamped as i64);
                                if let Ok(mut st) = state_thread.lock() {
                                    st.sub_pos = clamped;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetSubtitleFontSize(px) => {
                                let scale = font_size_to_mpv_sub_scale(px);
                                let _ = syms.set_prop_f64(ctx, "sub-scale", scale);
                                if let Ok(mut st) = state_thread.lock() {
                                    st.sub_scale = scale;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetPreferAssStyling(preserve) => {
                                let _ = syms.set_prop_str(
                                    ctx,
                                    "sub-ass-override",
                                    if preserve { "no" } else { "force" },
                                );
                                if let Ok(mut st) = state_thread.lock() {
                                    st.prefer_ass_styling = preserve;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetAspectRatio(ratio) => {
                                apply_aspect(&syms, ctx, &ratio);
                                if let Ok(mut st) = state_thread.lock() {
                                    st.aspect_ratio = ratio;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetVideoScale(scale) => {
                                let clamped = scale.clamp(0.5, 2.0);
                                let _ = syms.set_prop_f64(
                                    ctx,
                                    "video-zoom",
                                    scale_factor_to_mpv_zoom(clamped),
                                );
                                if let Ok(mut st) = state_thread.lock() {
                                    st.video_zoom = clamped;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::SetHardwareDecoding(enabled) => {
                                let mode = if enabled { "auto-safe" } else { "no" };
                                let _ = syms.set_prop_str(ctx, "hwdec", mode);
                                let current = syms.get_prop_str(ctx, "hwdec-current");
                                if let Ok(mut st) = state_thread.lock() {
                                    st.hwdec_mode = mode.to_string();
                                    st.hwdec_current = current;
                                    on_state(st.clone());
                                }
                            }
                            MpvCmd::Shutdown => {
                                running = false;
                                break;
                            }
                        }
                    }

                    if !running {
                        break;
                    }

                    let ev_ptr = unsafe { (syms.mpv_wait_event)(ctx, 0.08) };
                    if ev_ptr.is_null() {
                        continue;
                    }
                    let ev = unsafe { &*ev_ptr };
                    match ev.event_id {
                        MPV_EVENT_NONE => {
                            if dirty_time && last_emit.elapsed() >= Duration::from_millis(200) {
                                if let Ok(st) = state_thread.lock() {
                                    on_state(st.clone());
                                }
                                dirty_time = false;
                                last_emit = Instant::now();
                            }
                        }
                        MPV_EVENT_SHUTDOWN => {
                            running = false;
                        }
                        MPV_EVENT_LOG_MESSAGE => {
                            if !ev.data.is_null() {
                                let msg = unsafe { &*(ev.data as *const MpvEventLogMessage) };
                                let pfx = if msg.prefix.is_null() {
                                    String::new()
                                } else {
                                    unsafe { CStr::from_ptr(msg.prefix).to_string_lossy().to_string() }
                                };
                                let lvl = if msg.level.is_null() {
                                    String::new()
                                } else {
                                    unsafe { CStr::from_ptr(msg.level).to_string_lossy().to_string() }
                                };
                                let txt = if msg.text.is_null() {
                                    String::new()
                                } else {
                                    unsafe { CStr::from_ptr(msg.text).to_string_lossy().trim().to_string() }
                                };
                                if matches!(
                                    pfx.as_str(),
                                    "vo" | "vo/gpu-next" | "vo/gpu" | "vo/gpu-next/d3d11" | "vo/gpu/d3d11" | "vd" | "ad" | "cplayer" | "d3d11" | "sub" | "lavf"
                                ) || lvl == "error" || lvl == "warn"
                                {
                                    if let Ok(mut buf) = logs_thread.lock() {
                                        buf.push(format!("[{lvl}] [{pfx}] {txt}"));
                                        if buf.len() > 160 {
                                            let excess = buf.len() - 160;
                                            buf.drain(0..excess);
                                        }
                                    }
                                }
                            }
                        }
                        MPV_EVENT_START_FILE => {
                            if let Ok(mut st) = state_thread.lock() {
                                st.active = true;
                                st.buffering = true;
                                st.eof_reached = false;
                                st.error = None;
                                on_state(st.clone());
                            }
                        }
                        MPV_EVENT_FILE_LOADED => {
                            if let Some(sub) = pending_sub_file.take() {
                                let _ = syms.command(ctx, &["sub-add", &sub, "select"]);
                            }
                            if let Some(start) = pending_start_secs.take() {
                                let dur = syms.get_prop_f64(ctx, "duration").unwrap_or(0.0);
                                if dur <= 0.0 || start < dur - 8.0 {
                                    let val = format!("{start:.2}");
                                    let _ = syms.command(ctx, &["seek", &val, "absolute"]);
                                }
                            }
                            sync_full_snapshot(&syms, ctx, &state_thread);
                            if let Ok(st) = state_thread.lock() {
                                on_state(st.clone());
                            }
                            last_emit = Instant::now();
                        }
                        MPV_EVENT_END_FILE => {
                            if !ev.data.is_null() {
                                let end = unsafe { &*(ev.data as *const MpvEventEndFile) };
                                if let Ok(mut st) = state_thread.lock() {
                                    st.buffering = false;
                                    if end.reason == MPV_END_FILE_REASON_ERROR {
                                        st.playing = false;
                                        st.error = Some(format!(
                                            "libmpv playback error: {}",
                                            syms.error_string(end.error)
                                        ));
                                    } else if end.reason == MPV_END_FILE_REASON_EOF {
                                        st.eof_reached = true;
                                        st.playing = false;
                                    }
                                    on_state(st.clone());
                                }
                            }
                        }
                        MPV_EVENT_PROPERTY_CHANGE => {
                            if ev.data.is_null() {
                                continue;
                            }
                            let prop = unsafe { &*(ev.data as *const MpvEventProperty) };
                            if prop.name.is_null() {
                                continue;
                            }
                            let name = unsafe { CStr::from_ptr(prop.name).to_string_lossy() };
                            let mut emit_now = false;
                            let mut refresh_tracks = false;

                            if let Ok(mut st) = state_thread.lock() {
                                match (name.as_ref(), prop.format) {
                                    ("pause", MPV_FORMAT_FLAG) if !prop.data.is_null() => {
                                        let paused = unsafe { *(prop.data as *const c_int) } != 0;
                                        st.paused = paused;
                                        st.playing = st.active && !paused && !st.eof_reached;
                                        emit_now = true;
                                    }
                                    ("time-pos", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let pos = unsafe { *(prop.data as *const c_double) };
                                        if pos.is_finite() && pos >= 0.0 {
                                            st.position_secs = pos;
                                            dirty_time = true;
                                        }
                                    }
                                    ("duration", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let dur = unsafe { *(prop.data as *const c_double) };
                                        if dur.is_finite() && dur >= 0.0 {
                                            st.duration_secs = dur;
                                            emit_now = true;
                                        }
                                    }
                                    ("paused-for-cache", MPV_FORMAT_FLAG) if !prop.data.is_null() => {
                                        st.buffering = unsafe { *(prop.data as *const c_int) } != 0;
                                        emit_now = true;
                                    }
                                    ("eof-reached", MPV_FORMAT_FLAG) if !prop.data.is_null() => {
                                        st.eof_reached = unsafe { *(prop.data as *const c_int) } != 0;
                                        if st.eof_reached {
                                            st.playing = false;
                                        }
                                        emit_now = true;
                                    }
                                    ("volume", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let vol = unsafe { *(prop.data as *const c_double) };
                                        if vol.is_finite() {
                                            st.volume = (vol / 100.0).clamp(0.0, 1.0);
                                            emit_now = true;
                                        }
                                    }
                                    ("mute", MPV_FORMAT_FLAG) if !prop.data.is_null() => {
                                        st.muted = unsafe { *(prop.data as *const c_int) } != 0;
                                        emit_now = true;
                                    }
                                    ("speed", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let spd = unsafe { *(prop.data as *const c_double) };
                                        if spd.is_finite() {
                                            st.speed = spd;
                                            emit_now = true;
                                        }
                                    }
                                    ("hwdec-current", MPV_FORMAT_STRING) if !prop.data.is_null() => {
                                        let sptr = unsafe { *(prop.data as *const *const c_char) };
                                        if !sptr.is_null() {
                                            let val = unsafe { CStr::from_ptr(sptr).to_string_lossy().to_string() };
                                            st.hwdec_current = Some(val);
                                            emit_now = true;
                                        }
                                    }
                                    ("video-codec", MPV_FORMAT_STRING) if !prop.data.is_null() => {
                                        let sptr = unsafe { *(prop.data as *const *const c_char) };
                                        if !sptr.is_null() {
                                            st.video_codec = Some(unsafe { CStr::from_ptr(sptr).to_string_lossy().to_string() });
                                            emit_now = true;
                                        }
                                    }
                                    ("audio-codec-name", MPV_FORMAT_STRING) if !prop.data.is_null() => {
                                        let sptr = unsafe { *(prop.data as *const *const c_char) };
                                        if !sptr.is_null() {
                                            st.audio_codec = Some(unsafe { CStr::from_ptr(sptr).to_string_lossy().to_string() });
                                            emit_now = true;
                                        }
                                    }
                                    ("video-params/w", MPV_FORMAT_INT64) if !prop.data.is_null() => {
                                        let w = unsafe { *(prop.data as *const i64) };
                                        st.video_width = w.max(0) as u32;
                                        emit_now = true;
                                    }
                                    ("video-params/h", MPV_FORMAT_INT64) if !prop.data.is_null() => {
                                        let h = unsafe { *(prop.data as *const i64) };
                                        st.video_height = h.max(0) as u32;
                                        emit_now = true;
                                    }
                                    ("vo-configured", MPV_FORMAT_FLAG) if !prop.data.is_null() => {
                                        st.vo_configured = unsafe { *(prop.data as *const c_int) } != 0;
                                        emit_now = true;
                                    }
                                    ("current-vo", MPV_FORMAT_STRING) if !prop.data.is_null() => {
                                        let sptr = unsafe { *(prop.data as *const *const c_char) };
                                        if !sptr.is_null() {
                                            st.current_vo = Some(unsafe { CStr::from_ptr(sptr).to_string_lossy().to_string() });
                                            emit_now = true;
                                        }
                                    }
                                    ("estimated-vf-fps", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let fps = unsafe { *(prop.data as *const c_double) };
                                        if fps.is_finite() && fps >= 0.0 {
                                            st.estimated_fps = fps;
                                        }
                                    }
                                    ("track-list/count" | "aid" | "sid", _) => {
                                        refresh_tracks = true;
                                    }
                                    ("sub-delay", MPV_FORMAT_DOUBLE) if !prop.data.is_null() => {
                                        let d = unsafe { *(prop.data as *const c_double) };
                                        if d.is_finite() {
                                            st.sub_delay_ms = (d * 1000.0).round() as i64;
                                            emit_now = true;
                                        }
                                    }
                                    _ => {}
                                }

                                if emit_now && !refresh_tracks {
                                    on_state(st.clone());
                                    last_emit = Instant::now();
                                    dirty_time = false;
                                } else if dirty_time && last_emit.elapsed() >= Duration::from_millis(220) {
                                    on_state(st.clone());
                                    last_emit = Instant::now();
                                    dirty_time = false;
                                }
                            }

                            if refresh_tracks {
                                refresh_tracks_and_emit(&syms, ctx, &state_thread, &on_state);
                                last_emit = Instant::now();
                            }
                        }
                        _ => {}
                    }
                }

                if let Ok(mut w) = wakeup_thread.lock() {
                    *w = None;
                }
                unsafe { (syms.mpv_terminate_destroy)(ctx) };
            })
            .map_err(|e| format!("Failed to spawn libmpv thread: {e}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                tx,
                wakeup,
                state,
                logs,
                join: Some(join),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Timed out waiting for libmpv initialization".to_string()),
        }
    }

    pub fn recent_logs(&self) -> Vec<String> {
        self.logs.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn send(&self, cmd: MpvCmd) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "libmpv worker thread is no longer running".to_string())?;
        if let Ok(w) = self.wakeup.lock() {
            if let Some(handle) = *w {
                handle.wake();
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> NativePlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for EmbeddedLibMpv {
    fn drop(&mut self) {
        let _ = self.send(MpvCmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn apply_aspect(syms: &LibMpvSymbols, ctx: *mut c_void, ratio: &str) {
    match ratio {
        "fill" => {
            let _ = syms.set_prop_str(ctx, "video-aspect-override", "-1");
            let _ = syms.set_prop_f64(ctx, "panscan", 1.0);
        }
        "16:9" | "21:9" | "4:3" => {
            let _ = syms.set_prop_f64(ctx, "panscan", 0.0);
            let _ = syms.set_prop_str(ctx, "video-aspect-override", ratio);
        }
        _ => {
            let _ = syms.set_prop_f64(ctx, "panscan", 0.0);
            let _ = syms.set_prop_str(ctx, "video-aspect-override", "-1");
        }
    }
}

fn parse_track_list(raw_json: &str) -> (Vec<MediaTrack>, Vec<MediaTrack>, Option<i64>, Option<i64>) {
    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    let mut selected_audio_id = None;
    let mut selected_subtitle_id = None;

    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(raw_json) else {
        return (audio_tracks, subtitle_tracks, selected_audio_id, selected_subtitle_id);
    };

    for item in items {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "audio" && kind != "sub" {
            continue;
        }
        let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);
        let lang = item
            .get("lang")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let codec = item
            .get("codec")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let selected = item.get("selected").and_then(Value::as_bool).unwrap_or(false);
        let external = item.get("external").and_then(Value::as_bool).unwrap_or(false);
        let default_track = item.get("default").and_then(Value::as_bool).unwrap_or(false);
        let forced = item.get("forced").and_then(Value::as_bool).unwrap_or(false);
        let raw_title = item
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let display_title = match (&raw_title, &lang, &codec) {
            (Some(t), Some(l), Some(c)) => format!("{t} [{l} · {}]", c.to_uppercase()),
            (Some(t), Some(l), None) => format!("{t} [{l}]"),
            (Some(t), None, Some(c)) => format!("{t} [{}]", c.to_uppercase()),
            (Some(t), None, None) => t.clone(),
            (None, Some(l), Some(c)) => format!("{} ({})", l.to_uppercase(), c.to_uppercase()),
            (None, Some(l), None) => l.to_uppercase(),
            (None, None, Some(c)) => format!("Track {id} ({})", c.to_uppercase()),
            (None, None, None) => format!("Track {id}"),
        };

        let track = MediaTrack {
            id: id.to_string(),
            numeric_id: Some(id),
            kind: if kind == "sub" {
                "subtitle".to_string()
            } else {
                "audio".to_string()
            },
            title: display_title,
            language: lang,
            codec,
            selected,
            external,
            default_track,
            forced,
        };

        if kind == "audio" {
            if selected {
                selected_audio_id = Some(id);
            }
            audio_tracks.push(track);
        } else {
            if selected {
                selected_subtitle_id = Some(id);
            }
            subtitle_tracks.push(track);
        }
    }

    (audio_tracks, subtitle_tracks, selected_audio_id, selected_subtitle_id)
}

fn sync_full_snapshot(
    syms: &LibMpvSymbols,
    ctx: *mut c_void,
    state_lock: &Arc<Mutex<NativePlayerState>>,
) {
    let paused = syms.get_prop_flag(ctx, "pause").unwrap_or(false);
    let duration = syms.get_prop_f64(ctx, "duration").unwrap_or(0.0);
    let pos = syms.get_prop_f64(ctx, "time-pos").unwrap_or(0.0);
    let vol = syms.get_prop_f64(ctx, "volume").unwrap_or(90.0);
    let muted = syms.get_prop_flag(ctx, "mute").unwrap_or(false);
    let speed = syms.get_prop_f64(ctx, "speed").unwrap_or(1.0);
    let hwdec_current = syms.get_prop_str(ctx, "hwdec-current");
    let video_codec = syms.get_prop_str(ctx, "video-codec");
    let audio_codec = syms.get_prop_str(ctx, "audio-codec-name");
    let vw = syms.get_prop_i64(ctx, "video-params/w").unwrap_or(0).max(0) as u32;
    let vh = syms.get_prop_i64(ctx, "video-params/h").unwrap_or(0).max(0) as u32;
    let vo_configured = syms.get_prop_flag(ctx, "vo-configured").unwrap_or(false);
    let current_vo = syms.get_prop_str(ctx, "current-vo");
    let estimated_fps = syms.get_prop_f64(ctx, "estimated-vf-fps").unwrap_or(0.0);
    let track_json = syms.get_prop_str(ctx, "track-list").unwrap_or_default();
    let (audio_tracks, subtitle_tracks, selected_audio_id, selected_subtitle_id) =
        parse_track_list(&track_json);

    if let Ok(mut st) = state_lock.lock() {
        st.buffering = false;
        st.paused = paused;
        st.playing = st.active && !paused && !st.eof_reached;
        if duration.is_finite() && duration >= 0.0 {
            st.duration_secs = duration;
        }
        if pos.is_finite() && pos >= 0.0 {
            st.position_secs = pos;
        }
        st.volume = (vol / 100.0).clamp(0.0, 1.0);
        st.muted = muted;
        st.speed = speed;
        st.hwdec_current = hwdec_current;
        st.video_codec = video_codec;
        st.audio_codec = audio_codec;
        st.video_width = vw;
        st.video_height = vh;
        st.vo_configured = vo_configured;
        st.current_vo = current_vo;
        if estimated_fps.is_finite() && estimated_fps >= 0.0 {
            st.estimated_fps = estimated_fps;
        }
        st.audio_tracks = audio_tracks;
        st.subtitle_tracks = subtitle_tracks;
        st.selected_audio_id = selected_audio_id;
        st.selected_subtitle_id = selected_subtitle_id;
    }
}

fn refresh_tracks_and_emit(
    syms: &LibMpvSymbols,
    ctx: *mut c_void,
    state_lock: &Arc<Mutex<NativePlayerState>>,
    on_state: &Arc<dyn Fn(NativePlayerState) + Send + Sync>,
) {
    let track_json = syms.get_prop_str(ctx, "track-list").unwrap_or_default();
    let (audio_tracks, subtitle_tracks, selected_audio_id, selected_subtitle_id) =
        parse_track_list(&track_json);
    if let Ok(mut st) = state_lock.lock() {
        st.audio_tracks = audio_tracks;
        st.subtitle_tracks = subtitle_tracks;
        st.selected_audio_id = selected_audio_id;
        st.selected_subtitle_id = selected_subtitle_id;
        on_state(st.clone());
    }
}
