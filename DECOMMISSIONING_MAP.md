# DECOMMISSIONING_MAP.md

**Created**: 2025-04-13
**Purpose**: Comprehensive structural audit of the disco-code monorepo to identify, isolate, and map dependency footprints for safe staged decommissioning.

---

## 1. Executive Summary

| Deliverable ID | Package Name     | Purpose                                | Status        | Complexity |
| -------------- | ---------------- | -------------------------------------- | ------------- | ---------- |
| CORE-001       | opencode         | CLI tool - main entry point            | **KEEP**      | N/A        |
| CORE-002       | app              | Web UI (Solid.js SPA)                  | **KEEP**      | N/A        |
| CORE-003       | ui               | Shared UI components                   | **KEEP**      | N/A        |
| CORE-004       | util             | Shared utilities (zod, errors)         | **KEEP**      | N/A        |
| CORE-005       | sdk              | JS/TS SDK for programmatic access      | **KEEP**      | N/A        |
| CORE-006       | plugin           | Plugin system interface                | **KEEP**      | N/A        |
| CORE-007       | script           | Build scripts utility                  | **KEEP**      | N/A        |
| LEGACY-001     | web              | Marketing website (Astro + Cloudflare) | **REMOVE**    | 3          |
| LEGACY-002     | function         | Cloudflare Workers API                 | **REMOVE**    | 4          |
| LEGACY-003     | enterprise       | Enterprise dashboard                   | **REMOVE**    | 4          |
| LEGACY-004     | slack            | Slack bot integration                  | **REMOVE**    | 2          |
| LEGACY-005     | console/\*       | Console infrastructure (5 packages)    | **REMOVE**    | 5          |
| LEGACY-006     | desktop-electron | Electron desktop app                   | **DEPRECATE** | 3          |
| LEGACY-007     | desktop          | Tauri desktop app                      | **KEEP**      | N/A        |
| LEGACY-008     | storybook        | Component documentation                | **KEEP**      | 1          |
| LEGACY-009     | infra/\*         | SST cloud infrastructure (5 files)     | **REMOVE**    | 4          |

---

## 2. Deliverable Audit

### CORE-001: opencode (CLI Tool)

**Purpose**: Main CLI entry point for opencode AI coding assistant. Core product.

**Entry Points**:

- `packages/opencode/src/index.ts` (main CLI)
- `packages/opencode/bin/opencode` (executable)

**Exclusive Assets**:

- `src/cli/cmd/*` (all command modules)
- `src/session/*`
- `src/provider/*`
- `src/tool/*`
- `src/effect/*`
- `src/storage/*`

**Shared Dependencies**:

- `@opencode-ai/sdk`
- `@opencode-ai/util`
- `@opencode-ai/plugin`
- `@opencode-ai/script`

**External Dependencies**: ~90 packages (see package.json)

**Infrastructure Impact**:

- `.github/workflows/opencode.yml` - CI/CD
- `.github/workflows/typecheck.yml` - Type checking
- `.github/workflows/test.yml` - Test suite

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-002: app (Web UI)

**Purpose**: Solid.js SPA web interface for opencode

**Entry Points**:

- `packages/app/src/index.tsx`
- `packages/app/vite.js`

**Exclusive Assets**:

- `src/pages/*`
- `src/components/*`
- `src/stores/*`

**Shared Dependencies**:

- `@opencode-ai/ui`
- `@opencode-ai/sdk`
- `@opencode-ai/util`

**Infrastructure Impact**:

- `.github/workflows/beta.yml` - Beta deployments
- `.github/workflows/publish.yml` - Production

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-003: ui (Shared UI Components)

**Purpose**: Reusable UI component library (Solid.js)

**Entry Points**:

- `packages/ui/src/components/*.tsx`
- `packages/ui/src/context/*`

**Exclusive Assets**:

- `src/components/*`
- `src/hooks/*`
- `src/context/*`
- `src/i18n/*`
- `src/styles/*`

**Shared Dependencies**:

- `@opencode-ai/sdk` (types/api)
- `@opencode-ai/util` (utilities)

**Infrastructure Impact**:

- `.github/workflows/storybook.yml` (via storybook package)
- Used by: app, desktop, desktop-electron, storybook, enterprise

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-004: util (Shared Utilities)

**Purpose**: Common utilities, zod schemas, error handling

**Entry Points**:

- `packages/util/src/*.ts`

**Exclusive Assets**: All files in `src/`

**Shared Dependencies**: None (base package)

**Infrastructure Impact**: Used by ALL packages

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-005: sdk (JavaScript SDK)

**Purpose**: TypeScript/JavaScript SDK for programmatic opencode access

**Entry Points**:

- `packages/sdk/js/src/index.ts`
- `packages/sdk/js/src/client.ts`
- `packages/sdk/js/src/server.ts`

**Exclusive Assets**:

- `js/src/*`
- `js/script/*`

**Shared Dependencies**: None (generates from API)

**Infrastructure Impact**:

- `.github/workflows/publish.yml` - npm publishing

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-006: plugin (Plugin System)

**Purpose**: Plugin interface definitions for opencode extensions

**Entry Points**:

- `packages/plugin/src/index.ts`

**Exclusive Assets**:

- `src/index.ts`
- `src/tool.ts`
- `src/tui.ts`

**Shared Dependencies**:

- `@opencode-ai/sdk`

**Infrastructure Impact**: Used by opencode package

**Complexity Score**: N/A (Core - Not Removable)

---

### CORE-007: script (Build Scripts)

**Purpose**: Shared build and release scripts

**Entry Points**:

- `packages/script/src/index.ts`

**Exclusive Assets**: Minimal - just export utilities

**Shared Dependencies**: None

**Infrastructure Impact**: Used by opencode package for builds

**Complexity Score**: N/A (Core - Not Removable)

---

### LEGACY-001: web (Marketing Website)

**Purpose**: Marketing landing page (https://opencode.ai)

**Entry Points**:

- `packages/web/src/pages/index.astro`
- `packages/web/astro.config.mjs`

**Exclusive Assets**:

- `src/pages/*`
- `src/layouts/*`
- `src/components/*`

**Shared Dependencies**:

- `opencode` (workspace)

**Infrastructure Impact**:

- `.github/workflows/deploy.yml` (removed)
- Uses `@astrojs/cloudflare` adapter
- Cloudflare Pages deployment

**Complexity Score**: 3 (Decoupled)

**Risk Assessment**: Low. No core dependencies on opencode code, just references the product.

---

### LEGACY-002: function (Cloudflare Workers)

**Purpose**: Serverless API functions (auth, sessions, etc.)

**Entry Points**:

- `packages/function/src/index.ts`

**Exclusive Assets**:

- `src/*`
- `wrangler.toml`

**Shared Dependencies**:

- `@opencode-ai/sdk`
- `@opencode-ai/util`

**External Dependencies**:

- `@octokit/auth-app`
- `@octokit/rest`
- `hono`
- `zod`
- `@cloudflare/workers-types`

**Infrastructure Impact**:

- SST/Cloudflare deployment
- Part of sst.config.ts infrastructure

**Complexity Score**: 4 (Tightly Coupled)

**Risk Assessment**: HIGH. Contains auth logic, session handling. Requires careful extraction of shared code.

---

### LEGACY-003: enterprise (Enterprise Dashboard)

**Purpose**: Enterprise billing/admin dashboard

**Entry Points**:

- `packages/enterprise/src/app.tsx`

**Exclusive Assets**:

- `src/*`
- `vite.config.ts`

**Shared Dependencies**:

- `@opencode-ai/ui`
- `@opencode-ai/util`

**Infrastructure Impact**:

- SST/Cloudflare deployment
- `shell-prod` script using sst

**Complexity Score**: 4 (Tightly Coupled)

**Risk Assessment**: MEDIUM. Uses UI components, but business logic is isolated.

---

### LEGACY-004: slack (Slack Bot)

**Purpose**: Slack integration for team notifications

**Entry Points**:

- `packages/slack/src/index.ts`

**Exclusive Assets**:

- `src/*`

**Shared Dependencies**:

- `@opencode-ai/sdk`

**External Dependencies**:

- `@slack/bolt`

**Infrastructure Impact**: None (standalone deployment)

**Complexity Score**: 2 (Isolated)

**Risk Assessment**: LOW. Fully isolated, easy to remove.

---

### LEGACY-005: console/\* (Console Infrastructure)

**Purpose**: Internal admin console (5 packages)

**Packages**:

- `console/core` - Core services
- `console/app` - Console UI
- `console/resource` - Resource management
- `console/mail` - Email service
- `console/function` - Serverless functions

**Entry Points**:

- `console/*/src/index.ts`

**Shared Dependencies**:

- `@opencode-ai/console-core`
- `@opencode-ai/console-mail`
- `@opencode-ai/console-resource`

**Infrastructure Impact**:

- SST deployment
- Part of sst.config.ts
- Cloudflare Workers + R2

**Complexity Score**: 5 (Highly Coupled)

**Risk Assessment**: CRITICAL. Deeply integrated with cloud infrastructure. Requires full infrastructure review.

---

### LEGACY-006: desktop-electron (Electron App)

**Purpose**: Electron-based desktop application

**Entry Points**:

- `packages/desktop-electron/src/main/index.ts`
- `packages/desktop-electron/src/preload/index.ts`

**Exclusive Assets**:

- `src/main/*`
- `src/preload/*`
- `src/renderer/*`

**Shared Dependencies**:

- `@opencode-ai/app`
- `@opencode-ai/ui`

**Infrastructure Impact**: None

**Complexity Score**: 3 (Decoupled)

**Risk Assessment**: LOW. Can be deprecated (Tauri version is primary).

---

### LEGACY-007: desktop (Tauri Desktop App)

**Purpose**: Primary desktop application (Tauri-based)

**Entry Points**:

- `packages/desktop/src-tauri/src/main.rs`
- `packages/desktop/src/main.tsx`

**Exclusive Assets**:

- `src-tauri/*`
- `src/main.tsx`

**Shared Dependencies**:

- `@opencode-ai/app`
- `@opencode-ai/ui`

**Infrastructure Impact**:

- `.github/workflows/publish-vscode.yml`
- Build artifacts for Windows/macOS/Linux

**Complexity Score**: N/A (Core - Keep)

**Status**: PRIMARY desktop app, keep.

---

### LEGACY-008: storybook (Component Documentation)

**Purpose**: Storybook for UI component development

**Entry Points**:

- `.storybook/main.ts`

**Exclusive Assets**:

- `.storybook/*`
- `src/stories/*`

**Shared Dependencies**:

- `@opencode-ai/ui`

**Infrastructure Impact**:

- `.github/workflows/storybook.yml` (building)

**Complexity Score**: 1 (Fully Isolated)

**Risk Assessment**: LOW. Can remove storybook workflow but keep package for development.

---

### LEGACY-009: infra/\* (SST Infrastructure)

**Purpose**: Cloud infrastructure definitions (5 files)

**Files**:

- `infra/app.ts` - Main app infrastructure
- `infra/console.ts` - Console infrastructure
- `infra/enterprise.ts` - Enterprise infrastructure
- `infra/secret.ts` - Secrets management
- `infra/stage.ts` - Stage configuration

**Impact**:

- `sst.config.ts` - SST configuration

**Infrastructure Impact**:

- ALL cloud deployments
- Cloudflare Workers
- AWS/R2 storage
- PlanetScale database

**Complexity Score**: 4 (Tightly Coupled)

**Risk Assessment**: CRITICAL. Controls all cloud deployments. Need careful review before removal.

---

## 3. Dependency Graph

```
                    ┌─────────────────────────────────────────┐
                    │              opencode                  │
                    │         (CLI - CORE PRODUCT)            │
                    └─────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│    desktop       │   │       app         │   │      function     │
│   (Tauri App)    │   │   (Web UI)        │   │ (CF Workers - ⚠️)  │
└───────────────────┘   └───────────────────┘   └───────────────────┘
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│       ui          │   │       ui         │   │      sdk          │
│  (Components)     │   │  (Components)    │   │    (Shared)       │
└───────────────────┘   └───────────────────┘   └───────────────────┘
                                    │                           │
                                    ▼                           ▼
                            ┌───────────────────┐   ┌───────────────────┐
                            │       util       │   │       util        │
                            │   (Core Utils)   │   │   (Core Utils)    │
                            └───────────────────┘   └───────────────────┘
                                    │
        ┌───────────────────────────┴───────────────────────────┐
        │                                                       │
        ▼                                                       ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│   enterprise ⚠️   │   │    web ⚠️         │   │     slack ⚠️      │
└───────────────────┘   └───────────────────┘   └───────────────────┘
        │                                                   │
        ▼                                                   ▼
┌───────────────────┐                               ┌───────────────────┐
│       ui          │                               │      sdk          │
└───────────────────┘                               └───────────────────┘
```

**Legend**:

- ✅ = Core/Keep
- ⚠️ = Legacy/Remove

---

## 4. Risk Assessment & Removal Complexity

### Level 1: Fully Isolated (Easy Removal)

| Package   | Actions                                                                         |
| --------- | ------------------------------------------------------------------------------- |
| slack     | 1. Delete `packages/slack`<br>2. Remove from any references                     |
| storybook | 1. Delete workflow `.github/workflows/storybook.yml`<br>2. Keep package for dev |

### Level 2: Minor Shared Dependencies

| Package | Actions                                                                                           |
| ------- | ------------------------------------------------------------------------------------------------- |
| web     | 1. Delete `packages/web`<br>2. Remove `.github/workflows/deploy.yml`<br>3. No code changes needed |

### Level 3: Decoupled but Complex

| Package          | Actions                                                                                                  |
| ---------------- | -------------------------------------------------------------------------------------------------------- |
| desktop-electron | 1. Delete `packages/desktop-electron`<br>2. Update publish workflows<br>3. Keep Tauri version as primary |

### Level 4: Tightly Coupled (Refactoring Required)

| Package    | Actions                                                                                                      |
| ---------- | ------------------------------------------------------------------------------------------------------------ |
| function   | 1. Extract shared auth logic to sdk/util<br>2. Identify what's used by other packages<br>3. Delete remaining |
| enterprise | 1. Check which UI components are shared<br>2. Migrate enterprise features to main app or delete              |
| infra/\*   | 1. Document all environment variables<br>2. Ensure local dev works without infra<br>3. Delete files          |

### Level 5: Highly Coupled (Expert Review Required)

| Package    | Actions                                                                                                                   |
| ---------- | ------------------------------------------------------------------------------------------------------------------------- |
| console/\* | 1. Full audit of internal dependencies<br>2. Identify what's used by console vs core<br>3. May require splitting packages |

---

## 5. Recommended Removal Order

### Phase 1: Immediate (No Risk)

1. ✅ Remove GitHub workflows: stats, daily-_, duplicate, vouch-_, close-\*, etc. (DONE)
2. ✅ Remove script/github folder (DONE)
3. 🗑️ Remove `packages/slack` (Slack bot)
4. 🗑️ Remove `.github/workflows/storybook.yml` (keep package)

### Phase 2: Low Risk

5. 🗑️ Remove `packages/web` (marketing site)
6. 🗑️ Remove `packages/desktop-electron` (deprecated)
7. 🗑️ Remove `packages/function` (after code extraction)

### Phase 3: Medium Risk (Requires Review)

8. 🔄 Refactor `packages/enterprise` (extract shared UI components)
9. 🔄 Remove enterprise features or merge into main app
10. 🗑️ Remove `infra/*` (after verifying local dev works)

### Phase 4: High Risk (Expert Review)

11. 🔄 Audit `packages/console/*` thoroughly
12. 🔄 Decide: keep console or full removal
13. 🔄 Update `sst.config.ts` or delete entirely

---

## 6. Notes

### Grep Strategy for Verification

```bash
# Find all workspace references to a package
grep -r "@opencode-ai/slack" packages/
grep -r "@opencode-ai/enterprise" packages/
grep -r "@opencode-ai/function" packages/

# Find all imports from removed packages
grep -r "from.*packages/slack" packages/opencode/src/
```

### Zombie Code Detection

- Check for dynamic imports: `await import()`
- Check for reflection/eval usage
- Check for runtime configuration that loads modules

### Performance Impact

Removing packages can reduce:

- `bun install` time (fewer dependencies)
- Build time (fewer packages to compile)
- CI/CD time (fewer workflows to run)
- Bundle size (fewer unused exports)

---

## 7. Changes Already Made (2025-04-13)

### Provider System Cleanup (COMPLETE)

- Removed 19 AI provider packages from opencode
- Removed github-copilot plugin
- Removed github.ts command
- Removed @actions, @octokit dependencies
- Deleted 17 GitHub workflows
- Deleted script/github folder

### Next Steps

- Continue with Phase 1 recommendations above
- Verify all changes compile correctly
- Test core functionality remains working
