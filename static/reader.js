(function () {
  let overlay = null;
  let removeTimer = null;

  function removeOverlay() {
    if (removeTimer) {
      clearTimeout(removeTimer);
      removeTimer = null;
    }
    if (!overlay) return;
    overlay.remove();
    overlay = null;
    document.body.classList.remove('reader-open');
  }

  function closeReader() {
    if (!overlay) return;
    const frame = overlay.querySelector('iframe');
    try {
      if (frame && frame.contentWindow) {
        frame.contentWindow.postMessage({ type: 'llibrary-dispose' }, window.location.origin);
      }
    } catch (error) {
      /* ignore */
    }
    if (removeTimer) clearTimeout(removeTimer);
    removeTimer = setTimeout(removeOverlay, 800);
  }

  window.llibraryOpenReader = function (url, title) {
    removeOverlay();
    overlay = document.createElement('div');
    overlay.className = 'reader-overlay';
    overlay.setAttribute('role', 'dialog');
    overlay.setAttribute('aria-modal', 'true');

    const close = document.createElement('button');
    close.type = 'button';
    close.className = 'reader-overlay-close';
    close.textContent = '×';
    close.setAttribute('aria-label', '关闭浮层');
    close.addEventListener('click', closeReader);

    const frame = document.createElement('iframe');
    frame.title = title || '阅读';
    frame.src = url;

    overlay.append(frame, close);
    document.body.appendChild(overlay);
    document.body.classList.add('reader-open');
  };

  window.addEventListener('message', function (event) {
    if (event.origin !== window.location.origin || !event.data) return;
    if (event.data.type === 'llibrary-close-reader') {
      closeReader();
    } else if (event.data.type === 'llibrary-disposed') {
      removeOverlay();
    }
  });

  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') closeReader();
  });

  document.addEventListener('click', function (event) {
    if (!(event.target instanceof Element)) return;
    const trigger = event.target.closest('[data-reader-url]');
    if (!trigger) return;
    event.preventDefault();
    window.llibraryOpenReader(trigger.dataset.readerUrl, trigger.dataset.readerTitle);
  });
})();
