// tauri ipc and init
function invoke(cmd, args = {}) {
  return window.__TAURI__.core.invoke(cmd, args);
}

let keys = []; 
let totpIntervals = {}; 

// init

document.addEventListener('DOMContentLoaded', async () => {
  setupEventListeners();
  await loadKeysFromFile();
  renderKeyCards();
});

// event lisnters
function setupEventListeners() {
  const manualPanel = document.getElementById('manual-panel');
  const toggleBtn = document.getElementById('toggle-manual-panel');

  if (toggleBtn && manualPanel) {
    toggleBtn.addEventListener('click', () => {
      const isOpen = manualPanel.classList.toggle('open');
      toggleBtn.classList.toggle('open', isOpen);
    });
  }

  document.getElementById('add-manual-btn').addEventListener('click', () => {
    addKeyManual();
  });

  
  document.getElementById('toggle-password').addEventListener('click', () => {
    togglePasswordVisibility();
  });


  document.getElementById('secret-name').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') addKeyManual();
  });
  document.getElementById('secret-value').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') addKeyManual();
  });
}

function clearInputs() {
  document.getElementById('secret-name').value = '';
  document.getElementById('secret-value').value = '';
}

// password visibility toggle
function togglePasswordVisibility() {
  const input = document.getElementById('secret-value');
  const btn = document.getElementById('toggle-password');
  const isPassword = input.type === 'password';
  input.type = isPassword ? 'text' : 'password';
  btn.textContent = isPassword ? '🙈' : '👁️';
}

// function to validate Base32
function isValidBase32(input) {
  // Base32 alphabet (RFC 4648): A-Z, 2-7, padding with =
  const base32Regex = /^[A-Z2-7]+=*$/;
  return base32Regex.test(input.toUpperCase()) && input.length > 0;
}


// load keys from file
async function loadKeysFromFile() {
  try {
    const raw = await invoke('load_keys');
    if (Array.isArray(raw)) {
      keys = raw.map(k => Array.isArray(k) ? { name: k[0], secret: k[1] } : k);
    } else {
      keys = [];
    }
  } catch (error) {
    showToast(`Failed to load keys: ${error}`, 'error');
  }
}

async function addKeyToFile(name, secret) {
  try {
    await invoke('add_key', { name, secret });
  } catch (error) {
    throw new Error(`Failed to save key: ${error}`);
  }
}

async function deleteKeyFromFile(name) {
  try {
    await invoke('delete_key', { name });
  } catch (error) {
    throw new Error(`Failed to delete key: ${error}`);
  }
}

// manual key add
async function addKeyManual() {
  const name = document.getElementById('secret-name').value.trim();
  const secret = document.getElementById('secret-value').value.trim();

  if (!name) {
    showToast('Please enter a key name', 'error');
    return;
  }

  if (!secret) {
    showToast('Please enter a secret', 'error');
    return;
  }

  if (!isValidBase32(secret)) {
    showToast('Secret must be valid Base32 (A-Z, 2-7, optional padding)', 'error');
    return;
  }

  try {
  
    if (keys.some(k => k.name === name)) {
      showToast('A key with this name already exists (it will be updated)', 'error');
      return;
    }

  
    await addKeyToFile(name, secret);

    keys.push({ name, secret });

    renderKeyCards();
    clearInputs();
    showToast(`Key "${name}" added successfully`, 'success');
  } catch (error) {
    showToast(`${error.message}`, 'error');
  }
}

// add keys from a URI
async function addKeyFromURI() {
  const uri = document.getElementById('uri-input').value.trim();

  if (!uri) {
    showToast('Please enter a URI', 'error');
    return;
  }

  try {
    const parsedKeys = parseURI(uri);

    for (const parsedKey of parsedKeys) {
      const { name, secret } = parsedKey;

      
      if (keys.some(k => k.name === name)) {
        showToast(`Key "${name}" already exists (it will be updated)`, 'error');
        continue;
      }

    
      await addKeyToFile(name, secret);

      
      keys.push({ name, secret });
    }

  
    renderKeyCards();
    clearInputs();
    switchMode('manual');
    showToast(`${parsedKeys.length} key(s) added successfully`, 'success');
  } catch (error) {
    showToast(`${error.message}`, 'error');
  }
}

// delete a key
async function deleteKey(name) {
  if (!confirm(`Delete key "${name}"? This cannot be undone.`)) {
    return;
  }

  try {
    await deleteKeyFromFile(name);

    keys = keys.filter(k => k.name !== name);
    if (totpIntervals[name]) {
      clearInterval(totpIntervals[name]);
      delete totpIntervals[name];
    }

    renderKeyCards();
    showToast(`Key "${name}" deleted`, 'success');
  } catch (error) {
    showToast(`${error.message}`, 'error');
  }
}


/**
 * Generate TOTP for a key
 * with a timer
 */
async function generateTOTP(secret, counter) {
  try {
    const totp = await invoke('generate_totp', { secret, counter });
    return totp;
  } catch (error) {
    console.error(`Failed to generate TOTP: ${error}`);
    return 'ERROR';
  }
}

function getCurrentCounter() {
  return Math.floor(Date.now() / 1000 / 30);
}

function getRemainingSeconds() {
  return 30 - (Math.floor(Date.now() / 1000) % 30);
}


async function updateKeyTOTP(name, secret) {
  const counter = getCurrentCounter();
  const remaining = getRemainingSeconds();
  const totp = await generateTOTP(secret, counter);

  const card = document.querySelector(`[data-key-name="${escapeDataAttribute(name)}"]`);
  if (!card) return;

  const totpElement = card.querySelector('.key-totp');
  if (totpElement) {
    totpElement.textContent = totp;
  }

  const timerCircle = card.querySelector('.timer-circle');
  if (timerCircle) {
    const progress = remaining / 30;
    timerCircle.style.setProperty('--progress', progress);
    const timerText = timerCircle.querySelector('.timer-text');
    if (timerText) {
      timerText.textContent = remaining;
    }
  }
}

function startTOTPTimer(name, secret) {

  if (totpIntervals[name]) {
    clearInterval(totpIntervals[name]);
  }
  updateKeyTOTP(name, secret);

  totpIntervals[name] = setInterval(() => {
    updateKeyTOTP(name, secret);
  }, 1000);
}

// to render elements
// has templates inside
function renderKeyCards() {
  const container = document.getElementById('keys-container');
  
  Object.values(totpIntervals).forEach(id => clearInterval(id));
  totpIntervals = {};

  if (keys.length === 0) {
    container.innerHTML = `
      <div class="empty-state">
        <p>No keys added yet.</p>
        <p class="empty-state-hint">Add your first key to get started!</p>
      </div>
    `;
    return;
  }

  container.innerHTML = keys.map(({ name, secret }) => `
    <div class="key-card" data-key-name="${escapeDataAttribute(name)}">
      <div class="timer-circle" style="--progress: 1">
        <span class="timer-text">30</span>
      </div>
      <div class="key-content">
        <div class="key-name">${escapeHtml(name)}</div>
        <div class="key-totp-container">
          <span class="key-totp">000000</span>
          <span class="copy-indicator">Click to copy</span>
        </div>
      </div>
      <button class="delete-btn" type="button" title="Delete key">
        ✕
      </button>
    </div>
  `).join('');

  keys.forEach(({ name, secret }) => {
    const card = container.querySelector(`[data-key-name="${escapeDataAttribute(name)}"]`);
    
  
    card.addEventListener('click', async (e) => {
      if (e.target.classList.contains('delete-btn')) return;
      const counter = getCurrentCounter();
      const totp = await generateTOTP(secret, counter);
      await copyToClipboard(totp);
      showToast('TOTP copied to clipboard', 'success');
    });

  
    card.querySelector('.delete-btn').addEventListener('click', (e) => {
      e.stopPropagation();
      deleteKey(name);
    });

  
    startTOTPTimer(name, secret);
  });
}

// other utils
function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

function escapeDataAttribute(text) {
  return text.replace(/[^\w-]/g, '_');
}

async function copyToClipboard(text) {
  try {
    await invoke('copy_to_clipboard', { totp: text });
  } catch (error) {
    console.error('Failed to copy to clipboard:', error);
    showToast('Failed to copy to clipboard', 'error');
  }
}

function showToast(message, type = 'info') {
  const toast = document.getElementById('toast');
  toast.textContent = message;
  toast.className = `toast show ${type}`;

  setTimeout(() => {
    toast.classList.remove('show');
  }, 4000);
}
