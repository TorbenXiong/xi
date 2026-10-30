import assert from "node:assert/strict";
import test from "node:test";
import { mergeFetchedModels, modelReasoningEffort } from "../src/client/models.ts";

const model = (id, efforts, source = "provider") => ({ id, reasoning_efforts: efforts, reasoning_source: source });

test("switching model never forwards a level the next model does not support", () => {
  const first = model("first", ["low", "medium", "high"]);
  const second = model("second", ["low", "high", "max"]);
  assert.equal(modelReasoningEffort(first, "medium"), "medium");
  assert.equal(modelReasoningEffort(second, "medium"), undefined);
  assert.equal(modelReasoningEffort(second, "max"), "max");
});

test("service default omits reasoning while explicit none is retained", () => {
  const supported = model("configured", ["none", "high"]);
  assert.equal(modelReasoningEffort(supported, "auto"), undefined);
  assert.equal(modelReasoningEffort(supported, "none"), "none");
  assert.equal(modelReasoningEffort(model("unknown", []), "high"), undefined);
  assert.equal(modelReasoningEffort(undefined, "high"), undefined);
});

test("refresh removes retired models while preserving unsaved custom capabilities", () => {
  const custom = model("a", ["max"], "custom");
  const refreshed = mergeFetchedModels([custom, model("retired", [])], [model("a", ["high"]), model("new", [])]);
  assert.deepEqual(refreshed, [custom, model("new", [])]);
  assert.deepEqual(mergeFetchedModels([], [model("a", ["high"])]), [model("a", ["high"])]);
});
