import {
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
    status.textContent = "待听已清除";
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
  dashboard = snapshot;
  const tasks = sortedTasks(snapshot.tasks);
  elements.taskCount.textContent = `${tasks.length} 个任务`;
  elements.providerDot.className = `provider-dot ${snapshot.provider.configured ? "ready" : "offline"}`;
  const providerName = snapshot.provider.region === "minimax-cn" ? "MiniMax" : "北京区";
  elements.providerName.textContent = providerName;
  elements.providerState.textContent = snapshot.provider.configured
    ? `${providerName} · 已就绪`
    : `${providerName} · 未配置`;
  elements.asrModel.textContent = snapshot.provider.asr_model;
  elements.ttsModel.textContent = snapshot.provider.tts_model;
  elements.ttsVoice.textContent = snapshot.provider.voice;
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
      elements.slots.append(newRow);
    }
    row.classList.toggle("selected", slot.slot === selectedSlot);
    const clearButton = /** @type {HTMLButtonElement} */ (childElement(row, ".clear-queue-button"));
    clearButton.disabled = !slot.task_id || clearingSlots.has(slot.slot);
    clearButton.ariaLabel = `清除槽位 ${slot.slot} 的待听队列`;
    clearButton.title = "将这个槽位当前的待听总结标记为已读";
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
      select.add(
        new Option(`${slot.task_name ?? "不可用任务"} · 已移出列表`, selected),
      );
    }
    select.value = selected;
    /** @type {HTMLButtonElement} */ (
      childElement(row, ".open-button")
    ).disabled = !selected;
    const selectedTask = tasks.find((task) => task.task_id === selected);
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
  dashboard = null;
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
