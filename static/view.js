(function () {
  const root = document.documentElement;
  const buttons = document.querySelectorAll('[data-view]');
  if (!buttons.length) return;

  function sync() {
    const compact = root.classList.contains('compact-view');
    buttons.forEach(function (button) {
      button.classList.toggle('active', (button.dataset.view === 'compact') === compact);
    });
  }

  document.addEventListener('click', function (event) {
    if (!(event.target instanceof Element)) return;
    const button = event.target.closest('[data-view]');
    if (!button) return;
    const compact = button.dataset.view === 'compact';
    root.classList.toggle('compact-view', compact);
    try {
      localStorage.setItem('llibrary:view', compact ? 'compact' : 'detail');
    } catch (error) {
      /* ignore */
    }
    sync();
  });

  sync();
})();
