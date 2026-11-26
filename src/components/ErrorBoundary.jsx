import React from 'react';

export class ErrorBoundary extends React.Component {
  constructor(props) {
    super(props);
    this.state = { hasError: false, message: '' };
  }

  static getDerivedStateFromError(error) {
    return {
      hasError: true,
      message: error instanceof Error ? error.message : 'An unexpected rendering error occurred.'
    };
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <div className="error-boundary-screen">
        <div className="panel-card error-boundary-card">
          <span className="section-kicker">RECOVERY MODE</span>
          <h1>SeasonFetch encountered an unexpected error</h1>
          <p className="muted-copy">{this.state.message}</p>
          <button
            type="button"
            className="primary-button"
            onClick={() => window.location.reload()}
          >
            Reload Workspace
          </button>
        </div>
      </div>
    );
  }
}
