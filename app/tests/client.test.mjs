import assert from "node:assert/strict";
import test from "node:test";
import { createXiClient } from "../src/client/XiClient.ts";
import { createHttpTransport } from "../src/client/http.ts";

test("relational session operations share the injected HTTP adapter", async () => {
  const calls = [];
  const client = createXiClient(createHttpTransport({ baseUrl:"http://localhost:43127", token:async()=>"TEST_TOKEN",
    fetch:async(url,options)=>{ calls.push({url,method:options.method,body:options.body && JSON.parse(options.body)}); return Response.json({}); } }));
  await client.searchSessions("hello /?#"); await client.getHistory("a/b"); await client.exportSession("a/b");
  await client.moveSession("a/b","project"); await client.archiveSession("a/b"); await client.archiveProject("project");
  assert.equal(new URL(calls[0].url).searchParams.get("query"),"hello /?#");
  assert.equal(new URL(calls[1].url).searchParams.get("session_id"),"a/b");
  assert.equal(new URL(calls[2].url).pathname,"/api/sessions/export");
  assert.deepEqual(calls[3].body,{id:"a/b",project_id:"project"});
  assert.equal(new URL(calls[4].url).pathname,"/api/sessions/archive");
  assert.equal(new URL(calls[5].url).pathname,"/api/projects/archive");
});

test("XiClient can use an injected transport without browser or Tauri globals", async () => {
  const calls = [];
  const client = createXiClient({
    async request(operation, input) {
      calls.push({ operation, input });
      return { session_id: "new-session" };
    },
  });
  const created = await client.createSession("project-a");
  assert.equal(created.session_id, "new-session");
  assert.deepEqual(calls, [{ operation: "session/create", input: { project_id: "project-a", title: "新会话" } }]);
});

test("two clients send explicit session and Provider targets over a shared HTTP adapter", async () => {
  const calls = [];
  const transport = createHttpTransport({
    baseUrl: "http://localhost:43127/",
    token: async () => "TEST_TOKEN",
    fetch: async (url, options) => {
      calls.push({ url, options, body: options.body ? JSON.parse(options.body) : undefined });
      return Response.json({ message: { role: "assistant", content: "fixture" } });
    },
  });
  const first = createXiClient(transport);
  const second = createXiClient(transport);
  await first.sendMessage({ session_id: "a", provider_id: "provider-a", model: "model-a", content: "hello", reasoning_effort: "low" });
  await second.sendMessage({ session_id: "b", provider_id: "provider-b", model: "model-b", content: "world" });
  assert.equal(calls[0].url, "http://localhost:43127/api/chat");
  assert.equal(calls[0].options.headers.get("X-Xi-Token"), "TEST_TOKEN");
  assert.equal(calls[0].body.session_id, "a");
  assert.equal(calls[0].body.provider_id, "provider-a");
  assert.equal(calls[0].body.reasoning_effort, "low");
  assert.equal(calls[1].body.session_id, "b");
  assert.equal(calls[1].body.provider_id, "provider-b");
  assert.ok(!("reasoning_effort" in calls[1].body));
});

test("message lookup escapes session IDs and never relies on active-session state", async () => {
  const client = createXiClient(createHttpTransport({
    baseUrl: "http://localhost:43127",
    token: async () => "TEST_TOKEN",
    fetch: async (url, options) => {
      assert.equal(new URL(url).searchParams.get("session_id"), "session /?#");
      assert.equal(options.method, "GET");
      assert.equal(options.body, undefined);
      return Response.json([]);
    },
  }));
  assert.deepEqual(await client.getHistory("session /?#"), []);
});

test("configuration saves models without requiring a selected model", async () => {
  const client = createXiClient(createHttpTransport({
    baseUrl: "http://localhost:43127",
    token: async () => "TEST_TOKEN",
    fetch: async (_url, options) => {
      const body = JSON.parse(options.body);
      assert.deepEqual(body.models, [{ id: "model-a", reasoning_efforts: ["high"], reasoning_source: "custom" }]);
      assert.ok(!("model" in body));
      return Response.json({ id: "provider-a" });
    },
  }));
  const result = await client.createProvider({ name: "fixture", base_url: "http://localhost/v1", api_key: "TEST_ONLY", models: [{ id: "model-a", reasoning_efforts: ["high"], reasoning_source: "custom" }] });
  assert.equal(result.id, "provider-a");
});

test("HTTP and token failures reach the caller without empty success results", async () => {
  const client = createXiClient(createHttpTransport({
    baseUrl: "http://localhost:43127",
    token: async () => "INVALID_TEST_TOKEN",
    fetch: async () => Response.json({ error: "未授权请求" }, { status: 401 }),
  }));
  await assert.rejects(client.listProviders(), /未授权请求/);
  const unavailable = createXiClient(createHttpTransport({
    baseUrl: "http://localhost:43127",
    token: async () => { throw new Error("credentials unavailable"); },
    fetch: async () => { assert.fail("must not send before credentials are available"); },
  }));
  await assert.rejects(unavailable.listProviders(), /credentials unavailable/);
});

test("HTTP errors retain status when a proxy returns HTML or JSON null", async () => {
  for (const response of [new Response("<html>Bad Gateway</html>", { status: 502 }), Response.json(null, { status: 503 })]) {
    const client = createXiClient(createHttpTransport({
      baseUrl: "http://localhost:43127", token: async () => "TEST_TOKEN", fetch: async () => response,
    }));
    await assert.rejects(client.getWorkspace(), new RegExp(`HTTP ${response.status}`));
  }
});

test("invalid JSON success reports a Runtime protocol error", async () => {
  const client = createXiClient(createHttpTransport({
    baseUrl: "http://localhost:43127", token: async () => "TEST_TOKEN", fetch: async () => new Response("not json"),
  }));
  await assert.rejects(client.listProviders(), /Runtime.*JSON.*200/);
});
