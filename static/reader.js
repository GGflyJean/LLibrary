(function () {
  let overlay = null;

  function closeReader() {
    if (!overlay) return;
    overlay.remove();
    overlay = null;
    document.body.classList.remove('reader-open');
  }

  window.llibraryOpenReader = function (url, title) {
    closeReader();
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
    if (event.origin === window.location.origin && event.data && event.data.type === 'llibrary-close-reader') {
      closeReader();
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
