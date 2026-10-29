import type {
  ChatInput, ChatOutput, CreatedSession, Group, Message,
  Provider, ProviderInput, ProviderModelsInput, SessionDetail, Workspace, Conversation, StoredMessage,
} from "./types";

export type XiOperation =
  | "provider/list" | "provider/create" | "provider/update" | "provider/models" | "provider/delete"
  | "workspace/read" | "group/create" | "group/rename" | "group/delete"
  | "session/create" | "session/messages" | "session/rename" | "session/delete" | "session/reset"
  | "chat/send";

export type SessionOperation = "session/search" | "session/export" | "session/history" | "session/archive" | "session/move" | "group/archive";

export interface XiTransport {
  request<T>(operation: XiOperation | SessionOperation, input?: object): Promise<T>;
}

/** 组件只依赖业务方法；端口、Token、HTTP 和 Tauri 不属于这个接口。 */
export interface XiClient {
  listProviders(): Promise<Provider[]>;
  createProvider(input: ProviderInput): Promise<{ id: string }>;
  updateProvider(input: ProviderInput & { id: string }): Promise<{ saved: boolean }>;
  fetchModels(input: ProviderModelsInput): Promise<string[]>;
  deleteProvider(id: string): Promise<{ deleted: boolean }>;
  getWorkspace(): Promise<Workspace>;
  createGroup(name: string): Promise<Group>;
  renameGroup(id: string, name: string): Promise<Group>;
  deleteGroup(id: string): Promise<{ deleted: boolean }>;
  archiveGroup(id: string): Promise<{ archived: boolean }>;
  createSession(groupId: string, title?: string): Promise<CreatedSession>;
  getMessages(sessionId: string): Promise<Message[]>;
  renameSession(id: string, name: string): Promise<SessionDetail>;
  deleteSession(id: string): Promise<{ deleted: boolean }>;
  archiveSession(id: string): Promise<{ archived: boolean }>;
  moveSession(id: string, groupId: string): Promise<{ moved: boolean }>;
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
    createGroup: (name) => transport.request("group/create", { name }),
    renameGroup: (id, name) => transport.request("group/rename", { id, name }),
    deleteGroup: (id) => transport.request("group/delete", { id }),
    archiveGroup: (id) => transport.request("group/archive", { id }),
    createSession: (group_id, title = "新会话") => transport.request("session/create", { group_id, title }),
    getMessages: (session_id) => transport.request("session/messages", { session_id }),
    renameSession: (id, name) => transport.request("session/rename", { id, name }),
    deleteSession: (id) => transport.request("session/delete", { id }),
    archiveSession: (id) => transport.request("session/archive", { id }),
    moveSession: (id, group_id) => transport.request("session/move", { id, group_id }),
    searchSessions: (query) => transport.request("session/search", { query }),
    exportSession: (session_id) => transport.request("session/export", { session_id }),
    getHistory: (session_id) => transport.request("session/history", { session_id }),
    resetSession: (session_id) => transport.request("session/reset", { session_id }),
    sendMessage: (input) => transport.request("chat/send", input),
  };
}
