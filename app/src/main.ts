import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type AndroidModelState = {
  defaultModel: string;
  modelPath: string;
  models: string[];
  status?: string;
  error?: string | null;
};

type ModelsResponse = {
  data?: Array<{ id?: string }>;
};

declare global {
  interface Window {
    llmdAndroid?: {
      importDefaultModel(): void;
      getModelState(): string;
    };
  }
}

const output = document.querySelector<HTMLPreElement>("#output");
const health = document.querySelector<HTMLButtonElement>("#health");
const refreshModels = document.querySelector<HTMLButtonElement>("#refresh-models");
const importModel = document.querySelector<HTMLButtonElement>("#import-model");
const apiBase = document.querySelector<HTMLSpanElement>("#api-base");
const modelPath = document.querySelector<HTMLSpanElement>("#model-path");
const modelStatus = document.querySelector<HTMLParagraphElement>("#model-status");
const modelList = document.querySelector<HTMLUListElement>("#model-list");
const logs = document.querySelector<HTMLAnchorElement>("#logs");

const defaultApiBase = "http://127.0.0.1:11435";
const defaultModel = "gemma-4-E2B-it";

health?.addEventListener("click", async () => {
  try {
    const response = await invoke<Record<string, unknown>>("health");
    if (output) output.textContent = JSON.stringify(response, null, 2);
  } catch (error) {
    if (output) output.textContent = String(error);
  }
});

refreshModels?.addEventListener("click", () => {
  void refreshModelList();
});

importModel?.addEventListener("click", () => {
  if (!window.llmdAndroid) {
    setModelStatus("Model import is only available in the Android app.");
    return;
  }
  setModelStatus("Waiting for model selection...");
  window.llmdAndroid.importDefaultModel();
});

window.addEventListener("llmd-models-changed", (event) => {
  const state = (event as CustomEvent<AndroidModelState>).detail;
  renderAndroidModelState(state);
  void refreshModelList();
});

if (apiBase) apiBase.textContent = defaultApiBase;
if (logs) logs.href = `${defaultApiBase}/logs`;

loadAndroidModelState();
void refreshModelList();

function loadAndroidModelState() {
  if (window.llmdAndroid) {
    renderAndroidModelState(parseAndroidModelState(window.llmdAndroid.getModelState()));
    return;
  }

  if (modelPath) modelPath.textContent = "Android app private model directory";
  if (importModel) importModel.disabled = true;
}

async function refreshModelList() {
  try {
    const response = await fetch(`${defaultApiBase}/v1/models`);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const body = (await response.json()) as ModelsResponse;
    const models = body.data?.flatMap((model) => (model.id ? [model.id] : [])) ?? [];
    renderModels(models);
    setModelStatus(
      models.length > 0
        ? "Model is ready."
        : `${defaultModel} has not been imported yet.`,
    );
  } catch (error) {
    renderModels([]);
    setModelStatus(`Unable to read models: ${String(error)}`);
  }
}

function renderAndroidModelState(state: AndroidModelState) {
  if (modelPath) modelPath.textContent = state.modelPath;
  renderModels(state.models);

  if (state.error) {
    setModelStatus(`Import failed: ${state.error}`);
    return;
  }

  if (state.status === "imported") {
    setModelStatus("Model imported.");
  } else if (state.status === "cancelled") {
    setModelStatus("Model import cancelled.");
  } else if (state.models.length > 0) {
    setModelStatus("Model is ready.");
  } else {
    setModelStatus(`${state.defaultModel} has not been imported yet.`);
  }
}

function renderModels(models: string[]) {
  if (!modelList) return;
  modelList.replaceChildren(
    ...models.map((model) => {
      const item = document.createElement("li");
      item.textContent = model;
      return item;
    }),
  );

  if (models.length === 0) {
    const item = document.createElement("li");
    item.className = "empty";
    item.textContent = "No imported models";
    modelList.append(item);
  }
}

function setModelStatus(message: string) {
  if (modelStatus) modelStatus.textContent = message;
}

function parseAndroidModelState(json: string): AndroidModelState {
  return JSON.parse(json) as AndroidModelState;
}
