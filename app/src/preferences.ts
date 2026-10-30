export type Preferences = {
  sendShortcut: "enter" | "mod-enter";
  autoScroll: boolean;
  showInputHint: boolean;
  messageFontSize: 13 | 14 | 16;
  chatWidth: "standard" | "wide";
  compactSidebar: boolean;
  defaultProjectId: string;
};

export const preferencesKey = "xi.preferences";
export const defaultPreferences: Preferences = {
  sendShortcut: "enter", autoScroll: true, showInputHint: true,
  messageFontSize: 14, chatWidth: "standard", compactSidebar: false,
  defaultProjectId: "",
};

/** 逐项读取，损坏的字段不会影响其他有效偏好。 */
export function parsePreferences(value: string | null): Preferences {
  const result = { ...defaultPreferences };
  if (!value) return result;
  let input: unknown;
  try { input = JSON.parse(value); } catch { return result; }
  if (!input || typeof input !== "object" || Array.isArray(input)) return result;
  const data = input as Record<string, unknown>;
  if (data.sendShortcut === "enter" || data.sendShortcut === "mod-enter") result.sendShortcut = data.sendShortcut;
  if (typeof data.autoScroll === "boolean") result.autoScroll = data.autoScroll;
  if (typeof data.showInputHint === "boolean") result.showInputHint = data.showInputHint;
  if (data.messageFontSize === 13 || data.messageFontSize === 14 || data.messageFontSize === 16) result.messageFontSize = data.messageFontSize;
  if (data.chatWidth === "standard" || data.chatWidth === "wide") result.chatWidth = data.chatWidth;
  if (typeof data.compactSidebar === "boolean") result.compactSidebar = data.compactSidebar;
  if (typeof data.defaultProjectId === "string") result.defaultProjectId = data.defaultProjectId;
  return result;
}

export function loadPreferences(): Preferences {
  try { return parsePreferences(window.localStorage.getItem(preferencesKey)); }
  catch { return { ...defaultPreferences }; }
}

export function isSendKey(event: { key: string; shiftKey: boolean; ctrlKey: boolean; metaKey: boolean; isComposing?: boolean; keyCode?: number }, shortcut: Preferences["sendShortcut"]): boolean {
  if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.keyCode === 229) return false;
  return shortcut === "enter" ? !event.ctrlKey && !event.metaKey : event.ctrlKey || event.metaKey;
}
