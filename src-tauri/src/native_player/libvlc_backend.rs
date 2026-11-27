use libloading::Library;
use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::{MediaTrack, NativePlayerState};
use std::ffi::{c_char, c_float, c_int, c_uint, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW;

// libVLC 3.x C ABI structures & constants
const LIBVLC_MEDIA_SLAVE_TYPE_SUBTITLE: c_uint = 0;
const LIBVLC_STATE_OPENING: c_int = 1;
const LIBVLC_STATE_BUFFERING: c_int = 2;
const LIBVLC_STATE_PLAYING: c_int = 3;
const LIBVLC_STATE_PAUSED: c_int = 4;
const LIBVLC_STATE_STOPPED: c_int = 5;
const LIBVLC_STATE_ENDED: c_int = 6;
const LIBVLC_STATE_ERROR: c_int = 7;

#[repr(C)]
struct LibVlcTrackDescription {
    i_id: c_int,
    psz_name: *mut c_char,
    p_next: *mut LibVlcTrackDescription,
}

struct LibVlcSymbols {
    _lib: Library,
    libvlc_get_version: unsafe extern "C" fn() -> *const c_char,
    libvlc_new: unsafe extern "C" fn(c_int, *const *const c_char) -> *mut c_void,
    libvlc_release: unsafe extern "C" fn(*mut c_void),
    libvlc_media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    libvlc_media_new_location: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    libvlc_media_release: unsafe extern "C" fn(*mut c_void),
    libvlc_media_player_new: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    libvlc_media_player_set_media: unsafe extern "C" fn(*mut c_void, *mut c_void),
    libvlc_media_player_release: unsafe extern "C" fn(*mut c_void),
    libvlc_media_player_set_hwnd: unsafe extern "C" fn(*mut c_void, *mut c_void),
    libvlc_media_player_play: unsafe extern "C" fn(*mut c_void) -> c_int,
    libvlc_media_player_set_pause: unsafe extern "C" fn(*mut c_void, c_int),
    libvlc_media_player_stop: unsafe extern "C" fn(*mut c_void),
    libvlc_media_player_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    libvlc_media_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    libvlc_media_player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    libvlc_media_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    libvlc_audio_set_volume: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    libvlc_audio_set_mute: unsafe extern "C" fn(*mut c_void, c_int),
    libvlc_media_player_set_rate: unsafe extern "C" fn(*mut c_void, c_float) -> c_int,
    libvlc_audio_get_track: unsafe extern "C" fn(*mut c_void) -> c_int,
    libvlc_audio_set_track: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    libvlc_audio_get_track_description:
        unsafe extern "C" fn(*mut c_void) -> *mut LibVlcTrackDescription,
    libvlc_video_get_spu: unsafe extern "C" fn(*mut c_void) -> c_int,
    libvlc_video_set_spu: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    libvlc_video_get_spu_description:
        unsafe extern "C" fn(*mut c_void) -> *mut LibVlcTrackDescription,
    libvlc_video_set_spu_delay: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    libvlc_media_player_add_slave:
        unsafe extern "C" fn(*mut c_void, c_uint, *const c_char, bool) -> c_int,
    libvlc_video_set_aspect_ratio: unsafe extern "C" fn(*mut c_void, *const c_char),
    libvlc_video_set_scale: unsafe extern "C" fn(*mut c_void, c_float),
    libvlc_track_description_list_release: unsafe extern "C" fn(*mut LibVlcTrackDescription),
}

impl LibVlcSymbols {
    fn load(dll_path: &Path) -> Result<Self, String> {
        // Ensure libvlc.dll can locate sibling libvlccore.dll and plugins/
        if let Some(dir) = dll_path.parent() {
            let plugins = dir.join("plugins");
            if plugins.is_dir() {
                std::env::set_var("VLC_PLUGIN_PATH", &plugins);
            }
            #[cfg(windows)]
            unsafe {
                let wide: Vec<u16> = OsStr::new(dir)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                SetDllDirectoryW(wide.as_ptr());
            }
        }

        unsafe {
            let lib = Library::new(dll_path)
                .map_err(|e| format!("Failed to load libvlc.dll at '{}': {e}", dll_path.display()))?;

            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let s: libloading::Symbol<$ty> = lib
                        .get($name)
                        .map_err(|e| format!("Missing libvlc symbol {:?}: {e}", $name))?;
                    *s
                }};
            }

            let out = Self {
                libvlc_get_version: sym!(b"libvlc_get_version\0", unsafe extern "C" fn() -> *const c_char),
                libvlc_new: sym!(b"libvlc_new\0", unsafe extern "C" fn(c_int, *const *const c_char) -> *mut c_void),
                libvlc_release: sym!(b"libvlc_release\0", unsafe extern "C" fn(*mut c_void)),
                libvlc_media_new_path: sym!(b"libvlc_media_new_path\0", unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void),
                libvlc_media_new_location: sym!(b"libvlc_media_new_location\0", unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void),
                libvlc_media_release: sym!(b"libvlc_media_release\0", unsafe extern "C" fn(*mut c_void)),
                libvlc_media_player_new: sym!(b"libvlc_media_player_new\0", unsafe extern "C" fn(*mut c_void) -> *mut c_void),
                libvlc_media_player_set_media: sym!(b"libvlc_media_player_set_media\0", unsafe extern "C" fn(*mut c_void, *mut c_void)),
                libvlc_media_player_release: sym!(b"libvlc_media_player_release\0", unsafe extern "C" fn(*mut c_void)),
                libvlc_media_player_set_hwnd: sym!(b"libvlc_media_player_set_hwnd\0", unsafe extern "C" fn(*mut c_void, *mut c_void)),
                libvlc_media_player_play: sym!(b"libvlc_media_player_play\0", unsafe extern "C" fn(*mut c_void) -> c_int),
                libvlc_media_player_set_pause: sym!(b"libvlc_media_player_set_pause\0", unsafe extern "C" fn(*mut c_void, c_int)),
                libvlc_media_player_stop: sym!(b"libvlc_media_player_stop\0", unsafe extern "C" fn(*mut c_void)),
                libvlc_media_player_get_state: sym!(b"libvlc_media_player_get_state\0", unsafe extern "C" fn(*mut c_void) -> c_int),
                libvlc_media_player_get_time: sym!(b"libvlc_media_player_get_time\0", unsafe extern "C" fn(*mut c_void) -> i64),
                libvlc_media_player_set_time: sym!(b"libvlc_media_player_set_time\0", unsafe extern "C" fn(*mut c_void, i64)),
                libvlc_media_player_get_length: sym!(b"libvlc_media_player_get_length\0", unsafe extern "C" fn(*mut c_void) -> i64),
                libvlc_audio_set_volume: sym!(b"libvlc_audio_set_volume\0", unsafe extern "C" fn(*mut c_void, c_int) -> c_int),
                libvlc_audio_set_mute: sym!(b"libvlc_audio_set_mute\0", unsafe extern "C" fn(*mut c_void, c_int)),
                libvlc_media_player_set_rate: sym!(b"libvlc_media_player_set_rate\0", unsafe extern "C" fn(*mut c_void, c_float) -> c_int),
                libvlc_audio_get_track: sym!(b"libvlc_audio_get_track\0", unsafe extern "C" fn(*mut c_void) -> c_int),
                libvlc_audio_set_track: sym!(b"libvlc_audio_set_track\0", unsafe extern "C" fn(*mut c_void, c_int) -> c_int),
                libvlc_audio_get_track_description: sym!(b"libvlc_audio_get_track_description\0", unsafe extern "C" fn(*mut c_void) -> *mut LibVlcTrackDescription),
                libvlc_video_get_spu: sym!(b"libvlc_video_get_spu\0", unsafe extern "C" fn(*mut c_void) -> c_int),
                libvlc_video_set_spu: sym!(b"libvlc_video_set_spu\0", unsafe extern "C" fn(*mut c_void, c_int) -> c_int),
                libvlc_video_get_spu_description: sym!(b"libvlc_video_get_spu_description\0", unsafe extern "C" fn(*mut c_void) -> *mut LibVlcTrackDescription),
                libvlc_video_set_spu_delay: sym!(b"libvlc_video_set_spu_delay\0", unsafe extern "C" fn(*mut c_void, i64) -> c_int),
                libvlc_media_player_add_slave: sym!(b"libvlc_media_player_add_slave\0", unsafe extern "C" fn(*mut c_void, c_uint, *const c_char, bool) -> c_int),
                libvlc_video_set_aspect_ratio: sym!(b"libvlc_video_set_aspect_ratio\0", unsafe extern "C" fn(*mut c_void, *const c_char)),
                libvlc_video_set_scale: sym!(b"libvlc_video_set_scale\0", unsafe extern "C" fn(*mut c_void, c_float)),
                libvlc_track_description_list_release: sym!(b"libvlc_track_description_list_release\0", unsafe extern "C" fn(*mut LibVlcTrackDescription)),
                _lib: lib,
            };

            #[cfg(windows)]
            SetDllDirectoryW(std::ptr::null());

            Ok(out)
        }
    }
}

pub fn probe_libvlc_version(dll_path: &Path) -> Option<String> {
    let syms = LibVlcSymbols::load(dll_path).ok()?;
    unsafe {
        let ptr = (syms.libvlc_get_version)();
        if ptr.is_null() {
            None
        } else {
            Some(format!("libVLC {}", CStr::from_ptr(ptr).to_string_lossy()))
        }
    }
}

pub enum VlcCmd {
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
    SetSubtitleTrack(i64),
    AddSubtitleFile(String),
    SetSubtitleDelayMs(i64),
    SetAspectRatio(String),
    SetVideoScale(f64),
    Shutdown,
}

pub struct EmbeddedLibVlc {
    tx: Sender<VlcCmd>,
    state: Arc<Mutex<NativePlayerState>>,
    join: Option<JoinHandle<()>>,
}

impl EmbeddedLibVlc {
    pub fn spawn(
        dll_path: PathBuf,
        wid_hwnd: isize,
        on_state: Arc<dyn Fn(NativePlayerState) + Send + Sync>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<VlcCmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(NativePlayerState {
            engine: "embedded-libvlc".to_string(),
            ..NativePlayerState::default()
        }));
        let state_thread = Arc::clone(&state);

        let join = thread::Builder::new()
            .name("seasonfetch-libvlc".to_string())
            .spawn(move || {
                let syms = match LibVlcSymbols::load(&dll_path) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };

                let args = [
                    CString::new("--no-video-title-show").unwrap(),
                    CString::new("--quiet").unwrap(),
                    CString::new("--avcodec-hw=any").unwrap(),
                    CString::new("--http-reconnect").unwrap(),
                    CString::new("--network-caching=3000").unwrap(),
                    CString::new("--file-caching=1000").unwrap(),
                    CString::new("--no-mouse-events").unwrap(),
                    CString::new("--no-keyboard-events").unwrap(),
                ];
                let ptrs: Vec<*const c_char> = args.iter().map(|c| c.as_ptr()).collect();
                let inst = unsafe { (syms.libvlc_new)(ptrs.len() as c_int, ptrs.as_ptr()) };
                if inst.is_null() {
                    let _ = ready_tx.send(Err("libvlc_new failed (check VLC plugins directory)".to_string()));
                    return;
                }
                let mp = unsafe { (syms.libvlc_media_player_new)(inst) };
                if mp.is_null() {
                    unsafe { (syms.libvlc_release)(inst) };
                    let _ = ready_tx.send(Err("libvlc_media_player_new failed".to_string()));
                    return;
                }
                if wid_hwnd != 0 {
                    unsafe { (syms.libvlc_media_player_set_hwnd)(mp, wid_hwnd as *mut c_void) };
                }

                let _ = ready_tx.send(Ok(()));

                let mut pending_seek_ms: Option<i64> = None;
                let mut seen_opening_or_playing = false;
                let mut track_poll_ticks: u32 = 0;
                let mut running = true;

                while running {
                    match rx.recv_timeout(Duration::from_millis(220)) {
                        Ok(cmd) => match cmd {
                            VlcCmd::Load {
                                target,
                                start_secs,
                                subtitle_file,
                                prefs,
                            } => {
                                let is_url = target.starts_with("http://") || target.starts_with("https://");
                                let Ok(ctarget) = CString::new(target.as_str()) else {
                                    continue;
                                };
                                let media = unsafe {
                                    if is_url {
                                        (syms.libvlc_media_new_location)(inst, ctarget.as_ptr())
                                    } else {
                                        (syms.libvlc_media_new_path)(inst, ctarget.as_ptr())
                                    }
                                };
                                if media.is_null() {
                                    if let Ok(mut st) = state_thread.lock() {
                                        st.error = Some("libVLC could not open the media source.".to_string());
                                        on_state(st.clone());
                                    }
                                    continue;
                                }
                                unsafe {
                                    (syms.libvlc_media_player_set_media)(mp, media);
                                    (syms.libvlc_media_release)(media);
                                    (syms.libvlc_audio_set_volume)(
                                        mp,
                                        (prefs.default_volume.clamp(0.0, 1.0) * 100.0).round() as c_int,
                                    );
                                    (syms.libvlc_media_player_set_rate)(
                                        mp,
                                        prefs.playback_speed.clamp(0.25, 4.0) as c_float,
                                    );
                                    (syms.libvlc_video_set_spu_delay)(
                                        mp,
                                        prefs.subtitle_delay_ms * 1000,
                                    );
                                    let _ = (syms.libvlc_media_player_play)(mp);
                                }
                                if let Some(sub) = subtitle_file.filter(|s| !s.trim().is_empty()) {
                                    let uri = path_to_file_uri(&sub);
                                    if let Ok(curi) = CString::new(uri) {
                                        unsafe {
                                            let _ = (syms.libvlc_media_player_add_slave)(
                                                mp,
                                                LIBVLC_MEDIA_SLAVE_TYPE_SUBTITLE,
                                                curi.as_ptr(),
                                                true,
                                            );
                                        }
                                    }
                                }
                                pending_seek_ms = start_secs
                                    .filter(|s| *s > 2.0)
                                    .map(|s| (s * 1000.0).round() as i64);
                                seen_opening_or_playing = false;
                                track_poll_ticks = 0;
                                if let Ok(mut st) = state_thread.lock() {
                                    st.active = true;
                                    st.playing = false;
                                    st.paused = false;
                                    st.buffering = true;
                                    st.vo_configured = false;
                                    st.position_secs = 0.0;
                                    st.duration_secs = 0.0;
                                    st.eof_reached = false;
                                    st.error = None;
                                    st.hwdec_mode = if prefs.hardware_decoding {
                                        "avcodec-hw=any".to_string()
                                    } else {
                                        "no".to_string()
                                    };
                                    st.hwdec_current = Some(if prefs.hardware_decoding {
                                        "auto".to_string()
                                    } else {
                                        "no".to_string()
                                    });
                                    st.audio_tracks.clear();
                                    st.subtitle_tracks.clear();
                                    on_state(st.clone());
                                }
                            }
                            VlcCmd::Stop => {
                                unsafe { (syms.libvlc_media_player_stop)(mp) };
                                pending_seek_ms = None;
                                seen_opening_or_playing = false;
                                if let Ok(mut st) = state_thread.lock() {
                                    st.active = false;
                                    st.playing = false;
                                    st.paused = false;
                                    st.buffering = false;
                                    st.vo_configured = false;
                                    st.position_secs = 0.0;
                                    st.duration_secs = 0.0;
                                    st.eof_reached = false;
                                    st.error = None;
                                    st.audio_tracks.clear();
                                    st.subtitle_tracks.clear();
                                    on_state(st.clone());
                                }
                            }
                            VlcCmd::SetPaused(p) => {
                                unsafe { (syms.libvlc_media_player_set_pause)(mp, if p { 1 } else { 0 }) };
                            }
                            VlcCmd::TogglePause => {
                                let cur = unsafe { (syms.libvlc_media_player_get_state)(mp) };
                                unsafe {
                                    (syms.libvlc_media_player_set_pause)(
                                        mp,
                                        if cur == LIBVLC_STATE_PLAYING { 1 } else { 0 },
                                    );
                                }
                            }
                            VlcCmd::SeekAbsolute(secs) => {
                                unsafe {
                                    (syms.libvlc_media_player_set_time)(mp, (secs.max(0.0) * 1000.0).round() as i64);
                                }
                            }
                            VlcCmd::SeekRelative(delta) => {
                                let cur = unsafe { (syms.libvlc_media_player_get_time)(mp) }.max(0);
                                let next = (cur + (delta * 1000.0).round() as i64).max(0);
                                unsafe { (syms.libvlc_media_player_set_time)(mp, next) };
                            }
                            VlcCmd::SetVolume(vol) => {
                                unsafe {
                                    (syms.libvlc_audio_set_mute)(mp, 0);
                                    (syms.libvlc_audio_set_volume)(mp, (vol.clamp(0.0, 1.0) * 100.0).round() as c_int);
                                }
                            }
                            VlcCmd::SetMuted(m) => {
                                unsafe { (syms.libvlc_audio_set_mute)(mp, if m { 1 } else { 0 }) };
                            }
                            VlcCmd::SetSpeed(spd) => {
                                unsafe { (syms.libvlc_media_player_set_rate)(mp, spd.clamp(0.25, 4.0) as c_float) };
                            }
                            VlcCmd::SetAudioTrack(id) => {
                                unsafe { (syms.libvlc_audio_set_track)(mp, id as c_int) };
                            }
                            VlcCmd::SetSubtitleTrack(id) => {
                                let target = if id <= 0 { -1 } else { id as c_int };
                                unsafe { (syms.libvlc_video_set_spu)(mp, target) };
                            }
                            VlcCmd::AddSubtitleFile(path) => {
                                let uri = path_to_file_uri(&path);
                                if let Ok(curi) = CString::new(uri) {
                                    unsafe {
                                        let _ = (syms.libvlc_media_player_add_slave)(
                                            mp,
                                            LIBVLC_MEDIA_SLAVE_TYPE_SUBTITLE,
                                            curi.as_ptr(),
                                            true,
                                        );
                                    }
                                }
                            }
                            VlcCmd::SetSubtitleDelayMs(ms) => {
                                unsafe { (syms.libvlc_video_set_spu_delay)(mp, ms * 1000) };
                            }
                            VlcCmd::SetAspectRatio(ratio) => {
                                unsafe {
                                    if matches!(ratio.as_str(), "16:9" | "21:9" | "4:3") {
                                        if let Ok(cr) = CString::new(ratio.as_str()) {
                                            (syms.libvlc_video_set_aspect_ratio)(mp, cr.as_ptr());
                                        }
                                    } else {
                                        (syms.libvlc_video_set_aspect_ratio)(mp, std::ptr::null());
                                    }
                                }
                            }
                            VlcCmd::SetVideoScale(scale) => {
                                unsafe {
                                    (syms.libvlc_video_set_scale)(
                                        mp,
                                        if (scale - 1.0).abs() < 0.01 { 0.0 } else { scale as c_float },
                                    );
                                }
                            }
                            VlcCmd::Shutdown => {
                                running = false;
                            }
                        },
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            running = false;
                        }
                    }

                    if !running {
                        break;
                    }

                    // Poll live libVLC state while active
                    let is_active = state_thread.lock().map(|s| s.active).unwrap_or(false);
                    if is_active {
                        let vlc_state = unsafe { (syms.libvlc_media_player_get_state)(mp) };
                        let time_ms = unsafe { (syms.libvlc_media_player_get_time)(mp) };
                        let len_ms = unsafe { (syms.libvlc_media_player_get_length)(mp) };

                        if matches!(
                            vlc_state,
                            LIBVLC_STATE_OPENING | LIBVLC_STATE_BUFFERING | LIBVLC_STATE_PLAYING | LIBVLC_STATE_PAUSED
                        ) {
                            seen_opening_or_playing = true;
                        }

                        if vlc_state == LIBVLC_STATE_PLAYING {
                            if let Some(seek_ms) = pending_seek_ms.take() {
                                if len_ms <= 0 || seek_ms < len_ms - 8000 {
                                    unsafe { (syms.libvlc_media_player_set_time)(mp, seek_ms) };
                                }
                            }
                        }

                        let mut polled_tracks: Option<(Vec<MediaTrack>, Vec<MediaTrack>, Option<i64>, Option<i64>)> = None;
                        if matches!(vlc_state, LIBVLC_STATE_PLAYING | LIBVLC_STATE_PAUSED) {
                            track_poll_ticks = track_poll_ticks.wrapping_add(1);
                            if track_poll_ticks == 1 || track_poll_ticks % 5 == 0 {
                                let sel_audio = unsafe { (syms.libvlc_audio_get_track)(mp) } as i64;
                                let sel_sub = unsafe { (syms.libvlc_video_get_spu)(mp) } as i64;
                                let audio_tracks = read_vlc_tracks(
                                    &syms,
                                    unsafe { (syms.libvlc_audio_get_track_description)(mp) },
                                    "audio",
                                    sel_audio,
                                );
                                let subtitle_tracks = read_vlc_tracks(
                                    &syms,
                                    unsafe { (syms.libvlc_video_get_spu_description)(mp) },
                                    "subtitle",
                                    sel_sub,
                                );
                                polled_tracks = Some((
                                    audio_tracks,
                                    subtitle_tracks,
                                    if sel_audio > 0 { Some(sel_audio) } else { None },
                                    if sel_sub > 0 { Some(sel_sub) } else { None },
                                ));
                            }
                        }

                        if let Ok(mut st) = state_thread.lock() {
                            st.playing = vlc_state == LIBVLC_STATE_PLAYING;
                            st.paused = vlc_state == LIBVLC_STATE_PAUSED;
                            st.buffering = matches!(vlc_state, 0 | LIBVLC_STATE_OPENING | LIBVLC_STATE_BUFFERING);
                            st.eof_reached = vlc_state == LIBVLC_STATE_ENDED;
                            if vlc_state == LIBVLC_STATE_ERROR {
                                st.active = false;
                                st.buffering = false;
                                st.vo_configured = false;
                                st.error = Some(
                                    "libVLC failed to decode or open this stream. Switch to Embedded libmpv (recommended) for best torrent & HEVC/ASS playback."
                                        .to_string(),
                                );
                            } else if vlc_state == LIBVLC_STATE_STOPPED && seen_opening_or_playing {
                                st.active = false;
                                st.buffering = false;
                                st.vo_configured = false;
                                st.error = Some(
                                    "libVLC stopped while buffering the stream (HTTP timeout or unsupported codec). Switch to Embedded libmpv or retry."
                                        .to_string(),
                                );
                            }
                            if time_ms >= 0 {
                                st.position_secs = time_ms as f64 / 1000.0;
                            }
                            if len_ms > 0 {
                                st.duration_secs = len_ms as f64 / 1000.0;
                            }
                            st.vo_configured = st.playing || st.paused;
                            if let Some((audio_tracks, subtitle_tracks, sel_a, sel_s)) = polled_tracks {
                                st.audio_tracks = audio_tracks;
                                st.subtitle_tracks = subtitle_tracks;
                                st.selected_audio_id = sel_a;
                                st.selected_subtitle_id = sel_s;
                            }
                            on_state(st.clone());
                        }
                    }
                }

                unsafe {
                    (syms.libvlc_media_player_stop)(mp);
                    (syms.libvlc_media_player_release)(mp);
                    (syms.libvlc_release)(inst);
                }
            })
            .map_err(|e| format!("Failed to spawn libVLC thread: {e}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(25)) {
            Ok(Ok(())) => Ok(Self {
                tx,
                state,
                join: Some(join),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Timed out waiting for libVLC initialization".to_string()),
        }
    }

    pub fn send(&self, cmd: VlcCmd) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "libVLC worker thread is no longer running".to_string())
    }

    pub fn snapshot(&self) -> NativePlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for EmbeddedLibVlc {
    fn drop(&mut self) {
        let _ = self.send(VlcCmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn path_to_file_uri(path: &str) -> String {
    if path.starts_with("file://") || path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn read_vlc_tracks(
    syms: &LibVlcSymbols,
    head: *mut LibVlcTrackDescription,
    kind: &str,
    selected_id: i64,
) -> Vec<MediaTrack> {
    let mut out = Vec::new();
    if head.is_null() {
        return out;
    }
    let mut cur = head;
    while !cur.is_null() {
        unsafe {
            let node = &*cur;
            if node.i_id >= 0 {
                let name = if node.psz_name.is_null() {
                    format!("Track {}", node.i_id)
                } else {
                    CStr::from_ptr(node.psz_name).to_string_lossy().to_string()
                };
                out.push(MediaTrack {
                    id: node.i_id.to_string(),
                    numeric_id: Some(node.i_id as i64),
                    kind: kind.to_string(),
                    title: name,
                    language: None,
                    codec: None,
                    selected: node.i_id as i64 == selected_id,
                    external: false,
                    default_track: false,
                    forced: false,
                });
            }
            cur = node.p_next;
        }
    }
    unsafe { (syms.libvlc_track_description_list_release)(head) };
    out
}
