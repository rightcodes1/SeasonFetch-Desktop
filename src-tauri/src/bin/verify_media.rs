use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_desktop_lib::native_player::libmpv_backend::{EmbeddedLibMpv, MpvCmd};
use seasonfetch_desktop_lib::native_player::{detect_libmpv_dll, detect_libvlc_dll};
use seasonfetch_desktop_lib::native_player::libvlc_backend::probe_libvlc_version;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW,
    ShowWindow, TranslateMessage, MSG, PM_REMOVE, SW_SHOWNOACTIVATE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

#[cfg(windows)]
fn create_test_host_window() -> isize {
    let class_name: Vec<u16> = OsStr::new("SeasonFetchVerifyMpvHost")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = OsStr::new("SeasonFetch libmpv Real Media Verification")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            100,
            100,
            960,
            540,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        hwnd as isize
    }
}

#[cfg(windows)]
fn pump_messages_for(ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: verify_media <media_dir>");
        std::process::exit(2);
    }
    let media_dir = PathBuf::from(&args[1]);
    let dll = detect_libmpv_dll(None).expect("libmpv-2.dll not found");
    println!("libmpv DLL: {}", dll.display());

    if let Some(vlc_dll) = detect_libvlc_dll() {
        println!(
            "Optional system libVLC DLL: {} ({:?})",
            vlc_dll.display(),
            probe_libvlc_version(&vlc_dll)
        );
    }

    #[cfg(windows)]
    let host_hwnd = create_test_host_window();
    #[cfg(not(windows))]
    let host_hwnd = 0;

    #[cfg(windows)]
    let surface = seasonfetch_desktop_lib::native_player::win32_surface::Win32VideoSurface::create(
        host_hwnd,
    )
    .expect("Failed to create Win32VideoSurface");
    #[cfg(windows)]
    surface.show_and_fit();
    #[cfg(windows)]
    let wid = surface.raw_hwnd();
    #[cfg(not(windows))]
    let wid = 0;

    let latest = Arc::new(Mutex::new(
        seasonfetch_core::player::NativePlayerState::default(),
    ));
    let latest_cb = Arc::clone(&latest);
    let mpv = EmbeddedLibMpv::spawn(
        dll,
        wid,
        false,
        Arc::new(move |st| {
            if let Ok(mut guard) = latest_cb.lock() {
                *guard = st;
            }
        }),
    )
    .expect("Failed to initialize embedded libmpv");

    let cases = vec![
        (
            "MKV + HEVC + AC3 (Hardware D3D11VA + Multi-Track + ASS/SRT)",
            media_dir.join("sample_hevc_ac3_multitrack.mkv"),
            true,
            Some(media_dir.join("external_styled.ass")),
        ),
        (
            "MKV + HEVC + AC3 (Explicit Software Fallback hwdec=no)",
            media_dir.join("sample_hevc_ac3_multitrack.mkv"),
            false,
            None,
        ),
        (
            "MKV + H.264 + AAC (Hardware D3D11VA + External SRT)",
            media_dir.join("sample_h264_aac.mkv"),
            true,
            Some(media_dir.join("external_sample.srt")),
        ),
    ];

    let mut all_ok = true;
    for (label, path, hwdec, ext_sub) in cases {
        println!("\n=== TEST CASE: {label} ===");
        let mut prefs = PlayerPreferences::default();
        prefs.hardware_decoding = hwdec;
        prefs.default_volume = 0.0; // silent test run
        mpv.send(MpvCmd::Load {
            target: path.to_string_lossy().to_string(),
            start_secs: Some(0.0),
            subtitle_file: ext_sub.as_ref().map(|p| p.to_string_lossy().to_string()),
            prefs,
        })
        .expect("Failed to send load command");

        #[cfg(windows)]
        pump_messages_for(1400);

        // Exercise real track switching, subtitle delay, aspect ratio, speed, and seeking
        let snap1 = mpv.snapshot();
        if snap1.audio_tracks.len() >= 2 {
            let second_audio = snap1.audio_tracks[1].numeric_id.unwrap_or(2);
            let _ = mpv.send(MpvCmd::SetAudioTrack(second_audio));
        }
        if snap1.subtitle_tracks.len() >= 2 {
            let second_sub = snap1.subtitle_tracks[1].numeric_id.unwrap_or(2);
            let _ = mpv.send(MpvCmd::SetSubtitleTrack(second_sub));
        }
        let _ = mpv.send(MpvCmd::SetSubtitleDelayMs(250));
        let _ = mpv.send(MpvCmd::SetSubtitlePos(85));
        let _ = mpv.send(MpvCmd::SetSpeed(1.25));
        let _ = mpv.send(MpvCmd::SetAspectRatio("16:9".to_string()));
        let _ = mpv.send(MpvCmd::SeekAbsolute(1.5));

        #[cfg(windows)]
        pump_messages_for(900);

        let snap = mpv.snapshot();
        println!("  vo_configured   : {}", snap.vo_configured);
        println!("  video_codec     : {:?}", snap.video_codec);
        println!("  audio_codec     : {:?}", snap.audio_codec);
        println!("  resolution      : {}x{}", snap.video_width, snap.video_height);
        println!("  hwdec_mode      : {}", snap.hwdec_mode);
        println!("  hwdec_current   : {:?}", snap.hwdec_current);
        println!("  position / dur  : {:.2}s / {:.2}s", snap.position_secs, snap.duration_secs);
        println!("  speed / delay   : {}x / {}ms", snap.speed, snap.sub_delay_ms);
        println!("  audio_tracks    : {} (selected={:?})", snap.audio_tracks.len(), snap.selected_audio_id);
        for t in &snap.audio_tracks {
            println!("    - [{}] {} (codec={:?}, selected={})", t.id, t.title, t.codec, t.selected);
        }
        println!("  subtitle_tracks : {} (selected={:?})", snap.subtitle_tracks.len(), snap.selected_subtitle_id);
        for t in &snap.subtitle_tracks {
            println!("    - [{}] {} (codec={:?}, ext={}, selected={})", t.id, t.title, t.codec, t.external, t.selected);
        }
        println!("  error           : {:?}", snap.error);

        let case_ok = snap.error.is_none()
            && snap.vo_configured
            && snap.video_width > 0
            && snap.video_height > 0
            && snap.duration_secs > 1.0
            && snap.video_codec.is_some()
            && snap.audio_codec.is_some();

        if !case_ok {
            all_ok = false;
            eprintln!("  RESULT: FAILED");
        } else {
            println!("  RESULT: PASSED");
        }
    }

    let _ = mpv.send(MpvCmd::Stop);
    drop(mpv);

    // Optional secondary engine verification: Embedded libVLC (if 64-bit system VLC is installed)
    if let Some(vlc_dll) = detect_libvlc_dll() {
        println!("\n=== OPTIONAL ENGINE TEST: Embedded libVLC ({}) ===", vlc_dll.display());
        match seasonfetch_desktop_lib::native_player::libvlc_backend::EmbeddedLibVlc::spawn(
            vlc_dll,
            wid,
            Arc::new(|_| {}),
        ) {
            Ok(vlc) => {
                let mut prefs = PlayerPreferences::default();
                prefs.default_volume = 0.0;
                let hevc_path = media_dir.join("sample_hevc_ac3_multitrack.mkv");
                let _ = vlc.send(
                    seasonfetch_desktop_lib::native_player::libvlc_backend::VlcCmd::Load {
                        target: hevc_path.to_string_lossy().to_string(),
                        start_secs: Some(0.0),
                        subtitle_file: Some(
                            media_dir.join("external_sample.srt").to_string_lossy().to_string(),
                        ),
                        prefs,
                    },
                );
                #[cfg(windows)]
                pump_messages_for(1800);
                let vsnap = vlc.snapshot();
                println!("  libVLC playing        : {}", vsnap.playing);
                println!("  libVLC position / dur : {:.2}s / {:.2}s", vsnap.position_secs, vsnap.duration_secs);
                println!("  libVLC audio_tracks   : {}", vsnap.audio_tracks.len());
                println!("  libVLC sub_tracks     : {}", vsnap.subtitle_tracks.len());
                println!("  libVLC error          : {:?}", vsnap.error);
                let _ = vlc.send(seasonfetch_desktop_lib::native_player::libvlc_backend::VlcCmd::Stop);
                drop(vlc);
            }
            Err(err) => {
                println!("  libVLC init notice: {err}");
            }
        }
    }

    #[cfg(windows)]
    {
        drop(surface);
        unsafe {
            DestroyWindow(host_hwnd as _);
        }
    }

    if !all_ok {
        std::process::exit(1);
    }
    println!("\nALL REAL MEDIA VERIFICATION CASES PASSED!");
}
