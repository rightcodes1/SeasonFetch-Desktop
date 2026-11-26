export const AUTH_STATUS = {
  INITIALIZING: 'initializing',
  ANONYMOUS: 'anonymous',
  AUTHENTICATED: 'authenticated',
  SUSPENDED: 'suspended'
};

export function validateNewPassword(password, confirmPassword) {
  const value = String(password || '');
  if (value.length < 8) {
    return { valid: false, error: 'Password must be at least 8 characters long.' };
  }
  if (value.length > 128) {
    return { valid: false, error: 'Password must be 128 characters or fewer.' };
  }
  if (!/[A-Za-z]/.test(value) || !/\d/.test(value)) {
    return { valid: false, error: 'Use at least one letter and one number.' };
  }
  if (confirmPassword !== undefined && value !== String(confirmPassword)) {
    return { valid: false, error: 'The passwords do not match.' };
  }
  return { valid: true, error: '' };
}

export function normalizeAuthError(error, context = 'sign-in') {
  const raw = String(error?.message || '').trim();
  const code = String(error?.code || '').trim().toLowerCase();
  const lower = `${code} ${raw}`.toLowerCase();

  if (lower.includes('failed to fetch') || lower.includes('network') || lower.includes('timeout')) {
    return 'Network error while contacting the authentication service. Check your connection and try again.';
  }
  if (lower.includes('over_email_send_rate_limit') || lower.includes('email rate limit')) {
    return 'Too many email link requests. Please wait for the cooldown timer before trying again.';
  }
  if (lower.includes('rate limit') || lower.includes('too many requests') || error?.status === 429) {
    return 'Too many authentication attempts. Please wait before trying again.';
  }
  if (context === 'sign-in' && (lower.includes('invalid login credentials') || lower.includes('invalid credentials'))) {
    return 'Incorrect email or password. Please check your credentials and try again.';
  }
  if (context === 'sign-in' && lower.includes('email not confirmed')) {
    return 'Your email address has not been verified yet. Check your inbox or request a new verification email.';
  }
  if (context === 'register' && lower.includes('already registered')) {
    return 'An account with this email already exists. Try signing in or resetting your password.';
  }
  return raw || 'Authentication failed. Please try again.';
}

export function createInitialAuthState({ configured = true } = {}) {
  return {
    status: configured ? AUTH_STATUS.INITIALIZING : AUTH_STATUS.ANONYMOUS,
    user: null,
    profile: null,
    isAdmin: false,
    suspended: false,
    suspensionReason: '',
    profileLoading: false,
    profileError: '',
    sessionExpiredMessage: '',
    sessionEpoch: 0
  };
}

function clearAccountFields(state, overrides = {}) {
  return {
    ...state,
    user: null,
    profile: null,
    isAdmin: false,
    suspended: false,
    suspensionReason: '',
    profileLoading: false,
    profileError: '',
    sessionEpoch: state.sessionEpoch + 1,
    ...overrides
  };
}

export function authReducer(state, action) {
  switch (action.type) {
    case 'INITIAL_SESSION_RESOLVED': {
      const user = action.user || null;
      if (!user) {
        return clearAccountFields(state, { status: AUTH_STATUS.ANONYMOUS });
      }
      return {
        ...state,
        status: AUTH_STATUS.AUTHENTICATED,
        user,
        profileLoading: true,
        profileError: '',
        sessionExpiredMessage: '',
        sessionEpoch: state.sessionEpoch + 1
      };
    }

    case 'AUTH_EVENT': {
      const { event, user } = action;
      if (event === 'SIGNED_OUT') {
        return clearAccountFields(state, { status: AUTH_STATUS.ANONYMOUS });
      }
      if (event === 'SIGNED_IN') {
        if (!user) return state;
        if (
          (state.status === AUTH_STATUS.AUTHENTICATED || state.status === AUTH_STATUS.SUSPENDED)
          && state.user?.id === user.id
        ) {
          return { ...state, user, sessionExpiredMessage: '' };
        }
        return {
          ...state,
          status: AUTH_STATUS.AUTHENTICATED,
          user,
          profile: null,
          isAdmin: false,
          suspended: false,
          suspensionReason: '',
          profileLoading: true,
          profileError: '',
          sessionExpiredMessage: '',
          sessionEpoch: state.sessionEpoch + 1
        };
      }
      if (event === 'TOKEN_REFRESHED' || event === 'USER_UPDATED') {
        if (!user) {
          return clearAccountFields(state, {
            status: AUTH_STATUS.ANONYMOUS,
            sessionExpiredMessage: 'Your session expired. Sign in again to access your library.'
          });
        }
        if (state.user && state.user.id !== user.id) {
          return {
            ...state,
            status: AUTH_STATUS.AUTHENTICATED,
            user,
            profile: null,
            isAdmin: false,
            suspended: false,
            suspensionReason: '',
            profileLoading: true,
            profileError: '',
            sessionEpoch: state.sessionEpoch + 1
          };
        }
        return { ...state, user };
      }
      return state;
    }

    case 'ACCOUNT_STATUS_LOADING':
      if (action.epoch !== state.sessionEpoch) return state;
      return { ...state, profileLoading: true, profileError: '' };

    case 'ACCOUNT_STATUS_RESOLVED':
      if (action.epoch !== state.sessionEpoch) return state;
      if (!state.user || state.user.id !== action.userId) return state;
      return {
        ...state,
        status: action.suspended ? AUTH_STATUS.SUSPENDED : AUTH_STATUS.AUTHENTICATED,
        profile: action.profile || null,
        isAdmin: Boolean(action.isAdmin && !action.suspended),
        suspended: Boolean(action.suspended),
        suspensionReason: action.suspensionReason || '',
        profileLoading: false,
        profileError: ''
      };

    case 'ACCOUNT_STATUS_ERROR':
      if (action.epoch !== state.sessionEpoch) return state;
      return {
        ...state,
        profileLoading: false,
        profileError: action.message || 'Unable to verify account status right now.'
      };

    case 'SESSION_EXPIRED':
      return clearAccountFields(state, {
        status: AUTH_STATUS.ANONYMOUS,
        sessionExpiredMessage: action.message || 'Your session expired. Sign in again to continue.'
      });

    case 'DISMISS_SESSION_NOTICE':
      return { ...state, sessionExpiredMessage: '' };

    default:
      return state;
  }
}
