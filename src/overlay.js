const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

document.addEventListener('DOMContentLoaded', () => {
  const container = document.getElementById('overlay-container');
  const spinner = document.getElementById('status-spinner');
  const errorBanner = document.getElementById('error-banner');
  const errorTitle = document.getElementById('error-title');
  const errorDetail = document.getElementById('error-detail');
  let errorTimeout = null;
  let spinnerTimeout = null;

  listen('translation-update', (event) => {
    const payload = event.payload;
    // Hide spinner and error banner
    spinner.classList.add('hidden');
    errorBanner.classList.add('hidden');
    clearTimeout(spinnerTimeout);
    
    requestAnimationFrame(() => {
        // Clear old boxes
        container.innerHTML = '';
        
        // Render new boxes
        for (const box of payload.boxes) {
          const div = document.createElement('div');
          div.className = 'translation-box';
          if (box.is_vertical) {
            div.classList.add('vertical');
          }
          
          div.style.left = `${box.x}px`;
          div.style.top = `${box.y}px`;
          
          // Use min dimensions so the box can grow if text is longer
          div.style.minWidth = `${box.width}px`;
          div.style.minHeight = `${box.height}px`;
          
          div.style.backgroundColor = box.bg_color;
          div.style.color = box.fg_color;
          
          div.textContent = box.translated;
          div.title = box.original; // Tooltip shows original
          
          container.appendChild(div);
        }
    });
  });

  listen('translation-clear', () => {
    container.innerHTML = '';
    spinner.classList.add('hidden');
    errorBanner.classList.add('hidden'); // Clear error banner on reset
  });

  listen('translation-started', () => {
    spinner.classList.remove('hidden');
    errorBanner.classList.add('hidden'); // Clear error banner when starting
    // Safety timeout to hide spinner if update never comes (10s)
    clearTimeout(spinnerTimeout);
    spinnerTimeout = setTimeout(() => {
      spinner.classList.add('hidden');
    }, 10000);
  });

  listen('translation-error', (event) => {
    const payload = event.payload;
    const title = payload?.title || 'Contextura Notice';
    const message = payload?.message || 'Translation engine restarted.';
    const detail = payload?.detail || '';
    const level = payload?.level || 'warning';

    errorTitle.textContent = title;
    errorDetail.textContent = detail ? `${message} ${detail}` : message;
    errorBanner.dataset.level = level;
    errorBanner.classList.remove('hidden');
  });

  const retryBtn = document.getElementById('error-retry');
  retryBtn.addEventListener('click', async () => {
    retryBtn.disabled = true;
    retryBtn.textContent = 'Retrying...';
    try {
      await invoke('reload_runtime');
    } catch (e) {
      console.error(e);
    } finally {
      retryBtn.disabled = false;
      retryBtn.textContent = 'Retry Connection';
    }
  });
});
