(function () {
  const body = document.body;
  const workId = body && body.dataset ? body.dataset.workId : '';
  const panel = document.getElementById('reader-bookmarks');
  if (!panel || !workId) return;

  const list = document.getElementById('bookmark-list');
  const nameInput = document.getElementById('bookmark-name');
  const addButton = document.getElementById('bookmark-add');
  const toggle = document.getElementById('reader-bookmark-toggle');
  const close = document.getElementById('reader-bookmarks-close');
  const tocPanel = document.getElementById('reader-toc');
  let bookmarks = [];

  function url() {
    return '/api/bookmarks/' + encodeURIComponent(workId);
  }

  function open() {
    panel.hidden = false;
    if (tocPanel) tocPanel.hidden = true;
    load();
  }

  function closePanel() {
    panel.hidden = true;
  }

  async function load() {
    if (list) list.textContent = '正在加载书签…';
    try {
      const response = await fetch(url(), { credentials: 'same-origin' });
      bookmarks = response.ok ? await response.json() : [];
    } catch (error) {
      bookmarks = [];
    }
    render();
  }

  function render() {
    if (!list) return;
    list.replaceChildren();
    if (!bookmarks.length) {
      const empty = document.createElement('p');
      empty.className = 'bookmark-empty';
      empty.textContent = '还没有书签，输入名称后添加当前位置。';
      list.appendChild(empty);
      return;
    }
    bookmarks.forEach(function (bookmark) {
      const row = document.createElement('div');
      row.className = 'bookmark-item';

      const jump = document.createElement('button');
      jump.type = 'button';
      jump.className = 'bookmark-jump';
      jump.textContent = bookmark.name || 'default';
      jump.addEventListener('click', function () { jumpTo(bookmark); });

      const remove = document.createElement('button');
      remove.type = 'button';
      remove.className = 'bookmark-delete';
      remove.textContent = '删除';
      remove.addEventListener('click', function () { removeBookmark(bookmark); });

      row.append(jump, remove);
      list.appendChild(row);
    });
  }

  function jumpTo(bookmark) {
    if (typeof window.llibraryJumpTo === 'function') {
      window.llibraryJumpTo(bookmark);
    }
    closePanel();
  }

  async function addBookmark() {
    const current = typeof window.llibraryCurrentLocation === 'function'
      ? window.llibraryCurrentLocation()
      : null;
    if (!current) {
      window.alert('当前位置暂不可用，请等阅读器加载完成后再试。');
      return;
    }
    const name = (nameInput && nameInput.value ? nameInput.value : '').trim() || 'default';
    let response;
    try {
      response = await fetch(url(), {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: name,
          file_id: current.file_id,
          format: current.format,
          locator: current.locator,
          percent: current.percent
        })
      });
    } catch (error) {
      window.alert('添加书签失败：' + error.message);
      return;
    }
    if (response.status === 409) {
      window.alert('每本书最多保存 10 张书签，请先删除旧书签。');
      return;
    }
    if (!response.ok) {
      window.alert('添加书签失败：HTTP ' + response.status);
      return;
    }
    if (nameInput) nameInput.value = '';
    await load();
  }

  async function removeBookmark(bookmark) {
    if (!window.confirm('删除书签“' + (bookmark.name || 'default') + '”？')) return;
    try {
      await fetch('/api/bookmarks/' + encodeURIComponent(bookmark.id), {
        method: 'DELETE',
        credentials: 'same-origin'
      });
    } catch (error) {
      window.alert('删除书签失败：' + error.message);
      return;
    }
    await load();
  }

  if (toggle) toggle.addEventListener('click', open);
  if (close) close.addEventListener('click', closePanel);
  if (addButton) addButton.addEventListener('click', addBookmark);
  if (nameInput) nameInput.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') addBookmark();
  });

  document.addEventListener('click', function (event) {
    if (!(event.target instanceof Element)) return;
    if (event.target.closest('#reader-toc-toggle')) closePanel();
  }, true);
})();
