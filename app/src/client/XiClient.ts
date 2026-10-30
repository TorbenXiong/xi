import type {
  ChatInput, ChatOutput, CreatedSession, Project, ModelDescriptor,
  Provider, ProviderInput, ProviderModelsInput, Workspace, Conversation, StoredMessage,
} from "./types";

export type XiOperation =
  | "provider/list" | "provider/create" | "provider/update" | "provider/models" | "provider/delete"
  | "workspace/read" | "project/create" | "project/rename" | "project/delete"
  | "session/create" | "session/rename" | "session/delete" | "session/reset"
  | "chat/send";

export type SessionOperation = "session/search" | "session/export" | "session/history" | "session/archive" | "session/move" | "project/archive";

export interface XiTransport {
  request<T>(operation: XiOperation | SessionOperation, input?: object): Promise<T>;
}

/** 组件只依赖业务方法；端口、Token、HTTP 和 Tauri 不属于这个接口。 */
export interface XiClient {
  listProviders(): Promise<Provider[]>;
  createProvider(input: ProviderInput): Promise<{ id: string }>;
  updateProvider(input: ProviderInput & { id: string }): Promise<{ saved: boolean }>;
  fetchModels(input: ProviderModelsInput): Promise<ModelDescriptor[]>;
  deleteProvider(id: string): Promise<{ deleted: boolean }>;
  getWorkspace(): Promise<Workspace>;
  createProject(directory: string, name?: string): Promise<Project>;
  renameProject(id: string, name: string): Promise<Project>;
  deleteProject(id: string): Promise<{ deleted: boolean }>;
  archiveProject(id: string): Promise<{ archived: boolean }>;
  createSession(projectId: string | null, title?: string): Promise<CreatedSession>;
  renameSession(id: string, name: string): Promise<{ renamed: boolean }>;
  deleteSession(id: string): Promise<{ deleted: boolean }>;
  archiveSession(id: string): Promise<{ archived: boolean }>;
  moveSession(id: string, projectId: string | null): Promise<{ moved: boolean }>;
  searchSessions(query: string): Promise<Conversation[]>;
  exportSession(sessionId: string): Promise<{ content: string }>;
  getHistory(sessionId: string): Promise<StoredMessage[]>;
  resetSession(sessionId: string): Promise<{ reset: boolean }>;
  sendMessage(input: ChatInput): Promise<ChatOutput>;
}

export function createXiClient(transport: XiTransport): XiClient {
  return {
    listProviders: () => transport.request("provider/list"),
    createProvider: (input) => transport.request("provider/create", input),
    updateProvider: (input) => transport.request("provider/update", input),
    fetchModels: (input) => transport.request("provider/models", input),
    deleteProvider: (id) => transport.request("provider/delete", { id }),
    getWorkspace: () => transport.request("workspace/read"),
    createProject: (directory, name = "") => transport.request("project/create", { directory, name }),
    renameProject: (id, name) => transport.request("project/rename", { id, name }),
    deleteProject: (id) => transport.request("project/delete", { id }),
    archiveProject: (id) => transport.request("project/archive", { id }),
    createSession: (project_id, title = "新会话") => transport.request("session/create", { project_id, title }),
    renameSession: (id, name) => transport.request("session/rename", { id, name }),
    deleteSession: (id) => transport.request("session/delete", { id }),
    archiveSession: (id) => transport.request("session/archive", { id }),
    moveSession: (id, project_id) => transport.request("session/move", { id, project_id }),
    searchSessions: (query) => transport.request("session/search", { query }),
    exportSession: (session_id) => transport.request("session/export", { session_id }),
    getHistory: (session_id) => transport.request("session/history", { session_id }),
    resetSession: (session_id) => transport.request("session/reset", { session_id }),
    sendMessage: (input) => transport.request("chat/send", input),
  };
}
