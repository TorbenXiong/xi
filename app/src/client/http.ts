import type { XiOperation, XiTransport, SessionOperation } from "./XiClient";

const routes: Record<XiOperation | SessionOperation, { method: string; path: string }> = {
  "provider/list": { method: "GET", path: "/api/providers" },
  "provider/create": { method: "POST", path: "/api/providers" },
  "provider/update": { method: "POST", path: "/api/providers/update" },
  "provider/models": { method: "POST", path: "/api/providers/models" },
  "provider/delete": { method: "POST", path: "/api/providers/delete" },
  "workspace/read": { method: "GET", path: "/api/workspace" },
  "project/create": { method: "POST", path: "/api/projects" },
  "project/rename": { method: "POST", path: "/api/projects/rename" },
  "project/delete": { method: "POST", path: "/api/projects/delete" },
  "project/archive": { method: "POST", path: "/api/projects/archive" },
  "session/create": { method: "POST", path: "/api/sessions" },
  "session/rename": { method: "POST", path: "/api/sessions/rename" },
  "session/delete": { method: "POST", path: "/api/sessions/delete" },
  "session/archive": { method: "POST", path: "/api/sessions/archive" },
  "session/move": { method: "POST", path: "/api/sessions/move" },
  "session/search": { method: "GET", path: "/api/sessions/search" },
  "session/export": { method: "GET", path: "/api/sessions/export" },
  "session/history": { method: "GET", path: "/api/sessions/history" },
  "session/reset": { method: "POST", path: "/api/reset" },
  "chat/send": { method: "POST", path: "/api/chat" },
};

function httpError(status: number): Error {
  return new Error(`请求失败 (HTTP ${status})`);
}

export function createHttpTransport(options: {
  baseUrl: string;
  token: () => Promise<string>;
  fetch?: typeof fetch;
}): XiTransport {
  const send = options.fetch ?? globalThis.fetch.bind(globalThis);
  const base = options.baseUrl.replace(/\/$/, "");
  return {
    async request<T>(operation: XiOperation | SessionOperation, input?: object): Promise<T> {
      const route = routes[operation];
      const headers = new Headers({ "X-Xi-Token": await options.token() });
      let path = route.path;
      let body: string | undefined;
      if (route.method === "GET") {
        if (input) path += "?" + new URLSearchParams(input as Record<string, string>);
      } else {
        headers.set("Content-Type", "application/json");
        body = JSON.stringify(input ?? {});
      }
      const response = await send(base + path, { method: route.method, headers, body });
      let data: unknown;
      try { data = await response.json(); }
      catch {
        throw new Error(response.ok ? `Runtime 返回了无效的 JSON（HTTP ${response.status}）` : `请求失败（HTTP ${response.status}）`);
      }
      if (!response.ok) {
        const message = data && typeof data === "object" && "error" in data && typeof data.error === "string" ? data.error : undefined;
        throw new Error(message || `请求失败（HTTP ${response.status}）`);
      }
      return data as T;
    },
    async stream<T>(operation: XiOperation, input: object, onData: (data: unknown) => void): Promise<T> {
      const route = routes[operation];
      const headers = new Headers({ "X-Xi-Token": await options.token(), "Content-Type": "application/json" });
      const response = await send(base + route.path, {
        method: route.method,
        headers,
        body: JSON.stringify({ ...input, stream: true }),
      });
      if (!response.ok) {
        let message: string | undefined;
        try {
          const data = await response.json() as unknown;
          if (data && typeof data === "object" && "error" in data && typeof data.error === "string") message = data.error;
        } catch { /* Fall through to the HTTP status. */ }
        throw new Error(message || httpError(response.status).message);
      }
      if (!response.body) throw new Error("Runtime 未返回流式响应");
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = "";
      const sessionId = "session_id" in input && typeof input.session_id === "string" ? input.session_id : "";
      let answer = "";
      const consume = (event: string) => {
        const data = event.split(/\r?\n/)
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (!data || data === "[DONE]") return data === "[DONE]";
        let parsed: unknown;
        try { parsed = JSON.parse(data); }
        catch { throw new Error("Runtime 返回了无效的流式事件"); }
        if (parsed && typeof parsed === "object" && "error" in parsed && typeof parsed.error === "string") {
          throw new Error(parsed.error);
        }
        if (parsed && typeof parsed === "object" && "content" in parsed && typeof parsed.content === "string") {
          answer += parsed.content;
          onData(parsed);
        }
        return false;
      };
      let done = false;
      while (!done) {
        const chunk = await reader.read();
        pending += decoder.decode(chunk.value ?? new Uint8Array(), { stream: !chunk.done });
        let separator: number;
        while ((separator = pending.search(/\r?\n\r?\n/)) >= 0) {
          const event = pending.slice(0, separator);
          pending = pending.slice(separator + (pending[separator] === "\r" ? 4 : 2));
          if (consume(event)) { done = true; break; }
        }
        if (chunk.done) {
          pending += decoder.decode();
          if (pending.trim() && !done) consume(pending);
          break;
        }
      }
      if (!done) throw new Error("流式响应在完成标记前结束");
      return { session_id: sessionId, message: { role: "assistant", content: answer } } as T;
    },
  };
}
