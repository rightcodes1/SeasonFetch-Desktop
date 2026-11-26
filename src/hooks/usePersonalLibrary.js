import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { libraryApi } from '../services/library.js';
import {
  buildLibrarySnapshot,
  computeLibraryCounts,
  indexLibraryItems,
  libraryItemKey,
  removeLocalLibraryItem,
  resolveImdbId,
  resolveMediaType,
  upsertLocalLibraryItem
} from '../utils/libraryState.js';
import { libraryStatusLabel } from '../../shared/libraryConstants.js';

export function usePersonalLibrary({
  supabase,
  user,
  suspended = false,
  onRequireAuth,
  onSessionExpired,
  onToast,
  onNotice
}) {
  const [items, setItems] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [busyKeys, setBusyKeys] = useState({});
  const [pendingSave, setPendingSave] = useState(null);
  const activeDayPingedUserRef = useRef('');
  const pendingInFlightRef = useRef(false);
  const loadGenerationRef = useRef(0);

  const itemsByKey = useMemo(() => indexLibraryItems(items), [items]);
  const counts = useMemo(() => computeLibraryCounts(items), [items]);

  const setKeyBusy = useCallback((key, isBusy) => {
    setBusyKeys(current => {
      if (isBusy) return { ...current, [key]: true };
      const next = { ...current };
      delete next[key];
      return next;
    });
  }, []);

  const loadLibrary = useCallback(async () => {
    const requestGeneration = ++loadGenerationRef.current;
    if (!supabase || !user || suspended) {
      setItems([]);
      setLoading(false);
      setError('');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const data = await libraryApi.list(supabase);
      if (requestGeneration !== loadGenerationRef.current) return;
      setItems(Array.isArray(data.items) ? data.items : []);
    } catch (loadError) {
      if (requestGeneration !== loadGenerationRef.current) return;
      if (loadError?.status === 401) {
        onSessionExpired?.();
        return;
      }
      setError(loadError.message || 'Unable to load your library right now.');
    } finally {
      if (requestGeneration === loadGenerationRef.current) {
        setLoading(false);
      }
    }
  }, [supabase, user, suspended, onSessionExpired]);

  useEffect(() => {
    if (!supabase || !user || suspended) {
      loadGenerationRef.current += 1;
      setItems([]);
      setLoading(false);
      setError('');
      activeDayPingedUserRef.current = '';
      return;
    }

    if (!pendingSave && !pendingInFlightRef.current) {
      loadLibrary();
    }

    if (activeDayPingedUserRef.current !== user.id) {
      activeDayPingedUserRef.current = user.id;
      libraryApi.pingActiveDay(supabase).catch(() => {});
    }
  }, [supabase, user, suspended, pendingSave, loadLibrary]);

  const executeSaveSnapshot = useCallback(async snapshot => {
    const key = libraryItemKey(snapshot.mediaType, snapshot.imdbId);
    loadGenerationRef.current += 1;
    setKeyBusy(key, true);
    setError('');
    try {
      const result = await libraryApi.save(supabase, snapshot.mediaType, snapshot.imdbId, snapshot.payload);
      if (result.item) {
        setItems(current => upsertLocalLibraryItem(current, result.item));
        onToast?.(
          result.created
            ? `Saved "${result.item.title}" to My Library`
            : `Updated "${result.item.title}" in My Library`
        );
      }
      return result.item || null;
    } catch (saveError) {
      if (saveError?.status === 401) {
        onSessionExpired?.();
        return null;
      }
      const message = saveError.message || 'Unable to save this title right now.';
      setError(message);
      onNotice?.(message);
      return null;
    } finally {
      setKeyBusy(key, false);
    }
  }, [supabase, setKeyBusy, onToast, onNotice, onSessionExpired]);

  useEffect(() => {
    if (!user || suspended || !supabase || !pendingSave || pendingInFlightRef.current) return;
    const intent = pendingSave;
    pendingInFlightRef.current = true;
    setPendingSave(null);
    executeSaveSnapshot(intent)
      .then(() => loadLibrary())
      .finally(() => {
        pendingInFlightRef.current = false;
      });
  }, [user, suspended, supabase, pendingSave, executeSaveSnapshot, loadLibrary]);

  const saveItem = useCallback(async (media, { fallbackMediaType = 'series', status = 'want_to_watch', isFavorite } = {}) => {
    const snapshot = buildLibrarySnapshot(media, { fallbackMediaType, status, isFavorite });
    if (!snapshot) {
      onNotice?.('This title is missing a valid IMDb ID and cannot be saved to your library.');
      return null;
    }
    if (suspended) {
      onNotice?.('Account suspended. Personal Library actions are currently disabled.');
      return null;
    }
    if (!user) {
      if (!supabase) {
        onNotice?.('Personal Library requires Supabase authentication configuration.');
        return null;
      }
      setPendingSave(snapshot);
      onRequireAuth?.(snapshot);
      return null;
    }
    return executeSaveSnapshot(snapshot);
  }, [user, suspended, supabase, onRequireAuth, onNotice, executeSaveSnapshot]);

  const updateStatus = useCallback(async (item, nextStatus) => {
    if (!user || !supabase || !item) return null;
    const key = libraryItemKey(item.mediaType, item.imdbId);
    loadGenerationRef.current += 1;
    setKeyBusy(key, true);
    setError('');
    try {
      const result = await libraryApi.update(supabase, item.mediaType, item.imdbId, { status: nextStatus });
      if (result.item) {
        setItems(current => upsertLocalLibraryItem(current, result.item));
        onToast?.(`Moved "${result.item.title}" to ${libraryStatusLabel(result.item.status)}`);
      }
      return result.item || null;
    } catch (updateError) {
      if (updateError?.status === 401) {
        onSessionExpired?.();
        return null;
      }
      const message = updateError.message || 'Unable to update status right now.';
      setError(message);
      onNotice?.(message);
      return null;
    } finally {
      setKeyBusy(key, false);
    }
  }, [user, supabase, setKeyBusy, onToast, onNotice, onSessionExpired]);

  const toggleFavorite = useCallback(async item => {
    if (!user || !supabase || !item) return null;
    const key = libraryItemKey(item.mediaType, item.imdbId);
    const nextFavorite = !item.isFavorite;
    loadGenerationRef.current += 1;
    setKeyBusy(key, true);
    setError('');
    try {
      const result = await libraryApi.update(supabase, item.mediaType, item.imdbId, { isFavorite: nextFavorite });
      if (result.item) {
        setItems(current => upsertLocalLibraryItem(current, result.item));
        onToast?.(
          result.item.isFavorite
            ? `Added "${result.item.title}" to Favorites`
            : `Removed "${result.item.title}" from Favorites`
        );
      }
      return result.item || null;
    } catch (favError) {
      if (favError?.status === 401) {
        onSessionExpired?.();
        return null;
      }
      const message = favError.message || 'Unable to update favorite right now.';
      setError(message);
      onNotice?.(message);
      return null;
    } finally {
      setKeyBusy(key, false);
    }
  }, [user, supabase, setKeyBusy, onToast, onNotice, onSessionExpired]);

  const removeItem = useCallback(async item => {
    if (!user || !supabase || !item) return false;
    const key = libraryItemKey(item.mediaType, item.imdbId);
    loadGenerationRef.current += 1;
    setKeyBusy(key, true);
    setError('');
    try {
      await libraryApi.remove(supabase, item.mediaType, item.imdbId);
      setItems(current => removeLocalLibraryItem(current, item.mediaType, item.imdbId));
      onToast?.(`Removed "${item.title}" from My Library`);
      return true;
    } catch (removeError) {
      if (removeError?.status === 401) {
        onSessionExpired?.();
        return false;
      }
      const message = removeError.message || 'Unable to remove this title right now.';
      setError(message);
      onNotice?.(message);
      return false;
    } finally {
      setKeyBusy(key, false);
    }
  }, [user, supabase, setKeyBusy, onToast, onNotice, onSessionExpired]);

  const getSavedItem = useCallback((media, fallbackMediaType = 'series') => {
    const imdbId = resolveImdbId(media);
    if (!imdbId) return null;
    const mediaType = resolveMediaType(media, fallbackMediaType);
    return itemsByKey.get(libraryItemKey(mediaType, imdbId)) || null;
  }, [itemsByKey]);

  const isItemBusy = useCallback((media, fallbackMediaType = 'series') => {
    const imdbId = resolveImdbId(media);
    if (!imdbId) return false;
    const mediaType = resolveMediaType(media, fallbackMediaType);
    return Boolean(busyKeys[libraryItemKey(mediaType, imdbId)]);
  }, [busyKeys]);

  const cancelPendingSave = useCallback(() => {
    setPendingSave(null);
  }, []);

  return {
    items,
    counts,
    loading,
    error,
    busyKeys,
    pendingSave,
    loadLibrary,
    saveItem,
    updateStatus,
    toggleFavorite,
    removeItem,
    getSavedItem,
    isItemBusy,
    cancelPendingSave
  };
}
