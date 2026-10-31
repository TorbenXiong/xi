export function Icon({ name }: { name: "edit" | "delete" | "plus" | "chat" | "settings" | "send" | "menu" | "folder" | "home" | "search" | "refresh" | "spark" | "sun" | "keyboard" | "server" | "back" | "chevron" | "check" | "copy" | "close" | "folderPlus" }) {
  const paths = {
    edit: <><path d="m16 3 5 5-12 12-6 1 1-6Z" /><path d="m13 6 5 5" /></>,
    delete: <><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7" /></>,
    plus: <path d="M12 5v14M5 12h14" />,
    chat: <path d="M21 11.5a8.5 8.5 0 0 1-12.5 7.4L3 21l2.1-5.5A8.5 8.5 0 1 1 21 11.5Z" />,
    settings: <><path d="M4 7h16M4 17h16" /><circle cx="9" cy="7" r="3" /><circle cx="15" cy="17" r="3" /></>,
    send: <path d="m5 12 7-7 7 7M12 5v15" />,
    menu: <path d="M4 6h16M4 12h16M4 18h16" />,
    folder: <path d="M3 7V5h6l2 2h10v13H3Z" />,
    home: <><path d="m3 10 9-7 9 7v10H3Z" /><path d="M9 20v-7h6v7" /></>,
    search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 5 5" /></>,
    refresh: <><path d="M20 7v5h-5M4 17v-5h5" /><path d="M6 7a7 7 0 0 1 12-1l2 6M4 12l2 6a7 7 0 0 0 12-1" /></>,
    spark: <><path d="M12 3c1 5 4 8 9 9-5 1-8 4-9 9-1-5-4-8-9-9 5-1 8-4 9-9Z" /><path d="m19 2 1 3 3 1-3 1-1 3-1-3-3-1 3-1Z" /></>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M5 19l1.5-1.5M17.5 6.5 19 5" /></>,
    keyboard: <><rect x="2" y="5" width="20" height="14" rx="3" /><path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 12h.01M10 12h.01M14 12h.01M18 12h.01M7 15h10" /></>,
    server: <><rect x="3" y="3" width="18" height="7" rx="2" /><rect x="3" y="14" width="18" height="7" rx="2" /><path d="M7 6.5h.01M7 17.5h.01M16 6.5h2M16 17.5h2" /></>,
    back: <path d="m10 5-7 7 7 7M3 12h18" />,
    chevron: <path d="m8 10 4 4 4-4" />,
    check: <path d="m5 12 4 4L19 6" />,
    copy: <><rect x="8" y="8" width="13" height="13" rx="2" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></>,
    close: <path d="m6 6 12 12M6 18 18 6" />,
    folderPlus: <><path d="M21 11V7H11L9 5H3v15h10M3 9h18" /><path d="M18 14v8M14 18h8" /></>,
  };
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
    strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}
