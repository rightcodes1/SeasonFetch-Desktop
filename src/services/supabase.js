import { createClient } from '@supabase/supabase-js';
import { invoke } from '@tauri-apps/api/core';

const supabaseUrl = String(
  import.meta.env.VITE_SUPABASE_URL
  || (typeof __SEASONFETCH_SUPABASE_URL__ !== 'undefined' ? __SEASONFETCH_SUPABASE_URL__ : '')
).trim();

const supabaseKey = String(
  import.meta.env.VITE_SUPABASE_PUBLISHABLE_KEY
  || import.meta.env.VITE_SUPABASE_ANON_KEY
  || (typeof __SEASONFETCH_SUPABASE_KEY__ !== 'undefined' ? __SEASONFETCH_SUPABASE_KEY__ : '')
).trim();

function isTauri() {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

function createDesktopSecureStorageAdapter() {
  const memoryFallback = new Map();

  return {
    async getItem(key) {
      if (isTauri()) {
        return invoke('auth_storage_get', { key });
      }
      if (typeof window !== 'undefined' && window.seasonfetchDesktop?.authStorage) {
        return window.seasonfetchDesktop.authStorage.getItem(key);
      }
      return memoryFallback.has(key) ? memoryFallback.get(key) : null;
    },
    async setItem(key, value) {
      if (isTauri()) {
        await invoke('auth_storage_set', { key, value });
        return;
      }
      if (typeof window !== 'undefined' && window.seasonfetchDesktop?.authStorage) {
        await window.seasonfetchDesktop.authStorage.setItem(key, value);
        return;
      }
      memoryFallback.set(key, value);
    },
    async removeItem(key) {
      if (isTauri()) {
        await invoke('auth_storage_remove', { key });
        return;
      }
      if (typeof window !== 'undefined' && window.seasonfetchDesktop?.authStorage) {
        await window.seasonfetchDesktop.authStorage.removeItem(key);
        return;
      }
      memoryFallback.delete(key);
    }
  };
}

export const supabase = supabaseUrl && supabaseKey
  ? createClient(supabaseUrl, supabaseKey, {
      auth: {
        storageKey: 'seasonfetch-auth-session',
        storage: createDesktopSecureStorageAdapter(),
        persistSession: true,
        autoRefreshToken: true,
        detectSessionInUrl: false
      }
    })
  : null;

export const authConfigured = Boolean(supabase);
