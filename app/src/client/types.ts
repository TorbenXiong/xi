export type Message = { role: "system" | "user" | "assistant" | "tool"; content: string; status?: string; error?: string | null };
export type StoredMessage = { sequence: number; role: string; content: string; status: string; error: string | null };
export type Conversation = { id: string; group_id: string; title: string; model: string; updated_at: number };
export type Provider = {
  id: string;
  name: string;
  base_url: string;
  model: string;
  api_key_configured: boolean;
  active: boolean;
  models: string[];
};
export type ProviderInput = {
  id?: string;
  name: string;
  base_url: string;
  api_key: string;
  model?: string;
  models: string[];
};
export type ProviderModelsInput = { id?: string; base_url?: string; api_key?: string };
export type Group = { id: string; name: string; is_default: boolean };
export type Session = { id: string; group_id: string; title: string; message_count: number };
export type SessionDetail = { id: string; group_id: string; title: string; messages: Message[] };
export type Workspace = { groups: Group[]; sessions: Session[]; active_session_id: string | null };
export type CreatedSession = { created: boolean; session_id: string };
export type ChatInput = {
  content: string;
  session_id: string;
  provider_id: string;
  model: string;
  reasoning_effort?: string;
};
export type ChatOutput = { session_id: string; message: Message };
