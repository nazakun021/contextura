const { invoke } = window.__TAURI__.core;

const views = [
  {
    title: 'Allow screen capture.',
    body: 'Contextura only works after macOS grants Screen Recording permission. The overlay stays local and reads frames on-device.',
    status: 'Required before live translation',
    sideLabel: 'Open System Settings',
    sideAction: () => invoke('open_screen_recording_settings'),
    panel: () => `
      <strong>What to do</strong>
      <p>Open the <strong>Screen Recording</strong> privacy panel, enable Contextura, then relaunch if macOS asks.</p>
      <p>Once enabled, the OCR pipeline can start reading Japanese text from the active display.</p>
    `,
  },
  {
    title: 'Check your local model.',
    body: 'Contextura needs a decoder-only GGUF model for the bundled <code>llama-server</code> sidecar.',
    status: (status) => status.has_model ? 'Model detected' : 'Model not detected yet',
    sideLabel: 'Open Models Folder',
    sideAction: () => invoke('open_models_folder_command'),
    panel: (status) => `
      <strong>Current model</strong>
      <ul class="meta-list">
        <li><span>Active</span><span>${status.active_model_label || 'No model detected'}</span></li>
        <li><span>Tier</span><span>${status.active_model_tier || 'Unavailable'}</span></li>
        <li><span>Models folder</span><code>${status.models_dir}</code></li>
      </ul>
      <p>Recommended default: <strong>Qwen3-0.6B Q4_K_M</strong>. Add alternate GGUF files here if you want live model switching with <strong>Cmd+Shift+G</strong>.</p>
    `,
  },
  {
    title: 'Learn the live controls.',
    body: 'Contextura is designed for keyboard-first use. These shortcuts work globally once the app is running.',
    status: 'Hotkeys available',
    sideLabel: 'Open Models Folder',
    sideAction: () => invoke('open_models_folder_command'),
    panel: () => `
      <strong>Core shortcuts</strong>
      <ul class="meta-list">
        <li><span>Toggle overlay</span><span><code>Cmd+Shift+T</code></span></li>
        <li><span>Translate now</span><span><code>Cmd+Shift+R</code></span></li>
        <li><span>Clear memory</span><span><code>Cmd+Shift+M</code></span></li>
        <li><span>Switch model</span><span><code>Cmd+Shift+G</code></span></li>
      </ul>
    `,
  },
  {
    title: 'You are ready.',
    body: 'After you finish this setup, the overlay window appears and Contextura can start translating once the screen settles.',
    status: 'Setup can be completed now',
    sideLabel: 'Open Models Folder',
    sideAction: () => invoke('open_models_folder_command'),
    panel: (status) => `
      <strong>Before closing setup</strong>
      <p>${status.has_model ? 'A local model is already present, so you can start testing immediately.' : 'You still need to add a GGUF model before translations can appear.'}</p>
      <p>Live smoke verification is still recommended after setup: stop scrolling, wait for debounce, and confirm translated boxes line up with the Japanese source text.</p>
    `,
  },
];

let currentStep = 0;
let wizardStatus = {
  has_model: false,
  active_model_label: '',
  active_model_tier: '',
  models_dir: '',
};

const progressDots = [...document.querySelectorAll('.progress span')];
const stepLabel = document.getElementById('step-label');
const title = document.getElementById('title');
const body = document.getElementById('body');
const statusPill = document.getElementById('status-pill');
const panel = document.getElementById('panel');
const backBtn = document.getElementById('back-btn');
const sideBtn = document.getElementById('side-btn');
const nextBtn = document.getElementById('next-btn');
const skipBtn = document.getElementById('skip-btn');

async function loadStatus() {
  try {
    wizardStatus = await invoke('wizard_status');
  } catch (error) {
    console.error('Failed to load wizard status', error);
  }
}

function render() {
  const view = views[currentStep];
  stepLabel.textContent = `Step ${currentStep + 1} of ${views.length}`;
  title.textContent = view.title;
  body.innerHTML = view.body;
  statusPill.textContent = typeof view.status === 'function' ? view.status(wizardStatus) : view.status;
  panel.innerHTML = view.panel(wizardStatus);
  sideBtn.textContent = view.sideLabel;
  nextBtn.textContent = currentStep === views.length - 1 ? 'Finish Setup' : 'Continue';
  backBtn.disabled = currentStep === 0;
  progressDots.forEach((dot, index) => {
    dot.classList.toggle('active', index <= currentStep);
  });
}

backBtn.addEventListener('click', () => {
  currentStep = Math.max(0, currentStep - 1);
  render();
});

sideBtn.addEventListener('click', async () => {
  await views[currentStep].sideAction();
  if (currentStep === 1) {
    await loadStatus();
    render();
  }
});

skipBtn.addEventListener('click', async () => {
  currentStep = Math.min(views.length - 1, currentStep + 1);
  await loadStatus();
  render();
});

nextBtn.addEventListener('click', async () => {
  if (currentStep < views.length - 1) {
    currentStep += 1;
    await loadStatus();
    render();
    return;
  }

  try {
    await invoke('complete_wizard');
  } catch (error) {
    console.error('Failed to complete wizard', error);
  }
});

loadStatus().then(render);