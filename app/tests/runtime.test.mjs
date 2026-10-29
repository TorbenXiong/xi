import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm, rmdir } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const binary = fileURLToPath(new URL(
  process.platform === "win32" ? "../../target/debug/xi-agent-cli.exe" : "../../target/debug/xi-agent-cli",
  import.meta.url,
));

// 使用随机端口和临时工作目录；不读取开发环境的凭据或数据库。
async function startRuntime(cwd) {
  const child = spawn(binary, [], { cwd, env: { ...process.env, XI_DATA_DIR: join(cwd, "data") }, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  const endpoint = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.kill(); reject(new Error("Runtime startup timed out")); }, 10_000);
    child.once("error", (error) => { clearTimeout(timer); reject(error); });
    child.once("exit", () => { clearTimeout(timer); reject(new Error("Runtime exited before startup")); });
    child.stderr.resume();
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const match = output.match(/http:\/\/127\.0\.0\.1:\d+\/\?token=[a-f0-9]+/);
      if (match) { clearTimeout(timer); resolve(new URL(match[0])); }
    });
  });
  const token = endpoint.searchParams.get("token");
  return {
    async request(method, path, body, authorized = true) {
      return fetch(endpoint.origin + path, {
        method,
        headers: {
          ...(authorized ? { "X-Xi-Token": token } : {}),
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    },
    async stop() {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = once(child, "exit");
      child.kill();
      await exited;
    },
  };
}

test("built Runtime serves explicit targets and restores SQLite across process restarts", { timeout: 30_000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), "xi-runtime-"));
  const calls = [];
  const model = createServer(async (request, response) => {
    response.setHeader("Content-Type", "application/json");
    if (request.url === "/v1/models") {
      response.end(JSON.stringify({ data: [{ id: "fixture-b" }, { id: "fixture-a" }] }));
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    const input = JSON.parse(body);
    calls.push({ authorization: request.headers.authorization, input });
    if (input.stream) {
      response.setHeader("Content-Type", "text/event-stream");
      response.write('data: {"choices":[{"delta":{"content":"fixture stream one "}}]}\n\n');
      if (input.messages.at(-1).content === "interrupt") { response.end(); return; }
      setTimeout(() => response.end('data: {"choices":[{"delta":{"content":"fixture stream two"}}]}\n\ndata: [DONE]\n\n'), 30);
    } else response.end(JSON.stringify({ choices: [{ message: { role: "assistant", content: "fixture answer" } }] }));
  });
  model.listen(0, "127.0.0.1");
  await once(model, "listening");
  let runtime;
  try {
    runtime = await startRuntime(directory);
    assert.equal((await runtime.request("GET", "/api/workspace", undefined, false)).status, 401);
    const preflight = await runtime.request("OPTIONS", "/api/chat", undefined, false);
    assert.equal(preflight.status, 204);
    assert.equal(preflight.headers.get("Access-Control-Allow-Origin"), "*");
    assert.match(preflight.headers.get("Access-Control-Allow-Headers"), /X-Xi-Token/i);

    async function json(method, path, body) {
      const response = await runtime.request(method, path, body);
      assert.equal(response.status, 200, `Unexpected status for ${method} ${path}`);
      return response.json();
    }
    const workspace = await json("GET", "/api/workspace");
    assert.equal(workspace.groups[0].name, "未分组");
    assert.equal(workspace.sessions.length, 0);
    const base_url = `http://127.0.0.1:${model.address().port}/v1`;
    const models = await json("POST", "/api/providers/models", { base_url, api_key: "FIXTURE_ONLY" });
    assert.deepEqual(models, ["fixture-a", "fixture-b"]);
    const provider = await json("POST", "/api/providers", { base_url, api_key: "FIXTURE_ONLY", models });
    const providers = await json("GET", "/api/providers");
    assert.equal(providers[0].id, provider.id);
    assert.equal(providers[0].api_key_configured, true);
    assert.ok(!JSON.stringify(providers).includes("FIXTURE_ONLY"));
    const first = await json("POST", "/api/sessions", { group_id: workspace.groups[0].id });
    const second = await json("POST", "/api/sessions", { group_id: workspace.groups[0].id });
    const answer = await json("POST", "/api/chat", {
      session_id: first.session_id, provider_id: provider.id,
      content: "fixture question", model: "fixture-b", reasoning_effort: "high",
    });
    assert.equal(answer.session_id, first.session_id);
    assert.equal(answer.message.content, "fixture answer");
    assert.equal(calls[0].authorization, "Bearer FIXTURE_ONLY");
    assert.equal(calls[0].input.model, "fixture-b");
    assert.equal(calls[0].input.reasoning_effort, "high");
    assert.equal((await json("GET", `/api/messages?session_id=${first.session_id}`)).length, 2);
    assert.deepEqual(await json("GET", `/api/messages?session_id=${second.session_id}`), []);
    assert.deepEqual(await json("GET", "/api/messages"), []);
    const unknown = await runtime.request("GET", "/api/messages?session_id=missing%20session");
    assert.equal(unknown.status, 400);
    await unknown.arrayBuffer();

    await runtime.stop();
    runtime = await startRuntime(directory);
    assert.equal((await json("GET", `/api/messages?session_id=${first.session_id}`)).length, 2);
    assert.deepEqual((await json("GET", "/api/providers"))[0].models, models);
    assert.equal((await json("GET", "/api/sessions/search?query=fixture%20question"))[0].id, first.session_id);
    assert.equal((await json("GET", `/api/sessions/history?session_id=${first.session_id}`))[1].status, "complete");
    assert.match((await json("GET", `/api/sessions/export?session_id=${first.session_id}`)).content, /fixture answer/);
    const group = await json("POST", "/api/groups", { name: "move-fixture" });
    await json("POST", "/api/sessions/move", { id: first.session_id, group_id: group.id });
    await json("POST", "/api/groups/delete", { id: group.id });
    assert.equal((await json("GET", "/api/workspace")).sessions.find(s => s.id === first.session_id).group_id, workspace.groups[0].id);
    await json("POST", "/api/reset", { session_id: second.session_id });
    assert.equal((await json("GET", `/api/messages?session_id=${first.session_id}`)).length, 2);
    await json("POST", "/api/sessions/delete", { id: first.session_id });
    assert.equal((await json("GET", "/api/workspace")).sessions.length, 1);
    await json("POST", "/api/sessions/archive", { id: second.session_id });
    assert.equal((await json("GET", "/api/workspace")).sessions.length, 0);
    await runtime.stop();
    const cli = spawn(binary, ["--cli"], { cwd: directory, env: { ...process.env, XI_DATA_DIR: join(directory,"data") }, windowsHide: true });
    let output = "", errors = "";
    cli.stdout.setEncoding("utf8"); cli.stderr.setEncoding("utf8");
    cli.stdout.on("data", chunk => { output += chunk; });
    cli.stderr.on("data", chunk => { errors += chunk; });
    const exported = join(directory,"export.md"), partial = join(directory,"partial.md");
    cli.stdin.end([
      "/group-new cli-fixture", "/new cli-session", "cli question", "/rename renamed-cli", "/search cli question",
      "/export " + exported, "/group-rename cli-renamed", "/groups", "/group-delete", "/use renamed-cli",
      "/group-move renamed-cli 未分组", "/archive", "/new alias-session", "alias question", "/delete", "/new partial-cli", "interrupt", "/export " + partial, "/exit", "",
    ].join("\n"));
    const timer = setTimeout(() => cli.kill(), 10_000);
    const [code] = await once(cli,"exit"); clearTimeout(timer);
    assert.equal(code,0);
    assert.match(output,/fixture stream one fixture stream two/);
    assert.match(errors,/模型响应在结束事件前中断/);
    const { readFile } = await import("node:fs/promises");
    assert.match(await readFile(exported,"utf8"),/fixture stream two/);
    assert.match(await readFile(partial,"utf8"),/interrupted/);
    await rm(exported); await rm(partial);
    runtime = await startRuntime(directory);
    const search = await json("GET","/api/sessions/search?query=partial-cli");
    const history = await json("GET",`/api/sessions/history?session_id=${search[0].id}`);
    assert.equal(history[1].status,"interrupted");
    assert.equal(history[1].content,"fixture stream one ");
  } finally {
    if (runtime) await runtime.stop();
    await new Promise((resolve) => model.close(resolve));
    for (const file of ["xi.db", "xi.db-wal", "xi.db-shm", "xi.lock"]) await rm(join(directory, "data", file), { force: true });
    await rmdir(join(directory, "data"));
    await rmdir(directory);
  }
});
