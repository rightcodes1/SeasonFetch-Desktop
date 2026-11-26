import { useCallback, useEffect, useReducer, useRef } from 'react';
import {
  AUTH_STATUS,
  authReducer,
  createInitialAuthState
} from './authStateMachine.js';
import { desktopApiRequest } from '../services/desktopApi.js';

async function fetchAccountSession(accessToken) {
  const response = await desktopApiRequest({
    method: 'GET',
    path: '/api/account/session',
    headers: {
      Authorization: `Bearer ${accessToken}`
    },
    timeoutMs: 12_000
  });

  if (!response.ok) {
    const error = new Error(response.error || 'Unable to verify account status.');
    error.status = response.status;
    throw error;
  }

  return response.data || {};
}

export function useAuthController({ supabase, onToast, onNotice }) {
  const [state, dispatch] = useReducer(
    authReducer,
    undefined,
    () => createInitialAuthState({ configured: Boolean(supabase) })
  );

  const subscriptionRef = useRef(null);

  useEffect(() => {
    if (!supabase) return undefined;
    if (subscriptionRef.current) return undefined;

    let active = true;

    const { data: listener } = supabase.auth.onAuthStateChange((event, session) => {
      if (!active) return;
      dispatch({ type: 'AUTH_EVENT', event, user: session?.user || null });
    });
    subscriptionRef.current = listener.subscription;

    supabase.auth.getSession()
      .then(({ data, error }) => {
        if (!active) return;
        dispatch({ type: 'INITIAL_SESSION_RESOLVED', user: error ? null : (data.session?.user || null) });
      })
      .catch(() => {
        if (!active) return;
        dispatch({ type: 'INITIAL_SESSION_RESOLVED', user: null });
      });

    return () => {
      active = false;
      if (subscriptionRef.current) {
        subscriptionRef.current.unsubscribe();
        subscriptionRef.current = null;
      }
    };
  }, [supabase]);

  const syncAccountSession = useCallback(async (targetUser, targetEpoch) => {
    if (!supabase || !targetUser) return;
    dispatch({ type: 'ACCOUNT_STATUS_LOADING', epoch: targetEpoch });
    try {
      const { data } = await supabase.auth.getSession();
      const accessToken = data.session?.access_token;
      if (!accessToken || data.session?.user?.id !== targetUser.id) {
        dispatch({ type: 'SESSION_EXPIRED' });
        return;
      }

      const account = await fetchAccountSession(accessToken);
      dispatch({
        type: 'ACCOUNT_STATUS_RESOLVED',
        epoch: targetEpoch,
        userId: targetUser.id,
        profile: account.profile || null,
        isAdmin: Boolean(account.isAdmin),
        suspended: Boolean(account.suspended),
        suspensionReason: account.suspensionReason || ''
      });
    } catch (error) {
      if (error?.status === 401) {
        await supabase.auth.signOut({ scope: 'local' }).catch(() => {});
        dispatch({
          type: 'SESSION_EXPIRED',
          message: 'Your session expired or was revoked. Sign in again to continue.'
        });
        return;
      }
      dispatch({
        type: 'ACCOUNT_STATUS_ERROR',
        epoch: targetEpoch,
        message: error.message || 'Unable to verify account details right now.'
      });
    }
  }, [supabase]);

  useEffect(() => {
    if (!supabase || !state.user) return;
    if (state.status !== AUTH_STATUS.AUTHENTICATED && state.status !== AUTH_STATUS.SUSPENDED) return;
    syncAccountSession(state.user, state.sessionEpoch);
  }, [supabase, state.user?.id, state.sessionEpoch, state.status === AUTH_STATUS.AUTHENTICATED, syncAccountSession]);

  const signOut = useCallback(async () => {
    if (!supabase) {
      onNotice?.('Sign-out is unavailable because Supabase authentication is not configured.');
      return false;
    }
    try {
      const { error } = await supabase.auth.signOut();
      if (error) throw error;
      dispatch({ type: 'AUTH_EVENT', event: 'SIGNED_OUT', user: null });
      onToast?.('Signed out');
      return true;
    } catch (error) {
      onNotice?.(`Unable to sign out: ${error.message || 'Authentication service did not confirm sign-out.'}`);
      return false;
    }
  }, [supabase, onToast, onNotice]);

  const handleSessionExpired = useCallback(async message => {
    if (supabase) {
      await supabase.auth.signOut({ scope: 'local' }).catch(() => {});
    }
    dispatch({ type: 'SESSION_EXPIRED', message });
  }, [supabase]);

  const retryAccountSync = useCallback(() => {
    if (state.user) syncAccountSession(state.user, state.sessionEpoch);
  }, [state.user, state.sessionEpoch, syncAccountSession]);

  const dismissSessionNotice = useCallback(() => {
    dispatch({ type: 'DISMISS_SESSION_NOTICE' });
  }, []);

  const authenticatedUser = state.status === AUTH_STATUS.AUTHENTICATED || state.status === AUTH_STATUS.SUSPENDED
    ? state.user
    : null;

  return {
    ...state,
    authenticatedUser,
    signOut,
    handleSessionExpired,
    retryAccountSync,
    dismissSessionNotice
  };
}
