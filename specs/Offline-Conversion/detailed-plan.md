# Detailed Plan: Convert OpenCode to Offline-Only Version

## Overview
Remove all online model integrations, telemetry, cloud sharing, external services, and cloud infrastructure while preserving core local functionality. Leverage existing offline flags and modify key components to ensure no data leaks or external dependencies.

## Detailed Steps by Phase

### Phase 1: Documentation & Analysis (Current)
1. Create specs/Offline-Conversion/ folder ✅
2. Create specs/Offline-Conversion/summary-plan.md with high-level overview ✅
3. Create specs/Offline-Conversion/detailed-plan.md with full step-by-step plan ✅
4. Create specs/Offline-Conversion/context.md with comprehensive findings and analysis
5. Document all online features, dependencies, and removal targets
6. Identify existing offline flags and their coverage

### Phase 2: Provider System Removal
1. Modify packages/opencode/src/provider/provider.ts to remove all online model providers (keep only openai-compatible for local models)
2. Update packages/opencode/src/provider/models.ts to force local-only model loading
3. Remove 23+ @ai-sdk provider packages from dependencies
4. Set OPENCODE_DISABLE_MODELS_FETCH=1 and OPENCODE_MODELS_PATH to local JSON

### Phase 3: Tool System Cleanup
1. Remove packages/opencode/src/tool/websearch.ts (Exa API integration)
2. Remove packages/opencode/src/tool/codesearch.ts (Exa API integration)
3. Update tool registry to exclude online tools
4. *Depends on Phase 2*

### Phase 4: Sharing & Sync Removal
1. Enforce OPENCODE_DISABLE_SHARE=1 in all code paths
2. Modify packages/opencode/src/share/share-next.ts to disable sharing functionality
3. Remove cloud sync indicators from UIs
4. *Depends on Phase 1*

### Phase 5: Account & Authentication Removal
1. Remove packages/opencode/src/cli/cmd/account.ts commands (login, logout, switch, orgs, console)
2. Disable packages/opencode/src/account/ authentication flows
3. Remove device auth and token refresh mechanisms
4. *Depends on Phase 2*

### Phase 6: Telemetry & Analytics Removal
1. Remove PostHog integration from script/stats.ts
2. Disable OpenTelemetry traces
3. Set OPENCODE_DISABLE_TELEMETRY=1
4. *Depends on Phase 1*

### Phase 7: MCP & Plugin System Cleanup
1. Remove OAuth flows from packages/opencode/src/mcp/
2. Disable NPM plugin installation and registry lookups
3. Set OPENCODE_PURE=1 to disable plugins
4. Keep stdio-based local MCP support
5. *Depends on Phase 3*

### Phase 8: GitHub Integration Removal
1. Remove packages/opencode/src/cli/cmd/github.ts entirely
2. Remove @actions/github, @octokit packages
3. Disable PR posting and GitHub App features
4. *Depends on Phase 5*

### Phase 9: UI Component Updates
1. Remove share commands from packages/app/src/pages/session/use-session-commands.tsx
2. Replace hardcoded URLs in packages/app/src/entry.tsx, layout.tsx, error.tsx, sidebar-items.tsx
3. Update packages/desktop/src/menu.ts to remove external help links
4. Disable auto-updater in desktop apps
5. *Depends on Phase 4*

### Phase 10: Infrastructure Removal
1. Remove infra/app.ts, infra/console.ts, infra/enterprise.ts, infra/stage.ts
2. Remove sst.config.ts
3. Remove packages/function/ Cloudflare Workers code
4. Remove cloud-related packages: sst, @aws-sdk/client-s3, @astrojs/cloudflare, aws4fetch
5. *Depends on Phase 9*

### Phase 11: Build Configuration Updates
1. Update root package.json and subpackage package.json files
2. Modify build scripts to exclude cloud components
3. Add offline mode environment variables
4. *Depends on Phase 10*

### Phase 12: Testing & Validation
1. Run bun typecheck across all packages
2. Execute test suites for modified components
3. Build offline distribution
4. Test local model integration
5. *Depends on Phase 11*

## Relevant Files
- packages/opencode/src/provider/provider.ts — Remove online providers
- packages/opencode/src/provider/models.ts — Force local models
- packages/opencode/src/share/share-next.ts — Disable sharing
- packages/opencode/src/tool/websearch.ts — Remove web search
- packages/opencode/src/tool/codesearch.ts — Remove code search
- packages/opencode/src/cli/cmd/account.ts — Remove auth commands
- packages/opencode/src/account/index.ts — Disable auth flows
- script/stats.ts — Remove telemetry
- packages/app/src/pages/session/use-session-commands.tsx — Remove share UI
- packages/app/src/entry.tsx, layout.tsx, error.tsx, sidebar-items.tsx — Replace URLs
- packages/desktop/src/menu.ts — Update menu links
- infra/app.ts, console.ts, enterprise.ts, stage.ts — Remove cloud infra
- sst.config.ts — Remove deployment config
- packages/function/ — Remove Cloudflare code

## Verification Steps
1. Run `bun typecheck` from each package directory to ensure no type errors
2. Execute `bun test` in packages with test suites (opencode, app, etc.)
3. Build the project with `bun run build` and verify no cloud dependencies
4. Test CLI with offline flags: OPENCODE_DISABLE_SHARE=1 OPENCODE_PURE=1 OPENCODE_DISABLE_MODELS_FETCH=1
5. Verify no network calls in offline mode using network monitoring
6. Test local model loading with OPENCODE_MODELS_PATH
7. Run static analysis with tools like eslint or tsc --noEmit

## Decisions Made
- Scope includes removal of all cloud features, online model providers, telemetry, and external API calls
- Preserve core local functionality: CLI, file operations, local analysis, SQLite, Git, editing tools, UIs
- Use existing offline flags where possible, add new OPENCODE_OFFLINE_MODE=1 master flag
- Keep openai-compatible provider for local Ollama/servers
- Remove all hardcoded external URLs, replace with local or disable
- Eliminate all dependencies on Cloudflare, AWS, PlanetScale, Stripe

## Further Considerations
1. What local models should be bundled or supported (Ollama, LM Studio, etc.)?
2. How to handle UI feedback/help links without external sites?
3. Should the project name be changed to reflect offline-only nature?
4. Any specific testing scenarios for offline functionality?

## Time Estimates
- Phase 1: 2-3 hours (documentation)
- Phase 2: 4-5 hours (provider removal)
- Phase 3: 2-3 hours (tool cleanup)
- Phase 4: 2-3 hours (sharing removal)
- Phase 5: 3-4 hours (auth removal)
- Phase 6: 1-2 hours (telemetry)
- Phase 7: 2-3 hours (MCP/plugins)
- Phase 8: 1-2 hours (GitHub)
- Phase 9: 2-3 hours (UI updates)
- Phase 10: 1-2 hours (infra removal)
- Phase 11: 2-3 hours (build config)
- Phase 12: 3-4 hours (testing)
- **Total: 22-31 hours**