import { useEffect, useId, useRef } from "react";
import type { ReactNode } from "react";

export function Dialog({ title, description, onClose, busy = false, children }: {
  title: string;
  description?: string;
  onClose: () => void;
  busy?: boolean;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  return (
    <dialog ref={dialog} className="modal" aria-labelledby={titleId}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}>
      <div className="modal-head">
        <div><h2 id={titleId}>{title}</h2>{description && <p id={descriptionId}>{description}</p>}</div>
        <button type="button" className="icon-button" aria-label="关闭弹窗" disabled={busy} onClick={onClose}>×</button>
      </div>
      {children}
    </dialog>
  );
}
