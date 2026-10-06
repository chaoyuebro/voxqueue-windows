import {
  presentActivity,
  presentConnections,
  presentDashboardFailure,
  presentProbe,
  presentSlotStatus,
  sortedTasks,
} from "./view-model.js";

/** @param {string} selector @returns {HTMLElement} */
function requiredElement(selector) {
  const element = document.querySelector(selector);
  if (!(element instanceof HTMLElement)) {
    throw new Error(`missing UI element: ${selector}`);
  }
  return element;
}

const elements = {
  refresh: /** @type {HTMLButtonElement} */ (requiredElement("#refresh")),
  band: requiredElement("#status-band"),
  dot: requiredElement("#status-dot"),
  title: requiredElement("#status-title"),
  detail: requiredElement("#status-detail"),
  topStatusDot: requiredElement("#top-status-dot"),
  topStatusLabel: requiredElement("#top-status-label"),
  version: requiredElement("#host-version"),
  pid: requiredElement("#host-pid"),
  socket: requiredElement("#host-socket"),
  schema: requiredElement("#database-schema"),
  updated: requiredElement("#last-updated"),
  slots: requiredElement("#slots"),
  slotTemplate: /** @type {HTMLTemplateElement} */ (
    requiredElement("#slot-template")
  ),
  taskCount: requiredElement("#task-count"),
  providerDot: requiredElement("#provider-dot"),
  providerTitle: requiredElement("#provider-title"),
  providerName: requiredElement("#provider-name"),
  providerState: requiredElement("#provider-state"),
  asrModel: requiredElement("#asr-model"),
  ttsModel: requiredElement("#tts-model"),
  ttsVoice: requiredElement("#tts-voice"),
  lanDiagnostics: requiredElement("#lan-diagnostics"),
};

/** @type {import("./view-model.js").DashboardSnapshot | null} */
let dashboard = null;
const dirtySlots = new Set();
let selectedSlot = 1;
let selectedKey = 1;
const clearingSlots = new Set();

/** @param {number} slot @param {HTMLElement} row */
async function clearQueue(slot, row) {
  const current = slotSnapshot(slot);
  if (!current?.binding_generation || clearingSlots.has(slot)) return;
  const button = /** @type {HTMLButtonElement} */ (childElement(row, ".clear-queue-button"));
  const status = childElement(row, ".clear-queue-status");
  clearingSlots.add(slot);
  button.disabled = true;
  status.textContent = "正在清除…";
  try {
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) throw new Error("tauri_unavailable");
    renderDashboard(await invoke("clear_summary_queue", { slot, expectedGeneration: current.binding_generation }));
    status.textContent = "待听及失败提示已清除";
  } catch {
    status.textContent = "清除失败，请刷新后重试";
  } finally {
    clearingSlots.delete(slot);
    button.disabled = !slotSnapshot(slot)?.task_id;
  }
}

/** @param {number} slot @param {number} [key] */
function selectSlot(slot, key = slot) {
  selectedSlot = slot;
  selectedKey = key;
  requiredElement("#selected-slot-number").textContent = String(slot);
  requiredElement("#selected-slot-caption").textContent = `S${slot} 说话 · S${slot + 4} 听总结`;
  for (const button of Array.from(document.querySelectorAll(".device-key"))) {
    const active = Number(button.textContent?.trim().match(/^\d/)?.[0]) === key;
    button.classList.toggle("selected", active);
    button.setAttribute("aria-pressed", String(active));
  }
  for (const row of Array.from(elements.slots.querySelectorAll(".slot-row"))) {
    if (row instanceof HTMLElement) row.classList.toggle("selected", Number(row.dataset.slot) === slot);
  }
}

/** @param {import("./view-model.js").HostView} view */
function renderHealth(view) {
  elements.band.className = `status-band ${view.tone}`;
  elements.dot.className = `status-dot ${view.tone}`;
  elements.title.textContent = view.title;
  elements.detail.textContent = view.detail;
  elements.topStatusDot.className = view.tone;
  elements.topStatusLabel.textContent = view.tone === "ready" ? "Host 已连接" : "Host 未连接";
  elements.version.textContent = view.version;
  elements.pid.textContent = view.pid;
  elements.socket.textContent = view.socket;
  elements.schema.textContent = view.schema;
  elements.updated.textContent = `更新于 ${new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date())}`;
}

/** @param {Element | null} element @returns {HTMLElement} */
function rowElement(element) {
  if (!(element instanceof HTMLElement))
    throw new Error("invalid slot template");
  return element;
}

/** @param {HTMLElement} row @param {string} selector @returns {HTMLElement} */
function childElement(row, selector) {
  const element = row.querySelector(selector);
  if (!(element instanceof HTMLElement))
    throw new Error("invalid slot template");
  return element;
}

/** @param {number} slot @returns {import("./view-model.js").DashboardSlot | undefined} */
function slotSnapshot(slot) {
  return dashboard?.slots.find((candidate) => candidate.slot === slot);
}

/** @param {import("./view-model.js").DashboardSnapshot} snapshot */
function renderDashboard(snapshot) {
  renderConnections(snapshot);
  dashboard = snapshot;
  const tasks = sortedTasks(snapshot.tasks.filter((task) => !task.cli_created));
  elements.taskCount.textContent = `${tasks.length} 个任务`;
  elements.providerDot.className = `provider-dot ${snapshot.provider.configured ? "ready" : "offline"}`;
  const providerName = snapshot.provider.region === "minimax-cn" ? "MiniMax" : "北京区";
  elements.providerName.textContent = providerName;
  elements.providerState.textContent = snapshot.provider.configured
    ? `${providerName} · 已就绪`
    : `${providerName} · 未配置`;
  elements.asrModel.textContent = snapshot.provider.asr_model;
  elements.ttsModel.textContent = snapshot.provider.tts_model;
  elements.ttsVoice.textContent = savedAnswerVoiceName || snapshot.provider.voice;
  if (previewInFlight && snapshot.lan?.preview_status) {
    const labels = { waiting: "等待键盘开始试听…", streaming: "正在传送试听音频…", completed: "试听播放完成", failed: "试听播放失败，请重试" };
    const state = snapshot.lan.preview_status;
    if (labels[state]) requiredElement("#answer-voice-status").textContent = labels[state];
    if (state === "completed" || state === "failed") {
      previewInFlight = false;
      requiredElement("#answer-voice-preview").disabled = false;
    }
  }
  const keyboardVolume = snapshot.lan?.keyboard_volume_percent;
  requiredElement("#firmware-current-version").textContent = snapshot.lan?.keyboard_firmware_version
    || (typeof keyboardVolume === "number" ? "未提供版本号（旧固件）" : "键盘未连接，等待上报");
  requiredElement("#keyboard-volume-percent").textContent = typeof keyboardVolume === "number"
    ? `${keyboardVolume}%` : "等待上报（键盘未连接或固件需更新）";
  elements.lanDiagnostics.textContent = snapshot.lan
    ? `入站 ${snapshot.lan.udp_received} · 心跳 ${snapshot.lan.heartbeat_received}/${snapshot.lan.heartbeat_authenticated} · 信箱发送/失败 ${snapshot.lan.mailbox_sent}/${snapshot.lan.mailbox_send_failed} · 语音帧 ${snapshot.lan.audio_frames_accepted} · 结束包 ${snapshot.lan.audio_ends_accepted} · 完整录音 ${snapshot.lan.captures_ready} · 录音失败 ${snapshot.lan.captures_rejected} · 识别成功/失败 ${snapshot.lan.asr_succeeded}/${snapshot.lan.asr_failed} · 任务交付 ${snapshot.lan.prompts_delivered} · 入队/去重/拒绝 ${snapshot.lan.queue_inserted}/${snapshot.lan.queue_replayed}/${snapshot.lan.queue_rejected} · 语音认证拒绝 ${snapshot.lan.audio_auth_rejected} · 设备密钥${snapshot.lan.auth_key_loaded ? "已加载" : "缺失"}`
    : "Host 尚未提供诊断";
  for (let index = 1; index <= 5; index += 1) {
    const led = document.querySelector(`[data-led="${index}"]`);
    if (!led) continue;
    led.className = index === 5
      ? (snapshot.slots.some((slot) => slot.pending_jobs > 0) ? "busy" : "")
      : (snapshot.slots.find((slot) => slot.slot === index)?.unread_generation != null ? "unread" : "");
  }

  const existingRows = new Map(
    Array.from(elements.slots.querySelectorAll(".slot-row")).map((row) => [
      Number(rowElement(row).dataset.slot),
      rowElement(row),
    ]),
  );
  for (const slot of [...snapshot.slots].sort(
    (left, right) => left.slot - right.slot,
  )) {
    let row = existingRows.get(slot.slot);
    if (!row) {
      const fragment = elements.slotTemplate.content.cloneNode(true);
      if (!(fragment instanceof DocumentFragment))
        throw new Error("invalid slot template");
      const newRow = rowElement(fragment.querySelector(".slot-row"));
      row = newRow;
      newRow.dataset.slot = String(slot.slot);
      childElement(newRow, ".talk-key").textContent = `S${slot.slot}`;
      childElement(newRow, ".play-key").textContent = `S${slot.slot + 4}`;
      const label = /** @type {HTMLLabelElement} */ (
        childElement(newRow, ".slot-label")
      );
      const select = /** @type {HTMLSelectElement} */ (
        childElement(newRow, ".task-select")
      );
      const button = /** @type {HTMLButtonElement} */ (
        childElement(newRow, ".bind-button")
      );
      const openButton = /** @type {HTMLButtonElement} */ (
        childElement(newRow, ".open-button")
      );
      const selectId = `slot-${slot.slot}-task`;
      label.htmlFor = selectId;
      label.textContent = `槽位 ${slot.slot}`;
      select.id = selectId;
      select.ariaLabel = `槽位 ${slot.slot} Codex 任务`;
      button.ariaLabel = `绑定槽位 ${slot.slot}`;
      openButton.ariaLabel = `在 Codex 中打开槽位 ${slot.slot} 的任务`;
      select.addEventListener("change", () => {
        dirtySlots.add(slot.slot);
        openButton.disabled = !select.value;
      });
      button.addEventListener("click", () => void bindSlot(slot.slot, newRow));
      openButton.addEventListener("click", () => void openTask(newRow));
      childElement(newRow, ".clear-queue-button").addEventListener("click", () => void clearQueue(slot.slot, newRow));
      childElement(newRow, ".retry-button").addEventListener("click", async () => {
        const current = dashboard?.slots.find((entry) => entry.slot === slot.slot);
        if (!current?.failed_request_id || current.binding_generation == null) return;
        const retryButton = childElement(newRow, ".retry-button");
        retryButton.disabled = true;
        const status = childElement(newRow, ".clear-queue-status");
        try {
          renderDashboard(await window.__TAURI__.core.invoke("retry_failed_input", {
            slot: slot.slot, expectedGeneration: current.binding_generation, original: current.failed_request_id,
          }));
          status.textContent = "已重新排队，无需再次录音";
        } catch { status.textContent = "重试失败，请刷新后检查当前绑定"; }
        finally { retryButton.disabled = false; }
      });
      elements.slots.append(newRow);
    }
    childElement(row, ".retry-input").hidden = !slot.failed_request_id;
    childElement(row, ".failed-prompt").textContent = slot.failed_prompt ? `识别文字：${slot.failed_prompt}` : "";
    row.classList.toggle("selected", slot.slot === selectedSlot);
    const clearButton = /** @type {HTMLButtonElement} */ (childElement(row, ".clear-queue-button"));
    clearButton.disabled = !slot.task_id || clearingSlots.has(slot.slot);
    clearButton.ariaLabel = `清除槽位 ${slot.slot} 的待听队列`;
    clearButton.title = "清除这个槽位的待听总结、失败文字和重试提示";
    const select = /** @type {HTMLSelectElement} */ (
      childElement(row, ".task-select")
    );
    const selected = dirtySlots.has(slot.slot)
      ? select.value
      : (slot.task_id ?? "");
    const optionSignature = tasks
      .map((task) => `${task.task_id}:${task.cli_created}`)
      .join("|");
    if (select.dataset.options !== optionSignature) {
      select.replaceChildren(new Option("选择 Codex 任务", "", true, false));
      const placeholder = select.options.item(0);
      if (placeholder) placeholder.disabled = true;
      for (const task of tasks) {
        const marker = task.pinned ? "置顶 · " : "";
        const origin = task.cli_created ? "CLI" : "桌面";
        select.add(
          new Option(`${marker}${origin} · ${task.name} · ${task.project}`, task.task_id),
        );
      }
      select.dataset.options = optionSignature;
    }
    if (
      selected &&
      !Array.from(select.options).some((option) => option.value === selected)
    ) {
      const cliBinding = snapshot.tasks.some((task) => task.task_id === selected && task.cli_created);
      const option = new Option(cliBinding ? "当前绑定已隐藏 · 请选择桌面对话" : `${slot.task_name ?? "不可用任务"} · 已移出列表`, selected);
      option.hidden = cliBinding;
      option.disabled = cliBinding;
      select.add(option);
    }
    select.value = selected;
    /** @type {HTMLButtonElement} */ (
      childElement(row, ".open-button")
    ).disabled = !selected;
    const selectedTask = tasks.find((task) => task.task_id === selected);
    const activity = presentActivity(slot);
    const activityLabel = childElement(row, ".slot-activity");
    activityLabel.textContent = activity.label;
    activityLabel.dataset.tone = activity.tone;
    for (const step of row.querySelectorAll("[data-stage]")) {
      step.classList.toggle("active", step.getAttribute("data-stage") === activity.phase);
    }
    const status = presentSlotStatus(slot);
    childElement(row, ".slot-meta").textContent = snapshot.prompt_backend === "desktop"
      ? `${status} · 语音投递到 Codex 桌面`
      : selectedTask && !selectedTask.cli_created
        ? `${status} · 桌面任务请在 Codex 中继续`
        : status;
  }
  selectSlot(selectedSlot, selectedKey);
}

/** @param {HTMLElement} row */
async function openTask(row) {
  const select = /** @type {HTMLSelectElement} */ (
    childElement(row, ".task-select")
  );
  if (!select.value) return;
  const button = /** @type {HTMLButtonElement} */ (
    childElement(row, ".open-button")
  );
  button.disabled = true;
  try {
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) throw new Error("tauri_unavailable");
    await invoke("open_codex_task", { taskId: select.value });
  } catch {
    row.classList.add("failed");
    window.setTimeout(() => row.classList.remove("failed"), 1500);
  } finally {
    button.disabled = false;
  }
}

/** @param {number} slot @param {HTMLElement} row */
async function bindSlot(slot, row) {
  const select = /** @type {HTMLSelectElement} */ (
    childElement(row, ".task-select")
  );
  const button = /** @type {HTMLButtonElement} */ (
    childElement(row, ".bind-button")
  );
  if (!select.value || !dashboard) return;
  const current = slotSnapshot(slot);
  button.disabled = true;
  row.classList.add("saving");
  try {
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) throw new Error("tauri_unavailable");
    const updated = await invoke("bind_slot", {
      slot,
      taskId: select.value,
      expectedGeneration: current?.binding_generation ?? null,
    });
    dirtySlots.delete(slot);
    renderDashboard(updated);
    row.classList.add("saved");
    window.setTimeout(() => row.classList.remove("saved"), 900);
  } catch {
    row.classList.add("failed");
    window.setTimeout(() => row.classList.remove("failed"), 1500);
    await refreshDashboard();
  } finally {
    button.disabled = false;
    row.classList.remove("saving");
  }
}

async function refreshDashboard() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) throw new Error("tauri_unavailable");
  const probe = await invoke("host_dashboard");
  if (probe.connection === "healthy") {
    renderDashboard(probe.dashboard);
    return;
  }
  const unavailable = presentDashboardFailure(probe.connection);
  requiredElement("#keyboard-volume-percent").textContent = "Host 离线";
  requiredElement("#firmware-current-version").textContent = "Host 离线，无法读取";
  dashboard = null;
  renderConnections(null);
  for (const label of elements.slots.querySelectorAll(".slot-activity")) { label.textContent = "Host 离线，进度不可用"; label.dataset.tone = "idle"; }
  for (const step of elements.slots.querySelectorAll("[data-stage]")) step.classList.remove("active");
  if (previewInFlight) {
    previewInFlight = false;
    requiredElement("#answer-voice-preview").disabled = false;
    requiredElement("#answer-voice-status").textContent = "Host 离线，无法确认试听结果";
  }
  for (const button of Array.from(elements.slots.querySelectorAll(".clear-queue-button"))) {
    if (button instanceof HTMLButtonElement) button.disabled = true;
  }
  elements.taskCount.textContent = unavailable.taskCount;
  elements.providerDot.className = "provider-dot offline";
  elements.providerState.textContent = unavailable.providerState;
  elements.providerName.textContent = "--";
  elements.asrModel.textContent = unavailable.asrModel;
  elements.ttsModel.textContent = unavailable.ttsModel;
  elements.ttsVoice.textContent = unavailable.voice;
  for (const led of Array.from(document.querySelectorAll(".device-leds i"))) {
    led.className = "";
  }
}

async function refresh() {
  elements.refresh.disabled = true;
  elements.refresh.classList.add("spinning");
  try {
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) throw new Error("tauri_unavailable");
    const probe = await invoke("host_health");
    renderHealth(presentProbe(probe));
  } catch (error) {
    renderHealth(
      presentProbe({ connection: "offline", reason: "invoke_failed" }),
    );
    elements.detail.textContent = `界面通信失败：${String(error)}`;
  }
  try {
    await refreshDashboard();
  } catch (error) {
    elements.providerState.textContent = `任务列表读取失败：${String(error)}`;
  } finally {
    elements.refresh.disabled = false;
    elements.refresh.classList.remove("spinning");
  }
}

elements.refresh.addEventListener("click", refresh);
document.querySelector('a[href="#diagnostics-title"]')?.addEventListener("click", () => {
  const diagnostics = document.querySelector(".diagnostics");
  if (diagnostics instanceof HTMLDetailsElement) diagnostics.open = true;
});
for (const button of Array.from(document.querySelectorAll(".device-key"))) {
  button.addEventListener("click", () => {
    const key = Number(button.textContent?.trim().match(/^\d/)?.[0]);
    const slot = Number(button.getAttribute("data-key-slot"));
    if (Number.isInteger(key) && Number.isInteger(slot)) selectSlot(slot, key);
  });
}
void refresh();
window.setInterval(refresh, 3000);


const firmwareUI = {
  panel: requiredElement("#firmware-panel"),
  start: requiredElement("#flash-start"),
  cancel: requiredElement("#flash-cancel"),
  status: requiredElement("#flash-status"),
  progress: requiredElement("#flash-progress"),
  log: requiredElement("#flash-log"),
};
let currentFirmware = null;
let flashPhase = "";
let flashPollPending = false;

function showWorkspacePage(page) {
  const flashing = page === "firmware";
  const settings = page === "answer-settings";
  requiredElement(".workspace").classList.toggle("firmware-page", flashing);
  requiredElement(".workspace").classList.toggle("answer-settings-page", settings);
  requiredElement("#keyboard-overview").hidden = flashing || settings;
  requiredElement("#answer-settings-panel").hidden = !settings;
  firmwareUI.panel.hidden = !flashing;
  document.querySelectorAll("[data-page]").forEach((link) => {
    const active = link.getAttribute("data-page") === page;
    link.classList.toggle("active", active);
    if (active) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  });
  requiredElement(".top-nav-active").textContent = flashing ? "固件烧录" : settings ? "播报设置" : "语音键盘";
}
for (const link of document.querySelectorAll("[data-page]")) {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    showWorkspacePage(link.getAttribute("data-page"));
  });
}

function renderFlash(snapshot) {
  flashPhase = snapshot.phase || "";
  const busy = flashPhase === "waiting" || flashPhase === "flashing";
  firmwareUI.start.disabled = busy || !currentFirmware?.available;
  firmwareUI.start.textContent = busy ? "恢复进行中…" : "开始恢复";
  firmwareUI.cancel.hidden = flashPhase !== "waiting";
  firmwareUI.cancel.disabled = false;
  firmwareUI.status.textContent = snapshot.message || (currentFirmware?.available ? "完整恢复包已就绪" : "当前安装缺少恢复镜像或烧录工具");
  firmwareUI.status.dataset.phase = flashPhase;
  firmwareUI.progress.hidden = flashPhase !== "flashing" && flashPhase !== "completed";
  if (snapshot.progress == null) firmwareUI.progress.removeAttribute("value");
  else firmwareUI.progress.value = snapshot.progress;
  firmwareUI.log.textContent = snapshot.log?.join("\n") || "尚未开始";
  if (flashPhase === "failed") requiredElement("#flash-log-details").open = true;
}

async function pollFlash() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke || flashPollPending) return;
  flashPollPending = true;
  try {
    renderFlash(await invoke("firmware_flash_status"));
  } catch (error) {
    firmwareUI.status.textContent = `烧录状态读取失败：${String(error)}`;
  } finally {
    flashPollPending = false;
  }
}
firmwareUI.start.addEventListener("click", async () => {
  if (!currentFirmware?.available) return;
  firmwareUI.start.disabled = true;
  firmwareUI.status.textContent = "正在校验完整恢复包…";
  try {
    renderFlash(await window.__TAURI__.core.invoke("start_firmware_flash", {
      expectedSha256: currentFirmware.sha256,
    }));
  } catch (error) {
    firmwareUI.status.textContent = String(error);
    firmwareUI.status.dataset.phase = "failed";
    firmwareUI.start.disabled = false;
  }
});
firmwareUI.cancel.addEventListener("click", async () => {
  firmwareUI.cancel.disabled = true;
  try {
    await window.__TAURI__.core.invoke("cancel_firmware_flash");
    await pollFlash();
  } catch (error) {
    firmwareUI.status.textContent = String(error);
  }
});
async function loadFirmware() {
  try {
    currentFirmware = await window.__TAURI__.core.invoke("firmware_info");
    requiredElement("#firmware-name").textContent = currentFirmware.name;
    requiredElement("#firmware-latest-version").textContent = currentFirmware.latest_version;
    requiredElement("#firmware-sha").textContent = currentFirmware.sha256;
    const images = requiredElement("#firmware-images");
    images.replaceChildren();
    for (const image of currentFirmware.images || []) {
      const row = document.createElement("p");
      row.textContent = `${image.name} · 0x${image.address.toString(16)}\n${image.sha256}`;
      images.append(row);
    }
    await pollFlash();
  } catch (error) {
    firmwareUI.status.textContent = `当前固件不可用：${String(error)}`;
  }
}
showWorkspacePage("overview");
void loadFirmware();
window.setInterval(() => {
  if (!firmwareUI.panel.hidden || flashPhase === "waiting" || flashPhase === "flashing") void pollFlash();
}, 750);

const answerVoiceUI = {
  form: /** @type {HTMLFormElement} */ (requiredElement("#answer-voice-form")),
  voice: /** @type {HTMLSelectElement} */ (requiredElement("#answer-voice")),
  speed: /** @type {HTMLInputElement} */ (requiredElement("#answer-speed")),
  value: requiredElement("#answer-speed-value"),
  preview: /** @type {HTMLButtonElement} */ (requiredElement("#answer-voice-preview")),
  save: /** @type {HTMLButtonElement} */ (requiredElement("#answer-voice-save")),
  status: requiredElement("#answer-voice-status"),
};
let savedAnswerVoiceName = "";
let previewInFlight = false;
function showAnswerSpeed() {
  answerVoiceUI.value.textContent = `${Number(answerVoiceUI.speed.value).toFixed(2)}×`;
}
answerVoiceUI.speed.addEventListener("input", showAnswerSpeed);
answerVoiceUI.form.addEventListener("submit", async (event) => {
  event.preventDefault();
  answerVoiceUI.save.disabled = true;
  answerVoiceUI.status.textContent = "正在保存…";
  try {
    await window.__TAURI__.core.invoke("save_answer_voice_settings", {
      settings: { voice: answerVoiceUI.voice.value, speed: Number(answerVoiceUI.speed.value), volume: 1 },
    });
    savedAnswerVoiceName = answerVoiceUI.voice.selectedOptions[0]?.textContent || "";
    elements.ttsVoice.textContent = savedAnswerVoiceName;
    answerVoiceUI.status.textContent = "已保存，将用于之后生成的回答";
  } catch (error) {
    answerVoiceUI.status.textContent = `保存失败：${String(error)}`;
  } finally {
    answerVoiceUI.save.disabled = false;
  }
});
async function loadAnswerVoiceSettings() {
  try {
    const preferences = await window.__TAURI__.core.invoke("answer_voice_settings");
    answerVoiceUI.voice.replaceChildren();
    for (const voice of preferences.voices) {
      const option = document.createElement("option");
      option.value = voice.id;
      option.textContent = voice.name;
      answerVoiceUI.voice.append(option);
    }
    answerVoiceUI.voice.value = preferences.settings.voice;
    answerVoiceUI.speed.value = String(preferences.settings.speed);
    showAnswerSpeed();
    savedAnswerVoiceName = answerVoiceUI.voice.selectedOptions[0]?.textContent || "";
    elements.ttsVoice.textContent = savedAnswerVoiceName;
    answerVoiceUI.voice.disabled = false;
    answerVoiceUI.speed.disabled = false;
    answerVoiceUI.save.disabled = false;
    answerVoiceUI.preview.disabled = false;
    answerVoiceUI.status.textContent = "设置重启后保留";
  } catch (error) {
    answerVoiceUI.status.textContent = `设置读取失败：${String(error)}`;
  }
}
void loadAnswerVoiceSettings();

answerVoiceUI.preview.addEventListener("click", async () => {
  answerVoiceUI.preview.disabled = true;
  answerVoiceUI.status.textContent = "正在生成试听…";
  try {
    const result = await window.__TAURI__.core.invoke("preview_answer_voice", {
      settings: { voice: answerVoiceUI.voice.value, speed: Number(answerVoiceUI.speed.value), volume: 1 },
    });
    previewInFlight = true;
    answerVoiceUI.status.textContent = result;
    await refreshDashboard();
  } catch (error) { previewInFlight = false; answerVoiceUI.status.textContent = `试听失败：${String(error)}`; }
  finally { answerVoiceUI.preview.disabled = previewInFlight; }
});

function renderConnections(snapshot) {
  const status = presentConnections(snapshot);
  requiredElement("#keyboard-connection").textContent = status.keyboard;
  requiredElement("#keyboard-connection").dataset.connected = String(Boolean(snapshot?.lan.keyboard_connected));
  requiredElement("#codex-connection").textContent = status.codex;
  requiredElement("#codex-connection").dataset.connected = String(Boolean(snapshot?.codex_connected));
}
