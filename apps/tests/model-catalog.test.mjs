import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "model-catalog.ts"
);

async function loadModelCatalogModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-model-catalog-")
  );
  const tempFile = path.join(tempDir, "model-catalog.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const modelCatalog = await loadModelCatalogModule();

test("buildCodexAppVisibleModel emits the unified visible shape for third-party models", () => {
  const projected = modelCatalog.buildCodexAppVisibleModel({
    slug: "mimo-v2.5-pro",
    displayName: "MiMo V2.5 Pro",
    description: "chat adapter route",
    defaultReasoningLevel: null,
    supportedReasoningLevels: [],
    shellType: null,
    visibility: "list",
    supportedInApi: true,
    priority: 5,
    additionalSpeedTiers: [],
    availabilityNux: null,
    upgrade: null,
    baseInstructions: null,
    modelMessages: null,
    supportsReasoningSummaries: null,
    defaultReasoningSummary: null,
    supportVerbosity: null,
    defaultVerbosity: null,
    applyPatchToolType: null,
    webSearchToolType: null,
    truncationPolicy: null,
    supportsParallelToolCalls: null,
    supportsImageDetailOriginal: null,
    contextWindow: null,
    autoCompactTokenLimit: null,
    effectiveContextWindowPercent: null,
    experimentalSupportedTools: [],
    inputModalities: [],
    minimalClientVersion: null,
    supportsSearchTool: null,
    availableInPlans: [],
    sourceKind: "remote",
    userEdited: false,
    sortIndex: 12,
    updatedAt: 34,
  });

  assert.equal(projected.slug, "mimo-v2.5-pro");
  assert.equal(projected.display_name, "MiMo V2.5 Pro");
  assert.equal(projected.shell_type, "shell_command");
  assert.equal(projected.default_reasoning_summary, "auto");
  assert.deepEqual(projected.input_modalities, ["text", "image"]);
  assert.equal(projected.source_kind, "remote");
});

test("serializeManagedModelCatalogForCodexCache reuses the same visible projection for cache exports", () => {
  const models = modelCatalog.serializeManagedModelCatalogForCodexCache([
    {
      slug: "hidden-model",
      displayName: "Hidden Model",
      description: null,
      defaultReasoningLevel: null,
      supportedReasoningLevels: [],
      shellType: null,
      visibility: "hidden",
      supportedInApi: true,
      priority: 1,
      additionalSpeedTiers: [],
      availabilityNux: null,
      upgrade: null,
      baseInstructions: null,
      modelMessages: null,
      supportsReasoningSummaries: null,
      defaultReasoningSummary: null,
      supportVerbosity: null,
      defaultVerbosity: null,
      applyPatchToolType: null,
      webSearchToolType: null,
      truncationPolicy: null,
      supportsParallelToolCalls: null,
      supportsImageDetailOriginal: null,
      contextWindow: null,
      autoCompactTokenLimit: null,
      effectiveContextWindowPercent: null,
      experimentalSupportedTools: [],
      inputModalities: [],
      minimalClientVersion: null,
      supportsSearchTool: null,
      availableInPlans: [],
      sourceKind: "remote",
      userEdited: false,
      sortIndex: 0,
      updatedAt: 0,
    },
    {
      slug: "glm-5.1",
      displayName: "GLM 5.1",
      description: null,
      defaultReasoningLevel: null,
      supportedReasoningLevels: [],
      shellType: null,
      visibility: "list",
      supportedInApi: true,
      priority: 2,
      additionalSpeedTiers: [],
      availabilityNux: null,
      upgrade: null,
      baseInstructions: null,
      modelMessages: null,
      supportsReasoningSummaries: null,
      defaultReasoningSummary: null,
      supportVerbosity: null,
      defaultVerbosity: null,
      applyPatchToolType: null,
      webSearchToolType: null,
      truncationPolicy: null,
      supportsParallelToolCalls: null,
      supportsImageDetailOriginal: null,
      contextWindow: null,
      autoCompactTokenLimit: null,
      effectiveContextWindowPercent: null,
      experimentalSupportedTools: [],
      inputModalities: [],
      minimalClientVersion: null,
      supportsSearchTool: null,
      availableInPlans: [],
      sourceKind: "remote",
      userEdited: false,
      sortIndex: 1,
      updatedAt: 1,
    },
  ]);

  assert.equal(models.length, 1);
  assert.equal(models[0].slug, "glm-5.1");
  assert.equal(models[0].shell_type, "shell_command");
});
