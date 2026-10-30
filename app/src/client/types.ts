export type Message = { role: "system" | "user" | "assistant" | "tool"; content: string; status?: string; error?: string | null };
export type StoredMessage = { sequence: number; role: string; content: string; status: string; error: string | null };
export type Conversation = { id: string; project_id: string | null; title: string; model: string; updated_at: number };
export type ModelDescriptor = { id: string; reasoning_efforts: string[]; reasoning_source: "builtin" | "provider" | "custom" | "unknown" };
export type Provider = {
  id: string; name: string; base_url: string; api_key_configured: boolean; active: boolean; models: ModelDescriptor[];
};
export type ProviderInput = { id?: string; name: string; base_url: string; api_key: string; models: ModelDescriptor[] };
export type ProviderModelsInput = { id?: string; base_url?: string; api_key?: string };
export type Project = { id: string; name: string; directory: string };
export type Session = { id: string; project_id: string | null; title: string; message_count: number };
export type Workspace = { projects: Project[]; sessions: Session[]; active_session_id: string | null };
export type CreatedSession = { session_id: string };
export type ChatInput = { content: string; session_id: string; provider_id: string; model: string; reasoning_effort?: string };
export type ChatOutput = { session_id: string; message: Message };
