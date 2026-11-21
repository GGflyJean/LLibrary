(function () {
  const reader = document.getElementById('txt-reader');
  if (!reader) return;
  const url = reader.dataset.url;
  const viewport = document.getElementById('txt-viewport');
  const content = document.getElementById('txt-content');
  const pageInput = document.getElementById('txt-page');
  const pagesLabel = document.getElementById('txt-pages');
  const prev = document.getElementById('txt-prev');
  const next = document.getElementById('txt-next');
  const jump = document.getElementById('txt-jump');
  const zoomOut = document.getElementById('txt-zoom-out');
  const zoomIn = document.getElementById('txt-zoom-in');
  const zoomValue = document.getElementById('txt-zoom-value');
  const status = document.getElementById('txt-status');

  let page = 1;
  let pages = 1;
  let fontSize = 18;
  let pageHeight = 1;

  function update() {
    viewport.scrollTop = (page - 1) * pageHeight;
    pageInput.value = String(page);
    pageInput.max = String(pages);
    pagesLabel.textContent = String(pages);
    zoomValue.textContent = Math.round((fontSize / 18) * 100) + '%';
  }

  function measure() {
    content.style.fontSize = fontSize + 'px';
    const style = getComputedStyle(content);
    const lineHeight = parseFloat(style.lineHeight) || fontSize * 1.8;
    pageHeight = Math.max(lineHeight, Math.floor(viewport.clientHeight / lineHeight) * lineHeight);
    void content.offsetHeight;
    pages = Math.max(1, Math.ceil(content.scrollHeight / pageHeight));
    page = Math.min(Math.max(1, page), pages);
    update();
  }

  function layout(reset) {
    if (reset) page = 1;
    measure();
  }

  function goTo(number) {
    page = Math.min(Math.max(1, Number(number) || 1), pages);
    update();
    if (window.llibraryProgress) {
      const percent = pages > 0 ? (page - 1) / pages : 0;
      window.llibraryProgress.save('txt', String(page), percent);
    }
  }

  async function restoreProgress() {
    if (!window.llibraryProgress) return;
    const progress = await window.llibraryProgress.load();
    if (!progress) return;
    let target = 0;
    if (progress.format === 'txt' && Number(progress.locator) >= 1) {
      target = Number(progress.locator);
    } else if (progress.percent > 0) {
      target = Math.round(progress.percent * pages) + 1;
    }
    if (target > 0) goTo(target);
  }

  function relayout() {
    const ratio = pages > 1 ? (page - 1) / pages : 0;
    measure();
    page = Math.min(pages, Math.max(1, Math.round(ratio * pages) + 1));
    update();
  }

  async function load() {
    try {
      const response = await fetch(url, { credentials: 'same-origin' });
      if (!response.ok) throw new Error('HTTP ' + response.status);
      content.textContent = await response.text();
      if (status) status.hidden = true;
      layout(true);
      await restoreProgress();
    } catch (error) {
      content.textContent = '文本加载失败：' + error.message;
      if (status) {
        status.hidden = false;
        status.textContent = '文本加载失败：' + error.message;
      }
    }
  }

  prev.addEventListener('click', function () { goTo(page - 1); });
  next.addEventListener('click', function () { goTo(page + 1); });

  function dblclickNextEnabled() {
    const fn = window.llibraryDoubleClickNextEnabled;
    if (typeof fn === 'function') return fn();
    return localStorage.getItem('llibrary:dblclick-next') !== '0';
  }
  viewport.addEventListener('dblclick', function () {
    if (dblclickNextEnabled()) goTo(page + 1);
  });
  jump.addEventListener('click', function () { goTo(Number(pageInput.value) || 1); });
  pageInput.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') goTo(Number(pageInput.value) || 1);
  });

  function zoom(delta) {
    fontSize = Math.max(14, Math.min(28, fontSize + delta));
    relayout();
  }
  zoomOut.addEventListener('click', function () { zoom(-2); });
  zoomIn.addEventListener('click', function () { zoom(2); });

  let resizeTimer = null;
  window.addEventListener('resize', function () {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(relayout, 200);
  });

  window.llibraryCurrentLocation = function () {
    return {
      file_id: Number(document.body.dataset.fileId) || 0,
      format: 'txt',
      locator: String(page),
      percent: pages > 0 ? (page - 1) / pages : 0
    };
  };
  window.llibraryJumpTo = function (bookmark) {
    if (!bookmark) return;
    let target = 0;
    if (bookmark.format === 'txt' && Number(bookmark.locator) >= 1) {
      target = Number(bookmark.locator);
    } else if (Number(bookmark.percent) > 0) {
      target = Math.round(Number(bookmark.percent) * pages) + 1;
    }
    if (target > 0) goTo(target);
  };

  load();
})();
