/**
 * Gemini assistant endpoints previously lived on the website backend.
 * Because SeasonFetch Desktop restricts the website backend strictly to
 * account synchronization, this helper remains cleanly disabled until a
 * user-configured direct local/BYOK AI integration is enabled.
 */
export const geminiApi = {
  async getStatus() {
    return { configured: false };
  },

  async searchAssist() {
    return [];
  },

  async explainSource() {
    throw new Error('AI source explanation is disabled in independent desktop mode.');
  }
};
