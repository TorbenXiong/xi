import { invoke, isTauri } from "@tauri-apps/api/core";
import type { RuntimeConnection, RuntimeStatus } from "../client/runtime";

let tokenPromise: Promise<string> | undefined;

export function desktopRuntimeConnection(): RuntimeConnection | undefined {
  if (!isTauri()) return undefined;
  return {
    async status() {
      const [phase, , generation, error] = await invoke<[RuntimeStatus["phase"], number | null, number, string | null]>("runtime_status");
      return { phase, generation, error: error ?? undefined };
    },
    retry: () => invoke<void>("retry_runtime"),
  };
}

export function canChooseProjectDirectory(): boolean { return isTauri(); }

export async function chooseProjectDirectory(): Promise<string | null> {
  if (!isTauri()) throw new Error("浏览器中请手动输入项目目录");
  return invoke<string | null>("choose_project_directory");
}

export function runtimeToken(): Promise<string> {
  tokenPromise ??= (async () => {
    const queryToken = new URLSearchParams(window.location.search).get("token");
    if (queryToken) return queryToken;
    try {
      return await invoke<string>("runtime_token");
    } catch {
      throw new Error("无法获取 Runtime 凭据，请从桌面应用启动，或在前端 URL 中提供 token");
    }
  })().catch((error: unknown) => {
    tokenPromise = undefined;
    throw error;
  });
  return tokenPromise;
}
