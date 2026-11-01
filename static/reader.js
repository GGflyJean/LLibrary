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

    const bar = document.createElement('div');
    bar.className = 'reader-overlay-bar';
    const label = document.createElement('strong');
    label.textContent = title || '阅读';
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = '关闭';
    close.addEventListener('click', closeReader);
    bar.append(label, close);

    const frame = document.createElement('iframe');
    frame.title = title || '阅读';
    frame.src = url;

    overlay.append(bar, frame);
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
