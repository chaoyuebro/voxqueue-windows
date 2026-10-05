import { describe, expect, it } from "vitest";
import {
  presentDashboardFailure,
  presentProbe,
  presentSlotStatus,
  sortedTasks,
} from "../ui/view-model.js";

describe("desktop Host health view", () => {
  it("renders the authoritative healthy snapshot", () => {
    expect(
      presentProbe({
        connection: "healthy",
        health: {
          v: 1,
          status: "ready",
          host_version: "0.1.0",
          pid: 321,
          started_at_unix_ms: 1,
          socket: "run/host.sock",
          database_schema: 1,
          recovered_jobs_on_start: 2,
        },
      }),
    ).toEqual({
      tone: "ready",
      title: "Host 正常运行",
      detail: "启动时恢复了 2 个任务",
      version: "0.1.0",
      pid: "321",
      socket: "run/host.sock · 0600",
      schema: "v1",
    });
  });

  it("keeps offline distinct from invalid protocol", () => {
    expect(
      presentProbe({ connection: "offline", reason: "unreachable" }).tone,
    ).toBe("offline");
    expect(
      presentProbe({ connection: "protocol_error", reason: "invalid" }).tone,
    ).toBe("error");
  });
});

describe("desktop four-slot dashboard", () => {
  it("clears stale provider details and distinguishes protocol failures", () => {
    expect(presentDashboardFailure("protocol_error")).toEqual({
      taskCount: "状态不可用",
      providerState: "Host 响应异常",
      asrModel: "--",
      ttsModel: "--",
      voice: "--",
    });
    expect(presentDashboardFailure("offline").providerState).toBe(
      "Host 不可达",
    );
  });

  it("shows queue and retained unread coverage without exposing task IDs", () => {
    expect(
      presentSlotStatus({
        slot: 2,
        task_id: "019fa972-5cfa-75e1-9008-0b17ade9a347",
        task_name: "Task A",
        project: "Project A",
        binding_generation: 4,
        pending_jobs: 2,
        unread_generation: 3,
        unread_coverage: 5,
        latest_job_state: null,
        latest_job_failure: null,
        latest_job_updated_at: null,
      }),
    ).toBe("队列 2 · 待听总结 5 次");
  });

  it("shows when a retained failure happened instead of implying a new failure", () => {
    const status = presentSlotStatus({
      slot: 1,
      task_id: "019fa972-5cfa-75e1-9008-0b17ade9a347",
      task_name: "Task A",
      project: "Project A",
      binding_generation: 2,
      pending_jobs: 0,
      unread_generation: null,
      unread_coverage: null,
      latest_job_state: "failed",
      latest_job_failure: "exit_failure",
      latest_job_updated_at: 1790503715,
    });
    expect(status).toContain("上次失败");
    expect(status).toContain("exit_failure");
    expect(status).not.toContain("最近任务失败");
  });

  it("shows an unread summary and a retained failure together", () => {
    const status = presentSlotStatus({
      slot: 1,
      task_id: "019fa972-5cfa-75e1-9008-0b17ade9a347",
      task_name: "Task A",
      project: "Project A",
      binding_generation: 2,
      pending_jobs: 0,
      unread_generation: 3,
      unread_coverage: 1,
      latest_job_state: "failed",
      latest_job_failure: "exit_failure",
      latest_job_updated_at: 1790503715,
    });
    expect(status).toContain("待听总结 1 次");
    expect(status).toContain("上次失败");
  });

  it("orders pinned tasks before recent tasks without mutating the source", () => {
    const tasks = [
      {
        task_id: "b",
        name: "B",
        project: "P",
        updated_at_ms: 20,
        pinned: false,
        cli_created: true,
      },
      {
        task_id: "a",
        name: "A",
        project: "P",
        updated_at_ms: 10,
        pinned: true,
        cli_created: true,
      },
    ];
    expect(sortedTasks(tasks).map((task) => task.task_id)).toEqual(["a", "b"]);
    expect(tasks.map((task) => task.task_id)).toEqual(["b", "a"]);
  });
});
