export function isAbortError(error) {
  return Boolean(error && (error.name === 'AbortError' || error.code === 'ABORT_ERR'));
}

export function createSearchCoordinator() {
  let requestId = 0;
  let timer = null;
  let controller = null;

  function invalidate() {
    requestId += 1;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    if (controller) {
      controller.abort();
      controller = null;
    }
    return requestId;
  }

  function schedule(callback, delayMs = 0) {
    invalidate();
    const id = requestId;
    const activeController = typeof AbortController === 'function' ? new AbortController() : null;
    controller = activeController;
    timer = setTimeout(() => {
      timer = null;
      if (id === requestId) callback(id, activeController?.signal);
    }, Math.max(0, delayMs));
    return id;
  }

  function getSignal(id) {
    return id === requestId && controller ? controller.signal : undefined;
  }

  return {
    invalidate,
    schedule,
    getSignal,
    isCurrent: id => id === requestId
  };
}
