/**
 * @typedef {{
 *   v: number,
 *   status: "ready",
 *   host_version: string,
 *   pid: number,
 *   started_at_unix_ms: number,
 *   socket: string,
 *   database_schema: number,
 *   recovered_jobs_on_start: number
 * }} HealthSnapshot
 *
 * @typedef {
 *   | {connection: "healthy", health: HealthSnapshot}
 *   | {connection: "offline", reason: string}
 *   | {connection: "protocol_error", reason: string}
 * } HostProbe
 *
 * @typedef {{
 *   tone: "ready" | "offline" | "error",
 *   title: string,
 *   detail: string,
 *   version: string,
 *   pid: string,
 *   socket: string,
 *   schema: string
 * }} HostView
 *
 * @typedef {{
 *   task_id: string,
 *   name: string,
 *   project: string,
 *   updated_at_ms: number,
 *   pinned: boolean
 *   cli_created: boolean
 * }} DashboardTask
 *
 * @typedef {{
 *   slot: number,
 *   task_id: string | null,
 *   task_name: string | null,
 *   project: string | null,
 *   binding_generation: number | null,
 *   pending_jobs: number,
 *   unread_generation: number | null,
 *   unread_coverage: number | null,
 *   latest_job_state: string | null,
 *   latest_job_failure: string | null,
 *   latest_job_updated_at: number | null
 * }} DashboardSlot
 *
 * @typedef {{
 *   v: number,
 *   prompt_backend?: string,
 *   tasks: DashboardTask[],
 *   slots: DashboardSlot[],
 *   lan: {
 *     secret_file_present: boolean,
 *     secret_file_readable: boolean,
 *     auth_key_loaded: boolean,
 *     udp_received: number,
 *     audio_auth_rejected: number,
 *     heartbeat_received: number,
 *     heartbeat_authenticated: number,
 *     mailbox_sent: number,
 *     mailbox_send_failed: number,
 *     playback_received: number,
 *     audio_frames_accepted: number,
 *     audio_ends_accepted: number,
 *     captures_ready: number,
 *     captures_rejected: number,
 *     asr_succeeded: number,
 *     asr_failed: number,
 *     prompts_delivered: number,
 *     queue_inserted: number,
 *     queue_replayed: number,
 *     queue_rejected: number
 *   },
 *   provider: {
 *     configured: boolean,
 *     region: string,
 *     asr_model: string,
 *     tts_model: string,
 *     voice: string
 *   }
 * }} DashboardSnapshot
 */

/** @param {HostProbe} probe @returns {HostView} */
export function presentProbe(probe) {
  if (probe.connection === "healthy") {
    const recovered = probe.health.recovered_jobs_on_start;
    return {
      tone: "ready",
      title: "Host 正常运行",
      detail:
        recovered > 0 ? `启动时恢复了 ${recovered} 个任务` : "后台监控已就绪",
      version: probe.health.host_version,
      pid: String(probe.health.pid),
      socket: `${probe.health.socket} · 0600`,
      schema: `v${probe.health.database_schema}`,
    };
  }
  if (probe.connection === "offline") {
    return {
      tone: "offline",
      title: "Host 未连接",
      detail: "常驻服务当前不可达",
      version: "--",
      pid: "--",
      socket: "run/host.sock · 离线",
      schema: "--",
    };
  }
  return {
    tone: "error",
    title: "Health 协议异常",
    detail: "Host 响应未通过本地协议校验",
    version: "--",
    pid: "--",
    socket: "run/host.sock · 拒绝",
    schema: "--",
  };
}

/** @param {DashboardSlot} slot @returns {string} */
export function presentSlotStatus(slot) {
  const facts = [];
  if (slot.pending_jobs > 0) {
    facts.push(`队列 ${slot.pending_jobs}`);
  }
  if (slot.unread_generation !== null) {
    const coverage = slot.unread_coverage ?? 1;
    facts.push(`待听总结 ${coverage} 次`);
  }
  if (slot.latest_job_state === "failed") {
    const updatedAt = slot.latest_job_updated_at;
    const when = typeof updatedAt === "number" &&
      Number.isSafeInteger(updatedAt) && updatedAt > 0
      ? new Intl.DateTimeFormat("zh-CN", {
          month: "numeric",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
          hour12: false,
        }).format(new Date(updatedAt * 1000))
      : null;
    const failure = {
      desktop_unavailable: "请启动 Codex 桌面端",
      active_session: "请先在 Codex 桌面端打开该对话",
      delivery_uncertain: "投递结果未确认，已停止重发，请检查桌面对话",
    }[slot.latest_job_failure ?? ""] ?? slot.latest_job_failure;
    facts.push(`上次失败${when ? `（${when}）` : ""}${failure ? `：${failure}` : ""}`);
  }
  return facts.length > 0 ? facts.join(" · ") : slot.task_id ? "已绑定" : "未绑定";
}

/** @param {DashboardTask[]} tasks @returns {DashboardTask[]} */
export function sortedTasks(tasks) {
  return [...tasks].sort(
    (left, right) =>
      Number(right.pinned) - Number(left.pinned) ||
      right.updated_at_ms - left.updated_at_ms ||
      left.name.localeCompare(right.name, "zh-CN"),
  );
}

/** @param {"offline" | "protocol_error"} connection */
export function presentDashboardFailure(connection) {
  const protocolError = connection === "protocol_error";
  return {
    taskCount: protocolError ? "状态不可用" : "Host 离线",
    providerState: protocolError ? "Host 响应异常" : "Host 不可达",
    asrModel: "--",
    ttsModel: "--",
    voice: "--",
  };
}
