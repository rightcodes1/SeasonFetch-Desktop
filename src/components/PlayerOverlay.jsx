import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  Cpu,
  ExternalLink,
  FastForward,
  Loader2,
  Magnet,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  Radio,
  Rewind,
  Settings2,
  SkipForward,
  Subtitles,
  Volume2,
  VolumeX
} from 'lucide-react';
import {
  getActiveSubtitleCues,
  parseSubtitleDocument
} from '../player/subtitleEngine.js';
import {
  isTauriRuntime,
  launchCustomExternalPlayer,
  launchNativeMpv,
  nativePlayerCommand,
  nativePlayerLoad,
  nativePlayerStop,
  pickSubtitleFile,
  savePlaybackProgress,
  startTorrentStream,
  stopTorrentStream,
  subscribeNativePlayerState,
  subscribeStreamSessionStatus,
  toPlayableAssetUrl,
  windowControls
} from '../services/desktopApi.js';

const SPEED_OPTIONS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const ASPECT_OPTIONS = [
  { value: 'auto', label: 'Auto' },
  { value: '16:9', label: '16:9' },
  { value: '21:9', label: '21:9' },
  { value: '4:3', label: '4:3' },
  { value: 'fill', label: 'Fill' }
];

function formatTime(secs) {
  if (!Number.isFinite(secs) || secs < 0) return '0:00';
  const total = Math.floor(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function cueAlignmentStyle(alignment, verticalPosPct) {
  const align = Number(alignment) || 2;
  const isTop = align >= 7;
  const isMid = align >= 4 && align <= 6;
  const isLeft = align === 1 || align === 4 || align === 7;
  const isRight = align === 3 || align === 6 || align === 9;

  return {
    top: isTop ? '6%' : isMid ? '50%' : `${verticalPosPct}%`,
    transform: isMid ? 'translateY(-50%)' : isTop ? 'none' : 'translateY(-100%)',
    textAlign: isLeft ? 'left' : isRight ? 'right' : 'center',
    alignItems: isLeft ? 'flex-start' : isRight ? 'flex-end' : 'center'
  };
}

function resolveInitialEngine(playerPrefs, playerBackend) {
  if (!isTauriRuntime()) return 'webview';
  const pref = playerPrefs?.preferredEngine || 'auto';
  if (pref === 'external-player') return 'external-player';
  if (pref === 'external-mpv' && playerBackend?.mpvAvailable) return 'external-player';
  if (pref === 'webview') return 'webview';
  if (pref === 'embedded-libvlc' && playerBackend?.libvlcAvailable) return 'embedded-libvlc';
  if ((pref === 'embedded-libmpv' || pref === 'mpv') && playerBackend?.libmpvAvailable) return 'embedded-libmpv';
  if (playerBackend?.libmpvAvailable) return 'embedded-libmpv';
  if (playerBackend?.libvlcAvailable) return 'embedded-libvlc';
  return 'webview';
}

function formatSpeedBps(bps) {
  if (!Number.isFinite(bps) || bps <= 0) return '0 KB/s';
  if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${Math.round(bps / 1024)} KB/s`;
}

function formatSizeCompact(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '';
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

export function PlayerOverlay({
  session, // { mediaKey, title, subtitle, streamUrl, rawPath, magnetUri, torrentRequest, imdbId, mediaType, season, episode, resumeSecs, nextEpisode }
  playerPrefs,
  playerBackend,
  onUpdatePlayerPrefs,
  onOpenMagnet,
  onPlayNextEpisode,
  onClose,
  onToast
}) {
  const videoRef = useRef(null);
  const lastSavedRef = useRef(0);
  const hideHudTimerRef = useRef(null);
  const latestPosRef = useRef({ pos: session?.resumeSecs || 0, dur: 0 });
  const torrentStoppedRef = useRef(false);
  const externalLaunchedUrlRef = useRef(null);

  const [activeEngine, setActiveEngine] = useState(() =>
    resolveInitialEngine(playerPrefs, playerBackend)
  );
  const isExternalMode = activeEngine === 'external-player';
  const isNativeEngine = activeEngine === 'embedded-libmpv' || activeEngine === 'embedded-libvlc';
  const [externalLaunchedName, setExternalLaunchedName] = useState('');

  // Sync activeEngine if playerBackend finished loading after mount
  useEffect(() => {
    if (!playerBackend) return;
    if (activeEngine === 'webview' && (playerPrefs?.preferredEngine || 'auto') !== 'webview') {
      const resolved = resolveInitialEngine(playerPrefs, playerBackend);
      if (resolved !== activeEngine) {
        setActiveEngine(resolved);
      }
    }
  }, [activeEngine, playerBackend, playerPrefs]);

  const [playing, setPlaying] = useState(false);
  const [buffering, setBuffering] = useState(true);
  const [currentTime, setCurrentTime] = useState(session?.resumeSecs || 0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(playerPrefs?.defaultVolume ?? 0.9);
  const [muted, setMuted] = useState(false);
  const [speed, setSpeed] = useState(playerPrefs?.playbackSpeed ?? 1.0);
  const [aspectRatio, setAspectRatio] = useState(playerPrefs?.aspectRatio || 'auto');
  const [videoScale, setVideoScale] = useState(playerPrefs?.videoScale ?? 1.0);
  const [hwdecEnabled, setHwdecEnabled] = useState(playerPrefs?.hardwareDecoding ?? true);
  const [fullscreen, setFullscreen] = useState(false);
  const [hudVisible, setHudVisible] = useState(true);
  const [activeDrawer, setActiveDrawer] = useState(null); // 'subtitles' | 'settings' | null
  const [videoError, setVideoError] = useState('');

  // Live native engine telemetry (codec, D3D11VA/SW, voConfigured, tracks)
  const [nativeMeta, setNativeMeta] = useState({
    voConfigured: false,
    hwdecCurrent: null,
    videoCodec: null,
    audioCodec: null,
    videoWidth: 0,
    videoHeight: 0,
    audioTracks: [],
    subtitleTracks: [],
    selectedAudioId: null,
    selectedSubtitleId: null
  });

  // WebView2 HTML5 fallback subtitle state (ASS/SSA + SRT + VTT)
  const [subCues, setSubCues] = useState([]);
  const [subTrackName, setSubTrackName] = useState('Auto');
  const [subFilePath, setSubFilePath] = useState(session?.subtitleFile || null);
  const [subEnabled, setSubEnabled] = useState(true);
  const [subDelayMs, setSubDelayMs] = useState(playerPrefs?.subtitleDelayMs ?? 0);
  const [subVerticalPos, setSubVerticalPos] = useState(playerPrefs?.subtitleVerticalPos ?? 92);
  const [subFontSize, setSubFontSize] = useState(playerPrefs?.subtitleFontSize ?? 26);
  const [preferAssStyling, setPreferAssStyling] = useState(playerPrefs?.preferAssStyling ?? true);

  // Audio tracks for WebView fallback
  const [webAudioTracks, setWebAudioTracks] = useState([]);
  const [selectedWebAudioIdx, setSelectedWebAudioIdx] = useState(0);

  // Embedded BitTorrent streaming session state (Stremio-like workflow)
  const isTorrentSession = Boolean(session?.torrentRequest);
  const [resolvedTorrentUrl, setResolvedTorrentUrl] = useState(null);
  const [streamStatus, setStreamStatus] = useState(null);
  const [streamRetryTick, setStreamRetryTick] = useState(0);

  const rawTarget = session?.rawPath || resolvedTorrentUrl || session?.streamUrl || '';
  const playableSrc = useMemo(
    () => toPlayableAssetUrl(resolvedTorrentUrl || session?.streamUrl || session?.rawPath),
    [resolvedTorrentUrl, session]
  );

  // Start & clean up embedded BitTorrent streaming session when playing a Torrentio magnet release
  useEffect(() => {
    if (!session?.torrentRequest) {
      setResolvedTorrentUrl(null);
      setStreamStatus(null);
      return undefined;
    }

    let cancelled = false;
    torrentStoppedRef.current = false;
    externalLaunchedUrlRef.current = null;
    setExternalLaunchedName('');
    setResolvedTorrentUrl(null);
    setVideoError('');
    setBuffering(true);
    setStreamStatus({
      state: 'resolvingMetadata',
      title: session.torrentRequest.title || session.title,
      peersConnected: 0,
      peersSeen: 0,
      downloadSpeedBps: 0,
      progressPercent: 0,
      totalBytes: 0,
      statusMessage: 'Connecting to peers and resolving torrent metadata...'
    });

    const unsubStream = subscribeStreamSessionStatus(nextStatus => {
      if (cancelled || !nextStatus) return;
      setStreamStatus(nextStatus);
      if (nextStatus.state === 'error' && nextStatus.error) {
        setVideoError(nextStatus.error);
        setBuffering(false);
      }
    });

    startTorrentStream(session.torrentRequest)
      .then(ready => {
        if (cancelled) return;
        setStreamStatus(ready);
        if (ready?.streamUrl) {
          setResolvedTorrentUrl(ready.streamUrl);
        } else {
          setBuffering(false);
          setVideoError('Embedded streaming engine resolved metadata without a playable stream URL.');
        }
      })
      .catch(err => {
        if (cancelled) return;
        setBuffering(false);
        const msg = typeof err === 'string' ? err : err?.message || 'Unable to start embedded torrent stream.';
        setStreamStatus(prev => ({ ...(prev || {}), state: 'error', statusMessage: msg, error: msg }));
        setVideoError(msg);
      });

    return () => {
      cancelled = true;
      unsubStream();
      if (!torrentStoppedRef.current) {
        torrentStoppedRef.current = true;
        stopTorrentStream().catch(() => {});
      }
    };
  }, [session?.mediaKey, session?.torrentRequest, streamRetryTick]);

  // Auto-launch user's chosen external player once the stream URL is ready in external-player mode
  useEffect(() => {
    if (!isExternalMode || !rawTarget) return;
    if (externalLaunchedUrlRef.current === rawTarget) return;
    externalLaunchedUrlRef.current = rawTarget;
    setBuffering(false);
    launchCustomExternalPlayer({
      target: rawTarget,
      title: session?.title || 'SeasonFetch Playback',
      startSecs: session?.resumeSecs || 0,
      subtitleFile: subFilePath,
      executablePath: playerPrefs?.externalPlayerPath || null
    }).then(res => {
      if (res?.ok) {
        setExternalLaunchedName(res.engine || 'external player');
        onToast?.(`Launched stream in ${res.engine}`);
      } else {
        setVideoError(res?.error || 'Could not launch configured external player.');
      }
    });
  }, [isExternalMode, onToast, playerPrefs?.externalPlayerPath, rawTarget, session?.resumeSecs, session?.title, subFilePath]);

  const persistProgress = useCallback((pos, dur) => {
    if (!session?.mediaKey || !dur || dur <= 0) return;
    savePlaybackProgress({
      mediaKey: session.mediaKey,
      title: session.title,
      imdbId: session.imdbId || null,
      mediaType: session.mediaType || null,
      season: session.season ?? null,
      episode: session.episode ?? null,
      positionSecs: pos,
      durationSecs: dur,
      completed: pos / dur >= 0.92,
      updatedAt: new Date().toISOString()
    }).catch(() => {});
  }, [session]);

  const revealHud = useCallback(() => {
    setHudVisible(true);
    if (hideHudTimerRef.current) clearTimeout(hideHudTimerRef.current);
    hideHudTimerRef.current = setTimeout(() => {
      setHudVisible(false);
    }, 3200);
  }, []);

  // Enable transparent WebView2 canvas ONLY while native embedded engine (libmpv/libVLC) owns the Win32 surface below
  useEffect(() => {
    if (!isNativeEngine) {
      document.documentElement.classList.remove('sf-native-video-active');
      return undefined;
    }
    document.documentElement.classList.add('sf-native-video-active');
    return () => {
      document.documentElement.classList.remove('sf-native-video-active');
    };
  }, [isNativeEngine]);

  // Mount & drive native embedded playback engine (libmpv / libVLC)
  useEffect(() => {
    if (!isNativeEngine || !rawTarget) return undefined;

    let cancelled = false;
    setVideoError('');
    setBuffering(true);
    setPlaying(false);
    setNativeMeta(prev => ({ ...prev, voConfigured: false }));

    const unsubscribe = subscribeNativePlayerState(st => {
      if (cancelled || !st) return;
      setPlaying(Boolean(st.playing));
      setBuffering(Boolean(st.buffering));
      if (Number.isFinite(st.positionSecs)) {
        setCurrentTime(st.positionSecs);
        latestPosRef.current.pos = st.positionSecs;
      }
      if (Number.isFinite(st.durationSecs) && st.durationSecs > 0) {
        setDuration(st.durationSecs);
        latestPosRef.current.dur = st.durationSecs;
      }
      if (Number.isFinite(st.volume)) setVolume(st.volume);
      setMuted(Boolean(st.muted));
      if (Number.isFinite(st.speed)) setSpeed(st.speed);
      if (Number.isFinite(st.subDelayMs)) setSubDelayMs(st.subDelayMs);
      if (st.error) setVideoError(st.error);

      setNativeMeta({
        voConfigured: Boolean(st.voConfigured),
        hwdecCurrent: st.hwdecCurrent || null,
        videoCodec: st.videoCodec || null,
        audioCodec: st.audioCodec || null,
        videoWidth: st.videoWidth || 0,
        videoHeight: st.videoHeight || 0,
        audioTracks: Array.isArray(st.audioTracks) ? st.audioTracks : [],
        subtitleTracks: Array.isArray(st.subtitleTracks) ? st.subtitleTracks : [],
        selectedAudioId: st.selectedAudioId ?? null,
        selectedSubtitleId: st.selectedSubtitleId ?? null
      });

      if (st.durationSecs > 0 && Math.abs((st.positionSecs || 0) - lastSavedRef.current) >= 5) {
        lastSavedRef.current = st.positionSecs || 0;
        persistProgress(st.positionSecs || 0, st.durationSecs);
      }
    });

    nativePlayerLoad({
      target: rawTarget,
      title: session?.title || 'SeasonFetch Playback',
      engine: activeEngine,
      startSecs: session?.resumeSecs || 0,
      subtitleFile: subFilePath,
      hardwareDecoding: hwdecEnabled
    }).then(res => {
      if (cancelled) return;
      if (!res?.ok) {
        setBuffering(false);
        setVideoError(res?.error || `Failed to start ${activeEngine}.`);
      }
    });

    return () => {
      cancelled = true;
      unsubscribe();
      nativePlayerStop().catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeEngine, isNativeEngine, rawTarget, session?.mediaKey]);

  // Keep WebView2 fallback <video> synced when in fallback mode
  useEffect(() => {
    if (isNativeEngine) return;
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume;
    video.muted = muted;
    video.playbackRate = speed;
  }, [isNativeEngine, volume, muted, speed]);

  const togglePlay = useCallback(() => {
    if (isNativeEngine) {
      nativePlayerCommand('toggle_pause', null).catch(() => {});
    } else {
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) {
        video.play().catch(err => setVideoError(err.message || 'Playback blocked or unsupported codec.'));
      } else {
        video.pause();
      }
    }
    revealHud();
  }, [isNativeEngine, revealHud]);

  const seekAbsolute = useCallback(targetSecs => {
    const clamped = Math.max(0, Math.min(duration || targetSecs, targetSecs));
    setCurrentTime(clamped);
    if (isNativeEngine) {
      nativePlayerCommand('seek_absolute', clamped).catch(() => {});
    } else if (videoRef.current) {
      videoRef.current.currentTime = clamped;
    }
    revealHud();
  }, [duration, isNativeEngine, revealHud]);

  const seekBy = useCallback(delta => {
    if (isNativeEngine) {
      nativePlayerCommand('seek_relative', delta).catch(() => {});
      setCurrentTime(prev => Math.max(0, Math.min(duration || prev + delta, prev + delta)));
    } else {
      const video = videoRef.current;
      if (!video) return;
      const next = Math.max(0, Math.min(video.duration || 0, video.currentTime + delta));
      video.currentTime = next;
      setCurrentTime(next);
    }
    revealHud();
  }, [duration, isNativeEngine, revealHud]);

  const handleVolumeChange = useCallback(nextVol => {
    const clamped = Math.max(0, Math.min(1, Number(nextVol.toFixed(2))));
    setMuted(false);
    setVolume(clamped);
    onUpdatePlayerPrefs?.({ defaultVolume: clamped });
    if (isNativeEngine) {
      nativePlayerCommand('set_volume', clamped).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleMuteToggle = useCallback(() => {
    setMuted(prev => {
      const next = !prev;
      if (isNativeEngine) nativePlayerCommand('set_muted', next).catch(() => {});
      return next;
    });
    revealHud();
  }, [isNativeEngine, revealHud]);

  const toggleFullscreen = useCallback(async () => {
    const next = !fullscreen;
    await windowControls.setFullscreen(next).catch(() => {});
    setFullscreen(next);
    revealHud();
  }, [fullscreen, revealHud]);

  const togglePiP = useCallback(async () => {
    if (isNativeEngine) {
      onToast?.('Picture-in-Picture is only available in WebView2 fallback mode.');
      return;
    }
    const video = videoRef.current;
    if (!video) return;
    try {
      if (document.pictureInPictureElement) {
        await document.exitPictureInPicture();
      } else if (document.pictureInPictureEnabled && video.requestPictureInPicture) {
        await video.requestPictureInPicture();
      }
    } catch {
      onToast?.('Picture-in-Picture is not supported for this stream.');
    }
  }, [isNativeEngine, onToast]);

  const handleSpeedChange = useCallback(nextSpeed => {
    setSpeed(nextSpeed);
    onUpdatePlayerPrefs?.({ playbackSpeed: nextSpeed });
    if (isNativeEngine) {
      nativePlayerCommand('set_speed', nextSpeed).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleAspectChange = useCallback(nextAspect => {
    setAspectRatio(nextAspect);
    onUpdatePlayerPrefs?.({ aspectRatio: nextAspect });
    if (isNativeEngine) {
      nativePlayerCommand('set_aspect_ratio', nextAspect).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleScaleChange = useCallback(nextScale => {
    setVideoScale(nextScale);
    onUpdatePlayerPrefs?.({ videoScale: nextScale });
    if (isNativeEngine) {
      nativePlayerCommand('set_video_scale', nextScale).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleHwdecToggle = useCallback(enabled => {
    setHwdecEnabled(enabled);
    onUpdatePlayerPrefs?.({ hardwareDecoding: enabled });
    if (isNativeEngine) {
      nativePlayerCommand('set_hardware_decoding', enabled).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleSubDelayChange = useCallback(nextMs => {
    const clamped = Math.max(-30000, Math.min(30000, nextMs));
    setSubDelayMs(clamped);
    onUpdatePlayerPrefs?.({ subtitleDelayMs: clamped });
    if (isNativeEngine) {
      nativePlayerCommand('set_subtitle_delay_ms', clamped).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleSubPosChange = useCallback(nextPos => {
    setSubVerticalPos(nextPos);
    onUpdatePlayerPrefs?.({ subtitleVerticalPos: nextPos });
    if (isNativeEngine) {
      nativePlayerCommand('set_subtitle_pos', nextPos).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleSubFontSizeChange = useCallback(nextPx => {
    setSubFontSize(nextPx);
    onUpdatePlayerPrefs?.({ subtitleFontSize: nextPx });
    if (isNativeEngine) {
      nativePlayerCommand('set_subtitle_font_size', nextPx).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handlePreferAssToggle = useCallback(preserve => {
    setPreferAssStyling(preserve);
    onUpdatePlayerPrefs?.({ preferAssStyling: preserve });
    if (isNativeEngine) {
      nativePlayerCommand('set_prefer_ass_styling', preserve).catch(() => {});
    }
  }, [isNativeEngine, onUpdatePlayerPrefs]);

  const handleClosePlayer = useCallback(async () => {
    const stopStreamPromise =
      isTorrentSession && !torrentStoppedRef.current
        ? ((torrentStoppedRef.current = true), stopTorrentStream().catch(() => {}))
        : Promise.resolve();

    if (isNativeEngine) {
      const { pos, dur } = latestPosRef.current;
      if (dur > 0) persistProgress(pos, dur);
      await nativePlayerStop().catch(() => {});
    } else {
      const video = videoRef.current;
      if (video && video.duration > 0) {
        persistProgress(video.currentTime, video.duration);
      }
    }
    await stopStreamPromise;
    if (fullscreen) {
      await windowControls.setFullscreen(false).catch(() => {});
    }
    onClose?.();
  }, [fullscreen, isNativeEngine, isTorrentSession, onClose, persistProgress]);

  useEffect(() => {
    const handleKey = event => {
      if (event.target && ['INPUT', 'SELECT', 'TEXTAREA'].includes(event.target.tagName)) return;
      const key = event.key.toLowerCase();
      if (key === ' ' || key === 'k') {
        event.preventDefault();
        togglePlay();
      } else if (key === 'arrowleft' || key === 'j') {
        event.preventDefault();
        seekBy(-10);
      } else if (key === 'arrowright' || key === 'l') {
        event.preventDefault();
        seekBy(10);
      } else if (key === 'arrowup') {
        event.preventDefault();
        handleVolumeChange(volume + 0.05);
        revealHud();
      } else if (key === 'arrowdown') {
        event.preventDefault();
        handleVolumeChange(volume - 0.05);
        revealHud();
      } else if (key === 'm') {
        handleMuteToggle();
      } else if (key === 'f') {
        toggleFullscreen();
      } else if (key === 'p') {
        togglePiP();
      } else if (key === 's') {
        setActiveDrawer(cur => (cur === 'subtitles' ? null : 'subtitles'));
        revealHud();
      } else if (key === '[') {
        const idx = SPEED_OPTIONS.indexOf(speed);
        if (idx > 0) handleSpeedChange(SPEED_OPTIONS[idx - 1]);
      } else if (key === ']') {
        const idx = SPEED_OPTIONS.indexOf(speed);
        if (idx >= 0 && idx < SPEED_OPTIONS.length - 1) handleSpeedChange(SPEED_OPTIONS[idx + 1]);
      } else if (key === 'z') {
        handleSubDelayChange(subDelayMs - 100);
        revealHud();
      } else if (key === 'x') {
        handleSubDelayChange(subDelayMs + 100);
        revealHud();
      } else if (key === 'escape') {
        if (activeDrawer) setActiveDrawer(null);
        else handleClosePlayer();
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => {
      window.removeEventListener('keydown', handleKey);
      if (hideHudTimerRef.current) clearTimeout(hideHudTimerRef.current);
    };
  }, [
    activeDrawer,
    handleClosePlayer,
    handleMuteToggle,
    handleSpeedChange,
    handleSubDelayChange,
    handleVolumeChange,
    revealHud,
    seekBy,
    speed,
    subDelayMs,
    toggleFullscreen,
    togglePiP,
    togglePlay,
    volume
  ]);

  async function handleLoadSubtitleFile() {
    const picked = await pickSubtitleFile();
    if (!picked?.ok) return;
    setSubFilePath(picked.path);

    if (isNativeEngine && picked.path) {
      const res = await nativePlayerCommand('add_subtitle_file', picked.path);
      if (res?.ok) {
        onToast?.(`Loaded external subtitle via ${activeEngine === 'embedded-libmpv' ? 'libass' : 'libVLC'}: ${picked.name}`);
      } else {
        onToast?.(res?.error || 'Could not load external subtitle.');
      }
      return;
    }

    if (picked.content) {
      const parsed = parseSubtitleDocument(picked.content, picked.format);
      setSubCues(parsed.cues);
      setSubTrackName(`${picked.name} (${parsed.format.toUpperCase()} · ${parsed.cues.length} cues)`);
      setSubEnabled(true);
      onToast?.(`Loaded ${parsed.cues.length} ${parsed.format.toUpperCase()} subtitle cues`);
    }
  }

  async function handleLaunchExternalPlayer() {
    if (!rawTarget) return;
    if (isNativeEngine) {
      await nativePlayerCommand('set_paused', true).catch(() => {});
    } else if (videoRef.current) {
      videoRef.current.pause();
    }
    const customExe = playerPrefs?.externalPlayerPath?.trim() || null;
    const res = customExe
      ? await launchCustomExternalPlayer({
          target: rawTarget,
          title: session.title,
          startSecs: currentTime,
          subtitleFile: subFilePath,
          executablePath: customExe
        })
      : await launchNativeMpv({
          target: rawTarget,
          title: session.title,
          startSecs: currentTime,
          subtitleFile: subFilePath
        });
    if (res?.ok) {
      setExternalLaunchedName(res.engine || 'external player');
      onToast?.(`Opened in ${res.engine || 'external player'}`);
    } else {
      onToast?.(res?.error || 'External player could not be launched.');
    }
  }

  const activeCues = useMemo(
    () => (!isNativeEngine && subEnabled ? getActiveSubtitleCues(subCues, currentTime, subDelayMs) : []),
    [isNativeEngine, subEnabled, subCues, currentTime, subDelayMs]
  );

  const videoStyle = useMemo(() => {
    const objFit = aspectRatio === 'fill' ? 'cover' : 'contain';
    const ratioMap = { '16:9': '16 / 9', '21:9': '21 / 9', '4:3': '4 / 3' };
    return {
      objectFit: objFit,
      aspectRatio: ratioMap[aspectRatio] || undefined,
      transform: videoScale !== 1 ? `scale(${videoScale})` : undefined
    };
  }, [aspectRatio, videoScale]);

  const engineLabel =
    activeEngine === 'embedded-libmpv'
      ? 'libmpv'
      : activeEngine === 'embedded-libvlc'
        ? 'libVLC'
        : activeEngine === 'external-player'
          ? 'External'
          : 'WebView2';

  const hasExternalPlayerConfigured = Boolean(
    playerPrefs?.externalPlayerPath?.trim() ||
    playerBackend?.mpvAvailable ||
    playerBackend?.vlcAvailable
  );

  // Keep the player backdrop solid dark until native video output is actually
  // configured and presenting frames (prevents transparent window bleed-through).
  const showOpaqueBackdrop =
    !rawTarget ||
    isExternalMode ||
    (isNativeEngine && !nativeMeta.voConfigured && !playing);

  const hwBadge = useMemo(() => {
    if (isExternalMode) return 'Custom';
    if (!isNativeEngine) return 'HTML5';
    const hw = nativeMeta.hwdecCurrent;
    if (!hw || hw === 'no') return 'Software';
    return hw.toUpperCase();
  }, [isExternalMode, isNativeEngine, nativeMeta.hwdecCurrent]);

  const codecSummary = useMemo(() => {
    const parts = [engineLabel];
    if (nativeMeta.videoCodec) parts.push(nativeMeta.videoCodec.split(' ')[0].toUpperCase());
    if (nativeMeta.audioCodec) parts.push(nativeMeta.audioCodec.toUpperCase());
    if (nativeMeta.videoWidth && nativeMeta.videoHeight) {
      parts.push(`${nativeMeta.videoWidth}×${nativeMeta.videoHeight}`);
    }
    parts.push(hwBadge);
    return parts.join(' · ');
  }, [engineLabel, hwBadge, nativeMeta]);

  return (
    <div
      className={`sf-player-overlay ${isNativeEngine ? 'is-native-surface' : ''} ${showOpaqueBackdrop ? 'is-preloading' : ''} ${hudVisible || !playing ? 'hud-open' : 'hud-hidden'}`}
      onMouseMove={revealHud}
    >
      {/* External Player Hand-off Stage (keeps embedded torrent HTTP server alive) */}
      {isExternalMode ? (
        <div className="sf-native-stage">
          <div className="sf-stream-buffering-card" onClick={e => e.stopPropagation()}>
            <div className="sf-stream-buffering-head">
              {!rawTarget ? <Loader2 size={18} className="sf-spin" /> : <ExternalLink size={18} />}
              <div>
                <strong>
                  {!rawTarget
                    ? 'Starting Torrent Stream for External Player'
                    : externalLaunchedName
                      ? `Streaming to ${externalLaunchedName}`
                      : 'Ready for External Player'}
                </strong>
                <span>
                  {!rawTarget
                    ? streamStatus?.statusMessage || 'Connecting to peers...'
                    : 'Keep this window open while watching — SeasonFetch is serving the live local HTTP stream.'}
                </span>
              </div>
            </div>
            {streamStatus?.fileName && (
              <div className="sf-stream-file-line">
                <span>{streamStatus.fileName}</span>
                {streamStatus.totalBytes > 0 && (
                  <span className="sf-mono-sm">{formatSizeCompact(streamStatus.totalBytes)}</span>
                )}
              </div>
            )}
            {isTorrentSession && (
              <div className="sf-stream-metrics-row">
                <span>Peers: <strong>{streamStatus?.peersConnected ?? 0}</strong>{streamStatus?.peersSeen ? ` / ${streamStatus.peersSeen} seen` : ''}</span>
                <span>Speed: <strong>{formatSpeedBps(streamStatus?.downloadSpeedBps || 0)}</strong></span>
                <span>Buffered: <strong>{(streamStatus?.progressPercent || 0).toFixed(1)}%</strong></span>
              </div>
            )}
            {rawTarget && (
              <div className="sf-btn-row" style={{ marginTop: 12 }}>
                <button type="button" className="sf-primary-btn" onClick={handleLaunchExternalPlayer}>
                  <ExternalLink size={14} /> Relaunch External Player
                </button>
                {playerBackend?.libmpvAvailable && (
                  <button
                    type="button"
                    className="sf-secondary-btn"
                    onClick={() => setActiveEngine('embedded-libmpv')}
                  >
                    Play Inside SeasonFetch
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
      ) : isNativeEngine ? (
        <div
          className="sf-native-stage"
          onClick={() => {
            if (activeDrawer) setActiveDrawer(null);
            else if (rawTarget) togglePlay();
          }}
          onDoubleClick={toggleFullscreen}
        >
          {isTorrentSession && !videoError && (!rawTarget || (buffering && !playing)) ? (
            <div className="sf-stream-buffering-card" onClick={e => e.stopPropagation()}>
              <div className="sf-stream-buffering-head">
                <Loader2 size={18} className="sf-spin" />
                <div>
                  <strong>
                    {!rawTarget ? 'Starting Torrent Stream' : 'Buffering Video Stream'}
                  </strong>
                  <span>{streamStatus?.statusMessage || 'Connecting to peers...'}</span>
                </div>
              </div>
              {streamStatus?.fileName && (
                <div className="sf-stream-file-line">
                  <span>{streamStatus.fileName}</span>
                  {streamStatus.totalBytes > 0 && (
                    <span className="sf-mono-sm">{formatSizeCompact(streamStatus.totalBytes)}</span>
                  )}
                </div>
              )}
              <div className="sf-stream-metrics-row">
                <span>Peers: <strong>{streamStatus?.peersConnected ?? 0}</strong>{streamStatus?.peersSeen ? ` / ${streamStatus.peersSeen} seen` : ''}</span>
                <span>Speed: <strong>{formatSpeedBps(streamStatus?.downloadSpeedBps || 0)}</strong></span>
                <span>Buffered: <strong>{(streamStatus?.progressPercent || 0).toFixed(1)}%</strong></span>
              </div>
              <div className="sf-stream-progress-track">
                <div
                  className={`sf-stream-progress-fill ${!rawTarget ? 'is-indeterminate' : ''}`}
                  style={{ width: rawTarget ? `${Math.max(4, Math.min(100, streamStatus?.progressPercent || 4))}%` : '38%' }}
                />
              </div>
            </div>
          ) : buffering && !videoError && (
            <div className="sf-player-buffering-pill">
              <Cpu size={14} /> Initializing {engineLabel} native decoder...
            </div>
          )}
        </div>
      ) : (
        <>
          <video
            ref={videoRef}
            src={playableSrc}
            className="sf-player-video"
            style={videoStyle}
            autoPlay
            onClick={togglePlay}
            onDoubleClick={toggleFullscreen}
            onLoadedMetadata={e => {
              const vid = e.currentTarget;
              setDuration(vid.duration || 0);
              setBuffering(false);
              if (session?.resumeSecs && session.resumeSecs > 3 && session.resumeSecs < (vid.duration || 0) - 10) {
                vid.currentTime = session.resumeSecs;
              }
              vid.playbackRate = speed;
              vid.volume = volume;
              if (vid.audioTracks && vid.audioTracks.length > 0) {
                setWebAudioTracks(
                  Array.from(vid.audioTracks).map((t, i) => ({
                    index: i,
                    label: t.label || t.language || `Audio Track ${i + 1}`,
                    language: t.language || ''
                  }))
                );
              }
            }}
            onPlay={() => {
              setPlaying(true);
              setBuffering(false);
              setVideoError('');
            }}
            onPause={() => {
              setPlaying(false);
              if (videoRef.current) persistProgress(videoRef.current.currentTime, videoRef.current.duration);
            }}
            onTimeUpdate={e => {
              const vid = e.currentTarget;
              setCurrentTime(vid.currentTime);
              if (Math.abs(vid.currentTime - lastSavedRef.current) >= 5) {
                lastSavedRef.current = vid.currentTime;
                persistProgress(vid.currentTime, vid.duration);
              }
            }}
            onError={() => {
              setBuffering(false);
              setVideoError(
                'WebView2 HTML5 could not decode this container or codec. Switch to Embedded libmpv for native MKV/HEVC/AC3/DTS + libass playback.'
              );
            }}
          />

          {/* WebView2 fallback JS Subtitle Canvas */}
          {subEnabled && activeCues.length > 0 && (
            <div className="sf-subtitle-layer" aria-live="polite">
              {activeCues.map((cue, idx) => {
                const posStyle = cueAlignmentStyle(
                  preferAssStyling ? cue.style?.alignment : 2,
                  subVerticalPos
                );
                return (
                  <div key={idx} className="sf-subtitle-anchor" style={posStyle}>
                    <span
                      className="sf-subtitle-cue"
                      style={{
                        fontSize: `${subFontSize}px`,
                        fontFamily: (preferAssStyling && cue.style?.fontFamily) || playerPrefs?.subtitleFontFamily || 'DM Sans, sans-serif',
                        color: (preferAssStyling && cue.style?.color) || playerPrefs?.subtitleColor || '#ffffff',
                        fontWeight: preferAssStyling && cue.style?.bold ? 700 : 500,
                        fontStyle: preferAssStyling && cue.style?.italic ? 'italic' : 'normal',
                        backgroundColor: `rgba(8, 9, 13, ${playerPrefs?.subtitleBgOpacity ?? 0.65})`
                      }}
                    >
                      {cue.text}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}

      {videoError && (
        <div className="sf-player-error-card" onClick={e => e.stopPropagation()}>
          <h3>{isTorrentSession && !rawTarget ? 'Torrent stream unavailable' : 'Playback decoder notice'}</h3>
          <p>{videoError}</p>
          <div className="sf-btn-row">
            {isTorrentSession && (
              <button
                type="button"
                className="sf-primary-btn"
                onClick={() => {
                  setVideoError('');
                  setStreamRetryTick(n => n + 1);
                }}
              >
                Retry Stream
              </button>
            )}
            {session?.magnetUri && onOpenMagnet && (
              <button
                type="button"
                className="sf-secondary-btn"
                onClick={() => {
                  onOpenMagnet(session.magnetUri, session.title);
                  handleClosePlayer();
                }}
              >
                <Magnet size={14} /> Open Magnet Externally
              </button>
            )}
            {rawTarget && activeEngine !== 'embedded-libmpv' && playerBackend?.libmpvAvailable && (
              <button
                type="button"
                className="sf-primary-btn"
                onClick={() => {
                  setVideoError('');
                  setActiveEngine('embedded-libmpv');
                }}
              >
                Switch to Embedded libmpv
              </button>
            )}
            {rawTarget && isNativeEngine && hwdecEnabled && (
              <button
                type="button"
                className="sf-secondary-btn"
                onClick={() => {
                  setVideoError('');
                  handleHwdecToggle(false);
                }}
              >
                Retry with Software Decoding
              </button>
            )}
            {rawTarget && hasExternalPlayerConfigured && (
              <button type="button" className="sf-ghost-btn" onClick={handleLaunchExternalPlayer}>
                <ExternalLink size={14} /> Open External Player
              </button>
            )}
            <button type="button" className="sf-secondary-btn" onClick={handleClosePlayer}>
              Back to Details
            </button>
          </div>
        </div>
      )}

      {/* Top Overlay Header */}
      <div className="sf-player-top" onClick={e => e.stopPropagation()}>
        <button type="button" className="sf-player-icon-btn" onClick={handleClosePlayer}>
          <ArrowLeft size={18} />
          <span>Back</span>
        </button>
        <div className="sf-player-title-group">
          <strong>{session?.title}</strong>
          {session?.subtitle && <span>{session.subtitle}</span>}
        </div>
        <div className="sf-player-top-actions">
          {isTorrentSession && streamStatus && (
            <span className="sf-stream-pill" title={streamStatus.fileName || 'Embedded Torrent Engine'}>
              <Radio size={12} /> {streamStatus.peersConnected ?? 0} peers · {formatSpeedBps(streamStatus.downloadSpeedBps || 0)}
            </span>
          )}
          <span className="sf-engine-badge" title="Active native playback engine and hardware decoder status">
            <Cpu size={12} /> {codecSummary}
          </span>
          {rawTarget && hasExternalPlayerConfigured && !isExternalMode && (
            <button
              type="button"
              className="sf-player-pill-btn"
              onClick={handleLaunchExternalPlayer}
              title={`Open in external player (${playerPrefs?.externalPlayerPath || playerBackend?.mpvPath || playerBackend?.vlcPath})`}
            >
              <ExternalLink size={13} />
              <span>External Player</span>
            </button>
          )}
        </div>
      </div>

      {/* Subtitles Drawer (Native libass / libVLC embedded + external tracks) */}
      {activeDrawer === 'subtitles' && (
        <div className="sf-player-drawer" onClick={e => e.stopPropagation()}>
          <div className="sf-drawer-header">
            <strong>Subtitles ({activeEngine === 'embedded-libmpv' ? 'Native libass' : engineLabel})</strong>
            <button type="button" className="sf-ghost-btn sf-sm" onClick={handleLoadSubtitleFile}>
              Load .ass / .srt / .vtt
            </button>
          </div>

          {isNativeEngine ? (
            <div className="sf-track-list">
              <button
                type="button"
                className={`sf-track-item ${!nativeMeta.selectedSubtitleId ? 'is-active' : ''}`}
                onClick={() => nativePlayerCommand('set_subtitle_track', 0).catch(() => {})}
              >
                Off (No subtitles)
              </button>
              {nativeMeta.subtitleTracks.map(t => (
                <button
                  key={t.id}
                  type="button"
                  className={`sf-track-item ${t.selected ? 'is-active' : ''}`}
                  onClick={() => nativePlayerCommand('set_subtitle_track', t.numericId ?? Number(t.id)).catch(() => {})}
                >
                  <span>{t.title}</span>
                  {t.external && <span className="sf-mini-tag">EXT</span>}
                </button>
              ))}
              {nativeMeta.subtitleTracks.length === 0 && (
                <p className="sf-muted-sm">No embedded subtitle tracks detected in this file. Use Load .ass / .srt / .vtt above.</p>
              )}
            </div>
          ) : (
            <>
              <div className="sf-drawer-row">
                <span>Active track</span>
                <strong>{subTrackName}</strong>
              </div>
              <label className="sf-drawer-row">
                <span>Show subtitles</span>
                <input type="checkbox" checked={subEnabled} onChange={e => setSubEnabled(e.target.checked)} />
              </label>
            </>
          )}

          {activeEngine !== 'embedded-libvlc' && (
            <label className="sf-drawer-row">
              <span>Preserve ASS/SSA anime styling</span>
              <input
                type="checkbox"
                checked={preferAssStyling}
                onChange={e => handlePreferAssToggle(e.target.checked)}
              />
            </label>
          )}

          <label className="sf-drawer-slider">
            <span>Delay: {subDelayMs >= 0 ? `+${subDelayMs}` : subDelayMs} ms (Z / X)</span>
            <input
              type="range"
              min="-5000"
              max="5000"
              step="50"
              value={subDelayMs}
              onChange={e => handleSubDelayChange(Number(e.target.value))}
            />
          </label>

          {activeEngine !== 'embedded-libvlc' && (
            <>
              <label className="sf-drawer-slider">
                <span>Vertical Position: {subVerticalPos}%</span>
                <input
                  type="range"
                  min="15"
                  max="96"
                  value={subVerticalPos}
                  onChange={e => handleSubPosChange(Number(e.target.value))}
                />
              </label>
              <label className="sf-drawer-slider">
                <span>Font Size: {subFontSize}px</span>
                <input
                  type="range"
                  min="16"
                  max="48"
                  value={subFontSize}
                  onChange={e => handleSubFontSizeChange(Number(e.target.value))}
                />
              </label>
            </>
          )}
        </div>
      )}

      {/* Playback, Audio Track, Engine & Video Settings Drawer */}
      {activeDrawer === 'settings' && (
        <div className="sf-player-drawer" onClick={e => e.stopPropagation()}>
          <div className="sf-drawer-header">
            <strong>Playback, Audio &amp; Engine</strong>
          </div>

          <div className="sf-drawer-row">
            <span>Playback Engine</span>
            <select
              className="sf-select sf-sm"
              value={activeEngine}
              onChange={e => {
                const next = e.target.value;
                setVideoError('');
                setActiveEngine(next);
                onUpdatePlayerPrefs?.({ preferredEngine: next });
              }}
            >
              {playerBackend?.libmpvAvailable && (
                <option value="embedded-libmpv">Embedded libmpv (Recommended)</option>
              )}
              {playerBackend?.libvlcAvailable && (
                <option value="embedded-libvlc">Embedded libVLC (System VLC)</option>
              )}
              {hasExternalPlayerConfigured && (
                <option value="external-player">External Player (Custom .exe)</option>
              )}
              <option value="webview">WebView2 HTML5 (Fallback)</option>
            </select>
          </div>

          {activeEngine === 'embedded-libmpv' && (
            <label className="sf-drawer-row">
              <span>Hardware Decoding (D3D11VA)</span>
              <input
                type="checkbox"
                checked={hwdecEnabled}
                onChange={e => handleHwdecToggle(e.target.checked)}
              />
            </label>
          )}

          {isNativeEngine && nativeMeta.audioTracks.length > 0 && (
            <div className="sf-drawer-row">
              <span>Audio Track</span>
              <select
                className="sf-select sf-sm"
                value={nativeMeta.selectedAudioId ?? ''}
                onChange={e => {
                  const id = Number(e.target.value);
                  if (Number.isFinite(id)) {
                    nativePlayerCommand('set_audio_track', id).catch(() => {});
                  }
                }}
              >
                {nativeMeta.audioTracks.map(t => (
                  <option key={t.id} value={t.numericId ?? t.id}>
                    {t.title}
                  </option>
                ))}
              </select>
            </div>
          )}

          {!isNativeEngine && webAudioTracks.length > 1 && (
            <div className="sf-drawer-row">
              <span>Audio Track</span>
              <select
                className="sf-select sf-sm"
                value={selectedWebAudioIdx}
                onChange={e => {
                  const idx = Number(e.target.value);
                  setSelectedWebAudioIdx(idx);
                  if (videoRef.current?.audioTracks) {
                    Array.from(videoRef.current.audioTracks).forEach((t, i) => {
                      t.enabled = i === idx;
                    });
                  }
                }}
              >
                {webAudioTracks.map(t => (
                  <option key={t.index} value={t.index}>
                    {t.label}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className="sf-drawer-row">
            <span>Speed</span>
            <div className="sf-chip-group">
              {SPEED_OPTIONS.map(opt => (
                <button
                  key={opt}
                  type="button"
                  className={speed === opt ? 'is-active' : ''}
                  onClick={() => handleSpeedChange(opt)}
                >
                  {opt}x
                </button>
              ))}
            </div>
          </div>

          <div className="sf-drawer-row">
            <span>Aspect Ratio</span>
            <div className="sf-chip-group">
              {ASPECT_OPTIONS.map(opt => (
                <button
                  key={opt.value}
                  type="button"
                  className={aspectRatio === opt.value ? 'is-active' : ''}
                  onClick={() => handleAspectChange(opt.value)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>

          <label className="sf-drawer-slider">
            <span>Video Zoom: {Math.round(videoScale * 100)}%</span>
            <input
              type="range"
              min="0.75"
              max="1.6"
              step="0.05"
              value={videoScale}
              onChange={e => handleScaleChange(Number(e.target.value))}
            />
          </label>
        </div>
      )}

      {/* Bottom Transport HUD */}
      <div className="sf-player-hud" onClick={e => e.stopPropagation()}>
        <div className="sf-timeline-row">
          <span className="sf-time-label">{formatTime(currentTime)}</span>
          <input
            type="range"
            className="sf-timeline-slider"
            min="0"
            max={duration || 100}
            step="0.25"
            value={Math.min(currentTime, duration || 100)}
            onChange={e => seekAbsolute(Number(e.target.value))}
            aria-label="Seek playback position"
          />
          <span className="sf-time-label">{formatTime(duration)}</span>
        </div>

        <div className="sf-transport-row">
          <div className="sf-transport-left">
            <button type="button" className="sf-player-icon-btn" onClick={togglePlay} title="Play/Pause (Space)">
              {playing ? <Pause size={18} /> : <Play size={18} fill="currentColor" />}
            </button>
            <button type="button" className="sf-player-icon-btn" onClick={() => seekBy(-10)} title="Back 10s (J)">
              <Rewind size={16} />
            </button>
            <button type="button" className="sf-player-icon-btn" onClick={() => seekBy(10)} title="Forward 10s (L)">
              <FastForward size={16} />
            </button>
            <button type="button" className="sf-player-icon-btn" onClick={handleMuteToggle} title="Mute (M)">
              {muted || volume === 0 ? <VolumeX size={16} /> : <Volume2 size={16} />}
            </button>
            <input
              type="range"
              className="sf-volume-slider"
              min="0"
              max="1"
              step="0.02"
              value={muted ? 0 : volume}
              onChange={e => handleVolumeChange(Number(e.target.value))}
              aria-label="Volume"
            />
          </div>

          <div className="sf-transport-right">
            {session?.nextEpisode && (
              <button
                type="button"
                className="sf-player-pill-btn"
                onClick={() => onPlayNextEpisode?.(session.nextEpisode)}
              >
                <SkipForward size={14} /> Next: E{session.nextEpisode.number}
              </button>
            )}
            <button
              type="button"
              className={`sf-player-icon-btn ${activeDrawer === 'subtitles' ? 'is-active' : ''}`}
              onClick={() => setActiveDrawer(cur => (cur === 'subtitles' ? null : 'subtitles'))}
              title="Subtitles (S)"
            >
              <Subtitles size={17} />
            </button>
            <button
              type="button"
              className={`sf-player-icon-btn ${activeDrawer === 'settings' ? 'is-active' : ''}`}
              onClick={() => setActiveDrawer(cur => (cur === 'settings' ? null : 'settings'))}
              title="Engine, Audio, Speed & Aspect"
            >
              <Settings2 size={17} />
            </button>
            {!isNativeEngine && (
              <button type="button" className="sf-player-icon-btn" onClick={togglePiP} title="Picture-in-Picture (P)">
                <PictureInPicture2 size={16} />
              </button>
            )}
            <button type="button" className="sf-player-icon-btn" onClick={toggleFullscreen} title="Fullscreen (F)">
              {fullscreen ? <Minimize size={16} /> : <Maximize size={16} />}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
