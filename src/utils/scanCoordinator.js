export function createScanCoordinator({ onCheckingChange = () => {} } = {}) {
  let requestId = 0;
  let timer = null;
  let active = false;

  function invalidate() {
    requestId += 1;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    if (active) {
      active = false;
      onCheckingChange(false);
    }
    return requestId;
  }

  function start({ delayMs = 0, request, onSuccess, onError }) {
    if (typeof request !== 'function') throw new TypeError('A scan request function is required.');
    invalidate();
    const id = requestId;
    active = true;
    onCheckingChange(true);

    timer = setTimeout(async () => {
      timer = null;
      if (id !== requestId) return;
      let result;
      let requestError;
      try {
        result = await request();
      } catch (error) {
        requestError = error;
      }

      if (id !== requestId) return;
      try {
        if (requestError) onError?.(requestError);
        else onSuccess?.(result);
      } finally {
        if (id === requestId) {
          active = false;
          onCheckingChange(false);
        }
      }
    }, Math.max(0, delayMs));

    return id;
  }

  return { invalidate, start, isCurrent: id => id === requestId };
}
