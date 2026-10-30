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
  };
}
