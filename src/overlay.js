const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

document.addEventListener('DOMContentLoaded', () => {
  const container = document.getElementById('overlay-container');
  const spinner = document.getElementById('status-spinner');
  const errorBanner = document.getElementById('error-banner');
  let errorTimeout = null;
  let spinnerTimeout = null;

  listen('translation-update', (event) => {
    const payload = event.payload;
    // Hide spinner
    spinner.classList.add('hidden');
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
          div.style.width = `${box.width}px`;
          div.style.height = `${box.height}px`;
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
  });

  listen('translation-started', () => {
    spinner.classList.remove('hidden');
    // Safety timeout to hide spinner if update never comes (10s)
    clearTimeout(spinnerTimeout);
    spinnerTimeout = setTimeout(() => {
      spinner.classList.add('hidden');
    }, 10000);
  });

  listen('translation-error', (event) => {
    const payload = event.payload;
    if (payload && payload.message) {
      errorBanner.textContent = payload.message;
    } else {
      errorBanner.textContent = 'Translation engine restarted.';
    }
    
    errorBanner.classList.remove('hidden');
    
    clearTimeout(errorTimeout);
    errorTimeout = setTimeout(() => {
      errorBanner.classList.add('hidden');
    }, 4000); // 4s auto-dismiss per spec
  });
});
