import assert from "node:assert/strict";
import test from "node:test";
import { createHttpRuntimeConnection } from "../src/client/runtime.ts";

test("health checks authenticate by header and validate service identity", async () => {
  let calls = 0;
  const connection = createHttpRuntimeConnection({ baseUrl: "http://localhost:43127/", token: async () => "TEST_TOKEN",
    fetch: async (url, options) => {
      calls++;
      assert.equal(url, "http://localhost:43127/");
      assert.equal(options.headers["X-Xi-Token"], "TEST_TOKEN");
      assert.equal(options.cache, "no-store");
      assert.ok(options.signal instanceof AbortSignal);
      return Response.json({ service: "xi-agent-runtime", status: "ok" });
    } });
  assert.equal((await connection.status()).phase, "ready");
  await connection.retry();
  assert.equal(calls, 1);
});

test("failed health checks recover on the next probe without replaying requests", async () => {
  let healthy = false;
  const connection = createHttpRuntimeConnection({ baseUrl: "http://localhost", token: async () => "TEST_TOKEN",
    fetch: async () => { if (!healthy) throw new TypeError("connection closed");
      return Response.json({ service: "xi-agent-runtime", status: "ok" }); } });
  assert.equal((await connection.status()).phase, "failed");
  healthy = true;
  assert.equal((await connection.status()).phase, "ready");
});

test("wrong credentials, foreign services, malformed JSON and timeouts stay disconnected", async () => {
  for (const response of [Response.json({}, { status: 401 }), Response.json({}, { status: 503 }),
    Response.json({ service: "other", status: "ok" }), new Response("broken")]) {
    const connection = createHttpRuntimeConnection({ baseUrl: "http://localhost", token: async () => "TEST_TOKEN", fetch: async () => response });
    const status = await connection.status();
    assert.equal(status.phase, "failed");
    assert.ok(status.error);
    assert.ok(!status.error.includes("TEST_TOKEN"));
  }
  const connection = createHttpRuntimeConnection({ baseUrl: "http://localhost", token: async () => "TEST_TOKEN",
    fetch: async () => { throw new DOMException("timeout", "TimeoutError"); } });
  assert.match((await connection.status()).error, /超时/);
});
