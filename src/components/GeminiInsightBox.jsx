import React from 'react';

export function GeminiInsightBox({ analysis, loading }) {
  if (loading) {
    return (
      <div className="gemini-box">
        <span className="section-kicker">GEMINI SOURCE INTELLIGENCE</span>
        <p className="muted-copy">Analyzing candidate release metadata with SeasonFetch Gemini...</p>
      </div>
    );
  }

  if (!analysis) return null;

  if (analysis.error) {
    return (
      <div className="gemini-box gemini-error">
        <span className="section-kicker">GEMINI SOURCE INTELLIGENCE</span>
        <p className="error-copy">{analysis.error}</p>
      </div>
    );
  }

  const summary = analysis.summary || analysis.explanation || analysis.reason || '';
  const recommendation = analysis.recommendation || analysis.recommendedTitle || '';
  const highlights = Array.isArray(analysis.highlights)
    ? analysis.highlights
    : Array.isArray(analysis.notes)
      ? analysis.notes
      : [];

  return (
    <div className="gemini-box">
      <div className="gemini-header">
        <span className="section-kicker">GEMINI SOURCE EXPLANATION</span>
        {analysis.model && <span className="mono-caption">{analysis.model}</span>}
      </div>
      {recommendation && <strong className="gemini-recommendation">{recommendation}</strong>}
      {summary && <p className="gemini-summary">{summary}</p>}
      {highlights.length > 0 && (
        <ul className="gemini-list">
          {highlights.map((note, idx) => (
            <li key={idx}>{String(note)}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
