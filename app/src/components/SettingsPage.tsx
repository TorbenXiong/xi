import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Project } from "../client/types";
import type { Preferences } from "../preferences";
import { Icon } from "./Icon";
import "./SettingsPage.css";

export type SettingsSection = "general" | "appearance" | "providers" | "shortcuts";
const sections = [
  { id: "general", title: "常规", icon: "settings", category: "个人", keywords: "发送方式 默认项目 自动滚动 输入提示" },
  { id: "appearance", title: "外观", icon: "sun", category: "个人", keywords: "消息字号 显示宽度 紧凑侧栏" },
  { id: "shortcuts", title: "键盘快捷键", icon: "keyboard", category: "个人", keywords: "快捷键 发送消息 换行 新建会话 打开设置 返回对话" },
  { id: "providers", title: "模型服务", icon: "server", category: "集成", keywords: "模型配置 Provider API Key 地址 密钥 模型列表" },
] as const;

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return <button type="button" className="setting-switch" role="switch" aria-label={label} aria-checked={checked}
    onClick={() => onChange(!checked)}><span /></button>;
}

function SettingRow({ title, description, children }: { title: string; description: string; children: ReactNode }) {
  return <div className="setting-row"><div className="setting-copy"><div className="setting-label">{title}</div>
    <p>{description}</p></div><div className="setting-control">{children}</div></div>;
}

export function SettingsPage({ section, onSectionChange, onBack, preferences, onChange, projects, notice, providerContent, sidebarOpen, onSidebarClose }: {
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  onBack: () => void;
  preferences: Preferences;
  onChange: (patch: Partial<Preferences>) => void;
  projects: Project[];
  notice: string;
  providerContent: ReactNode;
  sidebarOpen: boolean;
  onSidebarClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const content = useRef<HTMLElement>(null);
  const needle = query.trim().toLocaleLowerCase();
  const matches = (text: string) => !needle || text.toLocaleLowerCase().includes(needle);
  const visible = sections.filter((item) => needle ? matches(item.title + item.keywords) : item.id === section);
  const [mobileNavigation, setMobileNavigation] = useState(false);
  const sendKeys = preferences.sendShortcut === "enter" ? "Enter" : "Ctrl / ⌘ + Enter";

  useEffect(() => { content.current?.scrollTo({ top: 0 }); }, [section, query]);

  function navigate(next: SettingsSection) {
    onSectionChange(next); setQuery(""); setMobileNavigation(false); onSidebarClose();
  }

  return <>
    <aside className={"sidebar settings-sidebar" + (mobileNavigation || sidebarOpen ? " settings-nav-open" : "")} aria-label="设置分类">
      <div className="settings-sidebar-title"><h1>设置</h1><button className="icon-button sidebar-close" aria-label="收起设置分类"
        onClick={() => { setMobileNavigation(false); onSidebarClose(); }}>×</button></div>
      <div className="settings-search"><Icon name="search" /><input type="search" aria-label="搜索设置" placeholder="搜索设置"
        value={query} onChange={(event) => setQuery(event.target.value)} /></div>
      <nav className="settings-navigation" aria-label="设置页面">
        {["个人", "集成"].map((category) => <section key={category}><p className="settings-nav-heading">{category}</p>
          {sections.filter((item) => item.category === category).map((item) => <button key={item.id} type="button"
            className={"settings-nav-item" + (section === item.id && !needle ? " selected" : "")}
            aria-current={section === item.id && !needle ? "page" : undefined} onClick={() => navigate(item.id)}>
            <Icon name={item.icon} />{item.title}
          </button>)}</section>)}
      </nav>
      <button className="settings-back" onClick={onBack}><Icon name="back" />返回对话</button>
    </aside>
    {(mobileNavigation || sidebarOpen) && <button className="settings-nav-scrim" aria-label="收起设置分类"
      onClick={() => { setMobileNavigation(false); onSidebarClose(); }} />}
    <main ref={content} className="main settings-main" aria-label="应用设置">
      <div className="settings-mobile-header"><button className="icon-button" aria-label="展开设置分类" onClick={() => setMobileNavigation(true)}><Icon name="menu" /></button>
        <button className="text-button" onClick={onBack}>返回对话</button></div>
      <div className="settings-content">
        <header className="settings-page-header"><h1>{needle ? "搜索结果" : sections.find((item) => item.id === section)?.title}</h1>
          <span className="settings-save-status" role="status">{section === "providers" && !needle ? "模型配置保存后生效" : notice || "偏好设置自动保存"}</span></header>
        {!visible.length && <div className="settings-no-results"><Icon name="search" /><p>没有找到相关设置</p><span>试试“模型”“字号”或“发送”。</span></div>}
        {visible.some((item) => item.id === "general") && <div className="settings-section" aria-label="常规设置">
          {needle && <h2>常规</h2>}
          <section className="settings-group"><h2>对话</h2><div className="settings-card">
            <SettingRow title="发送方式" description="选择发送消息时使用的按键。">
              <select aria-label="发送方式" value={preferences.sendShortcut} onChange={(event) => onChange({ sendShortcut: event.target.value as Preferences["sendShortcut"] })}>
                <option value="enter">Enter 发送</option><option value="mod-enter">Ctrl / ⌘ + Enter 发送</option></select>
            </SettingRow>
            <SettingRow title="新会话默认项目" description="从新会话按钮开始对话时，默认保存到此项目。">
              <select aria-label="新会话默认项目" value={projects.some((project) => project.id === preferences.defaultProjectId) ? preferences.defaultProjectId : ""}
                onChange={(event) => onChange({ defaultProjectId: event.target.value })}>
                <option value="">独立聊天</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
              </select>
            </SettingRow>
          </div></section>
          <section className="settings-group"><h2>阅读与输入</h2><div className="settings-card">
            <SettingRow title="自动滚动到最新消息" description="加载会话或收到回复时，自动显示最新内容。">
              <Toggle label="自动滚动到最新消息" checked={preferences.autoScroll} onChange={(autoScroll) => onChange({ autoScroll })} />
            </SettingRow>
            <SettingRow title="显示输入提示" description="在输入框下方显示发送和换行的按键提示。">
              <Toggle label="显示输入提示" checked={preferences.showInputHint} onChange={(showInputHint) => onChange({ showInputHint })} />
            </SettingRow>
          </div></section>
        </div>}
        {visible.some((item) => item.id === "appearance") && <div className="settings-section" aria-label="外观设置">
          {needle && <h2>外观</h2>}
          <section className="settings-group"><h2>消息显示</h2><div className="settings-card">
            <SettingRow title="消息字号" description="调整对话消息和输入框中的文字大小。">
              <select aria-label="消息字号" value={preferences.messageFontSize} onChange={(event) => onChange({ messageFontSize: Number(event.target.value) as Preferences["messageFontSize"] })}>
                <option value={13}>小 · 13 px</option><option value={14}>默认 · 14 px</option><option value={16}>大 · 16 px</option></select>
            </SettingRow>
            <SettingRow title="显示宽度" description="调整对话内容与输入框在大屏幕上的宽度。">
              <select aria-label="显示宽度" value={preferences.chatWidth} onChange={(event) => onChange({ chatWidth: event.target.value as Preferences["chatWidth"] })}>
                <option value="standard">标准</option><option value="wide">宽敞</option></select>
            </SettingRow>
          </div></section>
          <section className="settings-group"><h2>侧栏</h2><div className="settings-card">
            <SettingRow title="紧凑侧栏" description="缩小项目与会话的间距，在侧栏中显示更多内容。">
              <Toggle label="紧凑侧栏" checked={preferences.compactSidebar} onChange={(compactSidebar) => onChange({ compactSidebar })} />
            </SettingRow>
          </div></section>
          <div className="appearance-preview" style={{ fontSize: preferences.messageFontSize }}><Icon name="spark" /><div><strong>让想法更进一步。</strong><p>从一个问题开始，和 xi 一起探索。</p></div><span>字号预览</span></div>
        </div>}
        {visible.some((item) => item.id === "providers") && <div className="settings-section" aria-label="模型服务设置">
          {needle && <h2>模型服务</h2>}{providerContent}
        </div>}
        {visible.some((item) => item.id === "shortcuts") && <div className="settings-section" aria-label="键盘快捷键设置">
          {needle && <h2>键盘快捷键</h2>}
          <section className="settings-group"><h2>对话与导航</h2><div className="settings-card">
            <SettingRow title="发送消息" description="可在常规设置中调整发送方式。"><kbd>{sendKeys}</kbd></SettingRow>
            <SettingRow title="换行" description="在消息中插入新的一行。"><kbd>{preferences.sendShortcut === "enter" ? "Shift + Enter" : "Enter"}</kbd></SettingRow>
            <SettingRow title="新建会话" description="打开新的会话草稿。"><kbd>Ctrl / ⌘ + Shift + O</kbd></SettingRow>
            <SettingRow title="打开设置" description="从任意页面进入常规设置。"><kbd>Ctrl / ⌘ + ,</kbd></SettingRow>
            <SettingRow title="返回对话" description="离开设置页，继续当前会话。"><kbd>Esc</kbd></SettingRow>
          </div></section>
        </div>}
      </div>
    </main>
  </>;
}
