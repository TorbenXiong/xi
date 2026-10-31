import { useEffect, useRef, useState } from "react";
import type { CSSProperties, FormEvent } from "react";
import type { XiClient } from "./client/XiClient";
import type { Project, Message, Provider, ProviderInput, Session, Workspace } from "./client/types";
import { Dialog } from "./components/Dialog";
import { Icon } from "./components/Icon";
import { ProjectPicker } from "./components/ProjectPicker";
import { canChooseProjectDirectory, chooseProjectDirectory } from "./platform/runtime";
import { mergeFetchedModels, modelReasoningEffort, reasoningLabel } from "./client/models";
import { SettingsPage } from "./components/SettingsPage";
import { MessageContent } from "./components/MessageContent";
import type { SettingsSection } from "./components/SettingsPage";
import { isSendKey, loadPreferences, preferencesKey } from "./preferences";
import type { Preferences } from "./preferences";

const emptyProvider: ProviderInput = { name: "", base_url: "", api_key: "", models: [] };
type Selection = { sessionId: string | null; providerId: string | null };
type WorkspaceAction =
  | { kind: "createProject" }
  | { kind: "renameProject" | "deleteProject" | "archiveProject"; project: Project }
  | { kind: "renameSession" | "deleteSession" | "archiveSession" | "moveSession"; session: Session }
  | { kind: "deleteProvider"; provider: Provider };

function providerNameFromUrl(baseUrl: string): string {
  try {
    const labels = new URL(baseUrl).hostname.split(".").filter(Boolean);
    return labels.length >= 2 ? labels[labels.length - 2] : labels[0] ?? "";
  } catch { return ""; }
}

function WorkspaceDialog({ action, client, projects, onClose, onSaved }: {
  action: WorkspaceAction; client: XiClient; projects: Project[]; onClose: () => void; onSaved: (project?: Project) => Promise<void>;
}) {
  const deleting = action.kind.startsWith("delete");
  const archiving = action.kind.startsWith("archive");
  const confirming = deleting || archiving;
  const name = "project" in action ? action.project.name : "session" in action ? action.session.title : "provider" in action ? action.provider.name : "";
  const [value, setValue] = useState(action.kind === "moveSession" ? action.session.project_id ?? "" : name);
  const [directory, setDirectory] = useState("");
  const [pending, setPending] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [manualDirectory, setManualDirectory] = useState(!canChooseProjectDirectory());
  const [error, setError] = useState("");
  const titles = { createProject: "创建项目", renameProject: "重命名项目", deleteProject: "移除项目",
    renameSession: "重命名会话", deleteSession: "删除会话", deleteProvider: "删除模型配置",
    archiveProject: "归档项目", archiveSession: "归档会话", moveSession: "移动会话" };
  async function browse() {
    setChoosing(true); setError("");
    try {
      const path = await chooseProjectDirectory();
      if (path) {
        setDirectory(path);
        setValue((current) => current.trim() ? current : path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "");
      }
    }
    catch (cause) { setError((cause as Error).message); }
    finally { setChoosing(false); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending || choosing) return;
    setPending(true); setError("");
    let created: Project | undefined;
    try {
      switch (action.kind) {
        case "createProject": created = await client.createProject(directory.trim(), value.trim()); break;
        case "renameProject": await client.renameProject(action.project.id, value.trim()); break;
        case "deleteProject": await client.deleteProject(action.project.id); break;
        case "renameSession": await client.renameSession(action.session.id, value.trim()); break;
        case "deleteSession": await client.deleteSession(action.session.id); break;
        case "deleteProvider": await client.deleteProvider(action.provider.id); break;
        case "archiveProject": await client.archiveProject(action.project.id); break;
        case "archiveSession": await client.archiveSession(action.session.id); break;
        case "moveSession": await client.moveSession(action.session.id, value || null); break;
      }
    } catch (cause) { setError((cause as Error).message); setPending(false); return; }
    await onSaved(created); onClose();
  }
  return <Dialog title={titles[action.kind]} onClose={onClose} busy={pending || choosing} className={action.kind === "createProject" ? "project-create-modal" : ""}>
    <form onSubmit={submit}><div className="modal-body">
      {confirming ? <p className="delete-description">确定{archiving ? "归档" : action.kind === "deleteProject" ? "移除" : "删除"}「{name}」？
        {archiving ? "归档后不在工作区显示，历史仍保留。" : action.kind === "deleteProject" ? "会话将保留在独立聊天中，本地项目文件夹不会删除。"
          : action.kind === "deleteProvider" ? "已有会话和消息会保留。" : "此操作无法撤销。"}
      </p> : action.kind === "createProject" ? <>
        <label className="project-name-field"><Icon name="folder" /><input autoFocus aria-label="项目名称" maxLength={120} value={value} placeholder="项目名称" title="留空使用文件夹名称" disabled={pending || choosing} onChange={(event) => setValue(event.target.value)} /></label>
        <div className="project-source-heading"><span>源文件夹</span>
          <button type="button" className="text-button" disabled={pending || choosing} aria-expanded={manualDirectory} onClick={() => setManualDirectory(!manualDirectory)}>{manualDirectory ? "收起路径" : "输入路径"}</button>
        </div>
        <div className={"project-source-card" + (directory.trim() ? " has-directory" : "")}>
          {directory.trim() ? <><span className="project-source-icon"><Icon name="folder" /></span>
            <div className="project-source-copy"><strong>{directory.trim().replace(/[\\/]+$/, "").split(/[\\/]/).pop() || directory}</strong><span title={directory}>{directory}</span></div>
            <button type="button" className="source-folder-button" disabled={pending || choosing} onClick={() => canChooseProjectDirectory() ? browse() : setManualDirectory(true)}>{choosing ? "选择中…" : "更改"}</button>
          </> : <><p>在此电脑上添加文件夹</p><button type="button" className="source-folder-button" disabled={pending || choosing}
            onClick={() => canChooseProjectDirectory() ? browse() : setManualDirectory(true)}><Icon name="folderPlus" />{choosing ? "选择中…" : "添加"}</button></>}
        </div>
        {manualDirectory && <label className="project-directory-field">文件夹路径<input required aria-label="项目文件夹路径" value={directory} placeholder="输入本地文件夹的绝对路径" onChange={(event) => setDirectory(event.target.value)} disabled={pending || choosing} /></label>}
      </> : action.kind === "moveSession" ? <label>目标项目<select autoFocus value={value} disabled={pending} onChange={(event) => setValue(event.target.value)}>
        <option value="">独立聊天</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label>
        : <label>名称<input autoFocus required maxLength={120} value={value} placeholder="输入名称" onChange={(event) => setValue(event.target.value)} disabled={pending} /></label>}
      {error && <div className="inline-error" role="alert">{error}</div>}
    </div><div className="modal-actions">
      <button type="button" className="secondary-button" disabled={pending || choosing} onClick={onClose}>取消</button>
      <button className={deleting ? "danger-button" : "primary-button"} disabled={pending || choosing || (action.kind === "createProject" ? !directory.trim() : !confirming && action.kind !== "moveSession" && !value.trim())}>
        {pending ? "处理中…" : action.kind === "createProject" ? "创建项目" : action.kind === "deleteProject" ? "确认移除" : deleting ? "确认删除" : archiving ? "确认归档" : "保存"}
      </button>
    </div></form>
  </Dialog>;
}

export default function App({ client }: { client: XiClient }) {
  const [workspace, setWorkspace] = useState<Workspace>({ projects: [], sessions: [], active_session_id: null });
  const [providers, setProviders] = useState<Provider[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [sendingTo, setSendingTo] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("general");
  const [preferences, setPreferences] = useState(loadPreferences);
  const [preferencesNotice, setPreferencesNotice] = useState("");
  const [providerNotice, setProviderNotice] = useState("");
  const [providerOpen, setProviderOpen] = useState(false);
  const [providerForm, setProviderForm] = useState<ProviderInput>(emptyProvider);
  const [providerError, setProviderError] = useState("");
  const [modelEffortDrafts, setModelEffortDrafts] = useState<Record<string, string>>({});
  const [loadingModels, setLoadingModels] = useState(false);
  const [savingProvider, setSavingProvider] = useState(false);
  const [selectedModel, setSelectedModel] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState("auto");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [providerId, setProviderId] = useState<string | null>(null);
  const [draftProjectId, setDraftProjectId] = useState<string | null>(preferences.defaultProjectId || null);
  const [changingProject, setChangingProject] = useState(false);
  const [projectPickerRequest, setProjectPickerRequest] = useState(0);
  const [action, setAction] = useState<WorkspaceAction | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [messagesError, setMessagesError] = useState("");
  const [initializing, setInitializing] = useState(true);
  const [connecting, setConnecting] = useState(false);
  const [search, setSearch] = useState("");
  const [searching, setSearching] = useState(false);
  const [matches, setMatches] = useState<Set<string> | null>(null);
  const selection = useRef<Selection | null>(null);
  const formVersion = useRef(0);
  const modelRequest = useRef(0);
  const messagesRequest = useRef(0);
  const workspaceRequest = useRef(0);
  const drafts = useRef(new Map<string | null, string>());
  const submitLock = useRef(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const searchField = useRef<HTMLInputElement>(null);
  const messageEnd = useRef<HTMLDivElement>(null);

  const activeSession = workspace.sessions.find((item) => item.id === sessionId);
  const activeProvider = providers.find((provider) => provider.id === providerId);
  const availableModels = activeProvider?.models ?? [];
  const selectedModelDescriptor = availableModels.find((model) => model.id === selectedModel);
  const selectedReasoning = modelReasoningEffort(selectedModelDescriptor, reasoningEffort);
  const reasoningOptions = selectedModelDescriptor?.reasoning_efforts ?? [];
  const providerPending = loadingModels || savingProvider;
  const waitingHere = busy && sendingTo === sessionId;

  function select(next: Selection) {
    if (selection.current?.sessionId !== next.sessionId) setInput(drafts.current.get(next.sessionId) ?? "");
    selection.current = next;
    setSessionId(next.sessionId);
    setProviderId(next.providerId);
  }

  function changeInput(value: string) {
    drafts.current.set(selection.current?.sessionId ?? null, value);
    setInput(value);
  }

  async function loadMessages(id: string): Promise<Message[]> {
    return (await client.getHistory(id))
      .map((message) => ({ ...message, role: message.role as Message["role"] }));
  }

  async function showMessages(id: string | null, showLoading = false) {
    const request = ++messagesRequest.current;
    setMessagesError("");
    if (showLoading) setLoadingMessages(true);
    try {
      const next = id ? await loadMessages(id) : [];
      if (messagesRequest.current === request && selection.current?.sessionId === id) setMessages(next);
    } catch (cause) {
      if (messagesRequest.current === request && selection.current?.sessionId === id) setMessagesError((cause as Error).message);
    } finally { if (messagesRequest.current === request) setLoadingMessages(false); }
  }

  async function refresh() {
    const request = ++workspaceRequest.current;
    const [nextWorkspace, nextProviders] = await Promise.all([client.getWorkspace(), client.listProviders()]);
    if (request !== workspaceRequest.current) return;
    const previous = selection.current;
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
    setSelectedModel((current) => current && provider?.models.some((model) => model.id === current)
      ? current : provider?.models[0]?.id || "");
    await showMessages(next.sessionId, previous?.sessionId !== next.sessionId);
  }

  async function refreshSafely() {
    try { await refresh(); } catch (cause) { setError((cause as Error).message); }
  }

  async function reconnect() {
    if (connecting) return;
    setConnecting(true); setError("");
    try { await refresh(); } catch (cause) { setError((cause as Error).message); }
    finally { setConnecting(false); }
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
    initialize().finally(() => { if (!cancelled) setInitializing(false); });
    return () => { cancelled = true; };
  }, [client]);

  useEffect(() => {
    if (preferences.autoScroll) messageEnd.current?.scrollIntoView({ block: "end" });
  }, [messages, waitingHere, settingsOpen, preferences.autoScroll]);

  useEffect(() => {
    const element = textarea.current;
    if (element) { element.style.height = "auto"; element.style.height = Math.min(element.scrollHeight, 240) + "px"; }
  }, [input, settingsOpen]);

  useEffect(() => {
    let cancelled = false;
    setMatches(null);
    if (!search.trim()) { setSearching(false); return; }
    setSearching(true);
    const timer = setTimeout(() => {
      client.searchSessions(search.trim()).then((sessions) => {
        if (!cancelled) setMatches(new Set(sessions.map((session) => session.id)));
      }).catch((cause: Error) => { if (!cancelled) setError(cause.message); })
        .finally(() => { if (!cancelled) setSearching(false); });
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

  function openSettings(section: SettingsSection = "general") {
    setSettingsSection(section); setSettingsOpen(true); setSidebarOpen(false);
  }

  function closeSettings() {
    setSettingsOpen(false); setSidebarOpen(false);
    requestAnimationFrame(() => textarea.current?.focus());
  }

  function updatePreferences(patch: Partial<Preferences>) {
    const next = { ...preferences, ...patch };
    setPreferences(next);
    try {
      window.localStorage.setItem(preferencesKey, JSON.stringify(next));
      setPreferencesNotice("已自动保存");
    } catch { setPreferencesNotice("已在当前窗口生效，无法保存到本地。"); }
  }

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || event.isComposing || document.querySelector("dialog[open]")) return;
      const modified = event.ctrlKey || event.metaKey;
      if (modified && event.key === ",") { event.preventDefault(); openSettings(); }
      else if (modified && event.shiftKey && event.code === "KeyO") {
        event.preventDefault();
        if (event.altKey) {
          if (!busy && !initializing) setProjectPickerRequest((value) => value + 1);
        } else if (!busy && !initializing) newConversation();
      } else if (event.key === "Escape" && settingsOpen && !(event.target instanceof HTMLSelectElement)) {
        event.preventDefault(); closeSettings();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [settingsOpen, busy, changingProject, initializing, providerId, workspace.projects, preferences.defaultProjectId]);

  function openProvider(provider?: Provider) {
    openSettings("providers");
    setProviderNotice("");
    formVersion.current += 1;
    setProviderError(""); setModelEffortDrafts({});
    setProviderForm(provider ? { id: provider.id, name: provider.name, base_url: provider.base_url,
      models: provider.models, api_key: "" } : { ...emptyProvider });
    setProviderOpen(true);
  }

  function closeProvider() {
    formVersion.current += 1;
    setProviderOpen(false);
  }

  function changeProvider(patch: Partial<ProviderInput>) {
    if (patch.base_url !== undefined || patch.api_key !== undefined) setModelEffortDrafts({});
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
    if (!content || submitLock.current || changingProject || loadingMessages || messagesError || initializing) return;
    if (!providerId || !selectedModel) { setError("请先配置模型服务，并在输入框下方选择模型。"); return; }
    const previous = selection.current;
    let targetSession = previous?.sessionId ?? null;
    const originalInput = input;
    submitLock.current = true;
    setBusy(true); setSendingTo(targetSession); setError(""); changeInput("");
    setMessages((current) => [...current, { role: "user", content }]);
    try {
      if (!targetSession) {
        if (draftProjectId && !workspace.projects.some((project) => project.id === draftProjectId)) throw new Error("项目已移除，请重新选择项目。");
        const created = await client.createSession(draftProjectId);
        targetSession = created.session_id;
        setSendingTo(targetSession);
        if (selection.current?.sessionId === previous?.sessionId) {
          select({ sessionId: targetSession, providerId: selection.current?.providerId ?? providerId });
        }
      }
      setMessages((current) => [...current, { role: "assistant", content: "", status: "streaming" }]);
      await client.sendMessageStream({ content, session_id: targetSession, provider_id: providerId, model: selectedModel,
        reasoning_effort: selectedReasoning }, (delta) => {
          setMessages((current) => {
            const index = current.length - 1;
            const last = current[index];
            if (!last || last.role !== "assistant") return current;
            return [...current.slice(0, index), { ...last, content: last.content + delta, status: "streaming" }];
          });
        });
    } catch (cause) {
      drafts.current.set(targetSession, originalInput);
      if (selection.current?.sessionId === targetSession) setInput(originalInput);
      setError((cause as Error).message + " 输入已保留，请检查会话记录后再重试。");
    }
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
        setProviderForm((current) => ({ ...current, models: mergeFetchedModels(current.models, models) }));
        setModelEffortDrafts({});
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
    closeProvider(); setSavingProvider(false); setProviderNotice("模型服务配置已保存。");
    await refreshSafely();
  }

  function selectProvider(id: string) {
    select({ sessionId: selection.current?.sessionId ?? null, providerId: id });
    const provider = providers.find((item) => item.id === id);
    setSelectedModel(provider?.models[0]?.id || "");
    setReasoningEffort("auto");
  }

  function newConversation(projectId?: string | null) {
    if (busy || changingProject) return;
    closeSettings();
    setReasoningEffort("auto");
    select({ sessionId: null, providerId });
    messagesRequest.current += 1;
    const defaultProject = workspace.projects.find((project) => project.id === preferences.defaultProjectId)?.id ?? null;
    // 新建项目后的回调仍持有刷新前的列表；显式项目已由目录接口验证。
    setDraftProjectId(projectId === undefined ? defaultProject : projectId);
    setMessages([]); setMessagesError(""); setError(""); setLoadingMessages(false); setSidebarOpen(false);
    textarea.current?.focus();
  }

  async function selectSession(id: string) {
    const next = { sessionId: id, providerId: selection.current?.providerId ?? null };
    select(next); setMessages([]); setError(""); setSidebarOpen(false);
    await showMessages(id, true);
  }

  const currentProject = workspace.projects.find((project) => project.id === (activeSession ? activeSession.project_id : draftProjectId));

  async function changeConversationProject(projectId: string | null) {
    if (busy || changingProject || initializing) return;
    if (projectId === (currentProject?.id ?? null)) { textarea.current?.focus(); return; }
    if (!activeSession) {
      setDraftProjectId(projectId);
      textarea.current?.focus();
      return;
    }
    setChangingProject(true); setError("");
    try {
      await client.moveSession(activeSession.id, projectId);
      await refresh();
    } catch (cause) { setError((cause as Error).message); }
    finally { setChangingProject(false); textarea.current?.focus(); }
  }


  function renderSessions(sessions: Session[]) {
    return sessions.map((session) => <div className={"session-row" + (session.id === sessionId ? " active" : "")} key={session.id}>
      <button className="session" onClick={() => selectSession(session.id)} title={session.title} aria-current={session.id === sessionId ? "page" : undefined}><span>{session.title}</span></button>
      <div className="row-actions">
        <button className="icon-button" title="重命名会话" aria-label={"重命名会话「" + session.title + "」"} onClick={() => setAction({ kind: "renameSession", session })}><Icon name="edit" /></button>
        <button className="icon-button danger-icon" title="删除会话" aria-label={"删除会话「" + session.title + "」"} disabled={busy} onClick={() => setAction({ kind: "deleteSession", session })}><Icon name="delete" /></button>
      </div>
    </div>);
  }

  const providerContent = <>
    <section className="settings-group">
      <div className="settings-group-heading"><h2>已保存的服务</h2>
        <button type="button" className="secondary-button" disabled={providerPending} onClick={() => openProvider()}><Icon name="plus" />添加模型服务</button></div>
      <p className="settings-section-description">管理 API 地址与密钥，具体模型在对话输入框中选择。</p>
      <div className="settings-card settings-providers">
        {!providers.length && <div className="settings-provider-empty"><Icon name="server" /><p>尚未添加模型服务</p><span>添加一个服务，开始你的第一段对话。</span></div>}
        {providers.map((provider) => <div className="setting-row saved-provider" key={provider.id}>
          <span className="provider-symbol"><Icon name="server" /></span>
          <div className="setting-copy"><div className="setting-label">{provider.name}{provider.id === providerId && <span className="provider-active-badge">当前使用</span>}</div>
            <p title={provider.base_url}>{provider.base_url}</p>
            <span className="provider-meta">{provider.api_key_configured ? "密钥已配置" : "未配置密钥"} · {provider.models.length} 个模型</span></div>
          <div className="provider-row-actions">
            <button type="button" className="text-button" disabled={providerPending} onClick={() => openProvider(provider)}>编辑</button>
            <button type="button" className="text-button danger-text" disabled={providerPending || providers.length <= 1}
              title={providers.length <= 1 ? "至少保留一个模型服务" : "删除模型服务"} onClick={() => setAction({ kind: "deleteProvider", provider })}>删除</button>
          </div>
        </div>)}
      </div>
      {providerNotice && <p className="settings-provider-notice" role="status">{providerNotice}</p>}
    </section>
    {providerOpen && <section className="settings-group">
      <div className="settings-group-heading"><h2>{providerForm.id ? "编辑模型服务" : "添加模型服务"}</h2><span>保存后生效</span></div>
      <div className="settings-card settings-provider-editor">
        <form onSubmit={saveProvider}>
          <fieldset className="provider-fields settings-provider-fields" disabled={providerPending}>
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
              {providerForm.models.map((model) => <span className="model-chip" key={model.id}>{model.id}</span>)}
            </div>}
            {providerForm.models.length > 0 && <details className="model-capabilities">
              <summary>按模型配置推理选项</summary>
              <p>填写本服务支持的 reasoning_effort 值，用逗号分隔；留空时仅使用服务默认值。代理服务的选项可能与原厂不同。</p>
              <div className="model-capability-list">{providerForm.models.map((model) => <label className="model-capability-row" key={model.id}>
                <span title={model.id}>{model.id}<small>{({ builtin: "内置适配", provider: "服务声明", custom: "自定义", unknown: "未声明" })[model.reasoning_source]}</small></span>
                <input aria-label={model.id + " 的推理选项"} placeholder="例如 low, high, max"
                  value={modelEffortDrafts[model.id] ?? model.reasoning_efforts.join(", ")} onChange={(event) => {
                    const value = event.target.value;
                    setModelEffortDrafts((current) => ({ ...current, [model.id]: value }));
                    changeProvider({ models: providerForm.models.map((item) => item.id === model.id
                      ? { ...item, reasoning_efforts: value.split(/[,，\s]+/).filter(Boolean), reasoning_source: "custom" as const } : item) });
                  }} />
              </label>)}</div>
            </details>}
            {providerError && <div className="inline-error" role="alert">{providerError}</div>}
          </fieldset>
          <div className="settings-editor-actions"><button type="button" className="secondary-button" disabled={providerPending} onClick={closeProvider}>取消</button>
            <button className="primary-button" disabled={providerPending}>{savingProvider && <span className="spinner" />}{savingProvider ? "保存中…" : "保存配置"}</button></div>
        </form>
      </div>
    </section>}
  </>;

  return (
    <div className={"shell" + (sidebarOpen ? " sidebar-visible" : "") + (settingsOpen ? " settings-shell" : "") + (preferences.compactSidebar ? " compact-sidebar" : "")}
      style={{ "--message-font-size": `${preferences.messageFontSize}px`, "--conversation-width": preferences.chatWidth === "wide" ? "920px" : "740px",
        "--composer-width": preferences.chatWidth === "wide" ? "980px" : "800px" } as CSSProperties}>
      <nav className="activity-rail" aria-label="快捷导航">
        <img className="rail-brand" src="/xi-icon.png" alt="xi" />
        <button className={"icon-button" + (!settingsOpen ? " rail-home" : "")} title={settingsOpen ? "返回对话" : "新会话"} aria-label={settingsOpen ? "返回对话" : "打开新会话"} disabled={!settingsOpen && (busy || initializing)}
          onClick={() => settingsOpen ? closeSettings() : newConversation()}><Icon name="home" /></button>
        {!settingsOpen && <><button className="icon-button" title="搜索会话" aria-label="定位会话搜索" onClick={() => searchField.current?.focus()}><Icon name="search" /></button>
        <button className="icon-button" title="新建项目" aria-label="添加工作区项目" onClick={() => setAction({ kind: "createProject" })}><Icon name="folder" /></button></>}
        <button className={"icon-button rail-settings" + (settingsOpen ? " rail-selected" : "")} title="设置" aria-label="打开设置" aria-current={settingsOpen ? "page" : undefined} onClick={() => openSettings()}><Icon name="settings" /></button>
      </nav>
      {settingsOpen ? <SettingsPage section={settingsSection} onSectionChange={setSettingsSection} onBack={closeSettings}
        preferences={preferences} onChange={updatePreferences} projects={workspace.projects} notice={preferencesNotice} providerContent={providerContent}
        sidebarOpen={sidebarOpen} onSidebarClose={() => setSidebarOpen(false)} /> : <>
      {sidebarOpen && <button className="sidebar-scrim" aria-label="收起侧栏" onClick={() => setSidebarOpen(false)} />}
      <aside className="sidebar" aria-label="工作区导航">
        <div className="brand"><div>xi<span className="brand-caption">工作台</span></div>
          <button className="icon-button sidebar-close" aria-label="收起侧栏" onClick={() => setSidebarOpen(false)}>×</button>
        </div>
        <button className="new-chat-button" disabled={busy || initializing} onClick={() => newConversation()}><Icon name="edit" />新会话</button>
        <div className="sidebar-search"><Icon name="search" /><input ref={searchField} className="workspace-search" type="search" aria-label="搜索会话和消息" placeholder="搜索会话…" value={search} onChange={(event) => setSearch(event.target.value)} /></div>
        <div className="section-heading"><span>项目</span><button className="icon-button" aria-label="新建项目" title="新建项目"
          onClick={() => setAction({ kind: "createProject" })}><Icon name="plus" /></button></div>
        {searching && <p className="search-status" role="status">正在搜索…</p>}
        <nav className="tree" aria-label="项目和会话">
          {workspace.projects.map((project) => {
            const sessions = workspace.sessions.filter((session) => session.project_id === project.id && (!matches || matches.has(session.id)));
            if (matches && !sessions.length) return null;
            return <section className="project" key={project.id}>
              <div className="project-heading">
                <button className={"project-title" + (project.id === currentProject?.id ? " selected" : "")} disabled={busy} title={project.directory} onClick={() => newConversation(project.id)}>
                  <Icon name="folder" /><span>{project.name}</span><small>{sessions.length}</small>
                </button>
                <div className="row-actions">
                  <button className="icon-button" title="新会话" aria-label={"在「" + project.name + "」中新建会话"} disabled={busy} onClick={() => newConversation(project.id)}><Icon name="plus" /></button>
                  <button className="icon-button" title="重命名项目" aria-label={"重命名项目「" + project.name + "」"} onClick={() => setAction({ kind: "renameProject", project })}><Icon name="edit" /></button>
                  <button className="icon-button danger-icon" title="移除项目" aria-label={"移除项目「" + project.name + "」"} disabled={busy} onClick={() => setAction({ kind: "deleteProject", project })}><Icon name="delete" /></button>
                  <button className="icon-button" title="归档项目" aria-label={"归档项目「" + project.name + "」"} disabled={busy} onClick={() => setAction({ kind: "archiveProject", project })}>↓</button>
                </div>
              </div>
              {!sessions.length && <p className="project-empty">暂无会话</p>}
              {renderSessions(sessions)}
            </section>;
          })}
          {!workspace.projects.length && !matches && <p className="project-empty">打开文件夹，开始一个项目。</p>}
          <div className="section-heading local-chat-heading"><span>聊天</span><button className="icon-button" aria-label="新建独立聊天" disabled={busy} onClick={() => newConversation(null)}><Icon name="plus" /></button></div>
          {renderSessions(workspace.sessions.filter((session) => !session.project_id && (!matches || matches.has(session.id))))}
          {matches?.size === 0 && <p className="project-empty">没有匹配的会话</p>}
        </nav>
        <div className="sidebar-footer">
          <button className="settings-button" onClick={() => openSettings()}><Icon name="settings" />设置<span>偏好与模型服务</span></button>
        </div>
      </aside>
      <main className="main">
        <header className="conversation-header">
          <button className="sidebar-toggle icon-button" aria-label="展开侧栏" onClick={() => setSidebarOpen(true)}><Icon name="menu" /></button>
          <div className="conversation-heading"><strong title={activeSession?.title}>{activeSession?.title ?? "新会话"}</strong>
            <span>{currentProject?.name ?? "独立聊天"}</span></div>
          <button className="icon-button refresh-button" aria-label="刷新工作区" title={connecting || initializing ? "正在连接工作区" : "刷新工作区"}
            disabled={connecting || initializing || busy} onClick={reconnect}>
            {connecting || initializing ? <span className="spinner" /> : <Icon name="refresh" />}
          </button>
        </header>
        <section className="messages" aria-label="对话消息" aria-busy={loadingMessages}>
          {initializing ? <div className="empty" role="status"><span className="spinner" />正在连接工作区…</div>
            : loadingMessages ? <div className="empty" role="status"><span className="spinner" />正在加载会话…</div>
            : messagesError ? <div className="empty"><p role="alert">加载会话失败：{messagesError}</p>
              <button className="secondary-button" onClick={() => showMessages(sessionId, true)}>重新加载</button></div> : messages.length === 0 ? (
            <div className="empty">
              <div className="empty-mark"><Icon name="spark" /></div>
              <h1>{activeSession?.title ?? (currentProject ? "在「" + currentProject.name + "」中开始对话" : "我们可以一起做些什么？")}</h1>
              <p>{providers.length ? "随时提问，让想法更进一步。" : "连接模型服务，开始你的第一段对话。"}</p>
              {!providers.length ? <button className="primary-button" onClick={() => openProvider()}><Icon name="plus" />配置模型服务</button>
                : null}
            </div>
          ) : messages.map((message, index) => (
            <article className={"message " + message.role} key={index + "-" + message.role}>
              <div className="message-content"><MessageContent content={message.content} />
                {(message.status === "interrupted" || message.status === "failed") && <p className="message-warning">回复{message.status === "interrupted" ? "中断" : "失败"}：{message.error ?? "请重试"}</p>}
                {message.status === "streaming" && <p className="message-pending" role="status">回复尚未完成，稍后刷新查看。</p>}
              </div>
            </article>
          ))}
          {waitingHere && <div className="reply-status" role="status"><span className="spinner" />正在等待模型回复…</div>}
          <div ref={messageEnd} />
        </section>
        <div className="composer-area">
          {error && <div className="inline-error chat-error" role="alert"><span>{error}</span>
            <button className="icon-button" aria-label="关闭错误提示" onClick={() => setError("")}>×</button></div>}
          <div className="composer-context"><ProjectPicker projects={workspace.projects} selected={currentProject}
            disabled={busy || changingProject || initializing || loadingMessages} pending={changingProject}
            onSelect={changeConversationProject} onCreate={() => setAction({ kind: "createProject" })} openRequest={projectPickerRequest} /></div>
          <form className="composer" onSubmit={send}>
            <textarea ref={textarea} aria-label="消息内容" value={input} onChange={(event) => changeInput(event.target.value)}
              placeholder={providers.length ? "随心输入…" : "配置模型服务后，开始对话…"}
              disabled={busy || loadingMessages || initializing || !!messagesError} rows={2}
              onKeyDown={(event) => {
                if (isSendKey({ key: event.key, shiftKey: event.shiftKey, ctrlKey: event.ctrlKey, metaKey: event.metaKey,
                  isComposing: event.nativeEvent.isComposing, keyCode: event.nativeEvent.keyCode }, preferences.sendShortcut)) {
                  event.preventDefault();
                  if (!event.repeat) event.currentTarget.form?.requestSubmit();
                }
              }} />
            <div className="composer-toolbar">
              <button type="button" className="icon-button composer-new" aria-label="新建会话" title="新建会话" disabled={busy || initializing}
                onClick={() => newConversation(currentProject?.id ?? null)}><Icon name="plus" /></button>
              {activeSession && <details className="session-tools"><summary>会话操作</summary><div>
                <button type="button" disabled={busy} onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); setAction({kind:"moveSession",session:activeSession}); }}>移动到项目</button>
                <button type="button" disabled={busy} onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); setAction({kind:"archiveSession",session:activeSession}); }}>归档会话</button>
                <button type="button" onClick={(event) => { event.currentTarget.closest("details")?.removeAttribute("open"); exportConversation(activeSession); }}>导出 Markdown</button>
              </div></details>}
              <div className="chat-controls">
                <label className="provider-control"><span>模型服务</span><select aria-label="模型服务" disabled={busy || initializing} value={activeProvider?.id ?? ""} onChange={(event) => selectProvider(event.target.value)}>
                  {!providers.length && <option value="">未配置服务</option>}{providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
                </select></label>
                <label className="model-control"><span>模型</span><select disabled={busy || !availableModels.length} value={selectedModel} onChange={(event) => { setSelectedModel(event.target.value); setReasoningEffort("auto"); }} aria-label="选择模型">
                  {availableModels.length ? availableModels.map((model) => <option key={model.id} value={model.id}>{model.id}</option>)
                    : <option value="">请先获取模型</option>}
                </select></label>
                {reasoningOptions.length > 0 ? <label className="reasoning-control"><span>推理强度</span><select disabled={busy} value={selectedReasoning ?? "auto"} onChange={(event) => setReasoningEffort(event.target.value)} aria-label="推理强度">
                  <option value="auto">默认</option>{reasoningOptions.map((effort) => <option key={effort} value={effort}>{reasoningLabel(effort)}</option>)}
                </select></label> : <span className="reasoning-default" title="该模型未声明可调推理选项，使用服务默认值；可在模型服务设置中配置。">服务默认</span>}
              </div>
              <button type="submit" className="send-button" aria-label={busy ? "正在生成回复" : "发送消息"} title={preferences.sendShortcut === "enter" ? "发送消息（Enter）" : "发送消息（Ctrl / ⌘ + Enter）"}
                disabled={busy || changingProject || loadingMessages || initializing || !!messagesError || !input.trim() || !selectedModel || !providerId}>
                {busy ? <span className="spinner" /> : <Icon name="send" />}
              </button>
            </div>
          </form>
          {preferences.showInputHint && <p className="composer-hint">{preferences.sendShortcut === "enter" ? "Enter 发送" : "Ctrl / ⌘ + Enter 发送"} <span>·</span> {preferences.sendShortcut === "enter" ? "Shift + Enter 换行" : "Enter 换行"}</p>}
        </div>
      </main>
      </>}
      {action && <WorkspaceDialog action={action} client={client} projects={workspace.projects} onClose={() => setAction(null)} onSaved={async (created) => {
        if (action.kind === "deleteProject" && draftProjectId === action.project.id) setDraftProjectId(null);
        if (action.kind === "deleteProvider" && providerForm.id === action.provider.id) openProvider();
        await refreshSafely();
        if (created) newConversation(created.id);
      }} />}
    </div>
  );
}
