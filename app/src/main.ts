import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type AndroidModelState = {
  defaultModel: string;
  modelPath: string;
  models: string[];
  status?: string;
  error?: string | null;
};

type HealthState = Record<string, unknown>;

type Model = {
  id: string;
  ownedBy: string;
  path?: string;
};

type LogEntry = {
  message: string;
  tone: "info" | "success" | "error";
  time: string;
};

declare global {
  interface Window {
    llmdAndroid?: {
      deleteModel(model: string): void;
      getHealthState(): string;
      getModelState(): string;
      importDefaultModel(): void;
    };
  }
}

const app = document.querySelector<HTMLElement>("#app");
const defaultApiBase = "http://127.0.0.1:11435";
const defaultModel = "gemma-4-E2B-it";
const state: {
  health: HealthState | null;
  healthError: string | null;
  isCheckingHealth: boolean;
  isLoadingModels: boolean;
  isMutatingModel: boolean;
  modelError: string | null;
  mutationError: string | null;
  models: Model[];
  logs: LogEntry[];
} = {
  health: null,
  healthError: null,
  isCheckingHealth: false,
  isLoadingModels: false,
  isMutatingModel: false,
  modelError: null,
  mutationError: null,
  models: [],
  logs: [],
};

window.addEventListener("hashchange", render);
window.addEventListener("llmd-models-changed", (event) => {
  const androidState = (event as CustomEvent<AndroidModelState>).detail;
  state.models = modelsFromAndroid(androidState);
  state.modelError = null;
  state.mutationError = androidState.error
    ? `Model operation failed: ${androidState.error}`
    : null;
  state.isMutatingModel = false;
  const message = androidState.error
    ? `Model import failed: ${androidState.error}`
    : androidState.status === "cancelled"
      ? "Model import cancelled."
      : androidState.status === "deleted"
        ? "Model deleted."
        : "Model imported."
  addLog(message, androidState.error ? "error" : "success");
  if (androidState.status === "deleted") window.location.hash = "#/models";
  render();
});

addLog("llmd console started.", "info");
void refreshModels();
void checkHealth();
render();

function currentRoute(): string {
  const route = window.location.hash.replace(/^#\//, "");
  return route || "status";
}

function render() {
  if (!app) return;

  const route = currentRoute();
  const page = route.startsWith("models/")
    ? renderModelDetails(decodeURIComponent(route.slice("models/".length)))
    : route === "models"
      ? renderModelsPage()
      : route === "logs"
        ? renderLogsPage()
        : renderStatusPage();

  app.innerHTML = page;
  setActiveNavigation(route);
  bindPageEvents(route);
}

function renderStatusPage(): string {
  const isAndroid = Boolean(window.llmdAndroid);
  const health = state.health;
  const status = state.healthError ? "Unavailable" : health ? "Running" : "Checking";
  const tone = state.healthError ? "danger" : health ? "success" : "neutral";

  return `
    <header class="page-header">
      <div>
        <p class="eyebrow">Overview</p>
        <h1>Service status</h1>
        <p class="page-description">Monitor the local inference service and its connection.</p>
      </div>
      <button class="button secondary" id="check-health" ${state.isCheckingHealth ? "disabled" : ""}>
        ${state.isCheckingHealth ? "Checking…" : "Check health"}
      </button>
    </header>
    <section class="status-hero" aria-live="polite">
      <div class="status-icon ${tone}"></div>
      <div>
        <p class="eyebrow">Service</p>
        <h2>${status}</h2>
        <p>${state.healthError ?? (health ? "The local model service is ready to receive requests." : "Contacting the local service…")}</p>
      </div>
    </section>
    <section class="detail-grid">
      <article class="card">
        <p class="card-label">Connection</p>
        <p class="card-value">${isAndroid ? "Android Binder IPC" : defaultApiBase}</p>
        <p class="card-hint">${isAndroid ? "App-private native bridge" : "OpenAI-compatible HTTP API"}</p>
      </article>
      <article class="card">
        <p class="card-label">Available models</p>
        <p class="card-value">${state.models.length}</p>
        <p class="card-hint">${state.models.length === 1 ? "One model is ready" : "Open Models to manage them"}</p>
      </article>
      <article class="card">
        <p class="card-label">Default model</p>
        <p class="card-value compact">${defaultModel}</p>
        <p class="card-hint">Used when no model is specified</p>
      </article>
    </section>
    <section class="panel health-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">Health response</p>
          <h2>Service details</h2>
        </div>
      </div>
      <pre>${escapeHtml(health ? JSON.stringify(health, null, 2) : "No health response yet.")}</pre>
    </section>`;
}

function renderModelsPage(): string {
  const mutationError = state.mutationError
    ? `<p class="mutation-error" role="alert">${escapeHtml(state.mutationError)}</p>`
    : "";
  const content = state.isLoadingModels
    ? `<div class="empty-state"><span class="spinner"></span><h2>Loading models</h2><p>Reading local model inventory…</p></div>`
    : state.modelError
      ? `<div class="empty-state error-state"><h2>Could not load models</h2><p>${escapeHtml(state.modelError)}</p><button class="button secondary" id="refresh-models">Try again</button></div>`
      : state.models.length === 0
        ? `<div class="empty-state"><div class="empty-icon">⌁</div><h2>No models imported</h2><p>Import a LiteRT-LM model to start serving requests.</p><button class="button primary" id="import-model">Import model</button></div>`
        : `<div class="model-list">${state.models.map(renderModelRow).join("")}</div>`;

  return `
    <header class="page-header">
      <div>
        <p class="eyebrow">Library</p>
        <h1>Models</h1>
        <p class="page-description">Import, inspect, and remove local LiteRT-LM models.</p>
      </div>
      <div class="header-actions">
        <button class="button secondary" id="refresh-models" ${state.isLoadingModels ? "disabled" : ""}>Refresh</button>
        <button class="button primary" id="import-model" ${state.isMutatingModel ? "disabled" : ""}>Import model</button>
      </div>
    </header>
    ${mutationError}
    ${content}`;
}

function renderModelRow(model: Model): string {
  const href = `#/models/${encodeURIComponent(model.id)}`;
  return `
    <a class="model-row" href="${escapeHtml(href)}">
      <span class="model-icon">◈</span>
      <span class="model-main"><strong>${escapeHtml(model.id)}</strong><small>${escapeHtml(model.ownedBy)}</small></span>
      <span class="row-action">Details <span aria-hidden="true">→</span></span>
    </a>`;
}

function renderModelDetails(modelId: string): string {
  const model = state.models.find((item) => item.id === modelId);
  if (!model) {
    return `
      <header class="page-header"><div><p class="eyebrow">Models</p><h1>Model not found</h1><p class="page-description">This model is no longer available in the local inventory.</p></div></header>
      <a class="button secondary back-link" href="#/models">Back to models</a>`;
  }

  return `
    <header class="page-header">
      <div>
        <a class="breadcrumb" href="#/models">← Models</a>
        <p class="eyebrow">Model details</p>
        <h1>${escapeHtml(model.id)}</h1>
        <p class="page-description">Manage this locally available model.</p>
      </div>
      <button class="button danger" id="delete-model" ${state.isMutatingModel ? "disabled" : ""}>${state.isMutatingModel ? "Deleting…" : "Delete model"}</button>
    </header>
    <section class="panel">
      <div class="panel-heading"><div><p class="eyebrow">Information</p><h2>Model metadata</h2></div></div>
      <dl class="metadata-list">
        <div><dt>Model ID</dt><dd>${escapeHtml(model.id)}</dd></div>
        <div><dt>Provider</dt><dd>${escapeHtml(model.ownedBy)}</dd></div>
        <div><dt>Location</dt><dd>${escapeHtml(model.path ?? "Managed by LiteRT-LM")}</dd></div>
      </dl>
    </section>
    ${state.mutationError ? `<p class="mutation-error" role="alert">${escapeHtml(state.mutationError)}</p>` : ""}
    <section class="danger-zone">
      <div><h2>Danger zone</h2><p>Deleting a model removes its local files and stops it from being available for new requests.</p></div>
      <button class="button danger-outline" id="delete-model-secondary" ${state.isMutatingModel ? "disabled" : ""}>Delete model</button>
    </section>`;
}

function renderLogsPage(): string {
  return `
    <header class="page-header">
      <div><p class="eyebrow">Activity</p><h1>Logs</h1><p class="page-description">Recent actions from this llmd app session.</p></div>
    </header>
    <section class="panel log-panel">
      ${state.logs.length === 0 ? `<div class="empty-state"><h2>No events yet</h2><p>Actions you take in the app will appear here.</p></div>` : `<ol class="log-list">${state.logs.map((entry) => `<li><span class="log-dot ${entry.tone}"></span><div><strong>${escapeHtml(entry.message)}</strong><time>${escapeHtml(entry.time)}</time></div></li>`).join("")}</ol>`}
    </section>`;
}

function bindPageEvents(route: string) {
  document.querySelector<HTMLButtonElement>("#check-health")?.addEventListener("click", () => void checkHealth());
  document.querySelector<HTMLButtonElement>("#refresh-models")?.addEventListener("click", () => void refreshModels());
  document.querySelector<HTMLButtonElement>("#import-model")?.addEventListener("click", () => void importModel());
  if (route.startsWith("models/")) {
    document.querySelector<HTMLButtonElement>("#delete-model")?.addEventListener("click", () => void deleteCurrentModel());
    document.querySelector<HTMLButtonElement>("#delete-model-secondary")?.addEventListener("click", () => void deleteCurrentModel());
  }
}

function setActiveNavigation(route: string) {
  const active = route.startsWith("models/") ? "models" : route;
  document.querySelectorAll<HTMLAnchorElement>("[data-nav]").forEach((link) => {
    link.classList.toggle("active", link.dataset.nav === active);
  });
}

async function checkHealth() {
  state.isCheckingHealth = true;
  state.healthError = null;
  render();
  try {
    state.health = window.llmdAndroid
      ? parseJson(window.llmdAndroid.getHealthState())
      : await invoke<HealthState>("health");
    addLog("Service health check completed.", "success");
  } catch (error) {
    state.health = null;
    state.healthError = `Unable to reach service: ${String(error)}`;
    addLog(state.healthError, "error");
  } finally {
    state.isCheckingHealth = false;
    render();
  }
}

async function refreshModels() {
  state.isLoadingModels = true;
  state.modelError = null;
  render();
  try {
    if (window.llmdAndroid) {
      state.models = modelsFromAndroid(parseAndroidModelState(window.llmdAndroid.getModelState()));
    } else {
      const response = await fetch(`${defaultApiBase}/v1/models`);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const body = (await response.json()) as { data?: Array<{ id?: string; owned_by?: string }> };
      state.models = (body.data ?? []).flatMap((model) => model.id ? [{ id: model.id, ownedBy: model.owned_by ?? "litert-lm" }] : []);
    }
    addLog(`Model inventory refreshed (${state.models.length} found).`, "info");
  } catch (error) {
    state.models = [];
    state.modelError = `Unable to read models: ${String(error)}`;
    addLog(state.modelError, "error");
  } finally {
    state.isLoadingModels = false;
    render();
  }
}

async function importModel() {
  if (window.llmdAndroid) {
    state.isMutatingModel = true;
    state.mutationError = null;
    addLog("Waiting for a model file to be selected.", "info");
    render();
    window.llmdAndroid.importDefaultModel();
    return;
  }

  const model = window.prompt("Enter the LiteRT-LM model name to download:", defaultModel)?.trim();
  if (!model) return;

  state.isMutatingModel = true;
  state.mutationError = null;
  render();
  try {
    await invoke("import_model", { model });
    addLog(`Imported model ${model}.`, "success");
    await refreshModels();
  } catch (error) {
    state.mutationError = `Unable to import ${model}: ${String(error)}`;
    addLog(state.mutationError, "error");
  } finally {
    state.isMutatingModel = false;
    render();
  }
}

async function deleteCurrentModel() {
  const model = decodeURIComponent(currentRoute().slice("models/".length));
  if (!model || !window.confirm(`Delete ${model}? This removes its local files.`)) return;

  state.isMutatingModel = true;
  state.mutationError = null;
  render();
  try {
    if (window.llmdAndroid) {
      window.llmdAndroid.deleteModel(model);
      return;
    }
    await invoke("delete_model", { model });
    addLog(`Deleted model ${model}.`, "success");
    await refreshModels();
    window.location.hash = "#/models";
  } catch (error) {
    state.mutationError = `Unable to delete ${model}: ${String(error)}`;
    addLog(state.mutationError, "error");
  } finally {
    if (!window.llmdAndroid) {
      state.isMutatingModel = false;
      render();
    }
  }
}

function modelsFromAndroid(androidState: AndroidModelState): Model[] {
  return androidState.models.map((id) => ({ id, ownedBy: "litert-lm-android", path: androidState.modelPath }));
}

function parseAndroidModelState(json: string): AndroidModelState {
  return parseJson(json) as AndroidModelState;
}

function parseJson(json: string): Record<string, unknown> {
  return JSON.parse(json) as Record<string, unknown>;
}

function addLog(message: string, tone: LogEntry["tone"]) {
  state.logs.unshift({
    message,
    tone,
    time: new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(new Date()),
  });
  state.logs.splice(40);
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>'"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#039;", '"': "&quot;" })[character] ?? character);
}
