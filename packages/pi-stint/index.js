import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

const SUBCOMMANDS = [
  "status",
  "board",
  "show",
  "view",
  "next",
  "add",
  "claim",
  "propose",
  "approve",
  "reject",
  "gate",
  "done",
  "unclaim",
  "check",
  "guide",
  "schema",
];

/**
 * Walk upward from `startDir` looking for a `.stint/` directory.
 */
export function findStintWorkspace(startDir = process.cwd()) {
  let current = resolve(startDir);
  while (true) {
    if (existsSync(join(current, ".stint"))) {
      return current;
    }
    const parent = dirname(current);
    if (parent === current) {
      return null;
    }
    current = parent;
  }
}

/**
 * Run the `stint` CLI and return stdout/stderr/exitCode.
 */
export async function runStint(args, options = {}) {
  const cwd = options.cwd || process.cwd();
  const bin = options.bin || process.env.STINT_BIN || "stint";
  try {
    const { stdout, stderr } = await execFileAsync(bin, args, {
      cwd,
      maxBuffer: 4 * 1024 * 1024,
    });
    return { ok: true, stdout: stdout.trim(), stderr: stderr.trim(), code: 0 };
  } catch (err) {
    return {
      ok: false,
      stdout: (err.stdout || "").trim(),
      stderr: (err.stderr || err.message || "").trim(),
      code: typeof err.code === "number" ? err.code : 1,
    };
  }
}

/**
 * Run `stint <...args> --json` and parse the JSON payload.
 */
export async function runStintJson(args, options = {}) {
  const fullArgs = args.includes("--json") ? args : [...args, "--json"];
  const res = await runStint(fullArgs, options);
  const raw = res.stdout || res.stderr;
  let data = null;
  if (raw) {
    try {
      data = JSON.parse(raw);
    } catch {
      data = null;
    }
  }
  return { ...res, data };
}

/**
 * Load a complete snapshot of the `.stint/` workspace for UI rendering and prompt injection.
 */
export async function loadWorkspaceSnapshot(options = {}) {
  const workspaceRoot = findStintWorkspace(options.cwd || process.cwd());
  if (!workspaceRoot) {
    return null;
  }

  const execOpts = { ...options, cwd: workspaceRoot };
  const [statusRes, nextRes, listRes, allRes, proposedRes, gateRes] = await Promise.all([
    runStintJson(["status"], execOpts),
    runStintJson(["next"], execOpts),
    runStintJson(["list"], execOpts),
    runStintJson(["list", "--all"], execOpts),
    runStintJson(["list", "--status", "proposed"], execOpts),
    runStintJson(["gate", "list"], execOpts),
  ]);

  const status = statusRes.data || {
    open_count: 0,
    in_progress_count: 0,
    proposed_count: 0,
    backlog_count: 0,
    blocked: [],
    open_gates: [],
    sprint_progress: null,
  };
  const next = nextRes.data || { ready: [], blocked: [] };
  const activeTasks = Array.isArray(listRes.data)
    ? listRes.data.filter((t) => t.state === "active")
    : [];
  const allTasks = Array.isArray(allRes.data) ? allRes.data : [];
  const proposedTasks = Array.isArray(proposedRes.data) ? proposedRes.data : [];
  const openGates = Array.isArray(gateRes.data)
    ? gateRes.data
    : status.open_gates || [];

  return {
    workspaceRoot,
    status,
    ready: next.ready || [],
    active: activeTasks,
    all: allTasks,
    proposed: proposedTasks,
    blocked: status.blocked || [],
    openGates,
  };
}

/**
 * Format a compact status bar string for Pi's footer (`ctx.ui.setStatus("stint", ...)`).
 */
export function formatStatusBar(snapshot) {
  if (!snapshot) {
    return "";
  }

  const parts = [];
  if (snapshot.active.length > 0) {
    const first = snapshot.active[0];
    const pri = first.priority ? ` ${first.priority}` : "";
    const extra = snapshot.active.length > 1 ? ` +${snapshot.active.length - 1}` : "";
    parts.push(`⚡ ${first.id}${pri}${extra}`);
  }

  if (snapshot.openGates.length > 0) {
    parts.push(
      `🚧 ${snapshot.openGates.length} gate${snapshot.openGates.length === 1 ? "" : "s"}`
    );
  }

  if (snapshot.ready.length > 0) {
    parts.push(`▶ ${snapshot.ready.length} ready`);
  }

  if (snapshot.proposed.length > 0) {
    parts.push(`💡 ${snapshot.proposed.length} proposed`);
  }

  if (parts.length === 0) {
    return "stint: idle";
  }

  return `stint: ${parts.join(" · ")}`;
}

/**
 * Format a visual Markdown dashboard of the Stint workspace.
 */
export function formatDashboardMarkdown(snapshot) {
  if (!snapshot) {
    return "No `.stint/` workspace found in the current directory hierarchy. Run `stint init` to create one.";
  }

  const lines = [];
  lines.push("## Stint Workspace");
  lines.push(
    `**Active**: ${snapshot.active.length} · **Ready**: ${snapshot.ready.length} · **Open Gates**: ${snapshot.openGates.length} · **Blocked**: ${snapshot.blocked.length} · **Proposed**: ${snapshot.proposed.length} · **Icebox**: ${snapshot.status.backlog_count || 0}`
  );

  if (snapshot.status.sprint_progress) {
    const sp = snapshot.status.sprint_progress;
    lines.push(
      `**Sprint ${sp.sprint_id}**: ${sp.done_count}/${sp.task_count} done (${sp.logged_hours}h logged / ${sp.committed_hours}h committed)`
    );
  }

  if (snapshot.openGates.length > 0) {
    lines.push("\n### 🚧 Open Decision Gates (Awaiting Human Input)");
    for (const g of snapshot.openGates) {
      const blocks = Array.isArray(g.blocks) && g.blocks.length > 0
        ? ` (blocks ${g.blocks.join(", ")})`
        : "";
      lines.push(`- **${g.id}** — ${g.title}${blocks}`);
      if (g.ask) {
        lines.push(`  - **Ask**: ${g.ask}`);
      }
    }
  }

  if (snapshot.active.length > 0) {
    lines.push("\n### ⚡ In Progress");
    for (const t of snapshot.active) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      lines.push(`- **${t.id}** ${meta ? `[${meta}] ` : ""}${t.title}`);
    }
  }

  if (snapshot.ready.length > 0) {
    lines.push("\n### ▶ Ready Queue (`stint next`)");
    for (const t of snapshot.ready.slice(0, 8)) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      const areas = Array.isArray(t.area) && t.area.length > 0 ? ` (${t.area.join(",")})` : "";
      lines.push(`- **${t.id}** ${meta ? `[${meta}] ` : ""}${t.title}${areas}`);
    }
  }

  if (snapshot.proposed.length > 0) {
    lines.push("\n### 💡 Proposed Ideas (Outside Pipeline)");
    for (const t of snapshot.proposed) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      lines.push(`- **${t.id}** ${meta ? `[${meta}] ` : ""}${t.title}`);
    }
  }

  if (snapshot.blocked.length > 0) {
    lines.push("\n### ⛔ Blocked Tasks");
    for (const b of snapshot.blocked) {
      lines.push(`- **${b.id}** ${b.title} ← blocked by \`${(b.blockers || []).join(", ")}\``);
    }
  }

  return lines.join("\n");
}

/**
 * Format a clean, human-readable terminal dashboard string (no raw markdown asterisks or hashes)
 * suitable for notifications, toasts, or non-markdown terminal output.
 */
export function formatDashboardTerminal(snapshot) {
  if (!snapshot) {
    return "No .stint/ workspace found. Run `stint init` to create one.";
  }

  const lines = [];
  lines.push("── Stint Workspace ─────────────────────────────");
  lines.push(
    `Active: ${snapshot.active.length} · Ready: ${snapshot.ready.length} · Gates: ${snapshot.openGates.length} · Blocked: ${snapshot.blocked.length} · Proposed: ${snapshot.proposed.length} · Icebox: ${snapshot.status.backlog_count || 0}`
  );

  if (snapshot.status.sprint_progress) {
    const sp = snapshot.status.sprint_progress;
    lines.push(
      `Sprint ${sp.sprint_id}: ${sp.done_count}/${sp.task_count} done (${sp.logged_hours}h logged / ${sp.committed_hours}h committed)`
    );
  }

  if (snapshot.openGates.length > 0) {
    lines.push("\nOpen Decision Gates (Awaiting Human Input):");
    for (const g of snapshot.openGates) {
      const blocks = Array.isArray(g.blocks) && g.blocks.length > 0
        ? ` (blocks ${g.blocks.join(", ")})`
        : "";
      lines.push(`• ${g.id} — ${g.title}${blocks}`);
      if (g.ask) {
        lines.push(`  Ask: ${g.ask}`);
      }
    }
  }

  if (snapshot.active.length > 0) {
    lines.push("\nIn Progress:");
    for (const t of snapshot.active) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      lines.push(`• ${t.id} ${meta ? `[${meta}] ` : ""}${t.title}`);
    }
  }

  if (snapshot.ready.length > 0) {
    lines.push("\nReady Queue (stint next):");
    for (const t of snapshot.ready.slice(0, 8)) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      const areas = Array.isArray(t.area) && t.area.length > 0 ? ` (${t.area.join(",")})` : "";
      lines.push(`• ${t.id} ${meta ? `[${meta}] ` : ""}${t.title}${areas}`);
    }
  }

  if (snapshot.proposed.length > 0) {
    lines.push("\nProposed Ideas:");
    for (const t of snapshot.proposed) {
      const meta = [t.priority, t.size].filter(Boolean).join("/");
      lines.push(`• ${t.id} ${meta ? `[${meta}] ` : ""}${t.title}`);
    }
  }

  if (snapshot.blocked.length > 0) {
    lines.push("\nBlocked Tasks:");
    for (const b of snapshot.blocked) {
      lines.push(`• ${b.id} ${b.title} ← blocked by ${(b.blockers || []).join(", ")}`);
    }
  }

  return lines.join("\n");
}

/**
 * Format a single task payload (`stint show <id> --json`) as a clean, scannable Markdown card.
 */
export function formatTaskMarkdown(task) {
  if (!task) {
    return "Task not found.";
  }

  const stateBadges = {
    active: "⚡ active",
    ready: "▶ ready",
    blocked: "⛔ blocked",
    proposed: "💡 proposed",
    backlog: "🧊 backlog",
    done: "✅ done",
    archived: "📦 archived",
  };

  const stateLabel = stateBadges[task.state] || task.state || task.status;
  const metaParts = [`**State**: ${stateLabel} (\`${task.status}\`)`];
  if (task.priority) metaParts.push(`**Priority**: \`${task.priority}\``);
  if (task.size) metaParts.push(`**Size**: \`${task.size}\``);
  if (task.sprint) metaParts.push(`**Sprint**: \`${task.sprint}\``);
  if (task.estimate) metaParts.push(`**Estimate**: \`${task.estimate}\``);
  if (task.actual) metaParts.push(`**Actual**: \`${task.actual}\``);

  const lines = [
    `## [${task.id}] ${task.title}`,
    metaParts.join(" · "),
  ];

  const taxonomyParts = [];
  if (Array.isArray(task.area) && task.area.length > 0) {
    taxonomyParts.push(`**Area**: ${task.area.map((a) => `\`${a}\``).join(", ")}`);
  }
  if (Array.isArray(task.tags) && task.tags.length > 0) {
    taxonomyParts.push(`**Tags**: ${task.tags.map((t) => `\`${t}\``).join(", ")}`);
  }
  if (Array.isArray(task.active_blockers) && task.active_blockers.length > 0) {
    taxonomyParts.push(
      `**Blocked by**: ${task.active_blockers.map((b) => `\`${b}\``).join(", ")}`
    );
  }
  if (taxonomyParts.length > 0) {
    lines.push(taxonomyParts.join(" · "));
  }

  if (task.path) {
    lines.push(`**File**: \`${task.path}\``);
  }

  const body = (task.body || "").trim();
  if (body) {
    lines.push("\n---\n");
    lines.push(body);
  }

  return lines.join("\n");
}

/**
 * Build a concise system-prompt section injected before each agent turn so the agent
 * has live situational awareness and knows how to drive `stint` without external skills.
 */
export function buildSystemPromptSection(snapshot) {
  if (!snapshot) {
    return null;
  }

  const lines = [
    "This repository tracks tasks, proposals, and human decision gates with the `stint` CLI (`.stint/`).",
    `- Live counts: ${snapshot.active.length} in-progress, ${snapshot.ready.length} ready, ${snapshot.openGates.length} open gate(s), ${snapshot.proposed.length} proposed.`,
  ];

  if (snapshot.active.length > 0) {
    const activeSummary = snapshot.active
      .map((t) => `${t.id} (${t.title})`)
      .join(", ");
    lines.push(`- Active claimed stint(s): ${activeSummary}`);
  }

  if (snapshot.openGates.length > 0) {
    const gateSummary = snapshot.openGates
      .map((g) => `${g.id}: ${g.ask || g.title} [blocks ${(g.blocks || []).join(",")}]`)
      .join("; ");
    lines.push(`- Open human decision gate(s): ${gateSummary}`);
  }

  if (snapshot.ready.length > 0) {
    const topReady = snapshot.ready
      .slice(0, 3)
      .map((t) => `${t.id} [${[t.priority, t.size].filter(Boolean).join("/")}] ${t.title}`)
      .join("; ");
    lines.push(`- Top ready queue: ${topReady}`);
  }

  lines.push(
    "- Commands: `stint guide [overview|add|propose|implement|gate|check]`, `stint show <id>`, `stint next --json`, `stint claim <id>`, `stint done <id>`, `stint propose \"<title>\" --why ...`, `stint approve <id>`, `stint gate file <blocked_id> \"<title>\" --ask ... --options ... --good-answer ... --evidence ...`."
  );

  return lines.join("\n");
}

async function refreshStatusBar(ctx, options = {}) {
  const snapshot = await loadWorkspaceSnapshot(options);
  if (ctx?.ui?.setStatus) {
    ctx.ui.setStatus("stint", formatStatusBar(snapshot));
  }
  return snapshot;
}

function notify(ctx, message, level = "info") {
  if (ctx?.ui?.notify) {
    ctx.ui.notify(message, level);
  }
}

/**
 * Display rich markdown content in Pi's interactive TUI using @earendil-works/pi-tui,
 * or fall back gracefully to notification when outside TUI mode.
 */
async function showMarkdownModal(ctx, title, markdownContent) {
  if (ctx?.mode === "tui" && ctx?.ui?.custom) {
    try {
      const { Container, Markdown, Text, DynamicBorder, matchesKey } = await import(
        "@earendil-works/pi-tui"
      );
      const agentPkg = await import("@earendil-works/pi-coding-agent").catch(() => ({}));
      const getMarkdownTheme = agentPkg.getMarkdownTheme;

      await ctx.ui.custom((_tui, theme, _kb, done) => {
        const container = new Container();
        const border = new DynamicBorder((s) => theme.fg("accent", s));
        const mdTheme = typeof getMarkdownTheme === "function" ? getMarkdownTheme() : undefined;

        container.addChild(border);
        container.addChild(new Text(theme.fg("accent", theme.bold(title)), 1, 0));
        container.addChild(new Markdown(markdownContent, 1, 1, mdTheme));
        container.addChild(new Text(theme.fg("dim", "Press Enter or Esc to close"), 1, 0));
        container.addChild(border);

        return {
          render: (width) => container.render(width),
          invalidate: () => container.invalidate(),
          handleInput: (data) => {
            if (matchesKey(data, "enter") || matchesKey(data, "escape")) {
              done(undefined);
            }
          },
        };
      });
      return true;
    } catch (_err) {
      // Fall through to notification fallback
    }
  }

  return false;
}

async function interactiveShowFlow(ctx, snapshot, targetId, options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };
  let taskId = (targetId || "").trim();

  if (!taskId) {
    const candidates = snapshot.all && snapshot.all.length > 0
      ? snapshot.all
      : [...snapshot.active, ...snapshot.ready, ...snapshot.proposed];

    if (candidates.length === 0) {
      notify(ctx, "No tasks found in this workspace.", "info");
      return;
    }

    if (ctx?.ui?.select) {
      const choice = await ctx.ui.select(
        "Select a task to view",
        candidates.map((t) => {
          const meta = [t.state || t.status, t.priority, t.size]
            .filter(Boolean)
            .join(" · ");
          return `${t.id} [${meta}] ${t.title}`;
        })
      );
      if (!choice) return;
      taskId = choice.split(" ")[0];
    } else {
      taskId = candidates[0].id;
    }
  }

  const showRes = await runStintJson(["show", taskId], execOpts);
  if (!showRes.ok || !showRes.data) {
    notify(
      ctx,
      showRes.stderr || showRes.stdout || `Task ${taskId} not found`,
      "error"
    );
    return;
  }

  const task = showRes.data;
  const displayed = await showMarkdownModal(
    ctx,
    `Task ${task.id}: ${task.title}`,
    formatTaskMarkdown(task)
  );
  if (!displayed) {
    notify(ctx, formatTaskMarkdown(task), "info");
  }

  if (!ctx?.ui?.select) {
    return;
  }

  // Offer quick contextual actions for the viewed task when applicable
  const taskActions = [];
  if (task.is_gate && task.status !== "done" && task.status !== "archived") {
    taskActions.push(`🚧 Resolve gate ${task.id}`);
  }
  if (task.state === "ready") {
    taskActions.push(`⚡ Claim ${task.id}`);
  }
  if (task.state === "active") {
    taskActions.push(`✅ Mark ${task.id} done`);
    taskActions.push(`↩ Unclaim ${task.id}`);
  }
  if (task.status === "proposed" || task.status === "backlog") {
    if (!task.is_gate) {
      taskActions.push(`💡 Approve ${task.id} into todo`);
      taskActions.push(`🗑 Reject & archive ${task.id}`);
    }
  }

  if (taskActions.length === 0) {
    return;
  }

  taskActions.push("Close");
  const action = await ctx.ui.select(`Actions for ${task.id} — ${task.title}`, taskActions);
  if (!action || action === "Close") return;

  if (action.startsWith("🚧")) {
    await interactiveGateFlow(ctx, snapshot, task.id, execOpts);
  } else if (action.startsWith("⚡")) {
    const claimRes = await runStintJson(["claim", task.id], execOpts);
    if (claimRes.ok) {
      notify(ctx, `Claimed ${task.id} — ${task.title}`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, claimRes.stderr || "Failed to claim task", "error");
    }
  } else if (action.startsWith("✅")) {
    const doneRes = await runStintJson(["done", task.id], execOpts);
    if (doneRes.ok) {
      notify(ctx, `Marked ${task.id} done.`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, doneRes.stderr || "Failed to complete task", "error");
    }
  } else if (action.startsWith("↩")) {
    const unclaimRes = await runStintJson(["unclaim", task.id], execOpts);
    if (unclaimRes.ok) {
      notify(ctx, `Unclaimed ${task.id}.`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, unclaimRes.stderr || "Failed to unclaim task", "error");
    }
  } else if (action.startsWith("💡")) {
    await interactiveApproveFlow(ctx, snapshot, task.id, execOpts);
  } else if (action.startsWith("🗑")) {
    const rejRes = await runStintJson(["reject", task.id], execOpts);
    if (rejRes.ok) {
      notify(ctx, `Rejected and archived ${task.id}.`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, rejRes.stderr || "Failed to reject task", "error");
    }
  }
}

async function interactiveGateFlow(ctx, snapshot, targetGateId, options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };
  let gateId = targetGateId;

  if (!gateId) {
    if (snapshot.openGates.length === 0) {
      notify(ctx, "No open decision gates.", "info");
      return;
    }
    if (ctx?.ui?.select && snapshot.openGates.length > 1) {
      const choice = await ctx.ui.select(
        "Select an open decision gate to inspect / resolve",
        snapshot.openGates.map(
          (g) => `${g.id} — ${g.title} (blocks ${(g.blocks || []).join(", ")})`
        )
      );
      if (!choice) return;
      gateId = choice.split(" ")[0];
    } else {
      gateId = snapshot.openGates[0].id;
    }
  }

  const showRes = await runStintJson(["gate", "show", gateId], execOpts);
  if (!showRes.ok || !showRes.data) {
    notify(ctx, `Failed to load gate ${gateId}: ${showRes.stderr || showRes.stdout}`, "error");
    return;
  }

  const gate = showRes.data;
  const summary = [
    `Gate ${gate.id}: ${gate.title}`,
    `Blocks: ${(gate.blocks || []).join(", ") || "none"}`,
    gate.ask ? `\nAsk:\n${gate.ask}` : "",
    gate.options ? `\nOptions:\n${gate.options}` : "",
    gate.good_answer ? `\nGood answer:\n${gate.good_answer}` : "",
    gate.test ? `\nTest:\n${gate.test}` : "",
    gate.evidence ? `\nEvidence:\n${gate.evidence}` : "",
  ]
    .filter(Boolean)
    .join("\n");

  notify(ctx, summary, "info");

  if (ctx?.ui?.input) {
    const decision = await ctx.ui.input(
      `Resolve gate ${gate.id} (${gate.ask || gate.title}) — enter decision (leave blank to cancel):`,
      "Approved Option A: ..."
    );
    if (decision && decision.trim()) {
      const resolveRes = await runStintJson(
        ["gate", "resolve", gate.id, "--decision", decision.trim()],
        execOpts
      );
      if (resolveRes.ok) {
        const unblocked = (resolveRes.data?.unblocked || [])
          .map((u) => `${u.id} (${u.title})`)
          .join(", ");
        notify(
          ctx,
          `Resolved gate ${gate.id}.${unblocked ? ` Unblocked: ${unblocked}` : ""}`,
          "info"
        );
        await refreshStatusBar(ctx, execOpts);
      } else {
        notify(ctx, `Failed to resolve gate ${gate.id}: ${resolveRes.stderr}`, "error");
      }
    }
  }
}

async function interactiveApproveFlow(ctx, snapshot, targetId, options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };
  let taskId = targetId;

  if (!taskId) {
    if (snapshot.proposed.length === 0) {
      notify(ctx, "No proposed tasks waiting for approval.", "info");
      return;
    }
    if (ctx?.ui?.select) {
      const choice = await ctx.ui.select(
        "Select a proposed task to approve into todo",
        snapshot.proposed.map(
          (t) => `${t.id} [${[t.priority, t.size].filter(Boolean).join("/") || "unscoped"}] ${t.title}`
        )
      );
      if (!choice) return;
      taskId = choice.split(" ")[0];
    } else {
      taskId = snapshot.proposed[0].id;
    }
  }

  let res = await runStintJson(["approve", taskId], execOpts);
  if (!res.ok && ctx?.ui?.input) {
    // Prompt for missing pass criteria fields interactively.
    const priority = await ctx.ui.input(
      `Approve ${taskId}: Priority (p0|p1|p2|p3|p4, default p2):`,
      "p2"
    );
    const size = await ctx.ui.input(`Approve ${taskId}: Size (s|m|l, default m):`, "m");
    const why = await ctx.ui.input(
      `Approve ${taskId}: ## Why (leave blank to keep existing):`,
      "Problem statement & North Star alignment"
    );
    const doneWhen = await ctx.ui.input(
      `Approve ${taskId}: ## Done When pass criterion (leave blank to keep existing):`,
      "Concrete verification criterion"
    );

    const args = [
      "approve",
      taskId,
      "--priority",
      (priority || "p2").trim(),
      "--size",
      (size || "m").trim(),
    ];
    if (why && why.trim()) {
      args.push("--why", why.trim());
    }
    if (doneWhen && doneWhen.trim()) {
      args.push("--done-when", doneWhen.trim());
    }
    res = await runStintJson(args, execOpts);
  }

  if (res.ok) {
    notify(ctx, `Approved ${taskId} into the active todo pipeline.`, "info");
    await refreshStatusBar(ctx, execOpts);
  } else {
    notify(ctx, res.stderr || res.stdout || `Failed to approve ${taskId}`, "error");
  }
}

async function interactiveAddFlow(ctx, snapshot, initialTitle = "", options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };
  let title = (initialTitle || "").trim();

  if (!title && ctx?.ui?.input) {
    const inputTitle = await ctx.ui.input("Task title:", "Short imperative summary");
    title = (inputTitle || "").trim();
  }
  if (!title) {
    notify(ctx, "Usage: /stint add <title>", "warning");
    return;
  }

  let priority = "p2";
  let size = "m";
  let why = "";
  let doneWhen = "";

  if (ctx?.ui?.input) {
    const priInput = await ctx.ui.input("Priority (p0|p1|p2|p3|p4, default p2):", "p2");
    if (priInput && priInput.trim()) priority = priInput.trim();

    const sizeInput = await ctx.ui.input("Size (s|m|l, default m):", "m");
    if (sizeInput && sizeInput.trim()) size = sizeInput.trim();

    const whyInput = await ctx.ui.input(
      "Why (problem statement & North Star tie-in):",
      "Why this task matters"
    );
    if (whyInput && whyInput.trim()) why = whyInput.trim();

    const doneWhenInput = await ctx.ui.input(
      "Done When (checkable pass criterion):",
      "Tests pass and behavior is verified"
    );
    if (doneWhenInput && doneWhenInput.trim()) doneWhen = doneWhenInput.trim();
  }

  const args = [
    "add",
    title,
    "--priority",
    priority,
    "--size",
    size,
    "--no-edit",
  ];
  if (why) {
    args.push("--why", why);
  }
  if (doneWhen) {
    args.push("--done-when", doneWhen);
  }

  const res = await runStintJson(args, execOpts);
  if (res.ok) {
    notify(
      ctx,
      `Created ready task ${res.data?.id || ""} [${priority}/${size}]: ${title}`,
      "info"
    );
    await refreshStatusBar(ctx, execOpts);
  } else {
    notify(ctx, res.stderr || res.stdout || "Failed to create task", "error");
  }
}

async function interactiveProposeFlow(ctx, snapshot, initialTitle = "", options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };
  let title = (initialTitle || "").trim();

  if (!title && ctx?.ui?.input) {
    const inputTitle = await ctx.ui.input(
      "Proposal title (rough idea or draft PRD):",
      "Idea summary"
    );
    title = (inputTitle || "").trim();
  }
  if (!title) {
    notify(ctx, "Usage: /stint propose <title>", "warning");
    return;
  }

  let why = "";
  if (ctx?.ui?.input) {
    const whyInput = await ctx.ui.input(
      "Why / idea notes (optional — press Enter to skip):",
      "Problem or motivation (optional)"
    );
    if (whyInput && whyInput.trim()) {
      why = whyInput.trim();
    }
  }

  const args = ["propose", title];
  if (why) {
    args.push("--why", why);
  }

  const propRes = await runStintJson(args, execOpts);
  if (propRes.ok) {
    notify(
      ctx,
      `Proposed ${propRes.data?.id || ""} (status: proposed, outside active queue): ${title}`,
      "info"
    );
    await refreshStatusBar(ctx, execOpts);
  } else {
    notify(ctx, propRes.stderr || "Failed to create proposal", "error");
  }
}

async function interactiveDashboard(ctx, snapshot, options = {}) {
  const execOpts = { ...options, cwd: snapshot.workspaceRoot };

  if (!ctx?.ui?.select) {
    const displayed = await showMarkdownModal(
      ctx,
      "Stint Workspace Dashboard",
      formatDashboardMarkdown(snapshot)
    );
    if (!displayed) {
      notify(ctx, formatDashboardTerminal(snapshot), "info");
    }
    return;
  }

  const actions = [];
  actions.push("📊 View full workspace dashboard");
  if (snapshot.openGates.length > 0) {
    actions.push(`🚧 Resolve open decision gate (${snapshot.openGates.length})`);
  }
  if (snapshot.ready.length > 0) {
    const top = snapshot.ready[0];
    actions.push(`⚡ Claim top ready task: ${top.id} — ${top.title}`);
  }
  if (snapshot.active.length > 0) {
    for (const a of snapshot.active) {
      actions.push(`✅ Complete active task: ${a.id} — ${a.title}`);
    }
  }
  if (snapshot.proposed.length > 0) {
    actions.push(`💡 Review / approve proposed tasks (${snapshot.proposed.length})`);
  }
  actions.push("🔎 View / inspect a task (`stint show`)");
  actions.push("➕ Propose a new idea (`status: proposed`)");
  actions.push("📝 Create ready task (`status: todo`)");
  actions.push("🔍 Run strict graph & pass-criteria check (`stint check --strict`)");
  actions.push("📖 View Stint operator guide (`stint guide`)");

  const counts = [
    `${snapshot.active.length} active`,
    `${snapshot.ready.length} ready`,
    `${snapshot.openGates.length} gates`,
    `${snapshot.proposed.length} proposed`,
  ].join(" · ");
  const picked = await ctx.ui.select(`Stint Workspace [${counts}]`, actions);
  if (!picked) return;

  if (picked.startsWith("📊")) {
    const displayed = await showMarkdownModal(
      ctx,
      "Stint Workspace Dashboard",
      formatDashboardMarkdown(snapshot)
    );
    if (!displayed) {
      notify(ctx, formatDashboardTerminal(snapshot), "info");
    }
    await interactiveDashboard(ctx, snapshot, options);
    return;
  }

  if (picked.startsWith("🚧")) {
    await interactiveGateFlow(ctx, snapshot, null, execOpts);
  } else if (picked.startsWith("⚡")) {
    const top = snapshot.ready[0];
    const claimRes = await runStintJson(["claim", top.id], execOpts);
    if (claimRes.ok) {
      notify(ctx, `Claimed ${top.id} — ${top.title}`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, claimRes.stderr || "Failed to claim task", "error");
    }
  } else if (picked.startsWith("✅")) {
    const id = picked.split(":")[1].trim().split(" ")[0];
    const doneRes = await runStintJson(["done", id], execOpts);
    if (doneRes.ok) {
      notify(ctx, `Completed ${id}`, "info");
      await refreshStatusBar(ctx, execOpts);
    } else {
      notify(ctx, doneRes.stderr || "Failed to mark task done", "error");
    }
  } else if (picked.startsWith("💡")) {
    await interactiveApproveFlow(ctx, snapshot, null, execOpts);
  } else if (picked.startsWith("🔎")) {
    await interactiveShowFlow(ctx, snapshot, "", execOpts);
  } else if (picked.startsWith("➕")) {
    await interactiveProposeFlow(ctx, snapshot, "", execOpts);
  } else if (picked.startsWith("📝")) {
    await interactiveAddFlow(ctx, snapshot, "", execOpts);
  } else if (picked.startsWith("🔍")) {
    const checkRes = await runStintJson(["check", "--strict"], execOpts);
    if (checkRes.ok) {
      notify(ctx, "stint check --strict: OK (all tasks and gates valid)", "info");
    } else {
      const errs = (checkRes.data?.errors || [checkRes.stderr]).join("\n");
      notify(ctx, `stint check --strict failed:\n${errs}`, "warning");
    }
  } else if (picked.startsWith("📖")) {
    const guideRes = await runStint(["guide"], execOpts);
    notify(ctx, guideRes.stdout || guideRes.stderr, "info");
  }
}

/**
 * Pi extension entrypoint.
 */
export default function stintExtension(pi) {
  pi.on("session_start", async (_event, ctx) => {
    await refreshStatusBar(ctx);
  });

  pi.on("before_agent_start", async (event, ctx) => {
    const snapshot = await refreshStatusBar(ctx);
    const promptSection = buildSystemPromptSection(snapshot);
    if (!promptSection) {
      if (event?.systemPromptOptions?.sections) {
        delete event.systemPromptOptions.sections.stint;
      }
      return;
    }
    const sections = event?.systemPromptOptions?.sections;
    if (sections && typeof sections === "object") {
      sections.stint = promptSection;
      return;
    }
    if (Array.isArray(event?.systemPrompt)) {
      return { systemPrompt: [...event.systemPrompt, promptSection] };
    }
    if (event?.systemPrompt !== undefined) {
      const base = event.systemPrompt ? `${event.systemPrompt}\n\n` : "";
      return { systemPrompt: `${base}${promptSection}` };
    }
  });

  pi.on("agent_end", async (_event, ctx) => {
    await refreshStatusBar(ctx);
  });

  pi.registerCommand("stint", {
    description:
      "Stint task board, task viewer, proposal triage, decision gate resolution, and status bar",
    getArgumentCompletions: (prefix) => {
      const trimmed = (prefix || "").trim();
      return SUBCOMMANDS.filter((c) => c.startsWith(trimmed)).map((c) => ({
        value: c,
        label: c,
      }));
    },
    handler: async (args, ctx) => {
      const snapshot = await refreshStatusBar(ctx);
      if (!snapshot) {
        notify(
          ctx,
          "No .stint/ workspace found in current directory. Run `stint init` first.",
          "warning"
        );
        return;
      }

      const execOpts = { cwd: snapshot.workspaceRoot };
      const tokens = (args || "")
        .trim()
        .split(/\s+/)
        .filter(Boolean);
      const sub = tokens[0] || "";
      const rest = tokens.slice(1);

      if (!sub || sub === "board") {
        await interactiveDashboard(ctx, snapshot, execOpts);
        return;
      }

      if (sub === "status") {
        const displayed = await showMarkdownModal(
          ctx,
          "Stint Workspace Status",
          formatDashboardMarkdown(snapshot)
        );
        if (!displayed) {
          notify(ctx, formatDashboardTerminal(snapshot), "info");
        }
        return;
      }

      if (sub === "show" || sub === "view") {
        await interactiveShowFlow(ctx, snapshot, rest[0] || "", execOpts);
        return;
      }

      // Allow bare numeric ID shorthand: `/stint 0017` or `/stint 17`
      if (/^\d+$/.test(sub)) {
        await interactiveShowFlow(ctx, snapshot, sub, execOpts);
        return;
      }

      if (sub === "next") {
        if (snapshot.ready.length === 0) {
          notify(ctx, "No ready tasks in queue.", "info");
          return;
        }
        const lines = snapshot.ready.map(
          (t) => `${t.id} [${[t.priority, t.size].filter(Boolean).join("/")}] ${t.title}`
        );
        notify(ctx, `Ready Queue:\n${lines.join("\n")}`, "info");
        return;
      }

      if (sub === "add") {
        await interactiveAddFlow(ctx, snapshot, rest.join(" "), execOpts);
        return;
      }

      if (sub === "claim") {
        const claimArgs = rest.length > 0 ? ["claim", rest[0]] : ["claim"];
        const res = await runStintJson(claimArgs, execOpts);
        if (res.ok) {
          notify(
            ctx,
            `Claimed ${res.data?.id || rest[0] || "top task"}: ${res.data?.title || ""}`,
            "info"
          );
          await refreshStatusBar(ctx, execOpts);
        } else {
          notify(ctx, res.stderr || "Failed to claim task", "error");
        }
        return;
      }

      if (sub === "done") {
        if (rest.length === 0) {
          notify(ctx, "Usage: /stint done <id> [actual]", "warning");
          return;
        }
        const res = await runStintJson(["done", ...rest], execOpts);
        if (res.ok) {
          notify(ctx, `Marked ${rest[0]} done.`, "info");
          await refreshStatusBar(ctx, execOpts);
        } else {
          notify(ctx, res.stderr || "Failed to complete task", "error");
        }
        return;
      }

      if (sub === "unclaim") {
        if (rest.length === 0) {
          notify(ctx, "Usage: /stint unclaim <id>", "warning");
          return;
        }
        const res = await runStintJson(["unclaim", rest[0]], execOpts);
        if (res.ok) {
          notify(ctx, `Unclaimed ${rest[0]}.`, "info");
          await refreshStatusBar(ctx, execOpts);
        } else {
          notify(ctx, res.stderr || "Failed to unclaim task", "error");
        }
        return;
      }

      if (sub === "propose") {
        await interactiveProposeFlow(ctx, snapshot, rest.join(" "), execOpts);
        return;
      }

      if (sub === "approve") {
        await interactiveApproveFlow(ctx, snapshot, rest[0] || null, execOpts);
        return;
      }

      if (sub === "reject") {
        const target = rest[0];
        if (!target) {
          notify(ctx, "Usage: /stint reject <id> [reason]", "warning");
          return;
        }
        const reason = rest.slice(1).join(" ").trim();
        const rejectArgs = ["reject", target];
        if (reason) {
          rejectArgs.push("--reason", reason);
        }
        const res = await runStintJson(rejectArgs, execOpts);
        if (res.ok) {
          notify(ctx, `Rejected and archived ${target}.`, "info");
          await refreshStatusBar(ctx, execOpts);
        } else {
          notify(ctx, res.stderr || `Failed to reject ${target}`, "error");
        }
        return;
      }

      if (sub === "gate") {
        await interactiveGateFlow(ctx, snapshot, rest[0] || null, execOpts);
        return;
      }

      if (sub === "check") {
        const res = await runStintJson(["check", "--strict"], execOpts);
        if (res.ok) {
          notify(ctx, "stint check --strict: OK", "info");
        } else {
          const errs = (res.data?.errors || [res.stderr]).join("\n");
          notify(ctx, `stint check --strict failed:\n${errs}`, "warning");
        }
        return;
      }

      if (sub === "guide" || sub === "schema") {
        const res = await runStint([sub, ...rest], execOpts);
        notify(ctx, res.stdout || res.stderr, "info");
        return;
      }

      notify(
        ctx,
        `Unknown /stint subcommand '${sub}'. Available: ${SUBCOMMANDS.join(", ")}`,
        "warning"
      );
    },
  });
}
