export function Icon({ name }: { name: "edit" | "delete" | "plus" | "chat" | "settings" | "send" | "menu" | "folder" }) {
  const paths = {
    edit: <><path d="m16 3 5 5-12 12-6 1 1-6Z" /><path d="m13 6 5 5" /></>,
    delete: <><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7" /></>,
    plus: <path d="M12 5v14M5 12h14" />,
    chat: <path d="M21 11.5a8.5 8.5 0 0 1-12.5 7.4L3 21l2.1-5.5A8.5 8.5 0 1 1 21 11.5Z" />,
    settings: <><path d="M4 7h16M4 17h16" /><circle cx="9" cy="7" r="3" /><circle cx="15" cy="17" r="3" /></>,
    send: <path d="m5 12 7-7 7 7M12 5v15" />,
    menu: <path d="M4 6h16M4 12h16M4 18h16" />,
    folder: <path d="M3 7V5h6l2 2h10v13H3Z" />,
  };
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
    strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}
