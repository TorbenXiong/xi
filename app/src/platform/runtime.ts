import { invoke } from "@tauri-apps/api/core";

let tokenPromise: Promise<string> | undefined;

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
