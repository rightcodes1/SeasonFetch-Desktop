import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  RANKING_PREFERENCES,
  normalizeRankingPreference,
  rankingPreferenceDescription
} from '../shared/rankingPreferences.js';
import {
  cancelUpdateDownload,
  checkForUpdates,
  dismissUpdaterRecovery,
  downloadAvailableUpdate,
  fetchBackendHealth,
  fetchMediaDetail,
  getAppInfo,
  getDesktopDiagnostics,
  getDesktopSettings,
  getUpdaterState,
  inspectMagnetHandler,
  installDownloadedUpdate,
  openExternalUrl,
  postponeUpdateInstall,
  saveTextFile,
  searchCatalog,
  subscribeDesktopNavigation,
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
  getMediaTypeFallbackLabel,
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
import {
  AUDIO_PREFERENCES,
  RESOLUTION_PREFERENCES,
  getNoSourceCopy,
  getResolutionSummaryCopy
} from './utils/resolutionDisplay.js';
import { toGeminiCandidate } from './utils/geminiCandidates.js';
import { copyTextToClipboard } from './utils/clipboard.js';
import { UpdateBanner } from './components/UpdateBanner.jsx';
import { AuthModal } from './components/AuthModal.jsx';
import { ReleaseComparison } from './components/ReleaseComparison.jsx';
import { SaveToLibraryControl } from './components/SaveToLibraryControl.jsx';
import { LibraryView } from './components/LibraryView.jsx';
import { GeminiInsightBox } from './components/GeminiInsightBox.jsx';

function formatTitleInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

function getEpisodeNumber(row, index) {
  return Number.isInteger(row?.number) && row.number > 0 ? row.number : index + 1;
}

function statusBadgeLabel(status) {
  if (status === 'selected') return 'Selected';
  if (status === 'size-unknown') return 'Size unknown';
  if (status === 'provider-error') return 'Provider error';
  return 'Unavailable';
}

export default function App() {
  const [activeTab, setActiveTab] = useState('discover');
  const [appInfo, setAppInfo] = useState(null);
  const [settings, setSettings] = useState(null);
  const [updater, setUpdater] = useState(null);
  const [magnetHandler, setMagnetHandler] = useState(null);
  const [health, setHealth] = useState({ status: 'checking', latencyMs: null, error: null });
  const [isOnline, setIsOnline] = useState(() => (typeof navigator !== 'undefined' ? navigator.onLine : true));
  const [geminiAvailable, setGeminiAvailable] = useState(false);

  const [toast, setToast] = useState('');
  const [globalNotice, setGlobalNotice] = useState(null);
  const [showAuthModal, setShowAuthModal] = useState(false);
  const [authContextNote, setAuthContextNote] = useState('');

  // Search & discovery state
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

  // Selected title & Torrentio release intelligence state
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

  // Ranking & filter preferences
  const [rankingPreference, setRankingPreference] = useState('smallest-acceptable');
  const [audioPreference, setAudioPreference] = useState('any');
  const [resolutionPreference, setResolutionPreference] = useState('1080p');
  const [showAdvancedFilters, setShowAdvancedFilters] = useState(false);
  const [minSeeders, setMinSeeders] = useState('');
  const [maxSizeGb, setMaxSizeGb] = useState('');
  const [codecFilter, setCodecFilter] = useState('');
  const [hdrFilter, setHdrFilter] = useState('');
  const [settingsNotice, setSettingsNotice] = useState('');

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
    }, 2600);
  }, []);

  const showNotice = useCallback((message, { hint = '', magnetFallback = '', showDefaultApps = false } = {}) => {
    if (!message) {
      setGlobalNotice(null);
      return;
    }
    setGlobalNotice({ message, hint, magnetFallback, showDefaultApps });
  }, []);

  const auth = useAuthController({
    supabase,
    onToast: showToast,
    onNotice: message => showNotice(message)
  });

  const openSignInPrompt = useCallback(snapshot => {
    if (!supabase) {
      showNotice('Supabase authentication is not configured for this desktop build.');
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

  const refreshHealth = useCallback(async () => {
    setHealth(prev => ({ ...prev, status: 'checking', error: null }));
    const res = await fetchBackendHealth();
    if (res.ok) {
      setHealth({
        status: 'online',
        latencyMs: res.latencyMs,
        error: null,
        payload: res.data
      });
      if (res.data?.gemini === 'configured') {
        setGeminiAvailable(true);
      }
    } else {
      setHealth({
        status: 'offline',
        latencyMs: res.latencyMs,
        error: res.error || 'SeasonFetch backend is unreachable.'
      });
    }
  }, []);

  const refreshMagnetHandler = useCallback(async () => {
    const info = await inspectMagnetHandler();
    setMagnetHandler(info);
  }, []);

  useEffect(() => {
    let active = true;
    Promise.all([
      getAppInfo(),
      getDesktopSettings(),
      getUpdaterState(),
      geminiApi.getStatus(),
      inspectMagnetHandler()
    ]).then(([info, loadedSettings, updaterState, geminiStatus, handlerInfo]) => {
      if (!active) return;
      setAppInfo(info);
      setSettings(loadedSettings);
      setUpdater(updaterState);
      setGeminiAvailable(Boolean(geminiStatus?.configured));
      setMagnetHandler(handlerInfo);
      if (loadedSettings?.defaultSearchMode) {
        setMediaType(normalizeMediaType(loadedSettings.defaultSearchMode));
      }
      if (loadedSettings?.rankingPreference) {
        setRankingPreference(normalizeRankingPreference(loadedSettings.rankingPreference));
      }
      if (loadedSettings?.audioPreference) {
        setAudioPreference(loadedSettings.audioPreference);
      }
      if (loadedSettings?.resolutionPreference) {
        setResolutionPreference(loadedSettings.resolutionPreference);
      }
    });

    refreshHealth();
    const unsubscribeUpdater = subscribeUpdaterStatus(nextState => {
      setUpdater(nextState);
    });
    const unsubscribeNav = subscribeDesktopNavigation(payload => {
      if (payload?.tab) setActiveTab(payload.tab);
      if (payload?.focusSearch) {
        setTimeout(() => searchInputRef.current?.focus(), 40);
      }
    });

    const handleOnline = () => {
      setIsOnline(true);
      refreshHealth();
      showToast('Back online — reconnected to SeasonFetch');
    };
    const handleOffline = () => {
      setIsOnline(false);
    };

    const handleKeyDown = event => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setActiveTab('discover');
        setTimeout(() => searchInputRef.current?.focus(), 20);
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'l') {
        event.preventDefault();
        setActiveTab('library');
      } else if ((event.ctrlKey || event.metaKey) && event.key === ',') {
        event.preventDefault();
        setActiveTab('settings');
      }
    };

    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);
    window.addEventListener('keydown', handleKeyDown);

    return () => {
      active = false;
      unsubscribeUpdater();
      unsubscribeNav();
      autocompleteCoordinator.invalidate();
      scanCoordinator.invalidate();
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [autocompleteCoordinator, scanCoordinator, refreshHealth, showToast]);

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

  const handleSettingsPatch = useCallback(async (patch, { silent = false } = {}) => {
    if (!silent) setSettingsNotice('');
    const result = await updateDesktopSettings(patch);
    if (result?.ok) {
      setSettings(result.settings);
      if (!silent) {
        setSettingsNotice('Preferences saved.');
        setTimeout(() => setSettingsNotice(''), 2400);
      }
    } else if (!silent) {
      setSettingsNotice(result?.error || 'Could not save desktop preferences.');
    }
  }, []);

  const recordRecentSearch = useCallback((cleanQuery, mode) => {
    const current = Array.isArray(settings?.recentSearches) ? settings.recentSearches : [];
    const next = [
      { query: cleanQuery, mediaType: normalizeMediaType(mode) },
      ...current.filter(
        entry => !(entry.mediaType === normalizeMediaType(mode) && entry.query.toLowerCase() === cleanQuery.toLowerCase())
      )
    ].slice(0, 10);
    handleSettingsPatch({ recentSearches: next }, { silent: true });
  }, [settings?.recentSearches, handleSettingsPatch]);

  function handleRankingChange(nextValue) {
    const normalized = normalizeRankingPreference(nextValue);
    setRankingPreference(normalized);
    scanCoordinator.invalidate();
    setEpisodes([]);
    setMovieRelease(null);
    setSeasonIntegrity(null);
    setExpandedCompare({});
    setProviderMode('ready');
    handleSettingsPatch({ rankingPreference: normalized }, { silent: true });
    showToast(`Profile set to ${RANKING_PREFERENCES.find(p => p.value === normalized)?.label || normalized}`);
  }

  function handleAudioChange(nextValue) {
    setAudioPreference(nextValue);
    scanCoordinator.invalidate();
    setEpisodes([]);
    setMovieRelease(null);
    setSeasonIntegrity(null);
    setExpandedCompare({});
    setProviderMode('ready');
    handleSettingsPatch({ audioPreference: nextValue }, { silent: true });
  }

  function handleResolutionChange(nextValue) {
    setResolutionPreference(nextValue);
    scanCoordinator.invalidate();
    setEpisodes([]);
    setMovieRelease(null);
    setSeasonIntegrity(null);
    setExpandedCompare({});
    setProviderMode('ready');
    handleSettingsPatch({ resolutionPreference: nextValue }, { silent: true });
  }

  async function handleOpenMagnet(magnet) {
    if (!magnet) return;
    const result = await openExternalUrl(magnet);
    if (result?.ok) {
      showNotice(null);
      showToast('Handed magnet URI off to Windows default client');
    } else {
      showNotice(
        result?.error || 'Could not open magnet URI with your operating system.',
        {
          hint: result?.hint || 'Check Windows Default Apps or copy the magnet link manually.',
          magnetFallback: magnet,
          showDefaultApps: true
        }
      );
    }
  }

  async function handleCopyText(text, successLabel) {
    try {
      await copyTextToClipboard(text);
      showNotice(null);
      showToast(successLabel);
    } catch (error) {
      showNotice(error.message || 'Unable to copy to clipboard.');
    }
  }

  async function handleSaveSeasonMagnetsFile() {
    if (!seasonMagnets.length) return;
    const safeTitle = String(selectedItem?.title || 'season')
      .replace(/[^a-z0-9]+/gi, '-')
      .replace(/^-|-$/g, '')
      .toLowerCase() || 'season';
    const result = await saveTextFile({
      title: `Save Season ${season} Magnets`,
      defaultFilename: `${safeTitle}-season-${season}-magnets.txt`,
      content: `${seasonMagnets.join('\n')}\n`
    });
    if (result?.ok) {
      showToast(`Saved ${seasonMagnets.length} magnets to file`);
    } else if (!result?.canceled && result?.error) {
      showNotice(result.error);
    }
  }

  async function handleExportDiagnostics() {
    const report = await getDesktopDiagnostics();
    if (!report) {
      showNotice('Diagnostics export is only available inside the desktop application.');
      return;
    }
    const result = await saveTextFile({
      title: 'Export SeasonFetch Diagnostics Report',
      defaultFilename: `seasonfetch-diagnostics-v${report.app?.version || '0.1.0'}.txt`,
      content: `${JSON.stringify(report, null, 2)}\n`
    });
    if (result?.ok) {
      showToast('Redacted diagnostics report saved');
    } else if (!result?.canceled && result?.error) {
      showNotice(result.error);
    }
  }

  function handleQueryChange(nextValue) {
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
  }

  async function runFullSearch(overrideQuery, overrideMediaType) {
    const activeMode = normalizeMediaType(overrideMediaType || mediaType);
    const clean = normalizeSearchQueryText(overrideQuery ?? query);
    if (clean.length < 2) {
      setSearchError('Enter at least 2 characters or an IMDb ID (for example tt0903747).');
      return;
    }

    autocompleteCoordinator.invalidate();
    scanCoordinator.invalidate();
    setMediaType(activeMode);
    setSuggestions([]);
    setSuggestionsLoading(false);
    setSelectedItem(null);
    setEpisodes([]);
    setMovieRelease(null);
    setSeasonIntegrity(null);
    setDetailError('');
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

    recordRecentSearch(clean, activeMode);
    const filtered = filterResultsByMediaType(response.data?.results, activeMode);
    setResults(filtered);
    setResolution(response.data?.resolution || null);
    if (filtered.length === 0) {
      setSearchError(`No ${activeMode === 'movie' ? 'movies' : 'series'} found for "${clean}".`);
    }
  }

  async function handleSearchSubmit(event) {
    event?.preventDefault();
    await runFullSearch(query, mediaType);
  }

  async function handleGeminiSearchAssist() {
    const clean = normalizeSearchQueryText(query);
    if (clean.length < 2) return;
    setGeminiAssistLoading(true);
    setGeminiQueries([]);
    try {
      const suggested = await geminiApi.searchAssist(clean);
      setGeminiQueries(suggested);
      if (!suggested.length) {
        showNotice('Gemini did not suggest alternate titles for this query.');
      }
    } catch (error) {
      showNotice(error.message || 'Gemini search assist is currently unavailable.');
    } finally {
      setGeminiAssistLoading(false);
    }
  }

  const scanMovieSources = useCallback((movie, overrideFilters) => {
    const movieId = movie?.imdbId || movie?.id;
    if (!movieId || !/^tt\d+$/.test(movieId)) return;

    const usedFilters = overrideFilters || activeFiltersPayload;
    setMovieRelease(null);
    setExpandedCompare({});
    setGeminiExplain({});
    setProviderMode('ready');

    scanCoordinator.start({
      delayMs: 250,
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
    setActiveTab('discover');
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
      year: item.year || item.releaseYear || null,
      imdbRating: item.imdbRating || item.rating || null
    };
    setSelectedItem(baseItem);

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
  }, [autocompleteCoordinator, mediaType, scanCoordinator, scanMovieSources]);

  function handleCheckSeasonSources() {
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
      delayMs: 250,
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
        if (data?.status === 'partial') {
          showNotice(`${data.providerErrors || 'Some'} episode requests failed; available Torrentio results are shown.`);
        } else if (data?.status === 'error') {
          showNotice('Torrentio could not answer episode requests. Retry shortly.');
        }
        showToast(`Season ${season} Torrentio scan complete`);
      },
      onError: error => {
        setProviderMode('error');
        showNotice(error.message || 'Torrentio season scan failed. Try again shortly.');
      }
    });
  }

  function handleSelectManualEpisodeRelease(episodeId, candidate) {
    setEpisodes(current => selectEpisodeRelease(current, episodeId, candidate));
    showToast('Switched to manually selected release');
  }

  function handleRestoreEpisodeRecommendation(episodeId) {
    setEpisodes(current => restoreEpisodeRecommendation(current, episodeId));
    showToast('Restored automatic recommendation');
  }

  function handleSelectManualMovieRelease(candidate) {
    if (!movieRelease) return;
    const updated = selectEpisodeRelease([movieRelease], movieRelease.id, candidate)[0];
    setMovieRelease(updated);
    showToast('Switched to manually selected movie release');
  }

  function handleRestoreMovieRecommendation() {
    if (!movieRelease) return;
    const updated = restoreEpisodeRecommendation([movieRelease], movieRelease.id)[0];
    setMovieRelease(updated);
    showToast('Restored automatic movie recommendation');
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

  async function handleMediaModeSwitch(nextMode) {
    const normalized = normalizeMediaType(nextMode);
    if (normalized === mediaType) return;
    setMediaType(normalized);
    autocompleteCoordinator.invalidate();
    scanCoordinator.invalidate();
    setSuggestions([]);
    setResults([]);
    setResolution(null);
    setSelectedItem(null);
    setEpisodes([]);
    setMovieRelease(null);
    setSeasonIntegrity(null);
    setSearchError('');
    setGeminiQueries([]);
  }

  const seasonMagnets = useMemo(
    () => episodes.filter(ep => ep.magnet).map(ep => ep.magnet),
    [episodes]
  );

  const availableEpisodeCount = useMemo(
    () => episodes.filter(ep => !['unavailable', 'provider-error'].includes(ep.status)).length,
    [episodes]
  );

  const selectedSavedItem = selectedItem
    ? library.getSavedItem(selectedItem, selectedItem.mediaType || mediaType)
    : null;
  const selectedLibraryBusy = selectedItem
    ? library.isItemBusy(selectedItem, selectedItem.mediaType || mediaType)
    : false;

  const channelLabel = (updater?.channel || settings?.updateChannel || 'stable') === 'preview'
    ? 'Preview'
    : 'Stable';

  const accountDisplayName = auth.profile?.display_name
    || auth.profile?.username
    || auth.authenticatedUser?.user_metadata?.display_name
    || auth.authenticatedUser?.email
    || 'Signed in';

  const recentSearches = Array.isArray(settings?.recentSearches) ? settings.recentSearches : [];

  const navItems = [
    { id: 'discover', label: 'Discover', subtitle: 'Search & Release Engine' },
    {
      id: 'library',
      label: 'My Library',
      subtitle: auth.authenticatedUser ? `${library.counts.total} saved titles` : 'Sign in to sync'
    },
    { id: 'updates', label: 'Updates', subtitle: 'Channels & Status' },
    { id: 'settings', label: 'Settings', subtitle: 'Desktop Preferences' }
  ];

  return (
    <div className="desktop-frame">
      <div className="ambient ambient-one" aria-hidden="true" />
      <div className="ambient ambient-two" aria-hidden="true" />

      <aside className="sidebar">
        <div className="sidebar-brand">
          <img src="./favicon.png" alt="SeasonFetch logo" className="brand-emblem" />
          <div>
            <div className="brand-name">
              Season<span className="accent-text">Fetch</span>
            </div>
            <div className="brand-tag">WINDOWS DESKTOP</div>
          </div>
        </div>

        <nav className="sidebar-nav" aria-label="Primary navigation">
          {navItems.map(item => (
            <button
              key={item.id}
              type="button"
              className={`nav-item ${activeTab === item.id ? 'active' : ''}`}
              onClick={() => setActiveTab(item.id)}
            >
              <span>{item.label}</span>
              <small>{item.subtitle}</small>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className="account-card">
            {auth.authenticatedUser ? (
              <>
                <div className="account-card-top">
                  <div>
                    <span className="section-kicker">
                      {auth.suspended ? 'ACCOUNT SUSPENDED' : 'SIGNED IN'}
                    </span>
                    <strong className="account-name">{accountDisplayName}</strong>
                  </div>
                  {appInfo?.authVaultEncrypted && (
                    <span className="runtime-pill" title="Session tokens encrypted with Windows DPAPI">
                      DPAPI
                    </span>
                  )}
                </div>
                <p className="mono-caption">{auth.authenticatedUser.email}</p>
                {auth.profileError && (
                  <button type="button" className="inline-link" onClick={auth.retryAccountSync}>
                    Retry account sync
                  </button>
                )}
                <div className="account-card-actions">
                  <button
                    type="button"
                    className="ghost-button small-button"
                    onClick={() => setActiveTab('library')}
                  >
                    Library ({library.counts.total})
                  </button>
                  <button type="button" className="ghost-button small-button" onClick={auth.signOut}>
                    Sign out
                  </button>
                </div>
              </>
            ) : (
              <>
                <span className="section-kicker">SEASONFETCH ACCOUNT</span>
                <p className="muted-copy compact-copy">
                  Sync your Personal Library with SeasonFetch Web.
                </p>
                <button
                  type="button"
                  className="secondary-button small-button full-width"
                  disabled={!authConfigured}
                  onClick={() => {
                    setAuthContextNote('');
                    setShowAuthModal(true);
                  }}
                >
                  {authConfigured ? 'Sign in / Register' : 'Auth not configured'}
                </button>
              </>
            )}
          </div>

          <div className={`status-card status-${!isOnline ? 'offline' : health.status}`}>
            <div className="status-card-row">
              <span className="status-dot" />
              <strong>
                {!isOnline
                  ? 'Offline Mode'
                  : health.status === 'online'
                    ? 'Backend Connected'
                    : health.status === 'checking'
                      ? 'Checking Backend'
                      : 'Backend Offline'}
              </strong>
            </div>
            <p className="mono-caption">
              {appInfo?.backendUrl || 'https://season-fetch.vercel.app'}
              {isOnline && health.status === 'online' && health.latencyMs ? ` · ${health.latencyMs}ms` : ''}
            </p>
            {(health.status === 'offline' || !isOnline) && (
              <button type="button" className="inline-link" onClick={refreshHealth}>
                Retry connection
              </button>
            )}
          </div>

          <div className="version-card">
            <div>
              <span className="section-kicker">INSTALLED VERSION</span>
              <div className="version-value">
                v{appInfo?.version || '0.1.0'} <span className="channel-pill">{channelLabel}</span>
              </div>
            </div>
            <span className="runtime-pill">
              {appInfo?.isPackaged ? 'Packaged' : 'Dev Build'}
            </span>
          </div>
        </div>
      </aside>

      <div className="workspace-column">
        {!isOnline && (
          <div className="offline-banner" role="status">
            <strong>You are currently offline.</strong> Search, Torrentio release scanning, and library syncing will resume automatically when your connection returns.
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
          onOpenUpdates={() => setActiveTab('updates')}
        />

        {auth.sessionExpiredMessage && (
          <div className="global-alert" role="alert">
            <div>
              <strong>Session notice:</strong> {auth.sessionExpiredMessage}
            </div>
            <button type="button" className="ghost-button small-button" onClick={auth.dismissSessionNotice}>
              Dismiss
            </button>
          </div>
        )}

        {globalNotice && (
          <div className="global-alert" role="alert">
            <div>
              <strong>{globalNotice.message}</strong>
              {globalNotice.hint && <p className="alert-hint">{globalNotice.hint}</p>}
            </div>
            <div className="update-banner-actions">
              {globalNotice.magnetFallback && (
                <button
                  type="button"
                  className="secondary-button small-button"
                  onClick={() => handleCopyText(globalNotice.magnetFallback, 'Magnet link copied to clipboard')}
                >
                  Copy Magnet
                </button>
              )}
              {globalNotice.showDefaultApps && (
                <button
                  type="button"
                  className="secondary-button small-button"
                  onClick={() => openExternalUrl('ms-settings:defaultapps')}
                >
                  Windows Default Apps
                </button>
              )}
              <button type="button" className="ghost-button small-button" onClick={() => setGlobalNotice(null)}>
                Dismiss
              </button>
            </div>
          </div>
        )}

        <main className="main-stage">
          {activeTab === 'discover' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">CINEMETA · TORRENTIO · RELEASE INTELLIGENCE · GEMINI</span>
                  <h1>
                    Find the right source, <span>natively.</span>
                  </h1>
                  <p className="stage-copy">
                    Search movies and TV series, rank releases using SeasonFetch&apos;s deterministic intelligence, explain candidates with Gemini, and hand magnets straight to your download client.
                  </p>
                </div>
                <div className="shortcut-hint" title="Focus search">
                  <kbd>Ctrl</kbd> + <kbd>K</kbd>
                </div>
              </header>

              <div className="search-controls">
                <div className="search-controls-row">
                  <div className="search-mode" role="group" aria-label="Media type">
                    <button
                      type="button"
                      aria-pressed={mediaType === 'series'}
                      onClick={() => handleMediaModeSwitch('series')}
                    >
                      Series
                    </button>
                    <button
                      type="button"
                      aria-pressed={mediaType === 'movie'}
                      onClick={() => handleMediaModeSwitch('movie')}
                    >
                      Movies
                    </button>
                  </div>

                  {geminiAvailable && query.trim().length >= 2 && (
                    <button
                      type="button"
                      className="secondary-button small-button"
                      disabled={geminiAssistLoading}
                      onClick={handleGeminiSearchAssist}
                    >
                      {geminiAssistLoading ? 'Asking Gemini...' : '✦ Gemini Title Assist'}
                    </button>
                  )}
                </div>

                <form className="search-form" onSubmit={handleSearchSubmit}>
                  <input
                    ref={searchInputRef}
                    type="search"
                    value={query}
                    onChange={event => handleQueryChange(event.target.value)}
                    placeholder={
                      mediaType === 'movie'
                        ? 'Search a movie title or IMDb ID (e.g. Dune, tt0133093)...'
                        : 'Search a TV series or IMDb ID (e.g. Breaking Bad, Dark)...'
                    }
                    aria-label="Search SeasonFetch catalog"
                  />
                  {query && (
                    <button
                      type="button"
                      className="ghost-button"
                      onClick={() => handleQueryChange('')}
                    >
                      Clear
                    </button>
                  )}
                  <button type="submit" className="primary-button" disabled={searching || !isOnline}>
                    {searching ? 'Searching...' : 'Search'}
                  </button>
                </form>

                {!selectedItem && !query && recentSearches.length > 0 && (
                  <div className="recent-bar">
                    <span className="section-kicker">RECENT:</span>
                    {recentSearches.map((entry, idx) => (
                      <button
                        key={`${entry.mediaType}:${entry.query}:${idx}`}
                        type="button"
                        className="recent-chip"
                        onClick={() => {
                          setQuery(entry.query);
                          runFullSearch(entry.query, entry.mediaType);
                        }}
                      >
                        {entry.query} <small>{entry.mediaType === 'movie' ? 'Movie' : 'Series'}</small>
                      </button>
                    ))}
                    <button
                      type="button"
                      className="inline-link"
                      onClick={() => handleSettingsPatch({ recentSearches: [] }, { silent: true })}
                    >
                      Clear
                    </button>
                  </div>
                )}

                {geminiQueries.length > 0 && (
                  <div className="gemini-chip-bar">
                    <span className="section-kicker">GEMINI SUGGESTIONS:</span>
                    {geminiQueries.map((suggestion, idx) => (
                      <button
                        key={idx}
                        type="button"
                        className="suggestion-chip"
                        onClick={() => {
                          setQuery(suggestion);
                          runFullSearch(suggestion, mediaType);
                        }}
                      >
                        {suggestion}
                      </button>
                    ))}
                  </div>
                )}

                {(suggestionsLoading || suggestions.length > 0) && (
                  <div className="suggestions-panel" role="listbox" aria-label="Search suggestions">
                    {suggestionsLoading && (
                      <div className="muted-row">Searching SeasonFetch catalog...</div>
                    )}
                    {!suggestionsLoading && suggestions.map(item => (
                      <button
                        key={item.id}
                        type="button"
                        className="suggestion-row"
                        onClick={() => handleSelectTitle(item)}
                      >
                        <div className="mini-poster">
                          {item.poster ? (
                            <img src={item.poster} alt="" loading="lazy" />
                          ) : (
                            <span>{formatTitleInitials(item.title)}</span>
                          )}
                        </div>
                        <div className="suggestion-copy">
                          <strong>{item.title}</strong>
                          <span>
                            {getMediaTypeFallbackLabel(item, mediaType)}
                            {item.year ? ` · ${item.year}` : ''}
                            {item.imdbRating ? ` · ★ ${item.imdbRating}` : ''}
                          </span>
                        </div>
                        <span className="mono-caption">{item.id}</span>
                      </button>
                    ))}
                  </div>
                )}

                {searchError && (
                  <div className="notice-box" role="alert">
                    {searchError}
                  </div>
                )}
              </div>

              {selectedItem && (
                <div className="selected-workspace">
                  <div className="detail-card">
                    <div className="detail-poster">
                      {selectedItem.poster ? (
                        <img src={selectedItem.poster} alt={selectedItem.title} />
                      ) : (
                        <span>{formatTitleInitials(selectedItem.title)}</span>
                      )}
                    </div>
                    <div className="detail-body">
                      <div className="detail-topline">
                        <span className="section-kicker">
                          {getMediaTypeFallbackLabel(selectedItem, mediaType).toUpperCase()} · {selectedItem.imdbId}
                        </span>
                        <button
                          type="button"
                          className="ghost-button small-button"
                          onClick={() => {
                            scanCoordinator.invalidate();
                            setSelectedItem(null);
                            setEpisodes([]);
                            setMovieRelease(null);
                            setSeasonIntegrity(null);
                          }}
                        >
                          Change {selectedItem.mediaType === 'movie' ? 'movie' : 'series'}
                        </button>
                      </div>
                      <h2>{selectedItem.title}</h2>
                      <div className="detail-meta">
                        {selectedItem.year && <span>{selectedItem.year}</span>}
                        {selectedItem.imdbRating && <span>★ {selectedItem.imdbRating}</span>}
                        {selectedItem.mediaType === 'series' && selectedItem.seasonNumbers?.length > 0 && (
                          <span>{selectedItem.seasonNumbers.length} seasons</span>
                        )}
                        {Array.isArray(selectedItem.genres) && selectedItem.genres.length > 0 && (
                          <span>{selectedItem.genres.slice(0, 3).join(' · ')}</span>
                        )}
                      </div>

                      {detailLoading ? (
                        <p className="muted-copy">Loading metadata from SeasonFetch Cinemeta...</p>
                      ) : detailError ? (
                        <p className="error-copy">{detailError}</p>
                      ) : (
                        <p className="detail-overview">
                          {selectedItem.synopsis || selectedItem.overview || selectedItem.description || 'Metadata loaded from SeasonFetch.'}
                        </p>
                      )}

                      <div className="detail-library-bar">
                        <SaveToLibraryControl
                          media={selectedItem}
                          fallbackMediaType={selectedItem.mediaType || mediaType}
                          savedItem={selectedSavedItem}
                          busy={selectedLibraryBusy}
                          disabled={auth.suspended}
                          onSave={library.saveItem}
                          onUpdateStatus={library.updateStatus}
                          onToggleFavorite={library.toggleFavorite}
                          onRemove={library.removeItem}
                        />
                      </div>
                    </div>
                  </div>

                  {/* Release Intelligence & Filter Bar */}
                  <div className="panel-card intelligence-bar">
                    <div className="intelligence-controls">
                      <label className="control-field">
                        <span>Ranking Profile</span>
                        <select
                          className="desktop-select"
                          value={rankingPreference}
                          onChange={event => handleRankingChange(event.target.value)}
                        >
                          {RANKING_PREFERENCES.map(pref => (
                            <option key={pref.value} value={pref.value}>
                              {pref.label} — {pref.hint}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="control-field">
                        <span>Resolution</span>
                        <select
                          className="desktop-select"
                          value={resolutionPreference}
                          onChange={event => handleResolutionChange(event.target.value)}
                        >
                          {RESOLUTION_PREFERENCES.map(option => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="control-field">
                        <span>Audio</span>
                        <select
                          className="desktop-select"
                          value={audioPreference}
                          onChange={event => handleAudioChange(event.target.value)}
                        >
                          {AUDIO_PREFERENCES.map(option => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                      </label>

                      <div className="control-actions">
                        <button
                          type="button"
                          className="ghost-button small-button"
                          onClick={() => setShowAdvancedFilters(v => !v)}
                        >
                          {showAdvancedFilters ? 'Hide Filters' : 'More Filters'}
                        </button>

                        {selectedItem.mediaType === 'movie' ? (
                          <button
                            type="button"
                            className="primary-button"
                            disabled={checkingSources || detailLoading || !isOnline}
                            onClick={() => scanMovieSources(selectedItem)}
                          >
                            {checkingSources ? 'Scanning...' : 'Check Movie Sources'}
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="primary-button"
                            disabled={checkingSources || detailLoading || !isOnline}
                            onClick={handleCheckSeasonSources}
                          >
                            {checkingSources ? 'Scanning...' : `Check Season ${season}`}
                          </button>
                        )}
                      </div>
                    </div>

                    <p className="muted-copy compact-copy">
                      {rankingPreferenceDescription(rankingPreference)} · {getResolutionSummaryCopy(providerMode, resolutionPreference)}
                    </p>

                    {showAdvancedFilters && (
                      <div className="advanced-filters-grid">
                        <label className="control-field">
                          <span>Min Seeders</span>
                          <input
                            type="number"
                            min="0"
                            placeholder="Any"
                            value={minSeeders}
                            onChange={event => setMinSeeders(event.target.value)}
                          />
                        </label>
                        <label className="control-field">
                          <span>Max Size (GB)</span>
                          <input
                            type="number"
                            min="0.1"
                            step="0.5"
                            placeholder="Any"
                            value={maxSizeGb}
                            onChange={event => setMaxSizeGb(event.target.value)}
                          />
                        </label>
                        <label className="control-field">
                          <span>Codec</span>
                          <select
                            className="desktop-select"
                            value={codecFilter}
                            onChange={event => setCodecFilter(event.target.value)}
                          >
                            <option value="">Any codec</option>
                            <option value="H.264">H.264</option>
                            <option value="HEVC">HEVC / x265</option>
                            <option value="AV1">AV1</option>
                          </select>
                        </label>
                        <label className="control-field">
                          <span>HDR</span>
                          <select
                            className="desktop-select"
                            value={hdrFilter}
                            onChange={event => setHdrFilter(event.target.value)}
                          >
                            <option value="">Any</option>
                            <option value="HDR">HDR</option>
                            <option value="Dolby Vision">Dolby Vision</option>
                          </select>
                        </label>
                      </div>
                    )}
                  </div>

                  {/* SERIES RELEASE WORKSPACE */}
                  {selectedItem.mediaType === 'series' && (
                    <div className="series-workspace">
                      <div className="season-tabs-row">
                        <div className="filter-pills" role="tablist" aria-label="Seasons">
                          {(selectedItem.seasonNumbers || [1]).map(num => (
                            <button
                              key={num}
                              type="button"
                              role="tab"
                              aria-selected={season === num}
                              className={season === num ? 'active' : ''}
                              onClick={() => {
                                scanCoordinator.invalidate();
                                setSeason(num);
                                setEpisodes([]);
                                setSeasonIntegrity(null);
                                setExpandedCompare({});
                                setProviderMode('ready');
                              }}
                            >
                              {num === 0 ? 'Specials' : `Season ${num}`}
                            </button>
                          ))}
                        </div>

                        {seasonMagnets.length > 0 && (
                          <div className="button-row">
                            <button
                              type="button"
                              className="secondary-button small-button"
                              onClick={() => handleCopyText(seasonMagnets.join('\n'), `Copied ${seasonMagnets.length} season magnets`)}
                            >
                              Copy All ({seasonMagnets.length})
                            </button>
                            <button
                              type="button"
                              className="ghost-button small-button"
                              onClick={handleSaveSeasonMagnetsFile}
                            >
                              Save .txt
                            </button>
                          </div>
                        )}
                      </div>

                      {seasonIntegrity && (
                        <div className="integrity-strip">
                          <span>
                            <strong>{availableEpisodeCount}/{episodes.length}</strong> episodes ready
                          </span>
                          {seasonIntegrity.estimatedTotalSizeLabel && (
                            <span>Est. season size: <strong>{seasonIntegrity.estimatedTotalSizeLabel}</strong></span>
                          )}
                          {seasonIntegrity.dominantGroup && (
                            <span>Dominant group: <strong>{seasonIntegrity.dominantGroup}</strong></span>
                          )}
                          {seasonIntegrity.summary && (
                            <span className="muted-inline">{seasonIntegrity.summary}</span>
                          )}
                        </div>
                      )}

                      {episodes.length === 0 ? (
                        <div className="panel-card empty-state-card">
                          <h3>
                            {checkingSources
                              ? `Scanning Season ${season} via Torrentio...`
                              : `Season ${season} ready to scan`}
                          </h3>
                          <p className="muted-copy">
                            {checkingSources
                              ? 'Fetching episode streams and running SeasonFetch deterministic release ranking...'
                              : `${(selectedItem.episodesBySeason?.[season] || []).length} episodes indexed. Click Check Season ${season} to evaluate live Torrentio releases.`}
                          </p>
                        </div>
                      ) : (
                        <div className="episode-list">
                          {episodes.map(ep => {
                            const isExpanded = Boolean(expandedCompare[ep.id]);
                            return (
                              <article key={ep.id} className="episode-card">
                                <div className="episode-top">
                                  <div className="episode-title-block">
                                    <span className="episode-number">E{String(ep.number).padStart(2, '0')}</span>
                                    <div>
                                      <strong>{ep.title}</strong>
                                      <p className="mono-caption">
                                        {ep.format || ep.sourceError || getNoSourceCopy(resolutionPreference)}
                                      </p>
                                    </div>
                                  </div>
                                  <span className={`release-status status-${ep.status}`}>
                                    {statusBadgeLabel(ep.status)}
                                  </span>
                                </div>

                                <div className="episode-meta-row">
                                  <span><b>Size:</b> {ep.size || '—'}</span>
                                  <span><b>Resolution:</b> {ep.resolution || '—'}</span>
                                  <span><b>Seeders:</b> {Number.isInteger(ep.seeders) ? ep.seeders.toLocaleString() : '—'}</span>
                                  <span><b>Provider:</b> {ep.provider || '—'}</span>
                                  <span><b>Candidates:</b> {ep.sourceCount || 0}</span>
                                </div>

                                {ep.selectionReason && (
                                  <p className="selection-reason">{ep.selectionReason}</p>
                                )}

                                <div className="episode-actions">
                                  {ep.magnet && (
                                    <>
                                      <button
                                        type="button"
                                        className="primary-button small-button"
                                        onClick={() => handleOpenMagnet(ep.magnet)}
                                      >
                                        Open Magnet
                                      </button>
                                      <button
                                        type="button"
                                        className="secondary-button small-button"
                                        onClick={() => handleCopyText(ep.magnet, `Copied E${ep.number} magnet`)}
                                      >
                                        Copy Magnet
                                      </button>
                                    </>
                                  )}

                                  {ep.candidates?.length > 0 && (
                                    <button
                                      type="button"
                                      className="ghost-button small-button"
                                      onClick={() => setExpandedCompare(prev => ({ ...prev, [ep.id]: !prev[ep.id] }))}
                                    >
                                      {isExpanded ? 'Hide Candidates' : `Compare Releases (${ep.candidates.length})`}
                                    </button>
                                  )}

                                  {geminiAvailable && ep.candidates?.length > 0 && (
                                    <button
                                      type="button"
                                      className="ghost-button small-button"
                                      disabled={Boolean(geminiExplainLoading[ep.id])}
                                      onClick={() => handleExplainWithGemini(ep)}
                                    >
                                      {geminiExplainLoading[ep.id] ? 'Analyzing...' : '✦ Ask Gemini'}
                                    </button>
                                  )}
                                </div>

                                <GeminiInsightBox
                                  analysis={geminiExplain[ep.id]}
                                  loading={Boolean(geminiExplainLoading[ep.id])}
                                />

                                {isExpanded && (
                                  <ReleaseComparison
                                    item={ep}
                                    mediaLabel="episode"
                                    onSelect={candidate => handleSelectManualEpisodeRelease(ep.id, candidate)}
                                    onRestore={() => handleRestoreEpisodeRecommendation(ep.id)}
                                    onOpenExternal={openExternalUrl}
                                  />
                                )}
                              </article>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}

                  {/* MOVIE RELEASE WORKSPACE */}
                  {selectedItem.mediaType === 'movie' && (
                    <div className="panel-card movie-workspace">
                      <div className="panel-row">
                        <div>
                          <span className="section-kicker">MOVIE RELEASE INTELLIGENCE</span>
                          <h3>{selectedItem.title}</h3>
                        </div>
                        {movieRelease && (
                          <span className={`release-status status-${movieRelease.status}`}>
                            {statusBadgeLabel(movieRelease.status)}
                          </span>
                        )}
                      </div>

                      {checkingSources ? (
                        <p className="muted-copy">Scanning Torrentio movie releases...</p>
                      ) : !movieRelease ? (
                        <p className="muted-copy">Click Check Movie Sources to scan Torrentio releases with the active ranking profile.</p>
                      ) : (
                        <>
                          <p className="mono-caption">
                            {movieRelease.format || movieRelease.sourceError || getNoSourceCopy(resolutionPreference)}
                          </p>

                          <div className="episode-meta-row">
                            <span><b>Size:</b> {movieRelease.size || '—'}</span>
                            <span><b>Resolution:</b> {movieRelease.resolution || '—'}</span>
                            <span><b>Seeders:</b> {Number.isInteger(movieRelease.seeders) ? movieRelease.seeders.toLocaleString() : '—'}</span>
                            <span><b>Provider:</b> {movieRelease.provider || '—'}</span>
                            <span><b>Candidates:</b> {movieRelease.sourceCount || 0}</span>
                          </div>

                          {movieRelease.selectionReason && (
                            <p className="selection-reason">{movieRelease.selectionReason}</p>
                          )}

                          <div className="episode-actions">
                            {movieRelease.magnet && (
                              <>
                                <button
                                  type="button"
                                  className="primary-button small-button"
                                  onClick={() => handleOpenMagnet(movieRelease.magnet)}
                                >
                                  Open Magnet
                                </button>
                                <button
                                  type="button"
                                  className="secondary-button small-button"
                                  onClick={() => handleCopyText(movieRelease.magnet, 'Movie magnet link copied')}
                                >
                                  Copy Magnet
                                </button>
                              </>
                            )}

                            {movieRelease.candidates?.length > 0 && (
                              <button
                                type="button"
                                className="ghost-button small-button"
                                onClick={() => setExpandedCompare(prev => ({ ...prev, [movieRelease.id]: !prev[movieRelease.id] }))}
                              >
                                {expandedCompare[movieRelease.id]
                                  ? 'Hide Candidates'
                                  : `Compare Releases (${movieRelease.candidates.length})`}
                              </button>
                            )}

                            {geminiAvailable && movieRelease.candidates?.length > 0 && (
                              <button
                                type="button"
                                className="ghost-button small-button"
                                disabled={Boolean(geminiExplainLoading[movieRelease.id])}
                                onClick={() => handleExplainWithGemini(movieRelease, { isMovie: true })}
                              >
                                {geminiExplainLoading[movieRelease.id] ? 'Analyzing...' : '✦ Ask Gemini'}
                              </button>
                            )}
                          </div>

                          <GeminiInsightBox
                            analysis={geminiExplain[movieRelease.id]}
                            loading={Boolean(geminiExplainLoading[movieRelease.id])}
                          />

                          {expandedCompare[movieRelease.id] && (
                            <ReleaseComparison
                              item={movieRelease}
                              mediaLabel="movie"
                              onSelect={handleSelectManualMovieRelease}
                              onRestore={handleRestoreMovieRecommendation}
                              onOpenExternal={openExternalUrl}
                            />
                          )}
                        </>
                      )}
                    </div>
                  )}
                </div>
              )}

              {!selectedItem && results.length > 0 && (
                <div className="results-block">
                  <div className="results-header">
                    <div>
                      <span className="section-kicker">SEARCH RESULTS</span>
                      <h2>
                        {results.length} {results.length === 1 ? 'match' : 'matches'}
                      </h2>
                    </div>
                    {resolution?.confidence && (
                      <span className="confidence-pill">
                        Confidence: {resolution.confidence}
                      </span>
                    )}
                  </div>
                  <div className="results-grid">
                    {results.map(item => (
                      <button
                        key={item.id}
                        type="button"
                        className="result-card"
                        onClick={() => handleSelectTitle(item)}
                      >
                        <div className="result-poster">
                          {item.poster ? (
                            <img src={item.poster} alt="" loading="lazy" />
                          ) : (
                            <span>{formatTitleInitials(item.title)}</span>
                          )}
                        </div>
                        <div className="result-copy">
                          <strong>{item.title}</strong>
                          <span>
                            {getMediaTypeFallbackLabel(item, mediaType)}
                            {item.year ? ` · ${item.year}` : ''}
                            {item.imdbRating ? ` · ★ ${item.imdbRating}` : ''}
                          </span>
                          <span className="mono-caption">{item.id}</span>
                        </div>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </section>
          )}

          {activeTab === 'library' && (
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
          )}

          {activeTab === 'updates' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">AUTOMATIC UPDATE ARCHITECTURE</span>
                  <h1>
                    Desktop <span>Updates</span>
                  </h1>
                  <p className="stage-copy">
                    Signed background updates keep desktop shells in sync automatically while hosted backend improvements arrive immediately without reinstalling.
                  </p>
                </div>
              </header>

              <div className="panel-card">
                <div className="panel-row">
                  <div>
                    <span className="section-kicker">CURRENT STATUS</span>
                    <h2>SeasonFetch v{updater?.currentVersion || appInfo?.version || '0.1.0'}</h2>
                    <p className="muted-copy">{updater?.message || 'Loading updater state...'}</p>
                  </div>
                  <span className={`state-badge state-${updater?.status || 'idle'}`}>
                    {updater?.status || 'idle'}
                  </span>
                </div>

                {updater?.recoveryRequired && (
                  <div className="notice-box" style={{ marginTop: 14 }}>
                    <strong>Recovery Notice:</strong> {updater.recoveryMessage || updater.message}
                  </div>
                )}

                {updater?.status === 'downloading' && (
                  <div className="progress-block">
                    <div className="progress-track">
                      <div className="progress-fill" style={{ width: `${updater.progressPercent}%` }} />
                    </div>
                    <span className="mono-caption">{updater.progressPercent}% downloaded</span>
                  </div>
                )}

                <div className="meta-grid">
                  <div className="meta-cell">
                    <span className="section-kicker">UPDATE CHANNEL</span>
                    <strong>{channelLabel} ({updater?.updaterChannel || 'latest'})</strong>
                  </div>
                  <div className="meta-cell">
                    <span className="section-kicker">PUBLIC UPDATE FEED</span>
                    <strong className="mono-caption" style={{ marginTop: 0 }}>
                      {updater?.feedRepository || 'rightcodes1/SeasonFetch-Desktop-Releases'}
                    </strong>
                  </div>
                  <div className="meta-cell">
                    <span className="section-kicker">LAST CHECKED</span>
                    <strong>
                      {updater?.lastCheckedAt
                        ? new Date(updater.lastCheckedAt).toLocaleString()
                        : 'Not yet checked'}
                    </strong>
                  </div>
                </div>

                <div className="button-row">
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => checkForUpdates()}
                    disabled={updater?.status === 'checking' || updater?.status === 'downloading'}
                  >
                    Check for Updates
                  </button>

                  {updater?.status === 'downloading' && (
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => cancelUpdateDownload()}
                    >
                      Cancel Download
                    </button>
                  )}

                  {(updater?.status === 'available' || updater?.status === 'error' || updater?.status === 'recovery')
                    && updater?.errorCode !== 'SIGNATURE_VERIFICATION_FAILED'
                    && updater?.errorCode !== 'INTEGRITY_VERIFICATION_FAILED' && (
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => downloadAvailableUpdate()}
                    >
                      {updater?.status === 'available' ? `Download v${updater.availableVersion}` : 'Retry Download'}
                    </button>
                  )}

                  {updater?.status === 'downloaded' && (
                    <>
                      <button
                        type="button"
                        className="primary-button"
                        onClick={() => installDownloadedUpdate()}
                      >
                        Restart &amp; Install v{updater.availableVersion}
                      </button>
                      <button
                        type="button"
                        className="secondary-button"
                        onClick={() => postponeUpdateInstall()}
                      >
                        Install on Quit
                      </button>
                    </>
                  )}

                  {updater?.recoveryRequired && (
                    <button
                      type="button"
                      className="ghost-button"
                      onClick={() => dismissUpdaterRecovery()}
                    >
                      Clear Recovery Notice
                    </button>
                  )}

                  {updater?.releasesUrl && (
                    <button
                      type="button"
                      className="ghost-button"
                      onClick={() => openExternalUrl(updater.releasesUrl)}
                    >
                      Open Releases Fallback
                    </button>
                  )}
                </div>
              </div>
            </section>
          )}

          {activeTab === 'settings' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">LOCAL DESKTOP PREFERENCES</span>
                  <h1>
                    Desktop <span>Settings</span>
                  </h1>
                  <p className="stage-copy">
                    Non-sensitive desktop preferences are saved locally in your Windows user profile. Auth sessions are encrypted separately with Windows DPAPI.
                  </p>
                </div>
              </header>

              <div className="panel-card">
                <div className="setting-row">
                  <div>
                    <strong>Release Channel</strong>
                    <p className="muted-copy">
                      Stable delivers verified releases. Preview opts into early prerelease builds.
                    </p>
                  </div>
                  <div className="search-mode">
                    <button
                      type="button"
                      aria-pressed={(settings?.updateChannel || 'stable') === 'stable'}
                      onClick={() => handleSettingsPatch({ updateChannel: 'stable' })}
                    >
                      Stable
                    </button>
                    <button
                      type="button"
                      aria-pressed={settings?.updateChannel === 'preview'}
                      onClick={() => handleSettingsPatch({ updateChannel: 'preview' })}
                    >
                      Preview
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Default Catalog Tab</strong>
                    <p className="muted-copy">
                      Choose whether Discover opens in Series or Movies mode by default.
                    </p>
                  </div>
                  <div className="search-mode">
                    <button
                      type="button"
                      aria-pressed={(settings?.defaultSearchMode || 'series') === 'series'}
                      onClick={() => handleSettingsPatch({ defaultSearchMode: 'series' })}
                    >
                      Series
                    </button>
                    <button
                      type="button"
                      aria-pressed={settings?.defaultSearchMode === 'movie'}
                      onClick={() => handleSettingsPatch({ defaultSearchMode: 'movie' })}
                    >
                      Movies
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Check for updates automatically</strong>
                    <p className="muted-copy">
                      Check when SeasonFetch launches and every 4 hours while running.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    checked={Boolean(settings?.autoCheckUpdates ?? true)}
                    onChange={event => handleSettingsPatch({ autoCheckUpdates: event.target.checked })}
                  />
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Download updates in the background</strong>
                    <p className="muted-copy">
                      Verify signatures and stage updates automatically, then ask before restarting.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    checked={Boolean(settings?.autoDownloadUpdates ?? true)}
                    onChange={event => handleSettingsPatch({ autoDownloadUpdates: event.target.checked })}
                  />
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Minimize to System Tray on close</strong>
                    <p className="muted-copy">
                      Keep SeasonFetch running quietly in the Windows taskbar tray when closing the window.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    checked={Boolean(settings?.minimizeToTray ?? false)}
                    onChange={event => handleSettingsPatch({ minimizeToTray: event.target.checked })}
                  />
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Windows Magnet Protocol Handler</strong>
                    <p className="muted-copy">
                      {magnetHandler?.registered
                        ? `Detected (${magnetHandler.source}): ${magnetHandler.commandHint || magnetHandler.handlerName}`
                        : magnetHandler?.guidance || 'Inspecting Windows magnet: registration...'}
                    </p>
                  </div>
                  <div className="button-row">
                    <button type="button" className="ghost-button small-button" onClick={refreshMagnetHandler}>
                      Re-check
                    </button>
                    <button
                      type="button"
                      className="secondary-button small-button"
                      onClick={() => openExternalUrl('ms-settings:defaultapps')}
                    >
                      Default Apps
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Session Vault Security</strong>
                    <p className="muted-copy">
                      Supabase session tokens are stored via Electron <code>safeStorage</code> (Windows DPAPI) and never written in plaintext.
                    </p>
                  </div>
                  <span className="channel-pill">
                    {appInfo?.authVaultEncrypted ? 'DPAPI Active' : 'Memory Only'}
                  </span>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Connected Backend &amp; Diagnostics</strong>
                    <p className="mono-caption">{settings?.backendUrl || appInfo?.backendUrl || 'https://season-fetch.vercel.app'}</p>
                  </div>
                  <div className="button-row">
                    <button type="button" className="secondary-button small-button" onClick={refreshHealth}>
                      Test Health
                    </button>
                    <button type="button" className="ghost-button small-button" onClick={handleExportDiagnostics}>
                      Export Diagnostics
                    </button>
                  </div>
                </div>

                {settingsNotice && <div className="settings-toast">{settingsNotice}</div>}
              </div>
            </section>
          )}
        </main>
      </div>

      {showAuthModal && (
        <AuthModal
          supabase={supabase}
          contextNote={authContextNote}
          onClose={outcome => {
            setShowAuthModal(false);
            setAuthContextNote('');
            if (outcome !== 'authenticated') {
              library.cancelPendingSave();
            } else {
              showToast('Signed in to SeasonFetch');
            }
          }}
        />
      )}

      {toast && (
        <div className="toast-pill" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
