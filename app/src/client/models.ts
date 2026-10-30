import type { ModelDescriptor } from "./types";

export function modelReasoningEffort(model: ModelDescriptor | undefined, selection: string): string | undefined {
  return selection !== "auto" && model?.reasoning_efforts.includes(selection) ? selection : undefined;
}

export function mergeFetchedModels(previous: ModelDescriptor[], fetched: ModelDescriptor[]): ModelDescriptor[] {
  return fetched.map((model) => previous.find((saved) => saved.id === model.id && saved.reasoning_source === "custom") ?? model);
}

export function reasoningLabel(value: string): string {
  return ({ none: "关闭", minimal: "最低", low: "低", medium: "中", high: "高", xhigh: "极高", max: "最高" } as Record<string, string>)[value] ?? value;
}
