import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import stintExtension, {
  buildSystemPromptSection,
  formatDashboardMarkdown,
  formatDashboardTerminal,
  formatStatusBar,
  loadWorkspaceSnapshot,
  runStint,
  runStintJson,
} from "../index.js";

test("pi-stint integrates with stint CLI for proposals, approval, gates, and Pi UI hooks", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "pi-stint-test-"));
  try {
    const initRes = await runStint(["init"], { cwd: tmp });
    assert.equal(initRes.ok, true, initRes.stderr);

    // 1. Propose an idea (stays out of ready queue)
    const propRes = await runStintJson(
      ["propose", "Add Redis cache", "--why", "Reduce DB load"],
      { cwd: tmp }
    );
    assert.equal(propRes.ok, true, propRes.stderr);
    assert.equal(propRes.data.status, "proposed");

    let snap = await loadWorkspaceSnapshot({ cwd: tmp });
    assert.equal(snap.proposed.length, 1);
    assert.equal(snap.ready.length, 0);
    assert.equal(formatStatusBar(snap), "stint: 💡 1 proposed");

    // 2. Approve the proposal with pass criteria
    const approveRes = await runStintJson(
      [
        "approve",
        "0001",
        "--priority",
        "p1",
        "--size",
        "s",
        "--done-when",
        "Cache hit rate > 80%",
      ],
      { cwd: tmp }
    );
    assert.equal(approveRes.ok, true, approveRes.stderr);

    snap = await loadWorkspaceSnapshot({ cwd: tmp });
    assert.equal(snap.proposed.length, 0);
    assert.equal(snap.ready.length, 1);
    assert.equal(formatStatusBar(snap), "stint: ▶ 1 ready");

    // 3. Claim task 0001 via Pi extension command handler
    const events = {};
    const commands = {};
    const statuses = [];
    const notifications = [];

    const mockPi = {
      on(event, fn) {
        events[event] = fn;
      },
      registerCommand(name, spec) {
        commands[name] = spec;
      },
    };

    stintExtension(mockPi);
    assert.ok(commands.stint);

    const origCwd = process.cwd();
    process.chdir(tmp);
    try {
      const mockCtx = {
        ui: {
          setStatus(key, val) {
            statuses.push({ key, val });
          },
          notify(msg, level) {
            notifications.push({ msg, level });
          },
        },
      };

      await commands.stint.handler("claim 0001", mockCtx);
      assert.match(notifications.at(-1).msg, /Claimed 0001/);
      assert.equal(statuses.at(-1).val, "stint: ⚡ 0001 p1");

      // 4. File a decision gate blocking 0001 -> auto-unclaims 0001
      const gateRes = await runStintJson(
        [
          "gate",
          "file",
          "0001",
          "Eviction policy",
          "--ask",
          "Use volatile-lru or allkeys-lru?",
          "--options",
          "A: allkeys-lru (recommended)",
          "--options",
          "B: volatile-lru",
          "--good-answer",
          "Pick A or B",
          "--evidence",
          "All keys are cacheable",
        ],
        { cwd: tmp }
      );
      assert.equal(gateRes.ok, true, gateRes.stderr);
      assert.equal(gateRes.data.unclaimed, true);

      snap = await loadWorkspaceSnapshot({ cwd: tmp });
      assert.equal(snap.openGates.length, 1);
      assert.equal(snap.active.length, 0);
      assert.equal(formatStatusBar(snap), "stint: 🚧 1 gate");

      const promptSection = buildSystemPromptSection(snap);
      assert.match(promptSection, /Open human decision gate\(s\): 0002/);
      assert.match(formatDashboardMarkdown(snap), /Open Decision Gates/);

      // 5. Resolve gate interactively via /stint gate
      mockCtx.ui.input = async () => "Option A: allkeys-lru";
      await commands.stint.handler("gate 0002", mockCtx);
      assert.match(notifications.at(-1).msg, /Resolved gate 0002/);
      assert.equal(statuses.at(-1).val, "stint: ▶ 1 ready");

      // 6. Create a ready task via /stint add and verify strict check passes
      const addInputs = ["p1", "s", "Need metrics visibility", "Prometheus endpoint exported"];
      mockCtx.ui.input = async () => addInputs.shift();
      await commands.stint.handler("add Export cache metrics", mockCtx);
      assert.match(notifications.at(-1).msg, /Created ready task 0003 \[p1\/s\]/);
      assert.equal(statuses.at(-1).val, "stint: ▶ 2 ready");

      // 7. Create a proposal via /stint propose (interactive prompt) and verify it stays outside ready queue
      const proposeInputs = ["Experiment with DragonflyDB", "Explore drop-in Redis replacement"];
      mockCtx.ui.input = async () => proposeInputs.shift();
      await commands.stint.handler("propose", mockCtx);
      assert.match(notifications.at(-1).msg, /Proposed 0004/);
      assert.equal(statuses.at(-1).val, "stint: ▶ 2 ready · 💡 1 proposed");

      // 8. Cleanly view a task via `/stint show 0003` and bare `/stint 0004`
      await commands.stint.handler("show 0003", mockCtx);
      assert.match(notifications.at(-1).msg, /## \[0003\] Export cache metrics/);
      assert.match(notifications.at(-1).msg, /## Why\s+Need metrics visibility/);
      assert.match(notifications.at(-1).msg, /## Done When\s+- Prometheus endpoint exported/);

      await commands.stint.handler("0004", mockCtx);
      assert.match(notifications.at(-1).msg, /## \[0004\] Experiment with DragonflyDB/);
      assert.match(notifications.at(-1).msg, /\*\*State\*\*: 💡 proposed/);

      const strictCheck = await runStintJson(["check", "--strict"], { cwd: tmp });
      assert.equal(strictCheck.ok, true, strictCheck.stderr);

      // 9. Verify clean terminal dashboard formatting without unrendered markdown
      const terminalDashboard = formatDashboardTerminal(snap);
      assert.ok(!terminalDashboard.includes("##"), "must not include markdown headings");
      assert.ok(!terminalDashboard.includes("**"), "must not include markdown asterisks");
      assert.match(terminalDashboard, /Stint Workspace/);
      assert.match(terminalDashboard, /Active: \d+/);

      // 10. When running `/stint status` in fallback notify mode, it emits clean terminal text
      await commands.stint.handler("status", mockCtx);
      assert.ok(!notifications.at(-1).msg.includes("##"), "status notification must not contain markdown headings");
      assert.ok(!notifications.at(-1).msg.includes("**"), "status notification must not contain markdown asterisks");
    } finally {
      process.chdir(origCwd);
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
