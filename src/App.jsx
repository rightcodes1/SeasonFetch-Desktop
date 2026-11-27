import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { normalizeRankingPreference } from '../shared/rankingPreferences.js';
import {
  cancelUpdateDownload,
  checkForUpdates,
  checkTorrentioManifest,
  clearDesktopHistory,
  clearLocalCache,
  clearStreamCache,
  dismissUpdaterRecovery,
  downloadAvailableUpdate,
  downloadsCancel,
  downloadsList,
  downloadsOpenFolder,
  downloadsRemove,
  downloadsStart,
  fetchBackendHealth,
  fetchDiscoverCatalog,
  fetchMediaDetail,
  getAppInfo,
  getDesktopDiagnostics,
  getDesktopSettings,
  getPlaybackProgress,
  getPlayerBackendInfo,
  getStreamingStatus,
  getUpdaterState,
  inspectMagnetHandler,
  installDownloadedUpdate,
  launchCustomExternalPlayer,
  launchNativeMpv,
  listPlaybackProgress,
  openExternalUrl,
  pickExternalPlayerExe,
  pickLocalMediaFile,
  postponeUpdateInstall,
  recordActivity,
  recordRecentlyViewed,
  resolvePlaybackSource,
  saveTextFile,
  searchCatalog,
  sendDesktopNotification,
  subscribeDesktopNavigation,
  subscribeDownloadsUpdated,
  subscribeUpdaterStatus,
  updateDesktopSettings
} from './services/desktopApi.js';
import { authConfigured, supabase } from './services/supabase.js';
import { torrentio } from './services/torrentio.js';
import { geminiApi } from './services/gemini.js';
import { useAuthController } from './auth/useAuthController.js';
import { usePersonalLibrary } from './hooks/usePersonalLibrary.js';
import { createSearchCoordinator } from './utils/searchCoordinator.js';
import {
  createAutocompleteCache,
  filterResultsByMediaType,
  normalizeMediaType,
  normalizeSearchQueryText
} from './utils/searchSession.js';
import { createScanCoordinator } from './utils/scanCoordinator.js';
import {
  getEpisodeReleaseFields,
  getReleaseIdentity,
  restoreEpisodeRecommendation,
  selectEpisodeRelease
} from './utils/releaseSelection.js';
import { getNoSourceCopy } from './utils/resolutionDisplay.js';
import { toGeminiCandidate } from './utils/geminiCandidates.js';
import { copyTextToClipboard } from './utils/clipboard.js';

import { TitleBar } from './components/TitleBar.jsx';
import { NavRail } from './components/NavRail.jsx';
import { DiscoverView } from './components/DiscoverView.jsx';
import { SearchView } from './components/SearchView.jsx';
import { TitleDetailView } from './components/TitleDetailView.jsx';
import { LibraryView } from './components/LibraryView.jsx';
import { DownloadsView } from './components/DownloadsView.jsx';
import { ActivityView } from './components/ActivityView.jsx';
import { SettingsView } from './components/SettingsView.jsx';
import { PlayerOverlay } from './components/PlayerOverlay.jsx';
import { UpdateBanner } from './components/UpdateBanner.jsx';
import { AuthModal } from './components/AuthModal.jsx';
import { ContextMenu } from './components/ContextMenu.jsx';

const DISCOVER_CATEGORIES = [
  'popular_series',
  'popular_movies',
  'anime',
  'documentaries',
  'reality'
];

function getEpisodeNumber(row, index) {
  return Number.isInteger(row?.number) && row.number > 0 ? row.number : index + 1;
}

export default function App() {
  // Desktop navigation stack (supports Back/Forward arrows + Alt+Left/Right)
  const [navStack, setNavStack] = useState([{ tab: 'discover', selectedItem: null, settingsSection: 'general' }]);
  const [navIndex, setNavIndex] = useState(0);
  const currentView = navStack[navIndex] || { tab: 'discover', selectedItem: null, settingsSection: 'general' };

  const [appInfo, setAppInfo] = useState(null);
  const [settings, setSettings] = useState(null);
  const [updater, setUpdater] = useState(null);
  const [magnetHandler, setMagnetHandler] = useState(null);
  const [playerBackend, setPlayerBackend] = useState(null);
  const [streamingStatus, setStreamingStatus] = useState(null);
  const [offlineDownloads, setOfflineDownloads] = useState([]);
  const [playbackProgress, setPlaybackProgress] = useState([]);
  const [health, setHealth] = useState({ status: 'checking', latencyMs: null, error: null });
  const [isOnline, setIsOnline] = useState(() => (typeof navigator !== 'undefined' ? navigator.onLine : true));
  const [geminiAvailable, setGeminiAvailable] = useState(false);

  // Discover real Cinemeta catalogs
  const [catalogs, setCatalogs] = useState({});
  const [catalogLoading, setCatalogLoading] = useState({});
  const [catalogErrors, setCatalogErrors] = useState({});

  // Search state
  const [mediaType, setMediaType] = useState('series');
  const [query, setQuery] = useState('');
  const [suggestions, setSuggestions] = useState([]);
  const [suggestionsLoading, setSuggestionsLoading] = useState(false);
  const [results, setResults] = useState([]);
  const [resolution, setResolution] = useState(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState('');
  const [geminiQueries, setGeminiQueries] = useState([]);
  const [geminiAssistLoading, setGeminiAssistLoading] = useState(false);

  // Title detail & Torrentio release intelligence state
  const [selectedItem, setSelectedItem] = useState(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState('');
  const [season, setSeason] = useState(1);
  const [episodes, setEpisodes] = useState([]);
  const [seasonIntegrity, setSeasonIntegrity] = useState(null);
  const [movieRelease, setMovieRelease] = useState(null);
  const [checkingSources, setCheckingSources] = useState(false);
  const [providerMode, setProviderMode] = useState('ready');
  const [expandedCompare, setExpandedCompare] = useState({});
  const [geminiExplain, setGeminiExplain] = useState({});
  const [geminiExplainLoading, setGeminiExplainLoading] = useState({});

  // Ranking & filter controls
  const [rankingPreference, setRankingPreference] = useState('smallest-acceptable');
  const [audioPreference, setAudioPreference] = useState('any');
  const [resolutionPreference, setResolutionPreference] = useState('1080p');
  const [showAdvancedFilters, setShowAdvancedFilters] = useState(false);
  const [minSeeders, setMinSeeders] = useState('');
  const [maxSizeGb, setMaxSizeGb] = useState('');
  const [codecFilter, setCodecFilter] = useState('');
  const [hdrFilter, setHdrFilter] = useState('');

  // Player, modals, context menu, and feedback
  const [playerSession, setPlayerSession] = useState(null);
  const [contextMenu, setContextMenu] = useState(null);
  const [showAuthModal, setShowAuthModal] = useState(false);
  const [authContextNote, setAuthContextNote] = useState('');
  const [settingsNotice, setSettingsNotice] = useState('');
  const [toast, setToast] = useState('');
  const [globalNotice, setGlobalNotice] = useState(null);

  const autocompleteCoordinator = useMemo(() => createSearchCoordinator(), []);
  const autocompleteCache = useMemo(() => createAutocompleteCache(), []);
  const scanCoordinator = useMemo(
    () => createScanCoordinator({ onCheckingChange: setCheckingSources }),
    []
  );
  const searchInputRef = useRef(null);
  const toastTimerRef = useRef(null);
  const detailRequestRef = useRef(0);

  const showToast = useCallback(message => {
    setToast(message);
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    toastTimerRef.current = setTimeout(() => {
      setToast('');
      toastTimerRef.current = null;
    }, 2800);
  }, []);

  const showNotice = useCallback((message, { hint = '', magnetFallback = '', showDefaultApps = false } = {}) => {
    if (!message) {
      setGlobalNotice(null);
      return;
    }
    setGlobalNotice({ message, hint, magnetFallback, showDefaultApps });
  }, []);

  const pushNav = useCallback(entry => {
    setNavStack(prev => {
      const sliced = prev.slice(0, navIndex + 1);
      const next = [...sliced, { tab: 'discover', selectedItem: null, settingsSection: 'general', ...entry }];
      return next.slice(-25);
    });
    setNavIndex(prev => Math.min(prev + 1, 24));
  }, [navIndex]);

  const navigateBack = useCallback(() => {
    if (navIndex <= 0) return;
    const prevEntry = navStack[navIndex - 1];
    setNavIndex(navIndex - 1);
    setSelectedItem(prevEntry?.selectedItem || null);
  }, [navIndex, navStack]);

  const navigateForward = useCallback(() => {
    if (navIndex >= navStack.length - 1) return;
    const nextEntry = navStack[navIndex + 1];
    setNavIndex(navIndex + 1);
    setSelectedItem(nextEntry?.selectedItem || null);
  }, [navIndex, navStack]);

  const selectTab = useCallback((tab, extra = {}) => {
    setSelectedItem(null);
    pushNav({ tab, selectedItem: null, ...extra });
  }, [pushNav]);

  const auth = useAuthController({
    supabase,
    onToast: showToast,
    onNotice: message => showNotice(message)
  });

  const openSignInPrompt = useCallback(snapshot => {
    if (!supabase) {
      showNotice('Supabase authentication is not configured for this build.');
      return;
    }
    setAuthContextNote(
      snapshot?.payload?.title
        ? `Sign in to save "${snapshot.payload.title}" to your synced Personal Library.`
        : ''
    );
    setShowAuthModal(true);
  }, [showNotice]);

  const library = usePersonalLibrary({
    supabase,
    user: auth.authenticatedUser,
    suspended: auth.suspended,
    onRequireAuth: openSignInPrompt,
    onSessionExpired: auth.handleSessionExpired,
    onToast: showToast,
    onNotice: message => showNotice(message)
  });

  const loadCatalogCategory = useCallback(async (category, forceRefresh = false) => {
    setCatalogLoading(prev => ({ ...prev, [category]: true }));
    setCatalogErrors(prev => ({ ...prev, [category]: '' }));
    const res = await fetchDiscoverCatalog(category, forceRefresh);
    setCatalogLoading(prev => ({ ...prev, [category]: false }));
    if (res.ok && res.data) {
      setCatalogs(prev => ({ ...prev, [category]: res.data }));
    } else {
      setCatalogErrors(prev => ({ ...prev, [category]: res.error || 'Could not load catalog.' }));
    }
  }, []);

  const refreshAllCatalogs = useCallback((forceRefresh = false) => {
    DISCOVER_CATEGORIES.forEach(cat => loadCatalogCategory(cat, forceRefresh));
  }, [loadCatalogCategory]);

  const refreshHealth = useCallback(async () => {
    setHealth(prev => ({ ...prev, status: 'checking', error: null }));
    const res = await fetchBackendHealth();
    if (res.ok) {
      setHealth({ status: 'online', latencyMs: res.latencyMs, error: null, payload: res.data });
      if (res.data?.gemini === 'configured') setGeminiAvailable(true);
    } else {
      setHealth({
        status: 'offline',
        latencyMs: res.latencyMs,
        error: res.error || 'SeasonFetch backend is unreachable.'
      });
    }
  }, []);

  const refreshPlayerStatus = useCallback(async () => {
    const [backend, streaming, progress] = await Promise.all([
      getPlayerBackendInfo(),
      getStreamingStatus(),
      listPlaybackProgress()
    ]);
    setPlayerBackend(backend);
    setStreamingStatus(streaming);
    setPlaybackProgress(Array.isArray(progress) ? progress : []);
  }, []);

  const logActivity = useCallback(async entry => {
    const updated = await recordActivity(entry);
    if (updated) setSettings(updated);
  }, []);

  useEffect(() => {
    let active = true;
    Promise.all([
      getAppInfo(),
      getDesktopSettings(),
      getUpdaterState(),
      geminiApi.getStatus(),
      inspectMagnetHandler(),
      getPlayerBackendInfo(),
      getStreamingStatus(),
      listPlaybackProgress(),
      downloadsList()
    ]).then(([info, loadedSettings, updaterState, geminiStatus, handlerInfo, pBackend, sStatus, progressList, dlList]) => {
      if (!active) return;
      setAppInfo(info);
      setSettings(loadedSettings);
      setUpdater(updaterState);
      setGeminiAvailable(Boolean(geminiStatus?.configured));
      setMagnetHandler(handlerInfo);
      setPlayerBackend(pBackend);
      setStreamingStatus(sStatus);
      setPlaybackProgress(Array.isArray(progressList) ? progressList : []);
      setOfflineDownloads(Array.isArray(dlList) ? dlList : []);

      if (loadedSettings?.defaultSearchMode) setMediaType(normalizeMediaType(loadedSettings.defaultSearchMode));
      if (loadedSettings?.rankingPreference) setRankingPreference(normalizeRankingPreference(loadedSettings.rankingPreference));
      if (loadedSettings?.audioPreference) setAudioPreference(loadedSettings.audioPreference);
      if (loadedSettings?.resolutionPreference) setResolutionPreference(loadedSettings.resolutionPreference);
    });

    refreshHealth();
    refreshAllCatalogs(false);

    const unsubUpdater = subscribeUpdaterStatus(next => {
      setUpdater(prev => (typeof next === 'function' ? next(prev) : next));
    });
    const unsubDownloads = subscribeDownloadsUpdated(list => {
      if (Array.isArray(list)) setOfflineDownloads(list);
    });
    const unsubNav = subscribeDesktopNavigation(payload => {
      if (payload?.tab) selectTab(payload.tab, { settingsSection: payload.section || 'general' });
      if (payload?.focusSearch) setTimeout(() => searchInputRef.current?.focus(), 30);
      if (payload?.diagPlay?.target) {
        const dp = payload.diagPlay;
        setSettings(prev => {
          if (!prev) return prev;
          const nextPlayer = { ...(prev.player || {}) };
          if (dp.engine) nextPlayer.preferredEngine = dp.engine;
          if (typeof dp.hwdec === 'boolean') nextPlayer.hardwareDecoding = dp.hwdec;
          return { ...prev, player: nextPlayer };
        });
        setPlayerSession({
          mediaKey: `diag:${dp.target}`,
          title: dp.target.split(/[\\/]/).pop() || 'Diagnostic Playback',
          subtitle: `Diagnostic (${dp.engine || 'embedded-libmpv'})`,
          streamUrl: dp.target,
          rawPath: dp.target,
          resumeSecs: 0,
          subtitleFile: dp.subtitleFile || null
        });
      }
      if (payload?.diagMagnet?.magnetUri) {
        const dm = payload.diagMagnet;
        if (dm.engine) {
          setSettings(prev => {
            if (!prev) return prev;
            return { ...prev, player: { ...(prev.player || {}), preferredEngine: dm.engine } };
          });
        }
        setPlayerSession({
          mediaKey: `diag-magnet:${dm.title || 'stream'}`,
          title: dm.title || 'Diagnostic Torrent Stream',
          subtitle: dm.engine ? `SeasonFetch Embedded Torrent Engine (${dm.engine})` : 'SeasonFetch Embedded Torrent Engine',
          streamUrl: null,
          rawPath: null,
          magnetUri: dm.magnetUri,
          torrentRequest: {
            title: dm.title || 'Diagnostic Torrent Stream',
            magnetUri: dm.magnetUri,
            infoHash: null,
            fileIdx: Number.isInteger(dm.fileIdx) ? dm.fileIdx : null,
            season: null,
            episode: null,
            trackers: []
          },
          resumeSecs: 0
        });
      }
    });

    const handleOnline = () => {
      setIsOnline(true);
      refreshHealth();
      showToast('Back online — reconnected to SeasonFetch');
    };
    const handleOffline = () => setIsOnline(false);

    const handleKeyDown = event => {
      if (playerSession) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchInputRef.current?.focus();
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'l') {
        event.preventDefault();
        selectTab('library');
      } else if ((event.ctrlKey || event.metaKey) && event.key === ',') {
        event.preventDefault();
        selectTab('settings');
      } else if (event.altKey && event.key === 'ArrowLeft') {
        event.preventDefault();
        navigateBack();
      } else if (event.altKey && event.key === 'ArrowRight') {
        event.preventDefault();
        navigateForward();
      }
    };

    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);
    window.addEventListener('keydown', handleKeyDown);

    return () => {
      active = false;
      unsubUpdater();
      unsubDownloads();
      unsubNav();
      autocompleteCoordinator.invalidate();
      scanCoordinator.invalidate();
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [autocompleteCoordinator, navigateBack, navigateForward, playerSession, refreshAllCatalogs, refreshHealth, scanCoordinator, selectTab, showToast]);

  const handleSettingsPatch = useCallback(async (patch, { silent = false } = {}) => {
    if (!silent) setSettingsNotice('');
    const result = await updateDesktopSettings(patch);
    if (result?.ok) {
      setSettings(result.settings);
      if (patch.rankingPreference) setRankingPreference(normalizeRankingPreference(patch.rankingPreference));
      if (patch.audioPreference) setAudioPreference(patch.audioPreference);
      if (patch.resolutionPreference) setResolutionPreference(patch.resolutionPreference);
      if (patch.player) {
        getPlayerBackendInfo().then(setPlayerBackend).catch(() => {});
      }
      if (!silent) {
        setSettingsNotice('Preferences saved.');
        setTimeout(() => setSettingsNotice(''), 2200);
      }
    } else if (!silent) {
      setSettingsNotice(result?.error || 'Could not save desktop preferences.');
    }
  }, []);

  const activeFiltersPayload = useMemo(() => {
    const filters = { resolution: resolutionPreference };
    if (minSeeders !== '' && Number.isInteger(Number(minSeeders)) && Number(minSeeders) >= 0) {
      filters.minSeeders = Number(minSeeders);
    }
    if (maxSizeGb !== '' && Number.isFinite(Number(maxSizeGb)) && Number(maxSizeGb) > 0) {
      filters.maxSizeGb = Number(maxSizeGb);
    }
    if (codecFilter) filters.codec = codecFilter;
    if (hdrFilter) filters.hdr = hdrFilter;
    return filters;
  }, [resolutionPreference, minSeeders, maxSizeGb, codecFilter, hdrFilter]);

  const handleQueryChange = useCallback(nextValue => {
    setQuery(nextValue);
    setSearchError('');
    setGeminiQueries([]);
    const clean = normalizeSearchQueryText(nextValue);
    if (clean.length < 2) {
      autocompleteCoordinator.invalidate();
      setSuggestions([]);
      setSuggestionsLoading(false);
      return;
    }

    const cached = autocompleteCache.getReusable(clean, mediaType);
    if (cached) {
      setSuggestions(cached.results.slice(0, 6));
      setSuggestionsLoading(false);
      return;
    }

    setSuggestionsLoading(true);
    autocompleteCoordinator.schedule(async requestId => {
      const response = await searchCatalog(clean, mediaType, 'autocomplete');
      if (!autocompleteCoordinator.isCurrent(requestId)) return;
      setSuggestionsLoading(false);
      if (!response.ok) {
        setSuggestions([]);
        return;
      }
      const filtered = filterResultsByMediaType(response.data?.results, mediaType);
      autocompleteCache.set(clean, mediaType, response.data);
      setSuggestions(filtered.slice(0, 6));
    }, 240);
  }, [autocompleteCache, autocompleteCoordinator, mediaType]);

  const runFullSearch = useCallback(async (overrideQuery, overrideMediaType) => {
    const activeMode = normalizeMediaType(overrideMediaType || mediaType);
    const clean = normalizeSearchQueryText(overrideQuery ?? query);
    if (clean.length < 2) {
      setSearchError('Enter at least 2 characters or an IMDb ID (for example tt0903747).');
      return;
    }

    autocompleteCoordinator.invalidate();
    scanCoordinator.invalidate();
    setMediaType(activeMode);
    setQuery(clean);
    setSelectedItem(null);
    pushNav({ tab: 'search', selectedItem: null });
    setSuggestions([]);
    setSuggestionsLoading(false);
    setSearchError('');
    setSearching(true);

    const response = await searchCatalog(clean, activeMode, 'full');
    setSearching(false);

    if (!response.ok) {
      setResults([]);
      setResolution(null);
      setSearchError(response.error || 'Search failed.');
      return;
    }

    const currentRecent = Array.isArray(settings?.recentSearches) ? settings.recentSearches : [];
    const nextRecent = [
      { query: clean, mediaType: activeMode, timestamp: new Date().toISOString() },
      ...currentRecent.filter(
        e => !(e.mediaType === activeMode && e.query.toLowerCase() === clean.toLowerCase())
      )
    ].slice(0, 12);
    handleSettingsPatch({ recentSearches: nextRecent }, { silent: true });
    logActivity({
      kind: 'search',
      title: clean,
      subtitle: activeMode === 'movie' ? 'Movies search' : 'Series search',
      mediaType: activeMode
    });

    const filtered = filterResultsByMediaType(response.data?.results, activeMode);
    setResults(filtered);
    setResolution(response.data?.resolution || null);
    if (filtered.length === 0) {
      setSearchError(`No ${activeMode === 'movie' ? 'movies' : 'series'} found for "${clean}".`);
    }
  }, [autocompleteCoordinator, handleSettingsPatch, logActivity, mediaType, pushNav, query, scanCoordinator, settings?.recentSearches]);

  const scanMovieSources = useCallback((movie, overrideFilters) => {
    const movieId = movie?.imdbId || movie?.id;
    if (!movieId || !/^tt\d+$/.test(movieId)) return;

    const usedFilters = overrideFilters || activeFiltersPayload;
    setMovieRelease(null);
    setExpandedCompare({});
    setGeminiExplain({});
    setProviderMode('ready');

    scanCoordinator.start({
      delayMs: 220,
      request: () => torrentio.getMovieSources(movieId, rankingPreference, audioPreference, usedFilters),
      onSuccess: data => {
        const recommended = data?.selected || null;
        const candidates = Array.isArray(data?.sources) ? [...data.sources] : [];
        const recommendedId = getReleaseIdentity(recommended);
        if (recommended && recommendedId && !candidates.some(c => getReleaseIdentity(c) === recommendedId)) {
          candidates.unshift(recommended);
        }
        const fields = recommended ? getEpisodeReleaseFields(recommended) : null;
        const sourceError = recommended && !fields?.magnet
          ? 'Torrentio returned a recommended release without a valid matching magnet.'
          : data?.status === 'unavailable'
            ? getNoSourceCopy(usedFilters.resolution)
            : '';

        setMovieRelease({
          id: `movie-${movieId}`,
          title: movie.title,
          ...(fields || {
            status: data?.status === 'size-unknown' ? 'size-unknown' : 'unavailable',
            size: '—',
            format: '',
            hash: '',
            magnet: '',
            rawMagnet: '',
            sourceUrl: '',
            provider: '',
            resolution: null,
            seeders: null,
            audio: null,
            audioEvidence: 'unverified',
            releaseType: null,
            releaseGroup: null
          }),
          status: fields?.status || (data?.status === 'size-unknown' ? 'size-unknown' : 'unavailable'),
          selectionReason: data?.selectionReason || '',
          intelligence: data?.intelligence || null,
          sourceCount: candidates.length,
          sourceError,
          candidates,
          recommendedSource: recommended,
          recommendedSelectionId: recommendedId,
          activeSelectionId: recommendedId,
          manualSelectionId: '',
          recommendedSelectionReason: data?.selectionReason || ''
        });
        setProviderMode('live');
        showToast(`${movie.title} Torrentio lookup complete`);
      },
      onError: error => {
        setMovieRelease({
          id: `movie-${movieId}`,
          title: movie.title,
          status: 'provider-error',
          size: '—',
          format: '',
          hash: '',
          magnet: '',
          candidates: [],
          sourceCount: 0,
          sourceError: error.message || 'Torrentio could not retrieve movie releases.'
        });
        setProviderMode('error');
      }
    });
  }, [activeFiltersPayload, audioPreference, rankingPreference, scanCoordinator, showToast]);

  const handleSelectTitle = useCallback(async (item, forcedMediaType) => {
    const imdbId = item?.imdbId || item?.id;
    if (!imdbId) return;

    const itemType = normalizeMediaType(forcedMediaType || item.mediaType || mediaType);
    const requestId = ++detailRequestRef.current;

    autocompleteCoordinator.invalidate();
    scanCoordinator.invalidate();
    setMediaType(itemType);
    setSuggestions([]);
    setEpisodes([]);
    setSeasonIntegrity(null);
    setMovieRelease(null);
    setExpandedCompare({});
    setGeminiExplain({});
    setProviderMode('ready');
    setDetailError('');
    setDetailLoading(true);

    const baseItem = {
      ...item,
      id: imdbId,
      imdbId,
      mediaType: itemType,
      poster: item.poster || item.posterUrl || null,
      background: item.background || null,
      year: item.year || item.releaseYear || null,
      imdbRating: item.imdbRating || item.rating || null
    };
    setSelectedItem(baseItem);
    pushNav({ tab: 'detail', selectedItem: baseItem });

    recordRecentlyViewed(baseItem).then(updated => {
      if (updated) setSettings(updated);
    }).catch(() => {});
    logActivity({
      kind: 'opened_title',
      title: baseItem.title,
      subtitle: `${itemType === 'movie' ? 'Movie' : 'Series'} · ${imdbId}`,
      imdbId,
      mediaType: itemType
    });

    const response = await fetchMediaDetail(imdbId, itemType);
    if (requestId !== detailRequestRef.current) return;
    setDetailLoading(false);

    if (!response.ok) {
      setDetailError(response.error || 'Could not load full metadata from SeasonFetch.');
      return;
    }

    if (itemType === 'movie') {
      const movieMeta = response.data?.movie || response.data || {};
      const fullMovie = { ...baseItem, ...movieMeta, id: imdbId, imdbId, mediaType: 'movie' };
      setSelectedItem(fullMovie);
      scanMovieSources(fullMovie);
      return;
    }

    const seriesMeta = response.data?.series || {};
    const remoteEpisodes = Object.fromEntries(
      Object.entries(response.data?.episodes || {}).map(([seasonKey, list]) => [
        seasonKey,
        (Array.isArray(list) ? list : []).map((ep, idx) => ({
          title: ep.title || `Episode ${idx + 1}`,
          number: getEpisodeNumber(ep, idx)
        }))
      ])
    );
    const seasonNumbers = Object.keys(remoteEpisodes)
      .map(Number)
      .filter(Number.isInteger)
      .sort((a, b) => a - b);

    const initialSeason = seasonNumbers.includes(1) ? 1 : (seasonNumbers[0] ?? 1);
    setSeason(initialSeason);
    setSelectedItem({
      ...baseItem,
      ...seriesMeta,
      id: imdbId,
      imdbId,
      mediaType: 'series',
      episodesBySeason: remoteEpisodes,
      seasonNumbers: seasonNumbers.length ? seasonNumbers : [1],
      seasons: seasonNumbers.length || seriesMeta.seasons || 1
    });
  }, [autocompleteCoordinator, logActivity, mediaType, pushNav, scanCoordinator, scanMovieSources]);

  const handleCheckSeasonSources = useCallback(() => {
    if (!selectedItem || selectedItem.mediaType !== 'series') return;
    const baseRows = selectedItem.episodesBySeason?.[season] || [];
    if (!baseRows.length) {
      showNotice(`No episode metadata is available for ${season === 0 ? 'Specials' : `Season ${season}`}.`);
      return;
    }

    const episodeNumbers = baseRows.map(getEpisodeNumber);
    setEpisodes([]);
    setSeasonIntegrity(null);
    setExpandedCompare({});
    setGeminiExplain({});
    setProviderMode('ready');

    scanCoordinator.start({
      delayMs: 220,
      request: () => torrentio.getSeasonSources(
        selectedItem.imdbId,
        season,
        episodeNumbers,
        rankingPreference,
        audioPreference,
        activeFiltersPayload
      ),
      onSuccess: data => {
        const providerResults = Array.isArray(data?.results) ? data.results : [];
        const mapped = baseRows.map((row, index) => {
          const epNum = episodeNumbers[index];
          const provider = providerResults.find(entry => entry.episode === epNum);
          const source = provider?.selected || null;
          const status = provider?.status === 'error' ? 'provider-error' : (provider?.status || 'unavailable');
          const fields = source ? getEpisodeReleaseFields(source) : null;
          const candidates = Array.isArray(provider?.sources) ? provider.sources : [];
          const selectionId = getReleaseIdentity(source);

          return {
            id: `${selectedItem.imdbId}-${season}-${epNum}`,
            number: epNum,
            title: row.title || `Episode ${epNum}`,
            ...(fields || {}),
            size: fields?.size || (status === 'unavailable' ? '—' : 'Unknown'),
            status: fields?.status || status,
            selectionReason: provider?.selectionReason || '',
            intelligence: provider?.intelligence || null,
            sourceCount: candidates.length,
            sourceError: provider?.error || (source && !fields?.magnet ? 'Selected source lacks a valid matching magnet.' : ''),
            candidates,
            recommendedSource: source,
            recommendedSelectionId: selectionId,
            activeSelectionId: selectionId,
            manualSelectionId: '',
            recommendedSelectionReason: provider?.selectionReason || ''
          };
        });

        setEpisodes(mapped);
        setSeasonIntegrity(data?.integrity || null);
        setProviderMode(data?.status === 'partial' ? 'partial' : data?.status === 'error' ? 'error' : 'live');
        showToast(`Season ${season} scan complete`);
        if (document.hidden) {
          sendDesktopNotification(
            `${selectedItem.title} — Season ${season} Ready`,
            `${mapped.filter(e => e.magnet).length}/${mapped.length} episodes matched your profile.`
          ).catch(() => {});
        }
      },
      onError: error => {
        setProviderMode('error');
        showNotice(error.message || 'Torrentio season scan failed. Try again shortly.');
      }
    });
  }, [activeFiltersPayload, audioPreference, rankingPreference, scanCoordinator, season, selectedItem, showNotice, showToast]);

  async function handleOpenMagnet(magnet, label = 'Magnet release') {
    if (!magnet) return;
    const result = await openExternalUrl(magnet);
    if (result?.ok) {
      showNotice(null);
      showToast('Handed magnet URI off to Windows default client');
      logActivity({ kind: 'magnet_open', title: label, subtitle: 'Opened in OS magnet client' });
    } else {
      showNotice(result?.error || 'Could not open magnet URI.', {
        hint: result?.hint || 'Check Windows Default Apps or copy the magnet manually.',
        magnetFallback: magnet,
        showDefaultApps: true
      });
    }
  }

  async function handleCopyMagnet(magnet, successLabel) {
    try {
      await copyTextToClipboard(magnet);
      showNotice(null);
      showToast(successLabel);
      logActivity({ kind: 'magnet_copy', title: successLabel, subtitle: 'Copied to clipboard' });
    } catch (error) {
      showNotice(error.message || 'Unable to copy to clipboard.');
    }
  }

  async function handleOpenLocalVideo() {
    const picked = await pickLocalMediaFile();
    if (!picked?.ok || !picked.path) return;
    const mediaKey = `file:${picked.name}`;
    const existing = await getPlaybackProgress(mediaKey);

    if (settings?.player?.preferredEngine === 'external-player') {
      const extRes = await launchCustomExternalPlayer({
        target: picked.path,
        title: picked.name,
        startSecs: existing?.positionSecs || 0,
        executablePath: settings?.player?.externalPlayerPath || null
      });
      if (extRes?.ok) {
        showToast(`Playing ${picked.name} in ${extRes.engine}`);
        logActivity({ kind: 'playback', title: picked.name, subtitle: `Launched in ${extRes.engine}` });
        return;
      }
      showNotice(extRes?.error || 'Could not launch external player; falling back to built-in player.');
    }

    if (settings?.player?.preferredEngine === 'mpv' && playerBackend?.mpvAvailable) {
      const mpvRes = await launchNativeMpv({
        target: picked.path,
        title: picked.name,
        startSecs: existing?.positionSecs || 0
      });
      if (mpvRes?.ok) {
        showToast(`Playing ${picked.name} in native MPV`);
        logActivity({ kind: 'playback', title: picked.name, subtitle: 'Launched in Native MPV' });
        return;
      }
    }

    setPlayerSession({
      sourceKind: 'localFile',
      mediaKey,
      title: picked.name,
      subtitle: 'Local media file',
      releaseName: picked.name,
      streamUrl: picked.path,
      rawPath: picked.path,
      resumeSecs: existing?.positionSecs || 0
    });
    logActivity({ kind: 'playback', title: picked.name, subtitle: 'Local file playback' });
  }

  async function handleStartOfflineDownload(releaseItem, { isMovie = false } = {}) {
    const label = isMovie
      ? selectedItem?.title || releaseItem.title
      : `${selectedItem?.title || ''} · S${season}E${releaseItem.number} — ${releaseItem.title}`;
    try {
      await downloadsStart({
        title: label,
        magnetUri: releaseItem.magnet || null,
        infoHash: releaseItem.hash || null,
        fileIdx: Number.isInteger(releaseItem.fileIdx) ? releaseItem.fileIdx : null,
        poster: selectedItem?.poster || null,
        season: isMovie ? null : Number(season) || null,
        episode: isMovie ? null : Number(releaseItem.number) || null,
        trackers: []
      });
      showToast(`Saving "${label}" for offline playback`);
      logActivity({ kind: 'download', title: label, subtitle: 'Started offline download' });
    } catch (err) {
      showNotice(typeof err === 'string' ? err : err?.message || 'Could not start offline download.');
    }
  }

  async function handlePlayDownloadedEntry(entry) {
    if (!entry?.localPath) return;
    const mediaKey = `offline:${entry.id}`;
    const existing = await getPlaybackProgress(mediaKey);
    setPlayerSession({
      sourceKind: 'downloaded',
      mediaKey,
      title: entry.title,
      subtitle: 'Downloaded for offline playback',
      releaseName: entry.fileName || entry.title,
      streamUrl: entry.localPath,
      rawPath: entry.localPath,
      season: entry.season ?? null,
      episode: entry.episode ?? null,
      resumeSecs: existing?.positionSecs || 0
    });
    logActivity({ kind: 'playback', title: entry.title, subtitle: 'Offline local playback' });
  }

  async function handlePlayRelease(releaseItem, { isMovie = false, nextEpisode = null } = {}) {
    const label = isMovie
      ? selectedItem?.title || releaseItem.title
      : `${selectedItem?.title || ''} · S${season}E${releaseItem.number} — ${releaseItem.title}`;

    const mediaKey = isMovie
      ? `movie:${selectedItem?.imdbId}`
      : `series:${selectedItem?.imdbId}:${season}:${releaseItem.number}`;
    const existing = await getPlaybackProgress(mediaKey);

    // Play immediately from local disk if already downloaded for offline viewing
    const completedOffline = offlineDownloads.find(
      d =>
        d.status === 'completed' &&
        d.localPath &&
        ((releaseItem.hash && d.infoHash?.toLowerCase() === releaseItem.hash.toLowerCase() && (d.fileIdx == null || releaseItem.fileIdx == null || d.fileIdx === releaseItem.fileIdx)) ||
          (!isMovie && d.season === Number(season) && d.episode === Number(releaseItem.number) && d.title?.includes(selectedItem?.title || '')))
    );
    if (completedOffline) {
      setPlayerSession({
        sourceKind: 'downloaded',
        mediaKey,
        title: selectedItem?.title || releaseItem.title,
        subtitle: isMovie
          ? 'Downloaded for offline playback'
          : `Season ${season} · Episode ${releaseItem.number} — ${releaseItem.title}`,
        releaseName: completedOffline.fileName || releaseItem.format || releaseItem.title || label,
        streamUrl: completedOffline.localPath,
        rawPath: completedOffline.localPath,
        imdbId: selectedItem?.imdbId,
        mediaType: isMovie ? 'movie' : 'series',
        season: isMovie ? null : season,
        episode: isMovie ? null : releaseItem.number,
        resumeSecs: existing?.positionSecs || 0,
        nextEpisode
      });
      logActivity({ kind: 'playback', title: label, subtitle: 'Offline local playback' });
      return;
    }

    const resolved = await resolvePlaybackSource({
      title: label,
      directUrl: releaseItem.sourceUrl?.startsWith('http') ? releaseItem.sourceUrl : null,
      localFilePath: null,
      magnetUri: releaseItem.magnet || null,
      infoHash: releaseItem.hash || null,
      fileIdx: Number.isInteger(releaseItem.fileIdx) ? releaseItem.fileIdx : null
    });

    const hasTorrentTarget = Boolean(
      resolved?.magnetUri || resolved?.infoHash || releaseItem.magnet || releaseItem.hash
    );

    // Clicking Play ALWAYS streams inside SeasonFetch via the embedded torrent engine
    // (never ambushing the user with an external download-client popup).
    if (resolved?.kind === 'embeddedTorrent' || (!resolved?.streamUrl && hasTorrentTarget)) {
      setPlayerSession({
        sourceKind: 'embeddedTorrent',
        mediaKey,
        title: selectedItem?.title || releaseItem.title,
        subtitle: isMovie
          ? releaseItem.format || 'Embedded Torrent Stream'
          : `Season ${season} · Episode ${releaseItem.number} — ${releaseItem.title}`,
        releaseName: releaseItem.format || releaseItem.title || label,
        streamUrl: null,
        rawPath: null,
        magnetUri: resolved?.magnetUri || releaseItem.magnet || null,
        torrentRequest: {
          title: label,
          magnetUri: resolved?.magnetUri || releaseItem.magnet || null,
          infoHash: resolved?.infoHash || releaseItem.hash || null,
          fileIdx: Number.isInteger(releaseItem.fileIdx) ? releaseItem.fileIdx : null,
          season: isMovie ? null : Number(season) || null,
          episode: isMovie ? null : Number(releaseItem.number) || null,
          trackers: []
        },
        imdbId: selectedItem?.imdbId,
        mediaType: isMovie ? 'movie' : 'series',
        season: isMovie ? null : season,
        episode: isMovie ? null : releaseItem.number,
        resumeSecs: existing?.positionSecs || 0,
        nextEpisode
      });
      logActivity({
        kind: 'playback',
        title: label,
        subtitle: 'Streaming via SeasonFetch Embedded Torrent Engine'
      });
      return;
    }

    if (!resolved?.streamUrl) {
      showNotice(resolved?.reason || 'No playable media source was found for this release.');
      return;
    }

    if (settings?.player?.preferredEngine === 'external-player') {
      const extRes = await launchCustomExternalPlayer({
        target: resolved.streamUrl,
        title: label,
        startSecs: existing?.positionSecs || 0,
        executablePath: settings?.player?.externalPlayerPath || null
      });
      if (extRes?.ok) {
        showToast(`Playing ${label} in ${extRes.engine}`);
        logActivity({ kind: 'playback', title: label, subtitle: `Launched in ${extRes.engine}` });
        return;
      }
      showNotice(extRes?.error || 'Could not launch external player; falling back to built-in player.');
    }

    if (settings?.player?.preferredEngine === 'mpv' && playerBackend?.mpvAvailable) {
      const res = await launchNativeMpv({
        target: resolved.streamUrl,
        title: label,
        startSecs: existing?.positionSecs || 0
      });
      if (res?.ok) {
        showToast('Launched stream in native MPV');
        logActivity({ kind: 'playback', title: label, subtitle: 'Streamed in Native MPV' });
        return;
      }
    }

    setPlayerSession({
      sourceKind: 'directHttp',
      mediaKey,
      title: selectedItem?.title || releaseItem.title,
      subtitle: isMovie ? releaseItem.format : `Season ${season} · Episode ${releaseItem.number} — ${releaseItem.title}`,
      releaseName: releaseItem.format || releaseItem.title || label,
      streamUrl: resolved.streamUrl,
      rawPath: null,
      magnetUri: resolved.magnetUri || releaseItem.magnet || null,
      imdbId: selectedItem?.imdbId,
      mediaType: isMovie ? 'movie' : 'series',
      season: isMovie ? null : season,
      episode: isMovie ? null : releaseItem.number,
      resumeSecs: existing?.positionSecs || 0,
      nextEpisode
    });
    logActivity({ kind: 'playback', title: label, subtitle: resolved.subtitleNote || 'In-app stream playback' });
  }

  async function handleExplainWithGemini(targetItem, { isMovie = false } = {}) {
    if (!targetItem?.id) return;
    setGeminiExplainLoading(prev => ({ ...prev, [targetItem.id]: true }));
    setGeminiExplain(prev => ({ ...prev, [targetItem.id]: null }));
    const candidates = (targetItem.candidates?.length ? targetItem.candidates : [targetItem]).map(toGeminiCandidate);
    try {
      const data = await geminiApi.explainSource({
        seriesTitle: selectedItem?.title || targetItem.title,
        season: isMovie ? undefined : season,
        episode: isMovie ? undefined : targetItem.number,
        candidates
      });
      setGeminiExplain(prev => ({ ...prev, [targetItem.id]: data }));
    } catch (error) {
      setGeminiExplain(prev => ({ ...prev, [targetItem.id]: { error: error.message } }));
    } finally {
      setGeminiExplainLoading(prev => ({ ...prev, [targetItem.id]: false }));
    }
  }

  async function handleSaveSeasonMagnetsFile() {
    const magnets = episodes.filter(e => e.magnet).map(e => e.magnet);
    if (!magnets.length) return;
    const safeTitle = String(selectedItem?.title || 'season')
      .replace(/[^a-z0-9]+/gi, '-')
      .replace(/^-|-$/g, '')
      .toLowerCase() || 'season';
    const result = await saveTextFile({
      title: `Save Season ${season} Magnets`,
      defaultFilename: `${safeTitle}-season-${season}-magnets.txt`,
      content: `${magnets.join('\n')}\n`
    });
    if (result?.ok) {
      showToast(`Exported ${magnets.length} magnets`);
      logActivity({
        kind: 'export',
        title: `${selectedItem?.title} S${season} magnets`,
        subtitle: `${magnets.length} magnets saved to .txt`
      });
    }
  }

  async function handleExportDiagnostics() {
    const report = await getDesktopDiagnostics();
    if (!report) return;
    const res = await saveTextFile({
      title: 'Export SeasonFetch Diagnostics',
      defaultFilename: `seasonfetch-diagnostics-v${report.app?.version || '0.1.0'}.txt`,
      content: `${JSON.stringify(report, null, 2)}\n`
    });
    if (res?.ok) {
      showToast('Redacted diagnostics report exported');
      logActivity({ kind: 'export', title: 'Diagnostics report', subtitle: 'Redacted system & updater diagnostics' });
    }
  }

  function handleCardContextMenu(event, item) {
    const saved = library.getSavedItem(item, item.mediaType || mediaType);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      title: item.title,
      actions: [
        { label: 'Open Title Details', onClick: () => handleSelectTitle(item) },
        saved
          ? { label: 'Remove from Library', danger: true, onClick: () => library.removeItem(saved) }
          : { label: 'Save to Personal Library', onClick: () => library.saveItem(item, { fallbackMediaType: item.mediaType || mediaType }) },
        { label: 'Copy IMDb ID', onClick: () => handleCopyMagnet(item.imdbId || item.id, `Copied ${item.imdbId || item.id}`) }
      ]
    });
  }

  const accountDisplayName = auth.profile?.display_name
    || auth.profile?.username
    || auth.authenticatedUser?.user_metadata?.display_name
    || auth.authenticatedUser?.email
    || 'Signed in';

  const activeTheme = settings?.theme || 'midnight';
  const isDetailOpen = Boolean(selectedItem && currentView.tab === 'detail');

  return (
    <div className={`sf-app theme-${activeTheme}`}>
      <TitleBar
        canGoBack={navIndex > 0}
        canGoForward={navIndex < navStack.length - 1}
        onBack={navigateBack}
        onForward={navigateForward}
        globalQuery={query}
        onGlobalQueryChange={val => {
          handleQueryChange(val);
          if (currentView.tab !== 'search' && val.trim().length >= 2) {
            setSelectedItem(null);
            pushNav({ tab: 'search', selectedItem: null });
          }
        }}
        onGlobalSearchSubmit={e => {
          e.preventDefault();
          runFullSearch(query, mediaType);
        }}
        onOpenLocalVideo={handleOpenLocalVideo}
        searchInputRef={searchInputRef}
        health={health}
        isOnline={isOnline}
        user={auth.authenticatedUser}
        accountDisplayName={accountDisplayName}
        onOpenAuth={() => {
          setAuthContextNote('');
          setShowAuthModal(true);
        }}
        onOpenAccountSettings={() => selectTab('settings', { settingsSection: 'account' })}
      />

      <div className="sf-body">
        <NavRail
          activeTab={isDetailOpen ? 'discover' : currentView.tab}
          compact={Boolean(settings?.compactNav)}
          libraryCount={library.counts.total}
          downloadsCount={offlineDownloads.filter(d => d.status === 'completed' || d.status === 'downloading' || d.status === 'queued').length}
          activeDownloadsCount={offlineDownloads.filter(d => d.status === 'downloading' || d.status === 'queued').length}
          hasUpdate={updater?.status === 'available' || updater?.status === 'downloaded' || updater?.recoveryRequired}
          appInfo={appInfo}
          onSelectTab={tab => selectTab(tab)}
        />

        <div className="sf-main-column">
          {!isOnline && (
            <div className="sf-offline-strip" role="status">
              <strong>Offline Mode:</strong> Cached catalogs and local video playback remain available. Live backend search and Torrentio scans resume automatically when online.
            </div>
          )}

          <UpdateBanner
            updater={updater}
            onInstallNow={installDownloadedUpdate}
            onPostpone={postponeUpdateInstall}
            onRetryDownload={downloadAvailableUpdate}
            onCancelDownload={cancelUpdateDownload}
            onDismissRecovery={dismissUpdaterRecovery}
            onOpenReleases={openExternalUrl}
            onOpenUpdates={() => selectTab('settings', { settingsSection: 'updates' })}
          />

          {globalNotice && (
            <div className="sf-global-alert" role="alert">
              <div>
                <strong>{globalNotice.message}</strong>
                {globalNotice.hint && <p>{globalNotice.hint}</p>}
              </div>
              <div className="sf-btn-row">
                {globalNotice.magnetFallback && (
                  <button
                    type="button"
                    className="sf-secondary-btn sf-sm"
                    onClick={() => handleCopyMagnet(globalNotice.magnetFallback, 'Magnet copied')}
                  >
                    Copy Magnet
                  </button>
                )}
                {globalNotice.showDefaultApps && (
                  <button
                    type="button"
                    className="sf-secondary-btn sf-sm"
                    onClick={() => openExternalUrl('ms-settings:defaultapps')}
                  >
                    Windows Default Apps
                  </button>
                )}
                <button type="button" className="sf-ghost-btn sf-sm" onClick={() => setGlobalNotice(null)}>
                  Dismiss
                </button>
              </div>
            </div>
          )}

          <main className="sf-scroll-stage">
            {isDetailOpen ? (
              <TitleDetailView
                item={selectedItem}
                detailLoading={detailLoading}
                detailError={detailError}
                season={season}
                episodes={episodes}
                seasonIntegrity={seasonIntegrity}
                movieRelease={movieRelease}
                checkingSources={checkingSources}
                providerMode={providerMode}
                rankingPreference={rankingPreference}
                resolutionPreference={resolutionPreference}
                audioPreference={audioPreference}
                showAdvancedFilters={showAdvancedFilters}
                minSeeders={minSeeders}
                maxSizeGb={maxSizeGb}
                codecFilter={codecFilter}
                hdrFilter={hdrFilter}
                expandedCompare={expandedCompare}
                geminiAvailable={geminiAvailable}
                geminiExplain={geminiExplain}
                geminiExplainLoading={geminiExplainLoading}
                savedLibraryItem={library.getSavedItem(selectedItem, selectedItem.mediaType)}
                libraryBusy={library.isItemBusy(selectedItem, selectedItem.mediaType)}
                authSuspended={auth.suspended}
                isOnline={isOnline}
                streamingStatus={streamingStatus}
                onBack={navigateBack}
                onSelectSeason={num => {
                  scanCoordinator.invalidate();
                  setSeason(num);
                  setEpisodes([]);
                  setSeasonIntegrity(null);
                  setExpandedCompare({});
                  setProviderMode('ready');
                }}
                onCheckSeasonSources={handleCheckSeasonSources}
                onCheckMovieSources={() => scanMovieSources(selectedItem)}
                onRankingChange={val => {
                  const norm = normalizeRankingPreference(val);
                  setRankingPreference(norm);
                  handleSettingsPatch({ rankingPreference: norm }, { silent: true });
                }}
                onResolutionChange={val => {
                  setResolutionPreference(val);
                  handleSettingsPatch({ resolutionPreference: val }, { silent: true });
                }}
                onAudioChange={val => {
                  setAudioPreference(val);
                  handleSettingsPatch({ audioPreference: val }, { silent: true });
                }}
                onToggleAdvancedFilters={() => setShowAdvancedFilters(v => !v)}
                onMinSeedersChange={setMinSeeders}
                onMaxSizeGbChange={setMaxSizeGb}
                onCodecFilterChange={setCodecFilter}
                onHdrFilterChange={setHdrFilter}
                onToggleCompare={id => setExpandedCompare(prev => ({ ...prev, [id]: !prev[id] }))}
                onSelectEpisodeRelease={(epId, cand) => {
                  setEpisodes(cur => selectEpisodeRelease(cur, epId, cand));
                  showToast('Switched to manually selected release');
                }}
                onRestoreEpisodeRelease={epId => {
                  setEpisodes(cur => restoreEpisodeRecommendation(cur, epId));
                  showToast('Restored automatic recommendation');
                }}
                onSelectMovieRelease={cand => {
                  if (!movieRelease) return;
                  setMovieRelease(selectEpisodeRelease([movieRelease], movieRelease.id, cand)[0]);
                  showToast('Switched to manually selected movie release');
                }}
                onRestoreMovieRelease={() => {
                  if (!movieRelease) return;
                  setMovieRelease(restoreEpisodeRecommendation([movieRelease], movieRelease.id)[0]);
                  showToast('Restored automatic movie recommendation');
                }}
                onExplainGemini={handleExplainWithGemini}
                onPlayRelease={handlePlayRelease}
                onDownloadRelease={handleStartOfflineDownload}
                onPlayLocalFile={handleOpenLocalVideo}
                onOpenMagnet={handleOpenMagnet}
                onCopyMagnet={handleCopyMagnet}
                onCopyAllMagnets={() => {
                  const list = episodes.filter(e => e.magnet).map(e => e.magnet);
                  if (list.length) handleCopyMagnet(list.join('\n'), `Copied ${list.length} season magnets`);
                }}
                onSaveMagnetsFile={handleSaveSeasonMagnetsFile}
                onOpenExternal={openExternalUrl}
                onSaveLibrary={library.saveItem}
                onUpdateLibraryStatus={library.updateStatus}
                onToggleLibraryFavorite={library.toggleFavorite}
                onRemoveLibrary={library.removeItem}
                offlineDownloads={offlineDownloads}
              />
            ) : currentView.tab === 'discover' ? (
              <DiscoverView
                catalogs={catalogs}
                catalogLoading={catalogLoading}
                catalogErrors={catalogErrors}
                recentlyViewed={settings?.recentlyViewed || []}
                playbackProgress={playbackProgress}
                libraryItems={library.items}
                showSpotlight={Boolean(settings?.showSpotlight ?? true)}
                onSelectTitle={item => handleSelectTitle(item)}
                onSaveTitle={item => library.saveItem(item, { fallbackMediaType: item.mediaType })}
                onRefreshCatalog={loadCatalogCategory}
                onResumePlayback={rec => {
                  if (rec.imdbId) {
                    handleSelectTitle({
                      id: rec.imdbId,
                      imdbId: rec.imdbId,
                      mediaType: rec.mediaType || 'series',
                      title: rec.title
                    });
                  } else {
                    handleOpenLocalVideo();
                  }
                }}
                onCardContextMenu={handleCardContextMenu}
              />
            ) : currentView.tab === 'search' ? (
              <SearchView
                query={query}
                mediaType={mediaType}
                searching={searching}
                searchError={searchError}
                suggestions={suggestions}
                suggestionsLoading={suggestionsLoading}
                results={results}
                resolution={resolution}
                recentSearches={settings?.recentSearches || []}
                geminiAvailable={geminiAvailable}
                geminiQueries={geminiQueries}
                geminiAssistLoading={geminiAssistLoading}
                isOnline={isOnline}
                onQueryChange={handleQueryChange}
                onMediaTypeChange={mt => {
                  setMediaType(mt);
                  setSuggestions([]);
                  setResults([]);
                  setSearchError('');
                }}
                onSubmit={e => {
                  e.preventDefault();
                  runFullSearch(query, mediaType);
                }}
                onRunRecent={(q, mt) => runFullSearch(q, mt)}
                onClearRecent={() => handleSettingsPatch({ recentSearches: [] }, { silent: true })}
                onGeminiAssist={async () => {
                  setGeminiAssistLoading(true);
                  try {
                    const list = await geminiApi.searchAssist(query);
                    setGeminiQueries(list);
                  } catch (err) {
                    showNotice(err.message);
                  } finally {
                    setGeminiAssistLoading(false);
                  }
                }}
                onSelectTitle={item => handleSelectTitle(item)}
                onCardContextMenu={handleCardContextMenu}
              />
            ) : currentView.tab === 'library' ? (
              <LibraryView
                user={auth.authenticatedUser}
                suspended={auth.suspended}
                authConfigured={authConfigured}
                items={library.items}
                counts={library.counts}
                loading={library.loading}
                error={library.error}
                busyKeys={library.busyKeys}
                onSignIn={() => {
                  setAuthContextNote('');
                  setShowAuthModal(true);
                }}
                onRefresh={library.loadLibrary}
                onOpenTitle={item => handleSelectTitle(item, item.mediaType)}
                onUpdateStatus={library.updateStatus}
                onToggleFavorite={library.toggleFavorite}
                onRemove={library.removeItem}
              />
            ) : currentView.tab === 'downloads' ? (
              <DownloadsView
                downloads={offlineDownloads}
                onPlayDownload={handlePlayDownloadedEntry}
                onCancelDownload={async id => {
                  const next = await downloadsCancel(id);
                  if (Array.isArray(next)) setOfflineDownloads(next);
                  showToast('Download cancelled');
                }}
                onRemoveDownload={async (id, deleteFile) => {
                  const next = await downloadsRemove(id, deleteFile);
                  if (Array.isArray(next)) setOfflineDownloads(next);
                  showToast('Removed download');
                }}
                onOpenFolder={async p => {
                  const res = await downloadsOpenFolder(p);
                  if (!res?.ok && res?.error) showNotice(res.error);
                }}
              />
            ) : currentView.tab === 'activity' ? (
              <ActivityView
                activityLog={settings?.activityLog || []}
                recentSearches={settings?.recentSearches || []}
                onRunSearch={(q, mt) => runFullSearch(q, mt)}
                onOpenTitle={item => handleSelectTitle(item)}
                onClearHistory={async () => {
                  const next = await clearDesktopHistory();
                  if (next?.activityLog) setSettings(next);
                  showToast('Local history cleared');
                }}
              />
            ) : (
              <SettingsView
                initialSection={currentView.settingsSection || 'general'}
                appInfo={appInfo}
                settings={settings}
                health={health}
                updater={updater}
                magnetHandler={magnetHandler}
                playerBackend={playerBackend}
                streamingStatus={streamingStatus}
                user={auth.authenticatedUser}
                accountDisplayName={accountDisplayName}
                authConfigured={authConfigured}
                geminiAvailable={geminiAvailable}
                settingsNotice={settingsNotice}
                onPatchSettings={handleSettingsPatch}
                onRefreshHealth={refreshHealth}
                onRefreshMagnetHandler={() => inspectMagnetHandler().then(setMagnetHandler)}
                onRefreshCatalogs={() => {
                  refreshAllCatalogs(true);
                  showToast('Refreshing Cinemeta catalogs...');
                }}
                onRefreshPlayerStatus={() => {
                  refreshPlayerStatus();
                  showToast('Re-probed player & streaming status');
                }}
                onPickExternalPlayer={async () => {
                  const picked = await pickExternalPlayerExe();
                  if (!picked?.ok || !picked.path) return;
                  await handleSettingsPatch({
                    player: {
                      ...(settings?.player || {}),
                      externalPlayerPath: picked.path,
                      preferredEngine: 'external-player'
                    }
                  });
                  showToast(`External player set to ${picked.name}`);
                }}
                onCheckTorrentioAddon={async () => {
                  const res = await checkTorrentioManifest(settings?.torrentioBaseUrl || null);
                  if (res?.ok) {
                    showToast(`${res.name || 'Torrentio'} v${res.version || ''} reachable (${res.latencyMs}ms)`);
                  } else {
                    showNotice(res?.error || 'Could not reach Torrentio addon manifest.');
                  }
                }}
                onClearCache={async () => {
                  await clearLocalCache();
                  refreshAllCatalogs(true);
                  showToast('Local cache cleared');
                }}
                onClearStreamCache={async () => {
                  const res = await clearStreamCache();
                  await refreshPlayerStatus();
                  const mb = res?.freedBytes ? ` (${(res.freedBytes / (1024 * 1024)).toFixed(1)} MB freed)` : '';
                  showToast(`Torrent stream cache cleared${mb}`);
                }}
                onClearHistory={async () => {
                  const next = await clearDesktopHistory();
                  if (next?.activityLog) setSettings(next);
                  showToast('Local history cleared');
                }}
                onOpenExternal={openExternalUrl}
                onOpenAuth={() => {
                  setAuthContextNote('');
                  setShowAuthModal(true);
                }}
                onSignOut={auth.signOut}
                onCheckUpdates={checkForUpdates}
                onDownloadUpdate={downloadAvailableUpdate}
                onInstallUpdate={installDownloadedUpdate}
                onDismissRecovery={dismissUpdaterRecovery}
                onExportDiagnostics={handleExportDiagnostics}
              />
            )}
          </main>
        </div>
      </div>

      {playerSession && (
        <PlayerOverlay
          session={playerSession}
          playerPrefs={settings?.player}
          subtitlePrefs={settings?.subtitleProviders}
          playerBackend={playerBackend}
          offlineDownloadState={(() => {
            const hash = playerSession?.torrentRequest?.infoHash?.toLowerCase();
            if (!hash) return 'idle';
            const match = offlineDownloads.find(
              d =>
                d.infoHash?.toLowerCase() === hash &&
                (playerSession.torrentRequest.fileIdx == null || d.fileIdx == null || d.fileIdx === playerSession.torrentRequest.fileIdx)
            );
            if (!match) return 'idle';
            if (match.status === 'completed') return 'completed';
            if (match.status === 'queued' || match.status === 'downloading') return 'downloading';
            return 'idle';
          })()}
          onStartOfflineDownload={async (sess, status) => {
            if (!sess?.torrentRequest) return;
            try {
              await downloadsStart({
                title: sess.torrentRequest.title || sess.title,
                magnetUri: sess.torrentRequest.magnetUri || sess.magnetUri || null,
                infoHash: status?.infoHash || sess.torrentRequest.infoHash || null,
                fileIdx: status?.fileIdx ?? sess.torrentRequest.fileIdx ?? null,
                poster: selectedItem?.poster || null,
                season: sess.season ?? null,
                episode: sess.episode ?? null,
                trackers: sess.torrentRequest.trackers || []
              });
              showToast('Saving for offline playback in background');
            } catch (err) {
              showToast(typeof err === 'string' ? err : err?.message || 'Could not start offline download');
            }
          }}
          onUpdatePlayerPrefs={patch => handleSettingsPatch({ player: { ...(settings?.player || {}), ...patch } }, { silent: true })}
          onPlayNextEpisode={nextEp => {
            setPlayerSession(null);
            handlePlayRelease(nextEp, { isMovie: false });
          }}
          onClose={() => {
            setPlayerSession(null);
            refreshPlayerStatus().catch(() => {});
          }}
          onToast={showToast}
        />
      )}

      {showAuthModal && (
        <AuthModal
          supabase={supabase}
          contextNote={authContextNote}
          onClose={outcome => {
            setShowAuthModal(false);
            setAuthContextNote('');
            if (outcome !== 'authenticated') library.cancelPendingSave();
            else showToast('Signed in to SeasonFetch');
          }}
        />
      )}

      <ContextMenu menu={contextMenu} onClose={() => setContextMenu(null)} />

      {toast && (
        <div className="sf-toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
