import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, writeFile, rm, rmdir } from "node:fs/promises";
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

test("projects, model capabilities and explicit targets persist across Runtime and CLI restarts", { timeout: 30_000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), "xi-runtime-"));
  const projectDirectory = join(directory, "项目 A with spaces");
  await mkdir(projectDirectory);
  const sentinel = join(projectDirectory, "keep.txt");
  await writeFile(sentinel, "project files must remain");
  const calls = [];
  let advertised = [{ id: "fixture-a", reasoning_efforts: ["low", "high", "max"] }, { id: "fixture-b" }];
  const model = createServer(async (request, response) => {
    response.setHeader("Content-Type", "application/json");
    if (request.url === "/v1/models") { response.end(JSON.stringify({ data: advertised })); return; }
    let body = ""; for await (const chunk of request) body += chunk;
    const input = JSON.parse(body);
    calls.push({ authorization: request.headers.authorization, input });
    if (input.messages.at(-1).content === "fail") {
      response.statusCode = 503; response.end(JSON.stringify({ error: "fixture unavailable" })); return;
    }
    if (input.stream) {
      response.setHeader("Content-Type", "text/event-stream");
      response.write('data: {"choices":[{"delta":{"content":"fixture stream one "}}]}\n\n');
      if (input.messages.at(-1).content === "interrupt") { response.end(); return; }
      setTimeout(() => response.end('data: {"choices":[{"delta":{"content":"fixture stream two"}}]}\n\ndata: [DONE]\n\n'), 20);
    } else response.end(JSON.stringify({ choices: [{ message: { role: "assistant", content: "fixture answer" } }] }));
  });
  model.listen(0, "127.0.0.1"); await once(model, "listening");
  let runtime;
  try {
    runtime = await startRuntime(directory);
    async function json(method, path, body) {
      const response = await runtime.request(method, path, body);
      assert.equal(response.status, 200, `Unexpected status for ${method} ${path}: ${await response.clone().text()}`);
      return response.json();
    }
    assert.equal((await runtime.request("GET", "/api/workspace", undefined, false)).status, 401);
    assert.equal((await runtime.request("OPTIONS", "/api/chat", undefined, false)).status, 204);
    assert.equal((await runtime.request("POST", "/api/groups", { name: "removed" })).status, 404);
    assert.equal((await runtime.request("GET", "/api/messages")).status, 404);
    assert.equal((await runtime.request("POST", "/api/chat", { content: "missing targets" })).status, 400);
    assert.deepEqual((await json("GET", "/api/workspace")).projects, []);
    assert.equal((await runtime.request("POST", "/api/projects", { directory: "relative" })).status, 400);
    assert.equal((await runtime.request("POST", "/api/projects", { directory: sentinel })).status, 400);
    const project = await json("POST", "/api/projects", { directory: projectDirectory, name: "fixture-project" });
    const reopened = await json("POST", "/api/projects", { directory: projectDirectory, name: "ignored rename" });
    assert.equal(reopened.id, project.id);
    await json("POST", "/api/projects/rename", { id: project.id, name: "renamed-project" });
    const base_url = `http://127.0.0.1:${model.address().port}/v1`;
    const models = await json("POST", "/api/providers/models", { base_url, api_key: "FIXTURE_ONLY" });
    assert.deepEqual(models.map(model => model.id), ["fixture-a", "fixture-b"]);
    assert.deepEqual(models[0].reasoning_efforts, ["low", "high", "max"]);
    assert.equal(models[0].reasoning_source, "provider");
    assert.deepEqual(models[1].reasoning_efforts, []);
    const provider = await json("POST", "/api/providers", { name: "fixture", base_url, api_key: "FIXTURE_ONLY", models });
    assert.ok(!JSON.stringify(await json("GET", "/api/providers")).includes("FIXTURE_ONLY"));
    advertised = [{ id: "preview-only" }];
    await json("POST", "/api/providers/models", { id: provider.id, api_key: "PREVIEW_FIXTURE_ONLY" });
    assert.deepEqual((await json("GET", "/api/providers"))[0].models, models);
    const custom = [{ id: "fixture-a", reasoning_efforts: ["max"], reasoning_source: "custom" }];
    await json("POST", "/api/providers/update", { id: provider.id, name: "fixture", base_url, api_key: "", models: custom });
    advertised = [{ id: "fixture-a" }];
    assert.deepEqual(await json("POST", "/api/providers/models", { id: provider.id }), custom);
    assert.deepEqual((await json("GET", "/api/providers"))[0].models, custom);
    await json("POST", "/api/providers/update", { id: provider.id, name: "fixture", base_url, api_key: "", models });
    const first = await json("POST", "/api/sessions", { project_id: project.id });
    const second = await json("POST", "/api/sessions", { project_id: null, title: "outside" });
    assert.equal(first.created, undefined);
    assert.equal((await runtime.request("POST", "/api/chat", {
      session_id: first.session_id, provider_id: provider.id, model: "fixture-a", content: "invalid level", reasoning_effort: "medium",
    })).status, 400);
    assert.deepEqual(await json("GET", `/api/sessions/history?session_id=${first.session_id}`), []);
    const answer = await json("POST", "/api/chat", { session_id: first.session_id, provider_id: provider.id, model: "fixture-a", content: "fixture question", reasoning_effort: "high" });
    assert.equal(answer.message.content, "fixture answer");
    assert.equal(calls[0].input.reasoning_effort, "high");
    assert.equal(calls[0].authorization, "Bearer FIXTURE_ONLY");
    const secondProvider = await json("POST", "/api/providers", { name: "second", base_url, api_key: "OTHER_FIXTURE_ONLY", models });
    await json("POST", "/api/chat", { session_id: second.session_id, provider_id: secondProvider.id, model: "fixture-b", content: "service default" });
    assert.ok(!("reasoning_effort" in calls[1].input), "unknown capabilities must omit reasoning override");
    assert.equal(calls[1].authorization, "Bearer OTHER_FIXTURE_ONLY");
    assert.equal((await json("GET", `/api/sessions/history?session_id=${first.session_id}`)).length, 2);
    await json("POST", "/api/reset", { session_id: second.session_id });
    assert.deepEqual(await json("GET", `/api/sessions/history?session_id=${second.session_id}`), []);
    await json("POST", "/api/projects/archive", { id: project.id });
    assert.ok(!(await json("GET", "/api/workspace")).sessions.some(session => session.id === first.session_id));
    assert.equal((await json("POST", "/api/projects", { directory: projectDirectory })).id, project.id);
    assert.equal((await json("GET", `/api/sessions/history?session_id=${first.session_id}`)).length, 2);
    await json("POST", "/api/projects/delete", { id: project.id });
    assert.equal((await json("GET", "/api/workspace")).sessions.find(session => session.id === first.session_id).project_id, null);
    assert.equal(await readFile(sentinel, "utf8"), "project files must remain");
    await runtime.stop(); runtime = await startRuntime(directory);
    assert.equal((await json("GET", `/api/sessions/history?session_id=${first.session_id}`))[1].status, "complete");
    assert.equal((await json("GET", "/api/sessions/search?query=fixture%20question"))[0].id, first.session_id);
    assert.match((await json("GET", `/api/sessions/export?session_id=${first.session_id}`)).content, /fixture answer/);
    await json("POST", "/api/sessions/delete", { id: first.session_id });
    await json("POST", "/api/sessions/archive", { id: second.session_id });
    assert.equal((await json("GET", "/api/workspace")).sessions.length, 0);
    await runtime.stop();
    const cli = spawn(binary, ["--cli"], { cwd: directory, env: { ...process.env, XI_DATA_DIR: join(directory, "data") }, windowsHide: true });
    let output = "", errors = "";
    cli.stdout.setEncoding("utf8"); cli.stderr.setEncoding("utf8");
    cli.stdout.on("data", chunk => { output += chunk; }); cli.stderr.on("data", chunk => { errors += chunk; });
    const exported = join(directory, "export.md"), partial = join(directory, "partial.md");
    cli.stdin.end([
      "/project-open " + projectDirectory, "/project-rename cli-project", "/projects", "/models", "/model fixture-a", "/reasoning max",
      "/new cli-session", "cli question", "/rename renamed-cli", "/search cli question", "/export " + exported,
      "/project-remove", "/use renamed-cli", "/project-move renamed-cli -", "/archive", "/new partial-cli", "interrupt", "/export " + partial,
      "/delete", "/exit", "",
    ].join("\n"));
    const timer = setTimeout(() => cli.kill(), 10_000);
    const [code] = await once(cli, "exit"); clearTimeout(timer);
    assert.equal(code, 0);
    assert.match(output, /fixture stream one fixture stream two/);
    assert.match(errors, /模型响应在结束事件前中断/);
    assert.match(errors, /未知命令：\/delete/);
    assert.match(await readFile(exported, "utf8"), /fixture stream two/);
    assert.match(await readFile(partial, "utf8"), /interrupted/);
    runtime = await startRuntime(directory);
    const found = await json("GET", "/api/sessions/search?query=partial-cli");
    const history = await json("GET", `/api/sessions/history?session_id=${found[0].id}`);
    assert.equal(history[1].status, "interrupted");
    assert.equal(history[1].content, "fixture stream one ");
  } finally {
    if (runtime) await runtime.stop();
    await new Promise(resolve => model.close(resolve));
    for (const file of ["xi.db", "xi.db-wal", "xi.db-shm", "xi.lock"]) await rm(join(directory, "data", file), { force: true });
    for (const file of ["export.md", "partial.md"]) await rm(join(directory, file), { force: true });
    await rm(sentinel); await rmdir(projectDirectory); await rmdir(join(directory, "data")); await rmdir(directory);
  }
});
