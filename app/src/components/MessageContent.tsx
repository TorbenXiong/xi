import { useEffect, useId, useState } from "react";
import type { ReactNode } from "react";
import type mermaid from "mermaid";
import { Icon } from "./Icon";

type Mermaid = typeof mermaid;
let mermaidModule: Promise<Mermaid> | null = null;
let mermaidInitialized = false;

async function loadMermaid(): Promise<Mermaid> {
  const module = mermaidModule ??= import("mermaid").then(({ default: value }) => value);
  const instance = await module;
  if (mermaidInitialized) return instance;
  instance.initialize({ startOnLoad: false, securityLevel: "strict", theme: "base", suppressErrorRendering: true });
  mermaidInitialized = true;
  return instance;
}

function MermaidDiagram({ definition }: { definition: string }) {
  const id = `xi-mermaid-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const [svg, setSvg] = useState("");
  const [error, setError] = useState(false);
  const [showSource, setShowSource] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (copyStatus === "idle") return;
    const timeout = setTimeout(() => setCopyStatus("idle"), 2500);
    return () => clearTimeout(timeout);
  }, [copyStatus]);

  async function copySource() {
    try {
      await navigator.clipboard.writeText(definition.trim());
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
      setShowSource(true);
    }
  }

  useEffect(() => {
    let active = true;
    setSvg("");
    setError(false);
    loadMermaid().then((instance) => instance.render(id, definition.trim())).then((result) => {
      if (active) setSvg(result.svg);
    }).catch(() => {
      if (active) setError(true);
    });
    return () => { active = false; };
  }, [definition, id]);

  return <div className="mermaid-card">
    <div className="mermaid-toolbar">
      <div className="mermaid-view-switch" role="group" aria-label="Mermaid 展示方式">
        <button type="button" aria-pressed={!showSource} onClick={() => setShowSource(false)}>图表</button>
        <button type="button" aria-pressed={showSource} onClick={() => setShowSource(true)}>源码</button>
      </div>
      <button type="button" className="mermaid-copy" aria-label="复制 Mermaid 源码" onClick={copySource}>
        <Icon name={copyStatus === "copied" ? "check" : "copy"} />
        <span role="status">{copyStatus === "copied" ? "已复制" : "复制"}</span>
      </button>
    </div>
    {copyStatus === "failed" && <p className="mermaid-notice" role="alert">复制失败，请在源码中选中文字后复制。</p>}
    {error && !showSource && <p className="mermaid-notice" role="status">图表无法渲染，已保留源码。</p>}
    {showSource || error
      ? <pre className="mermaid-source" tabIndex={0}><code>{definition.trim()}</code></pre>
      : svg
        ? <div className="mermaid-diagram" dangerouslySetInnerHTML={{ __html: svg }} />
        : <div className="mermaid-diagram mermaid-loading" role="status">正在绘制图表…</div>}
  </div>;
}

export function MessageContent({ content }: { content: string }) {
  const parts: ReactNode[] = [];
  const fence = /```mermaid\s*\r?\n([\s\S]*?)```/gi;
  let cursor = 0;
  let match: RegExpExecArray | null;
  while ((match = fence.exec(content))) {
    if (match.index > cursor) {
      parts.push(<span className="message-text" key={`text-${cursor}`}>{content.slice(cursor, match.index)}</span>);
    }
    parts.push(<MermaidDiagram definition={match[1]} key={`mermaid-${match.index}`} />);
    cursor = match.index + match[0].length;
  }
  if (cursor < content.length || !parts.length) {
    parts.push(<span className="message-text" key={`text-${cursor}`}>{content.slice(cursor)}</span>);
  }
  return <>{parts}</>;
}
