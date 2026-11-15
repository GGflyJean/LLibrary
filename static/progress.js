(function () {
  const body = document.body;
  const workId = body && body.dataset ? body.dataset.workId : '';
  const fileId = body && body.dataset ? body.dataset.fileId : '';
  let pending = null;
  let timer = null;

  function url() {
    return '/api/progress/' + encodeURIComponent(workId);
  }

  function load() {
    if (!workId) return Promise.resolve(null);
    return fetch(url(), { credentials: 'same-origin' })
      .then(function (response) {
        if (response.status === 204) return null;
        if (!response.ok) return null;
        return response.json();
      })
      .catch(function () { return null; });
  }

  function flush() {
    if (!pending) return;
    const payload = pending;
    pending = null;
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    try {
      fetch(url(), {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        keepalive: true
      }).catch(function () {});
    } catch (error) {
      /* ignore */
    }
  }

  function save(format, locator, percent) {
    if (!workId) return;
    pending = {
      file_id: Number(fileId) || 0,
      format: String(format || ''),
      locator: String(locator === undefined || locator === null ? '' : locator),
      percent: Number(percent) || 0
    };
    if (timer) clearTimeout(timer);
    timer = setTimeout(flush, 600);
  }

  window.addEventListener('pagehide', flush);
  document.addEventListener('visibilitychange', function () {
    if (document.visibilityState === 'hidden') flush();
  });

  window.llibraryProgress = {
    workId: workId,
    fileId: fileId,
    load: load,
    save: save,
    flush: flush
  };
})();
