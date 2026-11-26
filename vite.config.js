import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '');

  // Only public Supabase URL and publishable/anon key are exposed to the renderer bundle.
  // Never pass service-role or signing keys here.
  const publicSupabaseUrl = String(
    env.VITE_SUPABASE_URL
    || env.NEXT_PUBLIC_SUPABASE_URL
    || env.SUPABASE_URL
    || ''
  ).trim();

  const publicSupabaseKey = String(
    env.VITE_SUPABASE_PUBLISHABLE_KEY
    || env.VITE_SUPABASE_ANON_KEY
    || env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY
    || env.NEXT_PUBLIC_SUPABASE_ANON_KEY
    || env.SUPABASE_PUBLISHABLE_KEY
    || env.SUPABASE_ANON_KEY
    || ''
  ).trim();

  return {
    base: './',
    plugins: [react()],
    define: {
      __SEASONFETCH_SUPABASE_URL__: JSON.stringify(publicSupabaseUrl),
      __SEASONFETCH_SUPABASE_KEY__: JSON.stringify(publicSupabaseKey)
    },
    server: {
      host: '127.0.0.1',
      port: 5175,
      strictPort: true
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
      rollupOptions: {
        output: {
          manualChunks: {
            'react-vendor': ['react', 'react-dom'],
            'supabase-vendor': ['@supabase/supabase-js']
          }
        }
      }
    }
  };
});
