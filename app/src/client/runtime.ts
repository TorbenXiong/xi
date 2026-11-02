export type RuntimeStatus = {
  phase: "starting" | "ready" | "recovering" | "failed" | "stopped";
  generation: number;
  error?: string;
};

export interface RuntimeConnection {
  status(): Promise<RuntimeStatus>;
  retry(): Promise<void>;
}

/** Web 客户端只探测连接；进程恢复由桌面壳负责。 */
export function createHttpRuntimeConnection(options: {
  baseUrl: string;
  token: () => Promise<string>;
  fetch?: typeof fetch;
}): RuntimeConnection {
  const send = options.fetch ?? globalThis.fetch.bind(globalThis);
  return {
    async status() {
      try {
        const token = await options.token();
        const response = await send(options.baseUrl.replace(/\/$/, "") + "/", {
          headers: { "X-Xi-Token": token }, signal: AbortSignal.timeout(2000), cache: "no-store",
        });
        if (response.status === 401) throw new Error("Runtime 凭据不匹配，请检查连接配置");
        if (!response.ok) throw new Error(`Runtime 健康检查失败（HTTP ${response.status}）`);
        const data = await response.json() as Record<string, unknown>;
        if (data?.service !== "xi-agent-runtime" || data.status !== "ok") throw new Error("Runtime 健康响应无效");
        return { phase: "ready", generation: 0 };
      } catch (cause) {
        const error = cause as Error;
        return { phase: "failed", generation: 0,
          error: error.name === "TimeoutError" ? "Runtime 健康检查超时" : error.message };
      }
    },
    async retry() { /* 下一次轮询立即重新检查连接，不重放业务请求。 */ },
  };
}
