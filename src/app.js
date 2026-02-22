const { invoke } = window.__TAURI__.core;
const { getCurrentWebview } = window.__TAURI__.webview;

const mainView = document.getElementById("main-view");
const settingsView = document.getElementById("settings-view");
const historyView = document.getElementById("history-view");
const settingsBtn = document.getElementById("settings-btn");
const historyBtn = document.getElementById("history-btn");
const backBtn = document.getElementById("back-btn");
const historyBackBtn = document.getElementById("history-back-btn");
const clearHistoryBtn = document.getElementById("clear-history-btn");
const historyList = document.getElementById("history-list");
const historyEmpty = document.getElementById("history-empty");
const dropZone = document.getElementById("drop-zone");
const status = document.getElementById("status");
const resultsBox = document.getElementById("results");
const resultsBody = document.getElementById("results-body");
const copyAllBtn = document.getElementById("copy-all-btn");
const settingsForm = document.getElementById("settings-form");
const modeToggle = document.getElementById("mode-toggle");
const clipToggle = document.getElementById("clip-toggle");

// State
let mode = "folder2";
let autoClip = true;
let lastResults = []; // [{file, url}]
let tokenMode = "static";
let isUploading = false;

// TTL elements
const ttlBar = document.getElementById("ttl-bar");
const ttlSelect = document.getElementById("ttl-select");
const ttlCustom = document.getElementById("ttl-custom");
const toggleTokenMode = document.getElementById("toggle-token-mode");
const dynamicTokenSettings = document.getElementById("dynamic-token-settings");
const defaultTtlSelect = document.getElementById("default-ttl-select");
const defaultTtlCustom = document.getElementById("default-ttl-custom");

const modeLeftLabel = document.querySelector(".toggle-group .toggle-label:first-child");
const modeRightLabel = document.querySelector(".toggle-group .toggle-label:last-child");

// Restore persisted toggle states
const savedMode = localStorage.getItem("b2u_mode");
if (savedMode === "folder1") {
    mode = "folder1";
    modeToggle.classList.remove("on");
    modeToggle.setAttribute("aria-checked", "false");
} else {
    modeToggle.classList.add("on");
}

const savedClip = localStorage.getItem("b2u_autoClip");
if (savedClip === "off") {
    autoClip = false;
    clipToggle.classList.remove("on");
    clipToggle.setAttribute("aria-checked", "false");
} else {
    clipToggle.classList.add("on");
}

modeToggle.addEventListener("click", () => {
    const isOn = modeToggle.classList.toggle("on");
    modeToggle.setAttribute("aria-checked", isOn);
    mode = isOn ? "folder2" : "folder1";
    localStorage.setItem("b2u_mode", mode);
});

clipToggle.addEventListener("click", () => {
    autoClip = clipToggle.classList.toggle("on");
    clipToggle.setAttribute("aria-checked", autoClip);
    localStorage.setItem("b2u_autoClip", autoClip ? "on" : "off");
});

// TTL select: show custom input when "Custom" selected
ttlSelect.addEventListener("change", () => {
    ttlCustom.classList.toggle("hidden", ttlSelect.value !== "custom");
});

defaultTtlSelect.addEventListener("change", () => {
    defaultTtlCustom.classList.toggle("hidden", defaultTtlSelect.value !== "custom");
});

function getCurrentTtl() {
    if (tokenMode !== "dynamic") return null;
    if (ttlSelect.value === "custom") {
        const v = parseInt(ttlCustom.value, 10);
        return v > 0 ? v : null;
    }
    return parseInt(ttlSelect.value, 10);
}

function applyTokenMode(mode) {
    tokenMode = mode;
    // Main view: show/hide TTL bar
    ttlBar.classList.toggle("hidden", mode !== "dynamic");
    // Settings view: show/hide static token fields and dynamic settings
    document.querySelectorAll(".static-token-field").forEach(el => {
        el.classList.toggle("hidden", mode === "dynamic");
    });
}

function showView(view) {
    mainView.classList.add("hidden");
    settingsView.classList.add("hidden");
    historyView.classList.add("hidden");
    view.classList.remove("hidden");
}

function showStatus(msg, type) {
    status.textContent = msg;
    status.className = type || "";
}

function hideResults() {
    resultsBox.classList.add("hidden");
    resultsBody.innerHTML = "";
    lastResults = [];
}

function addResultRow(fileName) {
    const card = document.createElement("div");
    card.className = "r-card";
    card.innerHTML = `
    <div class="r-header">
      <span class="r-file" title="${escapeAttr(fileName)}">${escapeHtml(fileName)}</span>
      <span class="r-status pending">uploading</span>
    </div>
    <div class="r-url"></div>
    <button class="r-copy-btn" disabled>Copy</button>
  `;
    resultsBody.appendChild(card);
    return card;
}

function setRowSuccess(card, url) {
    card.querySelector(".r-url").textContent = url;
    card.querySelector(".r-status").textContent = "done";
    card.querySelector(".r-status").className = "r-status success";
    const btn = card.querySelector(".r-copy-btn");
    btn.disabled = false;
    btn.addEventListener("click", async () => {
        await invoke("copy_to_clipboard", { text: url });
        btn.textContent = "Copied!";
        setTimeout(() => {
            btn.textContent = "Copy";
        }, 1500);
    });
}

function setRowError(card, msg) {
    card.querySelector(".r-url").textContent = msg;
    card.querySelector(".r-status").textContent = "failed";
    card.querySelector(".r-status").className = "r-status error";
}

copyAllBtn.addEventListener("click", async () => {
    if (lastResults.length === 0) return;
    const text = lastResults.map((r) => r.url).join("\n");
    await invoke("copy_to_clipboard", { text });
    copyAllBtn.textContent = "Copied All!";
    setTimeout(() => {
        copyAllBtn.textContent = "Copy All";
    }, 1500);
});

// History
const historySearch = document.getElementById("history-search");
let fullHistory = [];

historySearch.addEventListener("input", () => {
    renderHistoryList(historySearch.value.toLowerCase());
});

function renderHistoryList(filter) {
    historyList.innerHTML = "";
    const filtered = filter
        ? fullHistory.filter(e => e.file.toLowerCase().includes(filter) || e.url.toLowerCase().includes(filter))
        : fullHistory;
    if (filtered.length === 0) {
        historyEmpty.classList.remove("hidden");
        return;
    }
    historyEmpty.classList.add("hidden");
    const frag = document.createDocumentFragment();
    for (const entry of filtered) {
        frag.appendChild(createHistoryItem(entry));
    }
    historyList.appendChild(frag);
}

function createHistoryItem(entry) {
    const item = document.createElement("div");
    item.className = "history-item";
    item.setAttribute("role", "button");
    item.setAttribute("tabindex", "0");
    item.innerHTML = `
      <div class="h-file">${escapeHtml(entry.file)}</div>
      <div class="h-url">${escapeHtml(entry.url)}</div>
      <div class="h-meta">
        <span class="h-mode ${entry.mode === "shared" ? "shared" : ""}">${entry.mode}</span>
        <span>${entry.datetime}</span>
      </div>
    `;
    const copyHandler = async () => {
        await invoke("copy_to_clipboard", { text: entry.url });
        let copied = item.querySelector(".h-copied");
        if (!copied) {
            copied = document.createElement("div");
            copied.className = "h-copied";
            item.appendChild(copied);
        }
        copied.textContent = "Copied!";
        setTimeout(() => { copied.textContent = ""; }, 1500);
    };
    item.addEventListener("click", copyHandler);
    item.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); copyHandler(); }
    });
    return item;
}

async function renderHistory() {
    fullHistory = await invoke("get_history");
    historySearch.value = "";
    historyList.innerHTML = "";
    if (fullHistory.length === 0) {
        historyEmpty.classList.remove("hidden");
        return;
    }
    historyEmpty.classList.add("hidden");
    const frag = document.createDocumentFragment();
    for (const entry of fullHistory) {
        frag.appendChild(createHistoryItem(entry));
    }
    historyList.appendChild(frag);
}

function escapeHtml(str) {
    const div = document.createElement("div");
    div.textContent = str;
    return div.innerHTML;
}

function escapeAttr(str) {
    const el = document.createElement("span");
    el.textContent = str;
    return el.innerHTML.replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}

historyBtn.addEventListener("click", async () => {
    await renderHistory();
    showView(historyView);
});

historyBackBtn.addEventListener("click", () => showView(mainView));

clearHistoryBtn.addEventListener("click", async () => {
    if (fullHistory.length > 0 && !confirm("Clear all upload history?")) return;
    await invoke("clear_history");
    await renderHistory();
});

// Settings toggle elements
const toggleDateFolders = document.getElementById("toggle-date-folders");
const toggleUuidFilenames = document.getElementById("toggle-uuid-filenames");
const toggleOverwriteUploads = document.getElementById("toggle-overwrite-uploads");
const toggleNotifications = document.getElementById("toggle-notifications");

function setSettingsToggle(btn, on) {
    btn.classList.toggle("on", on);
    btn.setAttribute("aria-checked", on ? "true" : "false");
}

toggleDateFolders.addEventListener("click", () => {
    setSettingsToggle(toggleDateFolders, !toggleDateFolders.classList.contains("on"));
});
toggleUuidFilenames.addEventListener("click", () => {
    setSettingsToggle(toggleUuidFilenames, !toggleUuidFilenames.classList.contains("on"));
});
toggleOverwriteUploads.addEventListener("click", () => {
    setSettingsToggle(toggleOverwriteUploads, !toggleOverwriteUploads.classList.contains("on"));
});
toggleNotifications.addEventListener("click", () => {
    setSettingsToggle(toggleNotifications, !toggleNotifications.classList.contains("on"));
});

toggleTokenMode.addEventListener("click", () => {
    const on = !toggleTokenMode.classList.contains("on");
    setSettingsToggle(toggleTokenMode, on);
    dynamicTokenSettings.classList.toggle("hidden", !on);
    document.querySelectorAll(".static-token-field").forEach(el => {
        el.classList.toggle("hidden", on);
    });
});

function capitalize(str) {
    if (!str) return "";
    return str.charAt(0).toUpperCase() + str.slice(1);
}

function updateModeLabels(folder1, folder2) {
    const label1 = capitalize(folder1) || "Folder 1";
    const label2 = capitalize(folder2) || "Folder 2";
    // Toggle: left=folder1, right=folder2
    modeLeftLabel.textContent = label1;
    modeRightLabel.textContent = label2;
}

// Settings
settingsBtn.addEventListener("click", async () => {
    const settings = await invoke("get_settings");
    for (const [key, val] of Object.entries(settings)) {
        const input = settingsForm.elements[key];
        if (input) input.value = val;
    }
    // Populate toggles
    setSettingsToggle(toggleDateFolders, (settings.DATE_FOLDERS || "on") !== "off");
    setSettingsToggle(toggleUuidFilenames, (settings.UUID_FILENAMES || "on") !== "off");
    setSettingsToggle(toggleOverwriteUploads, (settings.OVERWRITE_UPLOADS || "no") === "yes");
    setSettingsToggle(toggleNotifications, (settings.NOTIFICATIONS || "on") !== "off");
    // Token mode
    const isDynamic = (settings.TOKEN_MODE || "static") === "dynamic";
    setSettingsToggle(toggleTokenMode, isDynamic);
    dynamicTokenSettings.classList.toggle("hidden", !isDynamic);
    document.querySelectorAll(".static-token-field").forEach(el => {
        el.classList.toggle("hidden", isDynamic);
    });
    if (settings.DEFAULT_TTL) {
        const presetValues = [...defaultTtlSelect.options].map(o => o.value).filter(v => v !== "custom");
        if (presetValues.includes(settings.DEFAULT_TTL)) {
            defaultTtlSelect.value = settings.DEFAULT_TTL;
            defaultTtlCustom.classList.add("hidden");
        } else {
            defaultTtlSelect.value = "custom";
            defaultTtlCustom.value = settings.DEFAULT_TTL;
            defaultTtlCustom.classList.remove("hidden");
        }
    }
    showView(settingsView);
});

backBtn.addEventListener("click", () => showView(mainView));

settingsForm.addEventListener("submit", async (e) => {
    e.preventDefault();
    const values = {};
    for (const input of settingsForm.querySelectorAll("input, select")) {
        if (input.name) values[input.name] = input.value;
    }
    // Add toggle values
    values.DATE_FOLDERS = toggleDateFolders.classList.contains("on") ? "on" : "off";
    values.UUID_FILENAMES = toggleUuidFilenames.classList.contains("on") ? "on" : "off";
    values.OVERWRITE_UPLOADS = toggleOverwriteUploads.classList.contains("on") ? "yes" : "no";
    values.NOTIFICATIONS = toggleNotifications.classList.contains("on") ? "on" : "off";
    values.TOKEN_MODE = toggleTokenMode.classList.contains("on") ? "dynamic" : "static";
    // If default TTL is "custom", use the custom input value
    if (defaultTtlSelect.value === "custom") {
        const customVal = defaultTtlCustom.value;
        if (customVal && parseInt(customVal, 10) > 0) {
            values.DEFAULT_TTL = customVal;
        }
    }
    await invoke("save_settings", { values });
    updateModeLabels(values.FOLDER_1, values.FOLDER_2);
    // Apply token mode to main view
    applyTokenMode(values.TOKEN_MODE);
    if (values.TOKEN_MODE === "dynamic" && values.DEFAULT_TTL) {
        // Sync main TTL select: use preset if it matches, otherwise set custom
        const presetValues = [...ttlSelect.options].map(o => o.value).filter(v => v !== "custom");
        if (presetValues.includes(values.DEFAULT_TTL)) {
            ttlSelect.value = values.DEFAULT_TTL;
            ttlCustom.classList.add("hidden");
        } else {
            ttlSelect.value = "custom";
            ttlCustom.value = values.DEFAULT_TTL;
            ttlCustom.classList.remove("hidden");
        }
    }
    showView(mainView);
    showStatus("Settings saved", "success");
});

// Test connection button
document.getElementById("test-connection-btn").addEventListener("click", async () => {
    const btn = document.getElementById("test-connection-btn");
    btn.disabled = true;
    btn.textContent = "Testing...";
    try {
        // Save current form values first so the backend uses them
        const values = {};
        for (const input of settingsForm.querySelectorAll("input, select")) {
            if (input.name) values[input.name] = input.value;
        }
        values.DATE_FOLDERS = toggleDateFolders.classList.contains("on") ? "on" : "off";
        values.UUID_FILENAMES = toggleUuidFilenames.classList.contains("on") ? "on" : "off";
        values.OVERWRITE_UPLOADS = toggleOverwriteUploads.classList.contains("on") ? "yes" : "no";
        values.TOKEN_MODE = toggleTokenMode.classList.contains("on") ? "dynamic" : "static";
        if (defaultTtlSelect.value === "custom") {
            const customVal = defaultTtlCustom.value;
            if (customVal && parseInt(customVal, 10) > 0) values.DEFAULT_TTL = customVal;
        }
        await invoke("save_settings", { values });
        const msg = await invoke("test_connection");
        btn.textContent = msg;
        btn.style.borderColor = "#a9dc76";
    } catch (err) {
        btn.textContent = err.toString();
        btn.style.borderColor = "#ff6188";
    }
    btn.disabled = false;
    setTimeout(() => {
        btn.textContent = "Test Connection";
        btn.style.borderColor = "";
    }, 3000);
});

// Drag and drop - use Tauri's native drag-drop events
async function handleFilePaths(paths) {
    if (paths.length === 0 || isUploading) return;
    isUploading = true;

    const hasSettings = await invoke("has_settings");
    if (!hasSettings) {
        showStatus("Please configure settings first", "error");
        return;
    }

    dropZone.classList.add("uploading");
    dropZone.querySelector("p").textContent =
        `Uploading ${paths.length} file${paths.length > 1 ? "s" : ""}...`;
    hideResults();
    resultsBox.classList.remove("hidden");
    showStatus("", "");

    const rows = paths.map((p) => {
        const name = p.split(/[\\/]/).pop() || p;
        return {
            name,
            path: p,
            tr: addResultRow(name),
        };
    });

    let succeeded = 0;
    let failed = 0;

    // Upload with max 5 concurrent
    const MAX_CONCURRENT = 5;
    let active = 0;
    let next = 0;
    await new Promise((resolveAll) => {
        function launch() {
            while (active < MAX_CONCURRENT && next < rows.length) {
                const row = rows[next++];
                active++;
                (async () => {
                    try {
                        const url = await invoke("upload_file", {
                            filePath: row.path,
                            mode,
                            autoClip: false,
                            ttl: getCurrentTtl(),
                        });
                        setRowSuccess(row.tr, url);
                        lastResults.push({ file: row.name, url });
                        succeeded++;
                    } catch (err) {
                        setRowError(row.tr, err.toString());
                        failed++;
                    }
                    active--;
                    if (next < rows.length) launch();
                    else if (active === 0) resolveAll();
                })();
            }
            if (rows.length === 0 || (next >= rows.length && active === 0))
                resolveAll();
        }
        launch();
    });

    // Auto-copy URLs to clipboard
    const didCopy = autoClip && lastResults.length > 0;
    if (didCopy) {
        const text = lastResults.map((r) => r.url).join("\n");
        await invoke("copy_to_clipboard", { text });
    }

    dropZone.classList.remove("uploading");
    dropZone.querySelector("p").textContent = "Drop or click to upload";

    const parts = [];
    if (succeeded > 0) parts.push(`${succeeded} uploaded`);
    if (failed > 0) parts.push(`${failed} failed`);
    if (didCopy) parts.push("copied to clipboard");
    showStatus(parts.join(" · "), failed > 0 ? "error" : "success");
    isUploading = false;

    // OS notification when uploads finish (if enabled in settings)
    try {
        const s = await invoke("get_settings");
        if ((s.NOTIFICATIONS || "on") !== "off" && window.__TAURI__.notification) {
            const { sendNotification, isPermissionGranted, requestPermission } = window.__TAURI__.notification;
            let permitted = await isPermissionGranted();
            if (!permitted) permitted = (await requestPermission()) === "granted";
            if (permitted) {
                sendNotification({
                    title: "B2Upload",
                    body: parts.filter(p => !p.includes("clipboard")).join(" - "),
                });
            }
        }
    } catch (_) {}
}

// Tauri native drag-drop
getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "over") {
        dropZone.classList.add("dragover");
    } else if (event.payload.type === "leave") {
        dropZone.classList.remove("dragover");
    } else if (event.payload.type === "drop") {
        dropZone.classList.remove("dragover");
        handleFilePaths(event.payload.paths);
    }
});

// Click-to-browse file picker
dropZone.addEventListener("click", async () => {
    if (isUploading) return;
    try {
        const selected = await window.__TAURI__.dialog.open({ multiple: true });
        if (selected) {
            const paths = Array.isArray(selected) ? selected : [selected];
            handleFilePaths(paths);
        }
    } catch (_) {}
});
dropZone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        dropZone.click();
    }
});

// Prevent default drag behavior on window
document.addEventListener("dragover", (e) => e.preventDefault());
document.addEventListener("drop", (e) => e.preventDefault());

// ── About Modal ──
const aboutModal = document.getElementById("about-modal");
const aboutVersion = document.getElementById("about-version");
const aboutBtn = document.getElementById("about-btn");
const aboutCloseBtn = document.getElementById("about-close-btn");
const terminalBody = document.getElementById("terminalBody");
const termStatusLine = document.getElementById("statusLine");
const termStatusSpinner = document.getElementById("statusSpinner");
const termScrollFade = document.getElementById("scrollFade");

let aboutRunning = false;
let aboutCancelled = false;

const NORMAL_W = 500, NORMAL_H = 600;
const ABOUT_W = 650, ABOUT_H = 650;

aboutBtn.addEventListener("click", async () => {
    aboutCancelled = false;
    aboutModal.classList.remove("hidden");
    try {
        const version = await window.__TAURI__.app.getVersion();
        aboutVersion.textContent = `v${version}`;
    } catch (_) {}
    await invoke("resize_window", { width: ABOUT_W, height: ABOUT_H });
    if (!aboutRunning) runTerminal();
});

aboutCloseBtn.addEventListener("click", closeAbout);

document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !aboutModal.classList.contains("hidden")) {
        closeAbout();
    }
});

async function closeAbout() {
    aboutCancelled = true;
    aboutModal.classList.add("hidden");
    terminalBody.innerHTML = "";
    aboutRunning = false;
    await invoke("resize_window", { width: NORMAL_W, height: NORMAL_H });
}

// Terminal helpers
let termLineCount = 0;

terminalBody.addEventListener("scroll", () => {
    termScrollFade.classList.toggle("visible", terminalBody.scrollTop > 10);
});

const spinnerFrames = ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"];
let spinnerIdx = 0;
let spinnerInterval = null;

function termScrollToBottom() { terminalBody.scrollTop = terminalBody.scrollHeight; }

function termCreateLine(content) {
    const div = document.createElement("div");
    div.className = "term-line";
    div.innerHTML = content;
    terminalBody.appendChild(div);
    termLineCount++;
    termStatusLine.textContent = `ln ${termLineCount}`;
    void div.offsetHeight;
    div.classList.add("visible");
    termScrollToBottom();
    return div;
}

function termPrompt() {
    return `<span class="t-prompt-user">b2upload</span><span class="t-prompt-at">@</span><span class="t-prompt-host">tauri</span><span class="t-prompt-colon">:</span><span class="t-prompt-path">~/uploads</span> <span class="t-prompt-git">(main)</span> <span class="t-prompt-symbol">❯</span> `;
}

function termSleep(ms) { return new Promise(r => setTimeout(r, ms)); }

async function termType(lineEl, parts, speed = 38) {
    const cursor = document.createElement("span");
    cursor.className = "t-cursor";
    lineEl.appendChild(cursor);
    const chars = [];
    for (const p of parts) {
        for (const c of p.text) chars.push({ char: c, cls: p.cls || "t-command" });
    }
    let curSpan = null, curCls = null;
    for (const { char, cls } of chars) {
        if (aboutCancelled) { cursor.remove(); return; }
        if (cls !== curCls) {
            curSpan = document.createElement("span");
            curSpan.className = cls;
            lineEl.insertBefore(curSpan, cursor);
            curCls = cls;
        }
        curSpan.textContent += char;
        termScrollToBottom();
        await termSleep(speed + (Math.random() * 24 - 12));
    }
    cursor.remove();
}

async function termSpinner(text, duration = 1200) {
    const line = termCreateLine("");
    const sc = document.createElement("span");
    sc.className = "t-spinner-char";
    line.appendChild(sc);
    const lb = document.createElement("span");
    lb.className = "t-output";
    lb.textContent = " " + text;
    line.appendChild(lb);
    const frames = ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"];
    let i = 0;
    const t0 = Date.now();
    while (Date.now() - t0 < duration) {
        if (aboutCancelled) return;
        sc.textContent = frames[i++ % frames.length];
        await termSleep(80);
    }
    sc.textContent = "✓";
    sc.style.color = "#a9dc76";
    lb.style.color = "#a9dc76";
}

async function termProgress(label, duration = 2000) {
    const line = termCreateLine("");
    line.innerHTML = `<span class="t-out-info">${label}</span> `;
    const bar = document.createElement("span");
    bar.className = "t-progress-bar";
    const fill = document.createElement("span");
    fill.className = "t-progress-fill";
    bar.appendChild(fill);
    line.appendChild(bar);
    const pct = document.createElement("span");
    pct.className = "t-progress-text";
    pct.textContent = "  0%";
    line.appendChild(pct);
    const steps = 50;
    const dt = duration / steps;
    for (let i = 1; i <= steps; i++) {
        if (aboutCancelled) return;
        const p = Math.round((i / steps) * 100);
        fill.style.width = p + "%";
        pct.textContent = `  ${p}%`;
        termScrollToBottom();
        await termSleep(dt + (Math.random() * 15 - 7));
    }
}

async function to(text, cls = "t-output", delay = 0) {
    if (aboutCancelled) return;
    if (delay) await termSleep(delay);
    termCreateLine(`<span class="${cls}">${text}</span>`);
}

async function runTerminal() {
    aboutRunning = true;
    termLineCount = 0;

    spinnerInterval = setInterval(() => {
        spinnerIdx = (spinnerIdx + 1) % spinnerFrames.length;
        termStatusSpinner.textContent = spinnerFrames[spinnerIdx] + " loading";
    }, 80);

    await termSleep(500);

    // neofetch-style splash for B2Upload
    await to("");
    await to('  <span class="t-out-info">      ▄▄▄▄▄▄▄▄▄      </span>  <span class="t-out-white">B2Upload</span><span class="t-output">@</span><span class="t-out-info">tauri</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">    ▄▀         ▀▄    </span>  <span class="t-output">─────────────────</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">   █   ●     ●   █   </span>  <span class="t-out-purple">App:</span> <span class="t-output">B2Upload v1.0.0</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">   █       ▄     █   </span>  <span class="t-out-purple">Backend:</span> <span class="t-output">Rust + Tauri 2</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">   █    ▀▀▀▀    █    </span>  <span class="t-out-purple">Storage:</span> <span class="t-output">IOTA Stronghold</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">    ▀▄         ▄▀    </span>  <span class="t-out-purple">Upload:</span> <span class="t-output">AWS S3 (Backblaze B2)</span>');
    await termSleep(60);
    await to('  <span class="t-out-info">      ▀▀▀▀▀▀▀▀▀      </span>  <span class="t-out-purple">Platform:</span> <span class="t-output">macOS (aarch64)</span>');
    await termSleep(60);
    await to("");
    await to('  <span class="t-out-err">███</span><span class="t-out-ok">███</span><span class="t-out-warn">███</span><span class="t-out-info">███</span><span class="t-out-purple">███</span><span class="t-out-orange">███</span><span class="t-out-white">███</span>');
    await to("");
    await termSleep(600);

    // Command 1: init stronghold vault
    const c1 = termCreateLine(termPrompt());
    await termType(c1, [
        { text: "b2upload", cls: "t-cmd-bin" },
        { text: " init", cls: "t-command" },
        { text: " --vault", cls: "t-cmd-flag" },
        { text: " stronghold", cls: "t-cmd-str" },
    ]);
    await termSleep(300);
    await termSpinner("Initializing encrypted vault...", 900);
    await to('<span class="t-out-ok">✓</span> <span class="t-output">Stronghold vault created at ~/Library/Application Support/</span>');
    await to('<span class="t-output">  Encryption: </span><span class="t-out-info">AES-256-GCM</span><span class="t-output"> | Key derivation: </span><span class="t-out-info">Argon2</span>');
    await to("", "t-output", 200);

    // Command 2: configure credentials
    const c2 = termCreateLine(termPrompt());
    await termType(c2, [
        { text: "b2upload", cls: "t-cmd-bin" },
        { text: " config", cls: "t-command" },
        { text: " set", cls: "t-cmd-func" },
        { text: " --endpoint", cls: "t-cmd-flag" },
        { text: " s3.us-east-005.backblazeb2.com", cls: "t-cmd-str" },
    ]);
    await termSleep(300);
    await to('<span class="t-out-ok">✓</span> <span class="t-output">7 credentials encrypted and saved</span>');
    await to("", "t-output", 200);

    // Command 3: upload a file
    const c3 = termCreateLine(termPrompt());
    await termType(c3, [
        { text: "b2upload", cls: "t-cmd-bin" },
        { text: " push", cls: "t-command" },
        { text: " --mode", cls: "t-cmd-flag" },
        { text: " shared", cls: "t-cmd-str" },
        { text: " screenshot.png", cls: "t-cmd-str" },
    ]);
    await termSleep(300);
    await to('<span class="t-output">Resolving endpoint...</span><span class="t-out-info"> us-east-005</span>', "t-output", 100);
    await to('<span class="t-output">Generating key: </span><span class="t-out-purple">shared/2026/02/20/a3f7c21e.png</span>', "t-output", 100);
    await termProgress("Uploading", 1800);
    await termSleep(150);
    await to('<span class="t-out-ok">✓</span> <span class="t-output">Upload complete</span>');
    await to('<span class="t-output">  URL: </span><span class="t-out-info">https://media.example.com/shared/2026/02/20/a3f7c21e.png?token=***</span>');
    await to('<span class="t-out-ok">✓</span> <span class="t-output">Copied to clipboard</span>');
    await to("", "t-output", 200);

    // Command 4: batch upload
    const c4 = termCreateLine(termPrompt());
    await termType(c4, [
        { text: "b2upload", cls: "t-cmd-bin" },
        { text: " push", cls: "t-command" },
        { text: " --mode", cls: "t-cmd-flag" },
        { text: " private", cls: "t-cmd-str" },
        { text: " --concurrent", cls: "t-cmd-flag" },
        { text: " 5", cls: "t-cmd-num" },
        { text: " *.png", cls: "t-cmd-str" },
    ]);
    await termSleep(300);

    const files = [
        ["design-v2.png", "142KB", true],
        ["mockup-final.png", "89KB", true],
        ["logo-dark.png", "23KB", true],
        ["hero-banner.png", "1.2MB", true],
        ["icon-set.png", "67KB", true],
    ];
    for (const [name, size, ok] of files) {
        if (aboutCancelled) break;
        const status = ok ? '<span class="t-out-ok">✓ done</span>' : '<span class="t-out-err">✗ failed</span>';
        await to(`  <span class="t-out-white">${name.padEnd(22)}</span><span class="t-output">${size.padEnd(8)}</span> ${status}`, "t-output", 250);
    }
    await to("", "t-output", 100);
    await to('<span class="t-out-purple">═══════════════════════════════════════════════════</span>');
    await to('<span class="t-out-ok">  5 uploaded</span><span class="t-output"> · </span><span class="t-out-err">0 failed</span><span class="t-output"> · </span><span class="t-out-info">elapsed 3.2s</span>');
    await to('<span class="t-out-purple">═══════════════════════════════════════════════════</span>');
    await to("", "t-output", 300);

    // Command 5: check history
    const c5 = termCreateLine(termPrompt());
    await termType(c5, [
        { text: "b2upload", cls: "t-cmd-bin" },
        { text: " history", cls: "t-command" },
        { text: " --last", cls: "t-cmd-flag" },
        { text: " 3", cls: "t-cmd-num" },
    ]);
    await termSleep(300);
    await to('<span class="t-output">┌──────────────────────┬──────────┬───────────────────┐</span>');
    await to('<span class="t-output">│ </span><span class="t-out-white">File</span><span class="t-output">                 │ </span><span class="t-out-white">Mode</span><span class="t-output">     │ </span><span class="t-out-white">Date</span><span class="t-output">              │</span>', "t-output", 60);
    await to('<span class="t-output">├──────────────────────┼──────────┼───────────────────┤</span>', "t-output", 60);
    await to('<span class="t-output">│ </span><span class="t-out-info">screenshot.png</span><span class="t-output">     │ </span><span class="t-out-info">shared</span><span class="t-output">   │ </span><span class="t-output">2026-02-20 21:45</span><span class="t-output">  │</span>', "t-output", 80);
    await to('<span class="t-output">│ </span><span class="t-out-purple">design-v2.png</span><span class="t-output">      │ </span><span class="t-out-purple">private</span><span class="t-output">  │ </span><span class="t-output">2026-02-20 21:46</span><span class="t-output">  │</span>', "t-output", 80);
    await to('<span class="t-output">│ </span><span class="t-out-purple">mockup-final.png</span><span class="t-output">   │ </span><span class="t-out-purple">private</span><span class="t-output">  │ </span><span class="t-output">2026-02-20 21:46</span><span class="t-output">  │</span>', "t-output", 80);
    await to('<span class="t-output">└──────────────────────┴──────────┴───────────────────┘</span>', "t-output", 60);
    await to("", "t-output", 400);

    // Final summary
    await to('<span class="t-out-ok">┌─────────────────────────────────────────────────┐</span>', "t-output", 80);
    await to('<span class="t-out-ok">│</span>  <span class="t-out-ok">✓</span> <span class="t-out-white">B2Upload — Fast. Encrypted. Simple.</span>        <span class="t-out-ok">│</span>', "t-output", 80);
    await to('<span class="t-out-ok">└─────────────────────────────────────────────────┘</span>', "t-output", 80);
    await to("", "t-output", 400);

    // Final prompt with blinking cursor
    const fin = termCreateLine(termPrompt());
    const cur = document.createElement("span");
    cur.className = "t-cursor";
    fin.appendChild(cur);
    termScrollToBottom();

    if (spinnerInterval) { clearInterval(spinnerInterval); spinnerInterval = null; }
    termStatusSpinner.textContent = "✓ done";

    // Wait then restart
    await termSleep(6000);
    if (!aboutCancelled) {
        terminalBody.innerHTML = "";
        termLineCount = 0;
        termStatusSpinner.textContent = "";
        runTerminal();
    }
}

// Check settings on load
(async () => {
    const has = await invoke("has_settings");
    if (!has) {
        showStatus("Open settings to configure", "");
    }
    // Load folder names and token mode to update UI
    try {
        const settings = await invoke("get_settings");
        updateModeLabels(settings.FOLDER_1, settings.FOLDER_2);
        applyTokenMode(settings.TOKEN_MODE || "static");
        if (settings.DEFAULT_TTL) {
            const presetValues = [...ttlSelect.options].map(o => o.value).filter(v => v !== "custom");
            if (presetValues.includes(settings.DEFAULT_TTL)) {
                ttlSelect.value = settings.DEFAULT_TTL;
            } else {
                ttlSelect.value = "custom";
                ttlCustom.value = settings.DEFAULT_TTL;
                ttlCustom.classList.remove("hidden");
            }
        }
    } catch (_) {}
})();
