'use strict';

const API = '';
const messagesEl = document.getElementById('messages');
const inputEl = document.getElementById('input');
const formEl = document.getElementById('input-form');
const sendBtn = document.getElementById('btn-send');
const sessionBadge = document.getElementById('session-id');
const newSessionBtn = document.getElementById('btn-new-session');
const sidebar = document.getElementById('sidebar');
const sessionListEl = document.getElementById('session-list');
const toggleSidebarBtn = document.getElementById('btn-toggle-sidebar');

const fileInput = document.getElementById('file-input');
const imageBtn = document.getElementById('btn-image');
const imagePreview = document.getElementById('image-preview');

let sessionId = null;
let sending = false;
let pendingImages = []; // Array of { media_type, data (base64, no prefix) }

// ── Markdown setup ───────────────────────────────────────

if (typeof marked !== 'undefined') {
  marked.use({
    breaks: true,
    gfm: true,
  });
}

function renderMarkdown(text) {
  if (typeof marked !== 'undefined') {
    return marked.parse(text);
  }
  return escapeHtml(text);
}

// ── Sidebar ──────────────────────────────────────────────

toggleSidebarBtn.addEventListener('click', () => {
  sidebar.classList.toggle('hidden');
  if (!sidebar.classList.contains('hidden')) {
    loadSessionList();
  }
});

async function loadSessionList() {
  try {
    const resp = await fetch(`${API}/api/sessions`);
    if (!resp.ok) return;
    const sessions = await resp.json();
    sessionListEl.innerHTML = '';
    for (const s of sessions) {
      const li = document.createElement('li');
      li.dataset.id = s.session_id;
      if (s.session_id === sessionId) li.className = 'active';
      li.innerHTML = `
        <span class="session-item-id">${escapeHtml(s.session_id.substring(0, 8))}</span>
        <span class="session-item-meta">${s.turn_count} turns</span>
      `;
      li.addEventListener('click', () => switchSession(s.session_id));
      sessionListEl.appendChild(li);
    }
  } catch (err) {
    // silently ignore
  }
}

async function switchSession(newId) {
  if (newId === sessionId) return;
  sessionId = newId;
  sessionBadge.textContent = sessionId.substring(0, 8);
  messagesEl.innerHTML = '';

  // Highlight active session in sidebar
  sessionListEl.querySelectorAll('li').forEach(li => {
    li.className = li.dataset.id === sessionId ? 'active' : '';
  });

  // Load session history from the API
  try {
    const resp = await fetch(`${API}/api/rpc`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ method: 'session/get', params: { session_id: sessionId } }),
    });
    if (resp.ok) {
      const data = await resp.json();
      const messages = data.result?.messages || data.messages || [];
      for (const msg of messages) {
        if (msg.role === 'user') {
          appendMessage('user', msg.content || msg.text || '');
        } else if (msg.role === 'assistant') {
          const el = appendMessage('assistant', '');
          el.classList.add('markdown');
          el.innerHTML = renderMarkdown(msg.content || msg.text || '');
          // Apply syntax highlighting
          if (typeof hljs !== 'undefined') {
            el.querySelectorAll('pre code').forEach(block => hljs.highlightElement(block));
          }
        }
      }
      scrollToBottom();
    }
  } catch (err) {
    // Session history load failed silently; user can still chat
  }
}

// ── Session management ────────────────────────────────────

async function createSession() {
  try {
    const resp = await fetch(`${API}/api/session`, { method: 'POST' });
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = await resp.json();
    sessionId = data.session_id;
    sessionBadge.textContent = sessionId.substring(0, 8);
    messagesEl.innerHTML = '';
    loadSessionList();
  } catch (err) {
    appendError(`Failed to create session: ${err.message}`);
  }
}

// ── Message rendering ─────────────────────────────────────

let messageIndex = 0;

function appendMessage(role, text, images) {
  const el = document.createElement('div');
  el.className = `msg ${role}`;
  const idx = messageIndex++;
  el.dataset.msgIndex = idx;

  if (role === 'user') {
    if (images && images.length > 0) {
      const imgRow = document.createElement('div');
      imgRow.className = 'msg-images';
      for (const img of images) {
        const imgEl = document.createElement('img');
        imgEl.src = `data:${img.media_type};base64,${img.data}`;
        imgEl.className = 'msg-thumb';
        imgEl.addEventListener('click', () => showImageOverlay(imgEl.src));
        imgRow.appendChild(imgEl);
      }
      el.appendChild(imgRow);
    }

    const textSpan = document.createElement('span');
    textSpan.className = 'msg-text';
    textSpan.textContent = text;
    el.appendChild(textSpan);

    const editBtn = document.createElement('button');
    editBtn.className = 'edit-btn';
    editBtn.title = 'Edit & resend';
    editBtn.innerHTML = '&#9998;';
    editBtn.addEventListener('click', () => enterEditMode(el, idx, text));
    el.appendChild(editBtn);
  } else {
    el.textContent = text;
  }

  messagesEl.appendChild(el);
  scrollToBottom();
  return el;
}

function enterEditMode(msgEl, msgIndex, originalText) {
  msgEl.innerHTML = '';
  msgEl.classList.add('editing');

  const textarea = document.createElement('textarea');
  textarea.className = 'edit-textarea';
  textarea.value = originalText;
  textarea.rows = 2;
  msgEl.appendChild(textarea);

  const actions = document.createElement('div');
  actions.className = 'edit-actions';

  const saveBtn = document.createElement('button');
  saveBtn.textContent = 'Save';
  saveBtn.className = 'edit-save';
  saveBtn.addEventListener('click', () => {
    const newText = textarea.value.trim();
    if (newText && newText !== originalText) {
      resendFromIndex(msgIndex, newText);
    } else {
      exitEditMode(msgEl, originalText, msgIndex);
    }
  });

  const cancelBtn = document.createElement('button');
  cancelBtn.textContent = 'Cancel';
  cancelBtn.className = 'edit-cancel';
  cancelBtn.addEventListener('click', () => exitEditMode(msgEl, originalText, msgIndex));

  actions.appendChild(saveBtn);
  actions.appendChild(cancelBtn);
  msgEl.appendChild(actions);

  textarea.focus();
}

function exitEditMode(msgEl, text, msgIndex) {
  msgEl.classList.remove('editing');
  msgEl.innerHTML = '';

  const textSpan = document.createElement('span');
  textSpan.className = 'msg-text';
  textSpan.textContent = text;
  msgEl.appendChild(textSpan);

  const editBtn = document.createElement('button');
  editBtn.className = 'edit-btn';
  editBtn.title = 'Edit & resend';
  editBtn.innerHTML = '&#9998;';
  editBtn.addEventListener('click', () => enterEditMode(msgEl, msgIndex, text));
  msgEl.appendChild(editBtn);
}

async function resendFromIndex(msgIndex, newText) {
  if (!sessionId || sending) return;

  // Remove all messages after this index from the DOM
  const allMsgs = messagesEl.querySelectorAll('.msg, .tool-indicator, .phase-indicator, .meta-alert');
  let removing = false;
  for (const el of allMsgs) {
    if (removing) {
      el.remove();
    } else if (el.dataset.msgIndex === String(msgIndex)) {
      // Update this message text and exit edit mode
      exitEditMode(el, newText, msgIndex);
      removing = true;
    }
  }

  // Reset messageIndex to msgIndex + 1 (the edited user message stays)
  messageIndex = msgIndex + 1;

  sending = true;
  sendBtn.disabled = true;

  const renderer = new StreamRenderer();

  try {
    const resp = await fetch(`${API}/api/turn/resend`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ session_id: sessionId, message_index: msgIndex, new_input: newText }),
    });

    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: { message: `HTTP ${resp.status}` } }));
      appendError(err.error?.message || `HTTP ${resp.status}`);
      return;
    }

    const reader = resp.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      let eventType = null;
      for (const line of lines) {
        if (line.startsWith('event: ')) {
          eventType = line.substring(7).trim();
        } else if (line.startsWith('data: ') && eventType) {
          handleResendSSE(eventType, line.substring(6));
          eventType = null;
        } else if (line === '') {
          eventType = null;
        }
      }
    }
  } catch (err) {
    appendError(`Connection error: ${err.message}`);
  } finally {
    sending = false;
    sendBtn.disabled = false;
    inputEl.focus();
    loadSessionList();
  }

  function handleResendSSE(type, dataStr) {
    let data;
    try { data = JSON.parse(dataStr); } catch { return; }

    switch (type) {
      case 'text':
        renderer.append(data.content);
        break;
      case 'tool':
        if (data.status === 'started') createToolIndicator(data.tool_name);
        else updateToolIndicator(data.tool_name, data.status);
        break;
      case 'phase':
        appendPhaseIndicator(data.phase, data.events_count);
        break;
      case 'meta':
        appendMetaAlert(data.kind, data.message);
        break;
      case 'done':
        renderer.finish();
        break;
      case 'error':
        renderer.finish();
        appendError(data.message || 'Unknown error');
        break;
    }
  }
}

let lastUserInput = '';

function appendError(text) {
  const el = document.createElement('div');
  el.className = 'msg error';
  const textSpan = document.createElement('span');
  textSpan.textContent = text;
  el.appendChild(textSpan);

  if (lastUserInput) {
    const retryBtn = document.createElement('button');
    retryBtn.className = 'retry-btn';
    retryBtn.textContent = 'Retry';
    const savedInput = lastUserInput;
    retryBtn.addEventListener('click', () => {
      el.remove();
      sendMessage(savedInput);
    });
    el.appendChild(retryBtn);
  }

  messagesEl.appendChild(el);
  scrollToBottom();
}

function createToolIndicator(toolName) {
  const el = document.createElement('div');
  el.className = 'tool-indicator';
  el.dataset.tool = toolName;
  el.dataset.startTime = String(Date.now());
  el.innerHTML = `
    <span class="tool-name">${escapeHtml(toolName)}</span>
    <span class="tool-status running">running</span>
    <span class="tool-elapsed"></span>
    <span class="dot-pulse"></span>
  `;
  messagesEl.appendChild(el);
  scrollToBottom();
  return el;
}

function updateToolIndicator(toolName, status) {
  const indicators = messagesEl.querySelectorAll(`.tool-indicator[data-tool="${toolName}"]`);
  const el = indicators[indicators.length - 1];
  if (!el) return;

  const statusEl = el.querySelector('.tool-status');
  const pulseEl = el.querySelector('.dot-pulse');
  const elapsedEl = el.querySelector('.tool-elapsed');

  // Calculate elapsed time
  const startTime = parseInt(el.dataset.startTime, 10);
  const elapsed = startTime ? ((Date.now() - startTime) / 1000).toFixed(1) : null;

  if (status === 'completed') {
    statusEl.textContent = 'done';
    statusEl.className = 'tool-status done';
    if (elapsed && elapsedEl) elapsedEl.textContent = `(${elapsed}s)`;
    if (pulseEl) pulseEl.remove();
  } else if (status === 'error') {
    statusEl.textContent = 'error';
    statusEl.className = 'tool-status error';
    if (elapsed && elapsedEl) elapsedEl.textContent = `(${elapsed}s)`;
    if (pulseEl) pulseEl.remove();
  }
}

function scrollToBottom() {
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function escapeHtml(s) {
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

function appendPhaseIndicator(phase, eventsCount) {
  const el = document.createElement('div');
  el.className = 'phase-indicator';
  el.innerHTML = `<span class="phase-label">${escapeHtml(phase)}</span> <span class="phase-events">${eventsCount} events</span>`;
  messagesEl.appendChild(el);
  scrollToBottom();
}

const META_ICONS = {
  DoomLoop: '\u{1F504}',
  Fatigue: '\u{26A1}',
  FrameAnchoring: '\u{1F3AF}',
  Duration: '\u{23F1}',
};

function appendMetaAlert(kind, message) {
  const icon = META_ICONS[kind] || '\u{26A0}';
  const kindClass = kind.toLowerCase().replace(/([A-Z])/g, '-$1').replace(/^-/, '');
  const el = document.createElement('div');
  el.className = `meta-alert meta-${kindClass}`;
  el.innerHTML = `<span class="meta-icon">${icon}</span><span class="meta-kind">${escapeHtml(kind)}</span> ${escapeHtml(message)}`;
  messagesEl.appendChild(el);
  scrollToBottom();
}

// ── Stream renderer (rAF throttled) ──────────────────────

class StreamRenderer {
  constructor() {
    this.el = null;
    this.text = '';
    this.dirty = false;
    this.rafId = null;
  }

  init() {
    this.el = appendMessage('assistant', '');
    this.el.classList.add('markdown');
    this.text = '';
    this.dirty = false;
    // Add typing cursor
    const cursor = document.createElement('span');
    cursor.className = 'typing-cursor';
    this.el.appendChild(cursor);
    this._startLoop();
  }

  append(chunk) {
    if (!this.el) this.init();
    this.text += chunk;
    this.dirty = true;
  }

  _startLoop() {
    const render = () => {
      if (this.dirty) {
        this.dirty = false;
        this.el.innerHTML = renderMarkdown(this.text);
        // Re-add cursor
        const c = document.createElement('span');
        c.className = 'typing-cursor';
        this.el.appendChild(c);
        scrollToBottom();
      }
      this.rafId = requestAnimationFrame(render);
    };
    this.rafId = requestAnimationFrame(render);
  }

  finish() {
    if (this.rafId) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
    if (this.el) {
      // Final render with full markdown
      this.el.innerHTML = renderMarkdown(this.text);
      // Apply syntax highlighting to code blocks
      if (typeof hljs !== 'undefined') {
        this.el.querySelectorAll('pre code').forEach(block => {
          hljs.highlightElement(block);
        });
      }
      // Add copy button
      const rawText = this.text;
      const copyBtn = document.createElement('button');
      copyBtn.className = 'copy-btn';
      copyBtn.title = 'Copy to clipboard';
      copyBtn.innerHTML = '\u{1F4CB}';
      copyBtn.addEventListener('click', () => {
        navigator.clipboard.writeText(rawText).then(() => {
          copyBtn.innerHTML = '\u{2705}';
          setTimeout(() => { copyBtn.innerHTML = '\u{1F4CB}'; }, 1500);
        });
      });
      this.el.appendChild(copyBtn);
      scrollToBottom();
    }
  }

  getElement() { return this.el; }
  getText() { return this.text; }
}

// ── WebSocket transport (default) ────────────────────────

let ws = null;
let wsConnected = false;
let wsReconnectTimer = null;
let wsRenderer = null;
let wsRpcCallbacks = {};
let wsRpcId = 0;

function connectWebSocket() {
  if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
    return;
  }
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(`${protocol}//${location.host}/api/ws`);

  ws.onopen = () => {
    wsConnected = true;
    console.log('WebSocket connected');
  };

  ws.onmessage = (event) => {
    let data;
    try { data = JSON.parse(event.data); } catch { return; }

    // Stream events have an "event" field
    if (data.event) {
      handleWsStreamEvent(data);
      return;
    }
    // JSON-RPC responses have a "jsonrpc" field
    if (data.jsonrpc || data.result !== undefined || data.error !== undefined) {
      const id = data.id;
      if (id != null && wsRpcCallbacks[id]) {
        wsRpcCallbacks[id](data);
        delete wsRpcCallbacks[id];
      }
    }
  };

  ws.onclose = () => {
    wsConnected = false;
    console.log('WebSocket disconnected');
    if (wsReconnectTimer) clearTimeout(wsReconnectTimer);
    wsReconnectTimer = setTimeout(connectWebSocket, 3000);
  };

  ws.onerror = () => {
    // onclose will fire after onerror
  };
}

function wsRpcCall(method, params) {
  return new Promise((resolve) => {
    const id = ++wsRpcId;
    wsRpcCallbacks[id] = resolve;
    ws.send(JSON.stringify({ jsonrpc: '2.0', method, params, id }));
  });
}

function handleWsStreamEvent(data) {
  switch (data.event) {
    case 'text':
      if (!wsRenderer) wsRenderer = new StreamRenderer();
      wsRenderer.append(data.data.content);
      break;
    case 'tool':
      if (data.data.status === 'started') createToolIndicator(data.data.tool_name);
      else updateToolIndicator(data.data.tool_name, data.data.status);
      break;
    case 'trace':
      // traces rendered as phase indicators
      break;
    case 'phase':
      appendPhaseIndicator(data.data.phase, data.data.events_count);
      break;
    case 'meta':
      appendMetaAlert(data.data.kind, data.data.message);
      break;
    case 'done':
      if (wsRenderer) wsRenderer.finish();
      wsRenderer = null;
      sending = false;
      sendBtn.disabled = false;
      inputEl.focus();
      loadSessionList();
      break;
    case 'error':
      if (wsRenderer) wsRenderer.finish();
      wsRenderer = null;
      appendError(data.data.message || 'Unknown error');
      sending = false;
      sendBtn.disabled = false;
      inputEl.focus();
      break;
  }
}

// ── SSE connection with reconnection (fallback) ─────────

function connectSSE(url, onEvent, options = {}) {
  const maxRetries = options.maxRetries || 10;
  const baseDelay = options.baseDelay || 1000;
  const maxDelay = options.maxDelay || 30000;
  let retries = 0;
  let controller = null;

  async function connect() {
    controller = new AbortController();
    try {
      const resp = await fetch(url, { signal: controller.signal });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);

      retries = 0; // Reset on successful connection
      const reader = resp.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';

        let eventType = null;
        for (const line of lines) {
          if (line.startsWith('event: ')) {
            eventType = line.substring(7).trim();
          } else if (line.startsWith('data: ') && eventType) {
            onEvent(eventType, line.substring(6));
            eventType = null;
          } else if (line === '') {
            eventType = null;
          }
        }
      }
    } catch (err) {
      if (err.name === 'AbortError') return; // Intentional disconnect
      console.warn(`SSE connection error (attempt ${retries + 1}):`, err.message);
    }

    // Reconnect with exponential backoff
    if (retries < maxRetries) {
      const delay = Math.min(baseDelay * Math.pow(2, retries), maxDelay);
      retries++;
      console.log(`SSE reconnecting in ${delay}ms...`);
      setTimeout(connect, delay);
    } else {
      console.error('SSE max retries reached, giving up.');
    }
  }

  connect();

  return {
    disconnect() {
      retries = maxRetries; // Prevent reconnection
      if (controller) controller.abort();
    }
  };
}

// ── Turn execution ───────────────────────────────────────

async function sendMessage(input) {
  if (!sessionId || !input.trim()) return;

  sending = true;
  sendBtn.disabled = true;
  lastUserInput = input;
  const msgImages = [...pendingImages];
  pendingImages = [];
  renderImagePreview();
  appendMessage('user', input, msgImages);

  // Prefer WebSocket when connected (no image support over WS yet)
  if (wsConnected && ws && ws.readyState === WebSocket.OPEN && msgImages.length === 0) {
    wsRenderer = null;
    ws.send(JSON.stringify({
      jsonrpc: '2.0',
      method: 'session/prompt',
      params: { session_id: sessionId, prompt: input },
      id: Date.now(),
    }));
    // Stream events handled by handleWsStreamEvent
    return;
  }

  // Fallback: SSE transport
  await sendMessageSSE(input, msgImages);
}

async function sendMessageSSE(input, msgImages) {
  const renderer = new StreamRenderer();

  try {
    const body = { session_id: sessionId, input: input };
    if (msgImages.length > 0) {
      body.images = msgImages;
    }
    const resp = await fetch(`${API}/api/turn/stream`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: { message: `HTTP ${resp.status}` } }));
      appendError(err.error?.message || `HTTP ${resp.status}`);
      return;
    }

    const reader = resp.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      let eventType = null;
      for (const line of lines) {
        if (line.startsWith('event: ')) {
          eventType = line.substring(7).trim();
        } else if (line.startsWith('data: ') && eventType) {
          const data = line.substring(6);
          handleSSEEvent(eventType, data);
          eventType = null;
        } else if (line === '') {
          eventType = null;
        }
      }
    }
  } catch (err) {
    appendError(`Connection error: ${err.message}`);
  } finally {
    sending = false;
    sendBtn.disabled = false;
    inputEl.focus();
    loadSessionList();
  }

  function handleSSEEvent(type, dataStr) {
    let data;
    try {
      data = JSON.parse(dataStr);
    } catch {
      return;
    }

    switch (type) {
      case 'text':
        renderer.append(data.content);
        break;

      case 'tool':
        if (data.status === 'started') {
          createToolIndicator(data.tool_name);
        } else {
          updateToolIndicator(data.tool_name, data.status);
        }
        break;

      case 'phase':
        appendPhaseIndicator(data.phase, data.events_count);
        break;

      case 'meta':
        appendMetaAlert(data.kind, data.message);
        break;

      case 'done':
        renderer.finish();
        break;

      case 'error':
        renderer.finish();
        appendError(data.message || 'Unknown error');
        break;
    }
  }
}

// ── Image handling ────────────────────────────────────────

imageBtn.addEventListener('click', () => fileInput.click());

fileInput.addEventListener('change', (e) => {
  for (const file of e.target.files) {
    addImageFile(file);
  }
  fileInput.value = '';
});

inputEl.addEventListener('paste', (e) => {
  const items = e.clipboardData?.items;
  if (!items) return;
  for (const item of items) {
    if (item.type.startsWith('image/')) {
      e.preventDefault();
      addImageFile(item.getAsFile());
    }
  }
});

function addImageFile(file) {
  if (!file || !file.type.startsWith('image/')) return;
  const reader = new FileReader();
  reader.onload = () => {
    const dataUrl = reader.result;
    const commaIdx = dataUrl.indexOf(',');
    const base64 = dataUrl.substring(commaIdx + 1);
    pendingImages.push({ media_type: file.type, data: base64 });
    renderImagePreview();
  };
  reader.readAsDataURL(file);
}

function showImageOverlay(src) {
  const overlay = document.createElement('div');
  overlay.className = 'image-overlay';
  const img = document.createElement('img');
  img.src = src;
  overlay.appendChild(img);
  overlay.addEventListener('click', () => overlay.remove());
  document.body.appendChild(overlay);
}

function renderImagePreview() {
  imagePreview.innerHTML = '';
  if (pendingImages.length === 0) {
    imagePreview.classList.remove('visible');
    return;
  }
  imagePreview.classList.add('visible');
  pendingImages.forEach((img, i) => {
    const wrap = document.createElement('div');
    wrap.className = 'preview-item';
    const imgEl = document.createElement('img');
    imgEl.src = `data:${img.media_type};base64,${img.data}`;
    wrap.appendChild(imgEl);
    const removeBtn = document.createElement('button');
    removeBtn.className = 'preview-remove';
    removeBtn.textContent = '\u00D7';
    removeBtn.addEventListener('click', () => {
      pendingImages.splice(i, 1);
      renderImagePreview();
    });
    wrap.appendChild(removeBtn);
    imagePreview.appendChild(wrap);
  });
}

// ── Input handling ────────────────────────────────────────

formEl.addEventListener('submit', (e) => {
  e.preventDefault();
  if (sending) return;
  const text = inputEl.value.trim();
  if (!text) return;
  inputEl.value = '';
  inputEl.style.height = 'auto';
  sendMessage(text);
});

inputEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    formEl.dispatchEvent(new Event('submit'));
  }
});

inputEl.addEventListener('input', () => {
  inputEl.style.height = 'auto';
  inputEl.style.height = Math.min(inputEl.scrollHeight, 150) + 'px';
});

newSessionBtn.addEventListener('click', () => {
  createSession();
});

// ── Init ──────────────────────────────────────────────────

connectWebSocket();
createSession();
