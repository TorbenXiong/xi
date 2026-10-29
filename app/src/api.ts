// 应用装配入口；平台连接细节不会进入 React 组件。
import { createXiClient } from "./client/XiClient";
import { createHttpTransport } from "./client/http";
import { runtimeToken } from "./platform/runtime";

export function createAppClient() {
  return createXiClient(createHttpTransport({
    baseUrl: (import.meta.env.VITE_XI_API_BASE as string | undefined) ?? "http://127.0.0.1:43127",
    token: runtimeToken,
  }));
}
