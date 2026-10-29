import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import type { XiClient } from "./client/XiClient";
import type { Group, Message, Provider, ProviderInput, Session, Workspace } from "./client/types";
import { Dialog } from "./components/Dialog";
import { Icon } from "./components/Icon";

const emptyProvider: ProviderInput = { name: "", base_url: "", api_key: "", model: "", models: [] };
type Selection = { sessionId: string | null; providerId: string | null };
type WorkspaceAction =
  | { kind: "createGroup" }
  | { kind: "renameGroup" | "deleteGroup" | "archiveGroup"; group: Group }
  | { kind: "renameSession" | "deleteSession" | "archiveSession" | "moveSession"; session: Session }
  | { kind: "deleteProvider"; provider: Provider };

function providerNameFromUrl(baseUrl: string): string {
  try {
    const labels = new URL(baseUrl).hostname.split(".").filter(Boolean);
    return labels.length >= 2 ? labels[labels.length - 2] : labels[0] ?? "";
  } catch { return ""; }
}

function WorkspaceDialog({ action, client, groups, onClose, onSaved }: {
  action: WorkspaceAction; client: XiClient; groups: Group[]; onClose: () => void; onSaved: () => Promise<void>;
}) {
  const deleting = action.kind.startsWith("delete");
  const archiving = action.kind.startsWith("archive");
  const confirming = deleting || archiving;
  const name = "group" in action ? action.group.name : "session" in action ? action.session.title
    : "provider" in action ? action.provider.name : "";
  const [value, setValue] = useState(action.kind === "moveSession" ? action.session.group_id : name);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const titles = { createGroup: "新建分组", renameGroup: "重命名分组", deleteGroup: "删除分组",
    renameSession: "重命名会话", deleteSession: "删除会话", deleteProvider: "删除模型配置",
    archiveGroup: "归档分组", archiveSession: "归档会话", moveSession: "移动会话" };

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending) return;
    setPending(true); setError("");
    try {
      switch (action.kind) {
        case "createGroup": await client.createGroup(value.trim()); break;
        case "renameGroup": await client.renameGroup(action.group.id, value.trim()); break;
        case "deleteGroup": await client.deleteGroup(action.group.id); break;
        case "renameSession": await client.renameSession(action.session.id, value.trim()); break;
        case "deleteSession": await client.deleteSession(action.session.id); break;
        case "deleteProvider": await client.deleteProvider(action.provider.id); break;
        case "archiveGroup": await client.archiveGroup(action.group.id); break;
        case "archiveSession": await client.archiveSession(action.session.id); break;
        case "moveSession": await client.moveSession(action.session.id, value); break;
      }
    } catch (cause) {
      setError((cause as Error).message); setPending(false); return;
    }
    await onSaved();
    onClose();
  }

  return <Dialog title={titles[action.kind]} onClose={onClose} busy={pending}>
    <form onSubmit={submit}>
      <div className="modal-body">
        {confirming ? <p className="delete-description">确定{archiving ? "归档" : "删除"}「{name}」？
          {archiving ? "归档后不在工作区显示，历史数据仍保留。" : action.kind === "deleteGroup" ? "会话和消息将保留，并移动到未分组。"
            : action.kind === "deleteProvider" ? "已有会话和消息会保留。" : "此操作无法撤销。"}
        </p> : action.kind === "moveSession" ? <label>目标分组<select autoFocus value={value} disabled={pending}
          onChange={(event) => setValue(event.target.value)}>{groups.map((group) => <option key={group.id} value={group.id}>{group.name}</option>)}</select></label>
          : <label>名称<input autoFocus required maxLength={120} value={value}
          placeholder="输入名称" onChange={(event) => setValue(event.target.value)} disabled={pending} /></label>}
        {error && <div className="inline-error" role="alert">{error}</div>}
      </div>
      <div className="modal-actions">
        <button type="button" className="secondary-button" disabled={pending} onClick={onClose}>取消</button>
        <button className={deleting ? "danger-button" : "primary-button"} disabled={pending || (!confirming && !value.trim())}>
          {pending ? "处理中…" : deleting ? "确认删除" : archiving ? "确认归档" : "保存"}
        </button>
      </div>
    </form>
  </Dialog>;
}

export default function App({ client }: { client: XiClient }) {
  const [workspace, setWorkspace] = useState<Workspace>({ groups: [], sessions: [], active_session_id: null });
  const [providers, setProviders] = useState<Provider[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [sendingTo, setSendingTo] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [providerOpen, setProviderOpen] = useState(false);
  const [providerForm, setProviderForm] = useState<ProviderInput>(emptyProvider);
  const [providerError, setProviderError] = useState("");
  const [loadingModels, setLoadingModels] = useState(false);
  const [savingProvider, setSavingProvider] = useState(false);
  const [selectedModel, setSelectedModel] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState("auto");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [providerId, setProviderId] = useState<string | null>(null);
  const [draftGroupId, setDraftGroupId] = useState<string | null>(null);
  const [action, setAction] = useState<WorkspaceAction | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [search, setSearch] = useState("");
  const [matches, setMatches] = useState<Set<string> | null>(null);
  const selection = useRef<Selection | null>(null);
  const formVersion = useRef(0);
  const modelRequest = useRef(0);
  const messagesRequest = useRef(0);
  const submitLock = useRef(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const messageEnd = useRef<HTMLDivElement>(null);

  const activeSession = workspace.sessions.find((item) => item.id === sessionId);
  const activeProvider = providers.find((provider) => provider.id === providerId);
  const availableModels = activeProvider?.models.length ? activeProvider.models : (activeProvider?.model ? [activeProvider.model] : []);
  const providerPending = loadingModels || savingProvider;
  const waitingHere = busy && sendingTo === sessionId;

  function select(next: Selection) {
    selection.current = next;
    setSessionId(next.sessionId);
    setProviderId(next.providerId);
  }

  async function loadMessages(id: string): Promise<Message[]> {
    return (await client.getHistory(id)).filter((message) => message.role !== "assistant" || message.content || message.status === "complete")
      .map((message) => ({ ...message, role: message.role as Message["role"] }));
  }

  async function showMessages(id: string | null, showLoading = false) {
    const request = ++messagesRequest.current;
    if (showLoading) setLoadingMessages(true);
    try {
      const next = id ? await loadMessages(id) : [];
      if (messagesRequest.current === request && selection.current?.sessionId === id) setMessages(next);
    } catch (cause) {
      if (messagesRequest.current === request && selection.current?.sessionId === id) setError((cause as Error).message);
    } finally { if (messagesRequest.current === request) setLoadingMessages(false); }
  }

  async function refresh() {
    const previous = selection.current;
    const [nextWorkspace, nextProviders] = await Promise.all([client.getWorkspace(), client.listProviders()]);
    if (selection.current !== previous) return;
    const requestedSession = previous ? previous.sessionId : nextWorkspace.active_session_id;
    const requestedProvider = previous?.providerId;
    const next: Selection = {
      sessionId: requestedSession && !nextWorkspace.sessions.some((item) => item.id === requestedSession)
        ? nextWorkspace.sessions[0]?.id ?? null : requestedSession,
      providerId: nextProviders.some((item) => item.id === requestedProvider)
        ? requestedProvider! : nextProviders.find((item) => item.active)?.id ?? nextProviders[0]?.id ?? null,
    };
    select(next);
    setWorkspace(nextWorkspace); setProviders(nextProviders);
    const provider = nextProviders.find((item) => item.id === next.providerId);
    setSelectedModel((current) => current && (provider?.models.includes(current) || provider?.model === current)
      ? current : provider?.models[0] || provider?.model || "");
    await showMessages(next.sessionId);
  }

  async function refreshSafely() {
    try { await refresh(); } catch (cause) { setError((cause as Error).message); }
  }

  useEffect(() => {
    let cancelled = false;
    async function initialize() {
      for (let attempt = 0; attempt < 8; attempt += 1) {
        if (cancelled) return;
        try { await refresh(); return; }
        catch (cause) {
          if (attempt === 7) { if (!cancelled) setError((cause as Error).message); return; }
          await new Promise((resolve) => setTimeout(resolve, 250));
        }
      }
    }
    initialize();
    return () => { cancelled = true; };
  }, [client]);

  useEffect(() => {
    messageEnd.current?.scrollIntoView({ block: "end" });
  }, [messages, waitingHere]);

  useEffect(() => {
    const element = textarea.current;
    if (element) { element.style.height = "auto"; element.style.height = Math.min(element.scrollHeight, 240) + "px"; }
  }, [input]);

  useEffect(() => {
    let cancelled = false;
    if (!search.trim()) { setMatches(null); return; }
    const timer = setTimeout(() => {
      client.searchSessions(search.trim()).then((sessions) => {
        if (!cancelled) setMatches(new Set(sessions.map((session) => session.id)));
      }).catch((cause: Error) => { if (!cancelled) setError(cause.message); });
    }, 200);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [search, client, workspace]);

  async function exportConversation(session: Session) {
    try {
      const { content } = await client.exportSession(session.id);
      const url = URL.createObjectURL(new Blob([content], { type: "text/markdown;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url; link.download = session.title.replace(/[<>:"/\\|?*\x00-\x1F]/g, "_") + ".md";
      document.body.append(link); link.click(); link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch (cause) { setError((cause as Error).message); }
  }

  function openProvider(provider?: Provider) {
    formVersion.current += 1;
    setProviderError("");
    setProviderForm(provider ? { id: provider.id, name: provider.name, base_url: provider.base_url,
      model: provider.model, models: provider.models, api_key: "" } : { ...emptyProvider });
    setProviderOpen(true);
  }

  function closeProvider() {
    formVersion.current += 1;
    setProviderOpen(false);
  }

  function changeProvider(patch: Partial<ProviderInput>) {
    formVersion.current += 1;
    setProviderError("");
    setProviderForm((current) => ({ ...current, ...patch }));
  }

  function fillProviderName() {
    setProviderForm((current) => current.name.trim() ? current : { ...current, name: providerNameFromUrl(current.base_url) });
  }

  async function send(event: FormEvent) {
    event.preventDefault();
    const content = input.trim();
    if (!content || submitLock.current || loadingMessages) return;
    if (!providerId || !selectedModel) { setError("请先配置模型服务，并在输入框下方选择模型。"); return; }
    const previous = selection.current;
    let targetSession = previous?.sessionId ?? null;
    submitLock.current = true;
    setBusy(true); setSendingTo(targetSession); setError(""); setInput("");
    setMessages((current) => [...current, { role: "user", content }]);
    try {
      if (!targetSession) {
        const group = workspace.groups.find((item) => item.id === draftGroupId) ?? workspace.groups.find((item) => item.is_default) ?? workspace.groups[0];
        if (!group) throw new Error("没有可用分组，请稍后重试。");
        const created = await client.createSession(group.id);
        targetSession = created.session_id;
        setSendingTo(targetSession);
        if (selection.current === previous) select({ sessionId: targetSession, providerId });
      }
      await client.sendMessage({ content, session_id: targetSession, provider_id: providerId, model: selectedModel,
        reasoning_effort: reasoningEffort === "auto" ? undefined : reasoningEffort });
    } catch (cause) { setError((cause as Error).message); }
    finally {
      await refreshSafely();
      submitLock.current = false; setBusy(false); setSendingTo(null);
      textarea.current?.focus();
    }
  }

  async function fetchModels() {
    if (providerPending) return;
    fillProviderName();
    const version = formVersion.current;
    const request = ++modelRequest.current;
    setLoadingModels(true); setProviderError("");
    try {
      const models = await client.fetchModels({
        id: providerForm.id, base_url: providerForm.base_url, api_key: providerForm.api_key,
      });
      if (formVersion.current === version) {
        setProviderForm((current) => ({ ...current, models }));
        if (!models.length) setProviderError("服务未返回可用模型，请检查 API 地址和密钥。");
      }
    } catch (cause) {
      if (formVersion.current === version) setProviderError((cause as Error).message);
    } finally { if (modelRequest.current === request) setLoadingModels(false); }
  }

  async function saveProvider(event: FormEvent) {
    event.preventDefault();
    if (providerPending) return;
    setSavingProvider(true); setProviderError("");
    const form = { ...providerForm, name: providerForm.name.trim() || providerNameFromUrl(providerForm.base_url) };
    try {
      if (form.id) await client.updateProvider({ ...form, id: form.id });
      else {
        const created = await client.createProvider(form);
        select({ sessionId: selection.current?.sessionId ?? null, providerId: created.id });
      }
    } catch (cause) { setProviderError((cause as Error).message); setSavingProvider(false); return; }
    closeProvider(); setSavingProvider(false);
    await refreshSafely();
  }

  function selectProvider(id: string) {
    select({ sessionId: selection.current?.sessionId ?? null, providerId: id });
    const provider = providers.find((item) => item.id === id);
    setSelectedModel(provider?.models[0] || provider?.model || "");
  }

  function newConversation(groupId?: string) {
    if (busy) return;
    select({ sessionId: null, providerId });
    messagesRequest.current += 1;
    setDraftGroupId(groupId ?? workspace.groups.find((group) => group.is_default)?.id ?? null);
    setMessages([]); setInput(""); setError(""); setLoadingMessages(false); setSidebarOpen(false);
    textarea.current?.focus();
  }

  async function selectSession(id: string) {
    const next = { sessionId: id, providerId: selection.current?.providerId ?? null };
    select(next); setMessages([]); setError(""); setSidebarOpen(false);
    await showMessages(id, true);
  }

  const draftGroup = workspace.groups.find((group) => group.id === draftGroupId);

  return (
    <div className={"shell" + (sidebarOpen ? " sidebar-visible" : "")}>
      {sidebarOpen && <button className="sidebar-scrim" aria-label="收起侧栏" onClick={() => setSidebarOpen(false)} />}
      <aside className="sidebar" aria-label="工作区导航">
        <div className="brand"><img className="brand-mark" src="/xi-icon.png" alt="" /><div>xi<span className="brand-caption">Agent 工作台</span></div>
          <button className="icon-button sidebar-close" aria-label="收起侧栏" onClick={() => setSidebarOpen(false)}>×</button>
        </div>
        <button className="new-chat-button" disabled={busy} onClick={() => newConversation()}><Icon name="plus" />新会话</button>
        <div className="section-heading"><span>工作区</span><button className="icon-button" aria-label="新建分组" title="新建分组"
          onClick={() => setAction({ kind: "createGroup" })}><Icon name="plus" /></button></div>
        <input className="workspace-search" type="search" aria-label="搜索会话和消息" placeholder="搜索会话和消息…" value={search} onChange={(event) => setSearch(event.target.value)} />
        <nav className="tree" aria-label="分组和会话">
          {workspace.groups.map((group) => {
            const sessions = workspace.sessions.filter((session) => session.group_id === group.id && (!matches || matches.has(session.id)));
            if (matches && !sessions.length) return null;
            return <section className="group" key={group.id}>
              <div className="group-heading">
                <span className="group-title"><Icon name="folder" /><span title={group.name}>{group.name}</span><small>{sessions.length}</small></span>
                <div className="row-actions">
                  <button className="icon-button" title={"在「" + group.name + "」中新建会话"} aria-label={"在「" + group.name + "」中新建会话"}
                    disabled={busy} onClick={() => newConversation(group.id)}><Icon name="plus" /></button>
                  {!group.is_default && <>
                    <button className="icon-button" title="重命名分组" aria-label={"重命名分组「" + group.name + "」"}
                      onClick={() => setAction({ kind: "renameGroup", group })}><Icon name="edit" /></button>
                    <button className="icon-button danger-icon" title="删除分组" aria-label={"删除分组「" + group.name + "」"}
                      disabled={busy} onClick={() => setAction({ kind: "deleteGroup", group })}><Icon name="delete" /></button>
                    <button className="icon-button" title="归档分组" aria-label={"归档分组「" + group.name + "」"}
                      disabled={busy} onClick={() => setAction({ kind: "archiveGroup", group })}>↓</button>
                  </>}
                </div>
              </div>
              {!sessions.length && <p className="group-empty">暂无会话</p>}
              {sessions.map((session) => <div className={"session-row" + (session.id === sessionId ? " active" : "")} key={session.id}>
                <button className="session" onClick={() => selectSession(session.id)} title={session.title}
                  aria-current={session.id === sessionId ? "page" : undefined}>
                  <Icon name="chat" /><span>{session.title}</span>
                </button>
                <div className="row-actions">
                  <button className="icon-button" title="重命名会话" aria-label={"重命名会话「" + session.title + "」"}
                    onClick={() => setAction({ kind: "renameSession", session })}><Icon name="edit" /></button>
                  <button className="icon-button danger-icon" title="删除会话" aria-label={"删除会话「" + session.title + "」"}
                    disabled={busy} onClick={() => setAction({ kind: "deleteSession", session })}><Icon name="delete" /></button>
                </div>
              </div>)}
            </section>;
          })}
          {matches?.size === 0 && <p className="group-empty">没有匹配的会话</p>}
        </nav>
        <div className="sidebar-footer">
          <span className="field-caption">模型服务</span>
          <select aria-label="模型服务" value={activeProvider?.id ?? ""} onChange={(event) => selectProvider(event.target.value)}>
            {!providers.length && <option value="">尚未配置模型服务</option>}
            {providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
          </select>
          <button className="settings-button" onClick={() => openProvider(activeProvider)}><Icon name="settings" />模型配置<span>管理服务与密钥</span></button>
        </div>
      </aside>
      <main className="main">
        <button className="sidebar-toggle icon-button" aria-label="展开侧栏" onClick={() => setSidebarOpen(true)}><Icon name="menu" /></button>
        <section className="messages" aria-label="对话消息" aria-busy={loadingMessages}>
          {loadingMessages ? <div className="empty"><span className="spinner" />正在加载会话…</div> : messages.length === 0 ? (
            <div className="empty">
              <img className="empty-mark" src="/xi-icon.png" alt="" />
              <h1>{activeSession?.title ?? "从一个想法开始"}</h1>
              <p>{providers.length ? "提个问题，或描述你想完成的事。" : "先连接模型服务，即可开始你的第一段对话。"}</p>
              {!providers.length ? <button className="primary-button" onClick={() => openProvider()}><Icon name="plus" />配置模型服务</button>
                : <span className="empty-hint">{draftGroup && !sessionId ? "新对话将保存到「" + draftGroup.name + "」" : "模型和推理强度可在下方随时调整"}</span>}
            </div>
          ) : messages.map((message, index) => (
            <article className={"message " + message.role} key={index + "-" + message.role}>
              <span className="avatar">{message.role === "user" ? "你" : "ξ"}</span>
              <div className="message-content">{message.content}
                {(message.status === "interrupted" || message.status === "failed") && <p className="message-warning">回复{message.status === "interrupted" ? "中断" : "失败"}：{message.error ?? "请重试"}</p>}
              </div>
            </article>
          ))}
          {waitingHere && <div className="reply-status" role="status"><span className="spinner" />正在等待模型回复…</div>}
          <div ref={messageEnd} />
        </section>
        <div className="composer-area">
          {error && <div className="inline-error chat-error" role="alert"><span>{error}</span>
            <button className="icon-button" aria-label="关闭错误提示" onClick={() => setError("")}>×</button></div>}
          <form className="composer" onSubmit={send}>
            <textarea ref={textarea} aria-label="消息内容" value={input} onChange={(event) => setInput(event.target.value)}
              placeholder={providers.length ? "输入消息，与 xi 一起探索…" : "配置模型服务后，在这里开始对话…"}
              disabled={busy || loadingMessages} rows={3}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229) {
                  event.preventDefault();
                  if (!event.repeat) event.currentTarget.form?.requestSubmit();
                }
              }} />
            <div className="composer-toolbar">
              {activeSession && <details className="session-tools"><summary>会话操作</summary><div>
                <button type="button" disabled={busy} onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); setAction({kind:"moveSession",session:activeSession}); }}>移动到分组</button>
                <button type="button" disabled={busy} onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); setAction({kind:"archiveSession",session:activeSession}); }}>归档会话</button>
                <button type="button" onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); exportConversation(activeSession); }}>导出 Markdown</button>
              </div></details>}
              <div className="chat-controls">
                <label><span>模型</span><select value={selectedModel} onChange={(event) => setSelectedModel(event.target.value)} aria-label="选择模型">
                  {availableModels.length ? availableModels.map((model) => <option key={model} value={model}>{model}</option>)
                    : <option value="">请先获取模型</option>}
                </select></label>
                <label><span>推理</span><select value={reasoningEffort} onChange={(event) => setReasoningEffort(event.target.value)} aria-label="推理强度">
                  <option value="auto">自动</option><option value="low">低</option><option value="medium">中</option><option value="high">高</option>
                </select></label>
              </div>
              <button type="submit" className="send-button" aria-label={busy ? "正在生成回复" : "发送消息"} title="发送消息（Enter）"
                disabled={busy || loadingMessages || !input.trim() || !selectedModel || !providerId}>
                {busy ? <span className="spinner" /> : <Icon name="send" />}
              </button>
            </div>
          </form>
          <p className="composer-hint">Enter 发送 <span>·</span> Shift + Enter 换行</p>
        </div>
      </main>
      {providerOpen && <Dialog title={providerForm.id ? "模型配置" : "添加模型服务"}
        description="连接服务、管理密钥；具体模型在对话时选择。" busy={providerPending} onClose={closeProvider}>
        <form onSubmit={saveProvider}>
          <fieldset className="modal-body provider-fields" disabled={providerPending}>
            <label>名称 <span className="optional">可选</span><input autoFocus value={providerForm.name}
              placeholder="留空时根据 API 地址生成" onChange={(event) => changeProvider({ name: event.target.value })} /></label>
            <label>API 地址<input required type="url" value={providerForm.base_url} placeholder="https://api.example.com/v1"
              onChange={(event) => changeProvider({ base_url: event.target.value, models: [] })} onBlur={fillProviderName} /></label>
            <label>API Key<input required={!providerForm.id} type="password" autoComplete="off" value={providerForm.api_key}
              placeholder={providerForm.id ? "留空使用已保存的密钥" : "输入服务商提供的 API Key"}
              onChange={(event) => changeProvider({ api_key: event.target.value, models: [] })} /></label>
            <div className="model-picker">
              <div className="model-summary">{providerForm.models.length ? providerForm.models.length + " 个模型已加载" : "尚未获取模型"}
                <small>保存后在对话输入区切换模型</small></div>
              <button type="button" className="secondary-button" disabled={providerPending} onClick={fetchModels}>
                {loadingModels && <span className="spinner" />}{loadingModels ? "获取中…" : "获取模型"}
              </button>
            </div>
            {providerForm.models.length > 0 && <div className="model-results" aria-label="可用模型列表">
              {providerForm.models.map((model) => <span className="model-chip" key={model}>{model}</span>)}
            </div>}
            {providerError && <div className="inline-error" role="alert">{providerError}</div>}
            {providers.length > 0 && <div className="provider-list"><div className="provider-list-heading"><span>已保存的服务</span>
              <button type="button" className="text-button" onClick={() => openProvider()}>＋ 添加</button></div>
              {providers.map((provider) => <div className="provider-row" key={provider.id}>
                <div className="provider-info"><strong>{provider.name}</strong><span title={provider.base_url}>{provider.base_url}</span></div>
                <button type="button" className="text-button" onClick={() => openProvider(provider)}>编辑</button>
                <button type="button" className="text-button danger-text" disabled={providers.length <= 1}
                  title={providers.length <= 1 ? "至少保留一个模型服务" : "删除模型配置"}
                  onClick={() => setAction({ kind: "deleteProvider", provider })}>删除</button>
              </div>)}
            </div>}
          </fieldset>
          <div className="modal-actions"><button type="button" className="secondary-button" disabled={providerPending} onClick={closeProvider}>取消</button>
            <button className="primary-button" disabled={providerPending}>{savingProvider && <span className="spinner" />}{savingProvider ? "保存中…" : "保存配置"}</button></div>
        </form>
      </Dialog>}
      {action && <WorkspaceDialog action={action} client={client} groups={workspace.groups} onClose={() => setAction(null)} onSaved={async () => {
        if (action.kind === "deleteProvider" && providerForm.id === action.provider.id) openProvider();
        await refreshSafely();
      }} />}
    </div>
  );
}
