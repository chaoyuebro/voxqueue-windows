import test from "node:test";
import assert from "node:assert/strict";
import {presentActivity,presentConnections} from "./view-model.js";
test("every task stage and failure has a distinct visible label",()=>{
 const labels=["recording","recognizing","waiting_delivery","executing","summarizing","unread"].map(activity_phase=>presentActivity({activity_phase}).label);
 assert.equal(new Set(labels).size,6);
 assert.equal(presentActivity({activity_phase:"recognition_failed"}).tone,"error");
 assert.equal(presentActivity({activity_phase:"idle"}).label,"空闲");
});
test("connection rendering distinguishes offline, started and usable IPC",()=>{
 assert.equal(presentConnections(null).codex,"Codex 状态不可用");
 assert.equal(presentConnections({lan:{keyboard_connected:false},codex_running:false}).codex,"Codex 未启动");
 assert.equal(presentConnections({lan:{keyboard_connected:true},codex_running:true,codex_connected:false}).codex,"Codex 已启动，接口未就绪");
 assert.equal(presentConnections({lan:{keyboard_connected:true},codex_running:true,codex_connected:true}).codex,"Codex 已连接");
});
