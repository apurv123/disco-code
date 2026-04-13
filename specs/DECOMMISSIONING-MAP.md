# DECOMMISSIONING_MAP.md

**Created**: 2025-04-13
**Updated**: 2025-04-13
**Purpose**: Comprehensive structural audit of the disco-code monorepo to identify, isolate, and map dependency footprints for safe staged decommissioning.

---

## 1. Executive Summary

| Deliverable ID | Package Name     | Purpose                                | Status   | Complexity |
| -------------- | ---------------- | -------------------------------------- | -------- | ---------- |
| CORE-001       | opencode         | CLI tool - main entry point            | **KEEP** | N/A        |
| CORE-002       | app              | Web UI (Solid.js SPA)                  | **KEEP** | N/A        |
| CORE-003       | ui               | Shared UI components                   | **KEEP** | N/A        |
| CORE-004       | util             | Shared utilities (zod, errors)         | **KEEP** | N/A        |
| CORE-005       | sdk              | JS/TS SDK for programmatic access      | **KEEP** | N/A        |
| CORE-006       | plugin           | Plugin system interface                | **KEEP** | N/A        |
| CORE-007       | script           | Build scripts utility                  | **KEEP** | N/A        |
| LEGACY-001     | web              | Marketing website (Astro + Cloudflare) | **TBD**  | 3          |
| LEGACY-002     | function         | Cloudflare Workers API                 | **TBD**  | 4          |
| LEGACY-003     | enterprise       | Enterprise dashboard                   | **TBD**  | 4          |
| LEGACY-004     | slack            | Slack bot integration                  | **TBD**  | 2          |
| LEGACY-005     | console/\*       | Console infrastructure (5 packages)    | **TBD**  | 5          |
| LEGACY-006     | desktop-electron | Electron desktop app                   | **TBD**  | 3          |
| LEGACY-007     | desktop          | Tauri desktop app                      | **KEEP** | N/A        |
| LEGACY-008     | storybook        | Component documentation                | **TBD**  | 1          |
| LEGACY-009     | infra/\*         | SST cloud infrastructure (5 files)     | **TBD**  | 4          |

---

## 2. Deliverable Audit Template

For each deliverable, the following structure applies:

```
### ID: [Name]

**Definition**: [One sentence defining what this is]

**Features**:
- [Feature 1]
- [Feature 2]
- [Feature 3]

**Why Keep**:
- [Reason 1 with data points]
- [Reason 2 with data points]

**Why Remove**:
- [Reason 1 with data points]
- [Reason 2 with data points]

**Data Points**:
- Lines of code: [X]
- Unique dependencies: [X]
- GitHub workflows: [X]
- Other packages depending on this: [X]
- Shared assets: [list]

---

**Removal Decision**: [BLANK - for decision maker]
```

---

## 3. Detailed Deliverables

### CORE-001: opencode (CLI Tool)

**Definition**: Main CLI entry point for the opencode AI coding assistant.

**Features**:

- Command-line interface with subcommands (chat, edit, pr, mcp, etc.)
- LLM provider integration system (19+ providers removed in offline conversion)
- Tool system for file operations (read, edit, write, glob, grep, bash, task)
- Session management with persistence
- LSP server for code intelligence
- Plugin system for extensions
- TUI (Terminal UI) with Solid.js webview

**Why Keep**:

- Core product - primary way users interact with opencode
- Contains all AI model integration logic
- Houses the provider system (now local-only with openai-compatible)
- No viable alternative for CLI functionality

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~50,000+ (largest package)
- Unique dependencies: ~90
- GitHub workflows: 3 (opencode.yml, typecheck.yml, test.yml)
- Other packages depending on this: 0 (root package)
- Shared assets: src/cli, src/session, src/provider, src/tool, src/effect, src/storage

---

### CORE-002: app (Web UI)

**Definition**: Solid.js SPA web interface for opencode.

**Features**:

- Interactive chat interface
- Model selection dialog
- Provider connection management
- Session history
- File tree browser
- Terminal emulator
- Settings panel

**Why Keep**:

- Primary web interface for non-CLI users
- Used by desktop app as embedded webview
- Contains UI for model/provider management

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~15,000
- Unique dependencies: ~20
- GitHub workflows: 2 (beta.yml, publish.yml)
- Other packages depending on this: desktop, desktop-electron, enterprise
- Shared assets: src/pages, src/components, src/stores

---

### CORE-003: ui (Shared UI Components)

**Definition**: Reusable UI component library built with Solid.js.

**Features**:

- Button, Input, Dialog, Dropdown components
- Theme system with dark/light modes
- i18n (internationalization) support
- Icon system
- Form components
- Layout components

**Why Keep**:

- Shared across all UI packages
- Critical dependency for app, desktop, enterprise
- Well-designed, maintained component library

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~10,000
- Unique dependencies: ~15
- GitHub workflows: 1 (storybook.yml via storybook package)
- Other packages depending on this: app, desktop, desktop-electron, enterprise, storybook
- Shared assets: src/components, src/hooks, src/context, src/i18n, src/styles

---

### CORE-004: util (Shared Utilities)

**Definition**: Common utilities, zod schemas, and error handling primitives.

**Features**:

- Zod schemas for validation
- Error handling utilities
- Common TypeScript types
- Helper functions

**Why Keep**:

- Used by EVERY package in the monorepo
- No viable alternative
- Foundation for type safety

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~5,000
- Unique dependencies: Minimal (base package)
- GitHub workflows: 0
- Other packages depending on this: ALL packages
- Shared assets: src/\*.ts

---

### CORE-005: sdk (JavaScript SDK)

**Definition**: TypeScript/JavaScript SDK for programmatic opencode access.

**Features**:

- Client library for API access
- Type definitions from API
- Server-side SDK
- Generated from OpenAPI spec

**Why Keep**:

- Enables programmatic usage
- Used by external tools
- Published to npm

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~3,000
- Unique dependencies: Minimal
- GitHub workflows: 1 (publish.yml for npm)
- Other packages depending on this: function, slack, console/\*
- Shared assets: js/src/_, js/script/_

---

### CORE-006: plugin (Plugin System)

**Definition**: Plugin interface definitions for opencode extensions.

**Features**:

- Plugin manifest schema
- Tool interface definitions
- TUI component interfaces
- Plugin loading system

**Why Keep**:

- Extensibility architecture
- Used by opencode for dynamic features

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~1,000
- Unique dependencies: 1 (@opencode-ai/sdk)
- GitHub workflows: 0
- Other packages depending on this: opencode
- Shared assets: src/index.ts, src/tool.ts, src/tui.ts

---

### CORE-007: script (Build Scripts)

**Definition**: Shared build and release scripts for the monorepo.

**Features**:

- Version management
- Release automation
- Build utilities

**Why Keep**:

- Used by opencode for builds
- Shared build infrastructure

**Why Remove**:

- N/A - Core product

**Data Points**:

- Lines of code: ~500
- Unique dependencies: Minimal
- GitHub workflows: 0
- Other packages depending on this: opencode
- Shared assets: src/index.ts

---

### LEGACY-001: web (Marketing Website)

**Definition**: Marketing landing page built with Astro and Cloudflare Pages.

**Features**:

- Static marketing pages
- SEO optimization
- Cloudflare Pages adapter
- Performance optimized

**Why Keep**:

- Provides public-facing website
- SEO value

**Why Remove**:

- Not core functionality
- Can be hosted elsewhere
- Reduces maintenance burden

**Data Points**:

- Lines of code: ~2,000
- Unique dependencies: ~20
- GitHub workflows: 1 (deploy.yml - removed)
- Other packages depending on this: 0
- Shared assets: src/pages, src/layouts, src/components

**Removal Impact**: LOW - No code dependencies on opencode, just references the product.

---

### LEGACY-002: function (Cloudflare Workers)

**Definition**: Serverless API functions for auth, sessions, and backend logic.

**Features**:

- OAuth authentication
- Session management
- User management endpoints
- API rate limiting
- Cloudflare Workers deployment

**Why Keep**:

- Handles authentication
- Session persistence
- Core backend functionality

**Why Remove**:

- Cloud-specific (vendor lock-in)
- Can be replaced with self-hosted alternative
- Part of online/cloud features being removed

**Data Points**:

- Lines of code: ~5,000
- Unique dependencies: ~15 (@octokit, hono, zod, etc.)
- GitHub workflows: 0 (SST deployment)
- Other packages depending on this: enterprise, console/\*
- Shared assets: src/\*, wrangler.toml

**Removal Impact**: HIGH - Contains auth logic, session handling. Requires extraction of shared code first.

---

### LEGACY-003: enterprise (Enterprise Dashboard)

**Definition**: Enterprise billing and admin dashboard.

**Features**:

- Billing management UI
- Admin controls
- User management
- Usage analytics
- SolidJS Start framework

**Why Keep**:

- Enterprise customers need this
- Revenue-critical functionality

**Why Remove**:

- Part of cloud/online services
- Can be deprecated with online features
- Build currently fails (pre-existing issue with SolidJS Start)

**Data Points**:

- Lines of code: ~8,000
- Unique dependencies: ~25
- GitHub workflows: 0 (SST deployment)
- Other packages depending on this: 0
- Shared assets: src/\*, vite.config.ts

**Removal Impact**: MEDIUM - Uses UI components but business logic is isolated. Build currently broken.

---

### LEGACY-004: slack (Slack Bot)

**Definition**: Slack integration for team notifications.

**Features**:

- Slack bot integration
- Team notifications
- Slash commands
- OAuth flow with Slack

**Why Keep**:

- Useful for teams using Slack
- Active integration

**Why Remove**:

- Online/cloud feature
- Fully isolated package
- Easy to remove

**Data Points**:

- Lines of code: ~1,000
- Unique dependencies: ~5 (@slack/bolt)
- GitHub workflows: 0
- Other packages depending on this: 0
- Shared assets: src/\*

**Removal Impact**: LOW - Fully isolated, easy to remove.

---

### LEGACY-005: console/\* (Console Infrastructure)

**Definition**: Internal admin console (5 packages).

**Packages**:

- console/core - Core services
- console/app - Console UI
- console/resource - Resource management
- console/mail - Email service
- console/function - Serverless functions

**Features**:

- Internal admin tools
- Email sending
- Resource tracking
- User management
- Billing operations

**Why Keep**:

- Internal operations required
- Critical for business operations

**Why Remove**:

- Entirely internal/cloud infrastructure
- Not customer-facing
- Part of online services being removed

**Data Points**:

- Lines of code: ~20,000 (5 packages)
- Unique dependencies: ~30
- GitHub workflows: 0 (SST deployment)
- Other packages depending on this: function, enterprise
- Shared assets: console/\*

**Removal Impact**: CRITICAL - Deeply integrated with cloud infrastructure. Requires full infrastructure review.

---

### LEGACY-006: desktop-electron (Electron App)

**Definition**: Electron-based desktop application (deprecated).

**Features**:

- Cross-platform desktop app
- Electron runtime
- Shared web UI with app package

**Why Keep**:

- None - deprecated in favor of Tauri

**Why Remove**:

- Deprecated (Tauri version is primary)
- Maintenance burden
- Duplicate functionality

**Data Points**:

- Lines of code: ~3,000
- Unique dependencies: ~15
- GitHub workflows: 0
- Other packages depending on this: 0
- Shared assets: src/main, src/preload, src/renderer

**Removal Impact**: LOW - Can be deprecated, keep Tauri version as primary.

---

### LEGACY-007: desktop (Tauri Desktop App)

**Definition**: Primary desktop application built with Tauri.

**Features**:

- Native desktop app
- Tauri v2 runtime
- System tray integration
- File system access
- Cross-platform

**Why Keep**:

- Primary desktop app
- Works offline (matches offline conversion goal)
- Active development

**Why Remove**:

- N/A - Keep

**Data Points**:

- Lines of code: ~4,000
- Unique dependencies: ~20
- GitHub workflows: 1 (publish-vscode.yml)
- Other packages depending on this: 0
- Shared assets: src-tauri, src/main.tsx

---

### LEGACY-008: storybook (Component Documentation)

**Definition**: Storybook for UI component development and documentation.

**Features**:

- Component playground
- Documentation generation
- Visual testing
- Component library browsing

**Why Keep**:

- Useful for development
- UI component documentation

**Why Remove**:

- Not core functionality
- Can be removed to reduce CI burden
- Package can be kept without workflow

**Data Points**:

- Lines of code: N/A (config only)
- Unique dependencies: ~10
- GitHub workflows: 1 (storybook.yml)
- Other packages depending on this: 0
- Shared assets: .storybook, src/stories

**Removal Impact**: LOW - Fully isolated, can remove workflow but keep package.

---

### LEGACY-009: infra/\* (SST Infrastructure)

**Definition**: Cloud infrastructure definitions using SST.

**Files**:

- infra/app.ts - Main app infrastructure
- infra/console.ts - Console infrastructure
- infra/enterprise.ts - Enterprise infrastructure
- infra/secret.ts - Secrets management
- infra/stage.ts - Stage configuration

**Features**:

- Cloudflare Workers deployment
- AWS/R2 storage
- PlanetScale database
- Secret management
- Staging environments

**Why Keep**:

- Controls all cloud deployments
- Required for online services

**Why Remove**:

- Entire infrastructure for online services
- Not needed for offline-only version
- Can be documented and preserved elsewhere if needed

**Data Points**:

- Lines of code: ~5,000 (5 files)
- Unique dependencies: ~10
- GitHub workflows: 0
- Other packages depending on this: function, enterprise, console/\*, web
- Shared assets: infra/\*, sst.config.ts

**Removal Impact**: CRITICAL - Controls all cloud deployments. Document env vars before removal.

---

## 4. Dependency Graph

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
                            │   (Core Utils)   │   │   (Core Utils)   │
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
- ⚠️ = Legacy/To Be Decided

---

## 5. Risk Assessment & Removal Complexity

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

## 6. Recommended Removal Order

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

## 7. Changes Already Made (2025-04-13)

### Provider System Cleanup (COMPLETE)

- Removed 19 AI provider packages from opencode
- Removed github-copilot plugin
- Removed github.ts command
- Removed @actions, @octokit dependencies
- Deleted 17 GitHub workflows
- Deleted script/github folder

### Models.dev Offline Conversion (COMPLETE)

- Removed all fetch logic from models.ts
- Removed auto-refresh on startup
- Removed OPENCODE_MODELS_URL flag
- Removed ModelId schema reference in config.ts
- Updated build.ts to use empty snapshot `{}`

### Next Steps

- Continue with Phase 1 recommendations above
- Verify all changes compile correctly
- Test core functionality remains working
- Resolve build-failure.md (effect/Context resolution issue - pre-existing)
