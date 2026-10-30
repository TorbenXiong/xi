import { useEffect, useId, useRef, useState } from "react";
import type { Project } from "../client/types";
import { Icon } from "./Icon";
import "./ProjectPicker.css";

export function ProjectPicker({ projects, selected, disabled, pending, onSelect, onCreate, openRequest }: {
  projects: Project[];
  selected?: Project;
  disabled: boolean;
  pending: boolean;
  onSelect: (id: string | null) => void;
  onCreate: () => void;
  openRequest?: number;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const term = query.trim().toLocaleLowerCase();
  const matches = projects.filter((project) => !term || (project.name + "\n" + project.directory).toLocaleLowerCase().includes(term));

  function close(restoreFocus = false) {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  }

  useEffect(() => {
    if (!open) return;
    function outside(event: PointerEvent) {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  useEffect(() => {
    if (openRequest) { setQuery(""); setOpen(true); }
  }, [openRequest]);

  return <div className="project-picker" ref={root}
    onBlur={(event) => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) close(); }}
    onKeyDown={(event) => {
      if (!open) return;
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); }
      else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const items = Array.from(root.current?.querySelectorAll<HTMLButtonElement>("[data-project-option]") ?? []);
        if (!items.length) return;
        event.preventDefault();
        const index = items.indexOf(document.activeElement as HTMLButtonElement);
        items[index < 0 ? (event.key === "ArrowDown" ? 0 : items.length - 1)
          : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length].focus();
      }
    }}>
    <button ref={trigger} type="button" className={"project-picker-trigger" + (open ? " is-open" : "")}
      aria-label={selected ? "切换项目" : "选择项目"} aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? panelId : undefined}
      title={selected?.directory ?? "选择一个项目或闲聊（Ctrl+Alt+Shift+O）"} disabled={disabled}
      onClick={() => { setQuery(""); setOpen(!open); }}>
      {pending ? <span className="spinner" /> : <Icon name="folder" />}
      <span>{selected?.name ?? "选择项目"}</span><Icon name="chevron" />
    </button>
    {open && <div className="project-picker-panel" id={panelId} role="dialog" aria-label="选择项目">
      <label className="project-picker-search"><Icon name="search" />
        <input autoFocus type="search" aria-label="搜索项目" placeholder="搜索项目" value={query} onChange={(event) => setQuery(event.target.value)} />
      </label>
      <div className="project-picker-list">
        {matches.map((project) => <button type="button" data-project-option key={project.id}
          className={"project-picker-item" + (selected?.id === project.id ? " is-selected" : "")}
          aria-pressed={selected?.id === project.id} title={project.directory}
          onClick={() => { close(); onSelect(project.id); }}>
          <Icon name="folder" /><span><span className="project-picker-name">{project.name}</span>
            <span className="project-picker-directory">{project.directory}</span></span>
          {selected?.id === project.id && <Icon name="check" />}
        </button>)}
        {!matches.length && <p className="project-picker-empty">{term ? "未找到匹配项目" : "暂无项目，添加文件夹开始工作"}</p>}
      </div>
      <div className="project-picker-footer">
        <button type="button" data-project-option className="project-picker-create" onClick={() => { close(); onCreate(); }}><Icon name="plus" />新建项目</button>
        <button type="button" data-project-option className="project-picker-detach" aria-pressed={!selected}
          onClick={() => { close(); onSelect(null); }}><Icon name="close" /><span>不在项目中工作</span>{!selected && <Icon name="check" />}</button>
      </div>
    </div>}
  </div>;
}
