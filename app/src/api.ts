// 应用装配入口；平台连接细节不会进入 React 组件。
import { createXiClient } from "./client/XiClient";
import { createHttpTransport } from "./client/http";
import { desktopRuntimeConnection, runtimeToken } from "./platform/runtime";
import { createHttpRuntimeConnection } from "./client/runtime";

const baseUrl = (import.meta.env.VITE_XI_API_BASE as string | undefined) ?? "http://127.0.0.1:43127";

export function createAppClient() {
  return createXiClient(createHttpTransport({
    baseUrl,
    token: runtimeToken,
  }));
}

export function createAppRuntimeConnection() {
  // 显式连接其他地址时，只检查该服务，不操作本机子进程。
  return (!import.meta.env.VITE_XI_API_BASE && desktopRuntimeConnection())
    || createHttpRuntimeConnection({ baseUrl, token: runtimeToken });
}
