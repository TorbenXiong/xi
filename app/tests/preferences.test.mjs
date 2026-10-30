import assert from "node:assert/strict";
import test from "node:test";
import { defaultPreferences, isSendKey, parsePreferences } from "../src/preferences.ts";

test("missing or corrupt preferences use a fresh set of defaults", () => {
  for (const value of [null, "", "not json", "null", "[]", "true", "42"]) {
    assert.deepEqual(parsePreferences(value), defaultPreferences);
  }
  const result = parsePreferences(null);
  result.autoScroll = false;
  assert.equal(defaultPreferences.autoScroll, true);
});

test("valid preferences round trip without changing user choices", () => {
  const preferences = {
    sendShortcut: "mod-enter", autoScroll: false, showInputHint: false,
    messageFontSize: 16, chatWidth: "wide", compactSidebar: true,
    defaultProjectId: "fixture-group",
  };
  assert.deepEqual(parsePreferences(JSON.stringify(preferences)), preferences);
});

test("invalid fields fall back independently while retaining valid choices", () => {
  const result = parsePreferences(JSON.stringify({
    sendShortcut: "anything", autoScroll: "false", showInputHint: false,
    messageFontSize: 1000, chatWidth: "full", compactSidebar: 1,
    defaultProjectId: 123,
  }));
  assert.deepEqual(result, { ...defaultPreferences, showInputHint: false });
});

test("unknown fields are not copied into preferences", () => {
  const result = parsePreferences(JSON.stringify({ autoScroll: false, unrelated: "fixture" }));
  assert.equal(result.autoScroll, false);
  assert.ok(!("unrelated" in result));
});

const enter = { key: "Enter", shiftKey: false, ctrlKey: false, metaKey: false };

test("Enter and modifier-Enter sending modes do not overlap", () => {
  assert.equal(isSendKey(enter, "enter"), true);
  assert.equal(isSendKey(enter, "mod-enter"), false);
  for (const modifier of [{ ctrlKey: true }, { metaKey: true }]) {
    assert.equal(isSendKey({ ...enter, ...modifier }, "enter"), false);
    assert.equal(isSendKey({ ...enter, ...modifier }, "mod-enter"), true);
  }
});

test("composing text and Shift-Enter never send in either mode", () => {
  for (const shortcut of ["enter", "mod-enter"]) {
    for (const patch of [{ shiftKey: true }, { isComposing: true }, { keyCode: 229 }, { key: "a" }]) {
      assert.equal(isSendKey({ ...enter, ctrlKey: shortcut === "mod-enter", ...patch }, shortcut), false);
    }
  }
});
