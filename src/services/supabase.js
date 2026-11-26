import { createClient } from '@supabase/supabase-js';

const supabaseUrl = String(
  import.meta.env.VITE_SUPABASE_URL
  || (typeof __SEASONFETCH_SUPABASE_URL__ !== 'undefined' ? __SEASONFETCH_SUPABASE_URL__ : '')
).trim();

const supabaseKey = String(
  import.meta.env.VITE_SUPABASE_PUBLISHABLE_KEY
  || import.meta.env.VITE_SUPABASE_ANON_KEY
  || (typeof __SEASONFETCH_SUPABASE_KEY__ !== 'undefined' ? __SEASONFETCH_SUPABASE_KEY__ : '')
).trim();

function createDesktopSecureStorageAdapter() {
  const memoryFallback = new Map();

  return {
    async getItem(key) {
      if (typeof window !== 'undefined' && window.seasonfetchDesktop?.authStorage) {
        return window.seasonfetchDesktop.authStorage.getItem(key);
      }
      return memoryFallback.has(key) ? memoryFallback.get(key) : null;
    },
    async setItem(key, value) {
      if (typeof window !== 'undefined' && window.seasonfetchDesktop?.authStorage) {
        await window.seasonfetchDesktop.authStorage.setItem(key, value);
        return;
      }
      memoryFallback.set(key, value);
    },
    async removeItem(key) {
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
