import React, { useState } from 'react';
import { normalizeAuthError, validateNewPassword } from '../auth/authStateMachine.js';
import {
  formatCooldownDuration,
  getCooldownState,
  isEmailRateLimitError,
  recordCooldownAttempt
} from '../auth/emailCooldown.js';

const WEB_RECOVERY_REDIRECT = 'https://season-fetch.vercel.app/auth/recovery';
const WEB_SIGNUP_REDIRECT = 'https://season-fetch.vercel.app';

export function AuthModal({ supabase, onClose, contextNote = '' }) {
  const [mode, setMode] = useState('sign-in');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const isRegister = mode === 'register';
  const isRecovery = mode === 'recovery';

  function switchMode(nextMode) {
    setMode(nextMode);
    setError('');
    setMessage('');
    setPassword('');
    setConfirmPassword('');
  }

  async function handleSubmit(event) {
    event.preventDefault();
    if (!supabase) {
      setError('Supabase authentication is not configured in this build.');
      return;
    }

    setBusy(true);
    setError('');
    setMessage('');

    try {
      const cleanEmail = email.trim();
      if (isRecovery) {
        const cooldown = getCooldownState('recovery');
        if (cooldown.active) {
          setError(`Please wait ${formatCooldownDuration(cooldown.remainingMs)} before requesting another reset link.`);
          return;
        }
        const { error: resetError } = await supabase.auth.resetPasswordForEmail(cleanEmail, {
          redirectTo: WEB_RECOVERY_REDIRECT
        });
        if (resetError) throw resetError;
        const next = recordCooldownAttempt('recovery');
        setMessage(`If an account exists for that email, a password reset link is on its way. Next request available in ${formatCooldownDuration(next.durationMs)}.`);
      } else if (isRegister) {
        const validation = validateNewPassword(password, confirmPassword);
        if (!validation.valid) {
          setError(validation.error);
          return;
        }
        const { data, error: signUpError } = await supabase.auth.signUp({
          email: cleanEmail,
          password,
          options: {
            emailRedirectTo: WEB_SIGNUP_REDIRECT,
            data: { display_name: displayName.trim().slice(0, 80) }
          }
        });
        if (signUpError) throw signUpError;
        if (data.session) {
          onClose('authenticated');
          return;
        }
        recordCooldownAttempt('verification');
        setMessage('Account created. Check your inbox to verify your email, then sign in here.');
        setMode('sign-in');
        setPassword('');
        setConfirmPassword('');
      } else {
        const { error: signInError } = await supabase.auth.signInWithPassword({
          email: cleanEmail,
          password
        });
        if (signInError) throw signInError;
        setPassword('');
        onClose('authenticated');
      }
    } catch (submitError) {
      if (isRecovery && isEmailRateLimitError(submitError)) {
        const next = recordCooldownAttempt('recovery');
        setError(`Email rate limit reached. Wait ${formatCooldownDuration(next.durationMs)} before retrying.`);
      } else {
        setError(normalizeAuthError(submitError, mode));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="modal-overlay"
      role="presentation"
      onMouseDown={event => event.target === event.currentTarget && !busy && onClose('dismissed')}
    >
      <section className="modal-card" role="dialog" aria-modal="true" aria-labelledby="desktop-auth-title">
        <div className="modal-header">
          <div>
            <span className="section-kicker">SEASONFETCH ACCOUNT</span>
            <h2 id="desktop-auth-title">
              {isRecovery ? 'Reset your password' : isRegister ? 'Create your account' : 'Sign in to SeasonFetch'}
            </h2>
          </div>
          <button
            type="button"
            className="ghost-button"
            onClick={() => !busy && onClose('dismissed')}
            aria-label="Close sign in dialog"
          >
            ✕
          </button>
        </div>

        <p className="muted-copy">
          {isRecovery
            ? 'Enter your account email and we will send a single-use password reset link.'
            : contextNote || 'Sign in to sync your Personal Library across SeasonFetch Web and Desktop. Session tokens are encrypted with Windows DPAPI.'}
        </p>

        <form className="auth-form" onSubmit={handleSubmit}>
          {isRegister && (
            <label className="field-group">
              <span>Display name</span>
              <input
                type="text"
                value={displayName}
                onChange={event => setDisplayName(event.target.value)}
                maxLength={80}
                disabled={busy}
                placeholder="Optional display name"
              />
            </label>
          )}

          <label className="field-group">
            <span>Email</span>
            <input
              type="email"
              value={email}
              onChange={event => setEmail(event.target.value)}
              required
              disabled={busy}
              autoComplete="email"
              placeholder="you@example.com"
            />
          </label>

          {!isRecovery && (
            <label className="field-group">
              <span>Password</span>
              <div className="password-row">
                <input
                  type={showPassword ? 'text' : 'password'}
                  value={password}
                  onChange={event => setPassword(event.target.value)}
                  required
                  minLength={isRegister ? 8 : 6}
                  disabled={busy}
                  autoComplete={isRegister ? 'new-password' : 'current-password'}
                />
                <button
                  type="button"
                  className="ghost-button"
                  onClick={() => setShowPassword(v => !v)}
                  disabled={busy}
                >
                  {showPassword ? 'Hide' : 'Show'}
                </button>
              </div>
              {isRegister && (
                <small className="field-hint">At least 8 characters including a letter and a number.</small>
              )}
            </label>
          )}

          {isRegister && (
            <label className="field-group">
              <span>Confirm password</span>
              <input
                type={showPassword ? 'text' : 'password'}
                value={confirmPassword}
                onChange={event => setConfirmPassword(event.target.value)}
                required
                minLength={8}
                disabled={busy}
                autoComplete="new-password"
              />
            </label>
          )}

          {error && <div className="notice-box" role="alert">{error}</div>}
          {message && <div className="success-box" role="status">{message}</div>}

          <button type="submit" className="primary-button full-width" disabled={busy}>
            {busy
              ? 'Please wait...'
              : isRecovery
                ? 'Send reset link'
                : isRegister
                  ? 'Create account'
                  : 'Sign in'}
          </button>
        </form>

        <div className="auth-footer-links">
          {!isRecovery && (
            <button type="button" className="inline-link" onClick={() => switchMode('recovery')}>
              Forgot password?
            </button>
          )}
          <button
            type="button"
            className="inline-link"
            onClick={() => switchMode(isRegister || isRecovery ? 'sign-in' : 'register')}
          >
            {isRegister || isRecovery ? 'Back to sign in' : 'Create an account'}
          </button>
        </div>
      </section>
    </div>
  );
}
