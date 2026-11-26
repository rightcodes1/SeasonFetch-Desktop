import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  ExternalLink,
  FastForward,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
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
  launchNativeMpv,
  pickSubtitleFile,
  savePlaybackProgress,
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

export function PlayerOverlay({
  session, // { mediaKey, title, subtitle, streamUrl, rawPath, imdbId, mediaType, season, episode, resumeSecs, nextEpisode }
  playerPrefs,
  playerBackend,
  onUpdatePlayerPrefs,
  onPlayNextEpisode,
  onClose,
  onToast
}) {
  const videoRef = useRef(null);
  const lastSavedRef = useRef(0);
  const hideHudTimerRef = useRef(null);

  const [playing, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(session?.resumeSecs || 0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(playerPrefs?.defaultVolume ?? 0.9);
  const [muted, setMuted] = useState(false);
  const [speed, setSpeed] = useState(playerPrefs?.playbackSpeed ?? 1.0);
  const [aspectRatio, setAspectRatio] = useState(playerPrefs?.aspectRatio || 'auto');
  const [videoScale, setVideoScale] = useState(playerPrefs?.videoScale ?? 1.0);
  const [fullscreen, setFullscreen] = useState(false);
  const [hudVisible, setHudVisible] = useState(true);
  const [activeDrawer, setActiveDrawer] = useState(null); // 'subtitles' | 'settings' | null
  const [videoError, setVideoError] = useState('');

  // Subtitle engine state (ASS/SSA + SRT + VTT)
  const [subCues, setSubCues] = useState([]);
  const [subTrackName, setSubTrackName] = useState('Off');
  const [subFilePath, setSubFilePath] = useState(null);
  const [subEnabled, setSubEnabled] = useState(true);
  const [subDelayMs, setSubDelayMs] = useState(playerPrefs?.subtitleDelayMs ?? 0);
  const [subVerticalPos, setSubVerticalPos] = useState(playerPrefs?.subtitleVerticalPos ?? 92);
  const [subFontSize, setSubFontSize] = useState(playerPrefs?.subtitleFontSize ?? 26);
  const [preferAssStyling, setPreferAssStyling] = useState(playerPrefs?.preferAssStyling ?? true);

  // Audio tracks exposed by media element where supported
  const [audioTracks, setAudioTracks] = useState([]);
  const [selectedAudioIdx, setSelectedAudioIdx] = useState(0);

  const playableSrc = useMemo(() => toPlayableAssetUrl(session?.streamUrl || session?.rawPath), [session]);

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

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume;
    video.muted = muted;
    video.playbackRate = speed;
  }, [volume, muted, speed]);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      video.play().catch(err => setVideoError(err.message || 'Playback blocked or unsupported codec.'));
    } else {
      video.pause();
    }
    revealHud();
  }, [revealHud]);

  const seekBy = useCallback(delta => {
    const video = videoRef.current;
    if (!video) return;
    const next = Math.max(0, Math.min(video.duration || 0, video.currentTime + delta));
    video.currentTime = next;
    setCurrentTime(next);
    revealHud();
  }, [revealHud]);

  const toggleFullscreen = useCallback(async () => {
    const next = !fullscreen;
    await windowControls.setFullscreen(next).catch(() => {});
    setFullscreen(next);
    revealHud();
  }, [fullscreen, revealHud]);

  const togglePiP = useCallback(async () => {
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
  }, [onToast]);

  const handleSpeedChange = useCallback(nextSpeed => {
    setSpeed(nextSpeed);
    onUpdatePlayerPrefs?.({ playbackSpeed: nextSpeed });
  }, [onUpdatePlayerPrefs]);

  const handleClosePlayer = useCallback(async () => {
    const video = videoRef.current;
    if (video && video.duration > 0) {
      persistProgress(video.currentTime, video.duration);
    }
    if (fullscreen) {
      await windowControls.setFullscreen(false).catch(() => {});
    }
    onClose?.();
  }, [fullscreen, onClose, persistProgress]);

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
        setVolume(v => Math.min(1, Number((v + 0.05).toFixed(2))));
        revealHud();
      } else if (key === 'arrowdown') {
        event.preventDefault();
        setVolume(v => Math.max(0, Number((v - 0.05).toFixed(2))));
        revealHud();
      } else if (key === 'm') {
        setMuted(m => !m);
        revealHud();
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
        setSubDelayMs(ms => ms - 100);
        revealHud();
      } else if (key === 'x') {
        setSubDelayMs(ms => ms + 100);
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
  }, [activeDrawer, handleClosePlayer, handleSpeedChange, revealHud, seekBy, speed, toggleFullscreen, togglePiP, togglePlay]);

  async function handleLoadSubtitleFile() {
    const picked = await pickSubtitleFile();
    if (!picked?.ok || !picked.content) return;
    const parsed = parseSubtitleDocument(picked.content, picked.format);
    setSubCues(parsed.cues);
    setSubTrackName(`${picked.name} (${parsed.format.toUpperCase()} · ${parsed.cues.length} cues)`);
    setSubFilePath(picked.path);
    setSubEnabled(true);
    onToast?.(`Loaded ${parsed.cues.length} ${parsed.format.toUpperCase()} subtitle cues`);
  }

  async function handleLaunchMpv() {
    const target = session?.rawPath || session?.streamUrl;
    if (!target) return;
    const video = videoRef.current;
    const pos = video ? video.currentTime : currentTime;
    if (video) video.pause();
    const res = await launchNativeMpv({
      target,
      title: session.title,
      startSecs: pos,
      subtitleFile: subFilePath
    });
    if (res?.ok) {
      onToast?.('Launched in native MPV with ASS/SSA subtitle settings');
    } else {
      onToast?.(res?.error || 'Could not launch native MPV.');
    }
  }

  const activeCues = useMemo(
    () => (subEnabled ? getActiveSubtitleCues(subCues, currentTime, subDelayMs) : []),
    [subEnabled, subCues, currentTime, subDelayMs]
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

  return (
    <div
      className={`sf-player-overlay ${hudVisible || !playing ? 'hud-open' : 'hud-hidden'}`}
      onMouseMove={revealHud}
    >
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
          if (session?.resumeSecs && session.resumeSecs > 3 && session.resumeSecs < (vid.duration || 0) - 10) {
            vid.currentTime = session.resumeSecs;
          }
          vid.playbackRate = speed;
          vid.volume = volume;
          if (vid.audioTracks && vid.audioTracks.length > 0) {
            setAudioTracks(
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
          setVideoError(
            'This container or codec could not be decoded by WebView2 (common with MKV/HEVC/AC3). Use "Open in Native MPV" for full hardware-decoded MKV + libass playback.'
          );
        }}
      />

      {/* Styled ASS/SSA & SRT/VTT Subtitle Canvas */}
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

      {videoError && (
        <div className="sf-player-error-card">
          <h3>Embedded decoder limitation</h3>
          <p>{videoError}</p>
          <div className="sf-btn-row">
            <button type="button" className="sf-primary-btn" onClick={handleLaunchMpv}>
              <ExternalLink size={14} /> Open in Native MPV
            </button>
            <button type="button" className="sf-secondary-btn" onClick={handleClosePlayer}>
              Back to Details
            </button>
          </div>
        </div>
      )}

      {/* Top Overlay Header */}
      <div className="sf-player-top">
        <button type="button" className="sf-player-icon-btn" onClick={handleClosePlayer}>
          <ArrowLeft size={18} />
          <span>Back</span>
        </button>
        <div className="sf-player-title-group">
          <strong>{session?.title}</strong>
          {session?.subtitle && <span>{session.subtitle}</span>}
        </div>
        <div className="sf-player-top-actions">
          <button
            type="button"
            className="sf-player-pill-btn"
            onClick={handleLaunchMpv}
            title={playerBackend?.mpvAvailable ? `Launch in ${playerBackend.mpvPath}` : 'Requires mpv installed on Windows'}
          >
            <ExternalLink size={13} />
            <span>{playerBackend?.mpvAvailable ? 'Hand off to Native MPV' : 'Native MPV (Not Detected)'}</span>
          </button>
        </div>
      </div>

      {/* Subtitles / Settings Popover Drawers */}
      {activeDrawer === 'subtitles' && (
        <div className="sf-player-drawer" onClick={e => e.stopPropagation()}>
          <div className="sf-drawer-header">
            <strong>Subtitles &amp; ASS/SSA Styling</strong>
            <button type="button" className="sf-ghost-btn sf-sm" onClick={handleLoadSubtitleFile}>
              Load .ass / .srt / .vtt
            </button>
          </div>
          <div className="sf-drawer-row">
            <span>Active track</span>
            <strong>{subTrackName}</strong>
          </div>
          <label className="sf-drawer-row">
            <span>Show subtitles</span>
            <input type="checkbox" checked={subEnabled} onChange={e => setSubEnabled(e.target.checked)} />
          </label>
          <label className="sf-drawer-row">
            <span>Preserve ASS/SSA anime styling</span>
            <input
              type="checkbox"
              checked={preferAssStyling}
              onChange={e => {
                setPreferAssStyling(e.target.checked);
                onUpdatePlayerPrefs?.({ preferAssStyling: e.target.checked });
              }}
            />
          </label>
          <label className="sf-drawer-slider">
            <span>Delay: {subDelayMs >= 0 ? `+${subDelayMs}` : subDelayMs} ms (Z / X)</span>
            <input
              type="range"
              min="-5000"
              max="5000"
              step="50"
              value={subDelayMs}
              onChange={e => {
                const v = Number(e.target.value);
                setSubDelayMs(v);
                onUpdatePlayerPrefs?.({ subtitleDelayMs: v });
              }}
            />
          </label>
          <label className="sf-drawer-slider">
            <span>Vertical Position: {subVerticalPos}%</span>
            <input
              type="range"
              min="15"
              max="96"
              value={subVerticalPos}
              onChange={e => {
                const v = Number(e.target.value);
                setSubVerticalPos(v);
                onUpdatePlayerPrefs?.({ subtitleVerticalPos: v });
              }}
            />
          </label>
          <label className="sf-drawer-slider">
            <span>Font Size: {subFontSize}px</span>
            <input
              type="range"
              min="16"
              max="48"
              value={subFontSize}
              onChange={e => {
                const v = Number(e.target.value);
                setSubFontSize(v);
                onUpdatePlayerPrefs?.({ subtitleFontSize: v });
              }}
            />
          </label>
        </div>
      )}

      {activeDrawer === 'settings' && (
        <div className="sf-player-drawer" onClick={e => e.stopPropagation()}>
          <div className="sf-drawer-header">
            <strong>Playback &amp; Video Settings</strong>
          </div>
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
                  onClick={() => {
                    setAspectRatio(opt.value);
                    onUpdatePlayerPrefs?.({ aspectRatio: opt.value });
                  }}
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
              onChange={e => {
                const v = Number(e.target.value);
                setVideoScale(v);
                onUpdatePlayerPrefs?.({ videoScale: v });
              }}
            />
          </label>
          {audioTracks.length > 1 && (
            <div className="sf-drawer-row">
              <span>Audio Track</span>
              <select
                className="sf-select sf-sm"
                value={selectedAudioIdx}
                onChange={e => {
                  const idx = Number(e.target.value);
                  setSelectedAudioIdx(idx);
                  if (videoRef.current?.audioTracks) {
                    Array.from(videoRef.current.audioTracks).forEach((t, i) => {
                      t.enabled = i === idx;
                    });
                  }
                }}
              >
                {audioTracks.map(t => (
                  <option key={t.index} value={t.index}>
                    {t.label}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
      )}

      {/* Bottom Transport HUD */}
      <div className="sf-player-hud">
        <div className="sf-timeline-row">
          <span className="sf-time-label">{formatTime(currentTime)}</span>
          <input
            type="range"
            className="sf-timeline-slider"
            min="0"
            max={duration || 100}
            step="0.25"
            value={Math.min(currentTime, duration || 100)}
            onChange={e => {
              const t = Number(e.target.value);
              if (videoRef.current) videoRef.current.currentTime = t;
              setCurrentTime(t);
            }}
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
            <button type="button" className="sf-player-icon-btn" onClick={() => setMuted(m => !m)} title="Mute (M)">
              {muted || volume === 0 ? <VolumeX size={16} /> : <Volume2 size={16} />}
            </button>
            <input
              type="range"
              className="sf-volume-slider"
              min="0"
              max="1"
              step="0.02"
              value={muted ? 0 : volume}
              onChange={e => {
                const v = Number(e.target.value);
                setMuted(false);
                setVolume(v);
                onUpdatePlayerPrefs?.({ defaultVolume: v });
              }}
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
              title="Speed, Aspect & Audio"
            >
              <Settings2 size={17} />
            </button>
            <button type="button" className="sf-player-icon-btn" onClick={togglePiP} title="Picture-in-Picture (P)">
              <PictureInPicture2 size={16} />
            </button>
            <button type="button" className="sf-player-icon-btn" onClick={toggleFullscreen} title="Fullscreen (F)">
              {fullscreen ? <Minimize size={16} /> : <Maximize size={16} />}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
