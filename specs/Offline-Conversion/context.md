# Context: OpenCode Offline Conversion Analysis

## Project Overview
**OpenCode** (disco-code) is an AI-powered coding agent with:
- CLI tool (packages/opencode) - main entry point
- Web UI (Solid.js SPA - packages/app)
- Desktop apps (Tauri & Electron variants)
- JavaScript SDK for programmatic access
- Extensive cloud infrastructure (Cloudflare, PlanetScale, AWS/SST)

## Online Integrations Identified

### 1. Cloud Infrastructure
- **Cloudflare Workers**: API endpoint (api.opencode.ai), Auth service (auth.opencode.ai)
- **Cloudflare R2 Buckets**: File storage for sessions and data
- **Durable Objects**: Real-time sync server (SyncServer)
- **PlanetScale MySQL**: Database for user data, sessions, projects
- **AWS via SST**: Infrastructure orchestration
- **Stripe**: Payment processing for enterprise features
- **Cloudflare KV**: Session storage for auth

### 2. AI Model Providers (23+ supported)
- **OpenAI**: GPT-4, GPT-3.5, etc.
- **Anthropic**: Claude models
- **Azure OpenAI**: Microsoft's hosted OpenAI
- **Google AI**: Gemini models
- **AWS Bedrock**: Amazon's model marketplace
- **Groq, Mistral, XAI, Cohere, DeepInfra, Cerebras, TogetherAI, Perplexity**
- **OpenRouter, GitLab AI, Venice AI**
- **Models fetched from https://models.dev** with cloud gateway

### 3. External Services & APIs
- **Web Search**: https://mcp.exa.ai (Exa AI MCP server)
- **Code Search**: https://mcp.exa.ai (same endpoint)
- **Telemetry**: PostHog (https://us.i.posthog.com) for analytics
- **Social Cards**: https://social-cards.sst.dev for PR sharing
- **Discord & Feishu/Lark**: Team integrations

### 4. Session Sharing & Sync
- Cloud-hosted share URLs (opncd.ai, opencode.ai)
- Durable Objects for real-time collaboration
- Account system with device flow authentication

### 5. Plugin System
- NPM-based plugin installation
- Plugin registry lookups
- MCP (Model Context Protocol) with OAuth flows

### 6. GitHub Integration
- GitHub App installation for PR comments
- Session links posted to PRs
- @actions/github, @octokit packages

## Data Sent to Cloud
- **Sessions & conversations**: Shared explicitly to opencode.ai
- **Model telemetry**: Download tracking via PostHog
- **User accounts**: Synced to account service
- **Credentials**: API keys for providers stored
- **Project metadata**: Via shares and sync features

## What Must Be Removed for Offline-Only

### Critical Removals (20+ components)
1. **Provider System** (packages/opencode/src/provider/)
   - All 23 @ai-sdk/* model providers
   - Force local models only (keep @ai-sdk/openai-compatible)

2. **Search Tools** (packages/opencode/src/tool/)
   - websearch.ts (Exa API integration)
   - codesearch.ts (Exa API integration)

3. **Session Sharing** (packages/opencode/src/share/)
   - share-next.ts cloud sync mechanism
   - Already has OPENCODE_DISABLE_SHARE=1 flag

4. **Account System** (packages/opencode/src/account/, cli/cmd/account.ts)
   - LoginCommand, LogoutCommand, SwitchCommand, OrgsCommand, ConsoleCommand
   - Device auth flow and token refresh

5. **Telemetry** (script/stats.ts)
   - PostHog analytics calls
   - OpenTelemetry traces

6. **Infrastructure** (infra/, sst.config.ts)
   - All Cloudflare/AWS resources
   - PlanetScale database (use existing SQLite)
   - Stripe payment processing

7. **Plugin System** (packages/opencode/src/mcp/)
   - OAuth flows for remote MCP servers
   - NPM plugin installation (keep stdio-based local)

8. **GitHub Integration** (packages/opencode/src/cli/cmd/github.ts)
   - Entire GitHub command suite
   - @actions/github, @octokit dependencies

9. **UI Components** (packages/app/, desktop/)
   - Share commands in session UI
   - Hardcoded external URLs (4 locations)
   - Auto-updater integration

### Dependencies to Remove
**Production packages** (~150-230 MB savings):
- sst (3.18.10) - infrastructure framework
- @aws-sdk/client-s3 - S3 storage
- @astrojs/cloudflare (12.6.3) - deployment adapter
- aws4fetch (1.0.20) - AWS request signing
- @cloudflare/workers-types - Worker types
- 23+ @ai-sdk/* provider packages

**Total removal**: 45-55% node_modules size reduction

## What Stays (Core Offline Functionality)

### Fully Functional Offline
- **CLI tool**: All local commands work
- **File operations**: read, write, edit, bash execution
- **Local analysis**: glob, grep, ls, tree navigation
- **SQLite database**: sessions, messages, projects
- **Git operations**: commit, diff, status
- **Built-in tools**: code editing, refactoring
- **Desktop/Web UIs**: without cloud features
- **JavaScript SDK**: programmatic access

### Existing Offline Support
The project already has several offline-compatibility flags:
- `OPENCODE_DISABLE_SHARE=1` - Disables session sharing
- `OPENCODE_PURE=1` - Disables plugins
- `OPENCODE_DISABLE_LSP_DOWNLOAD=1` - No auto LSP downloads
- `OPENCODE_DISABLE_MODELS_FETCH=1` - No network model fetch
- `OPENCODE_MODELS_PATH=/path/to/local.json` - Local models only

## Implementation Complexity Assessment

### Feasibility: HIGH ✅
- **Existing infrastructure**: Many offline flags already implemented
- **Modular design**: Components can be cleanly disabled
- **Local alternatives**: SQLite already supported, Drizzle ORM flexible
- **Build system**: Can exclude cloud components

### Risk Areas
- **Provider system**: 23 providers to remove, ensure local fallback works
- **UI dependencies**: Hardcoded URLs need replacement strategy
- **Build configuration**: May need significant package.json updates
- **Testing**: Need comprehensive offline validation

### Migration Path
1. **Database**: PlanetScale → SQLite (already supported)
2. **Storage**: R2/S3 → Local file system
3. **Auth**: Cloud auth → None (remove entirely)
4. **Models**: Cloud providers → Local JSON configuration
5. **Sync**: Durable Objects → None (remove real-time features)

## Key Files to Modify (Prioritized)

### High Priority (Core functionality)
1. `packages/opencode/src/provider/provider.ts` - Remove online providers
2. `packages/opencode/src/provider/models.ts` - Force local loading
3. `packages/opencode/src/share/share-next.ts` - Disable sharing
4. `packages/opencode/src/tool/websearch.ts` - Remove web search
5. `packages/opencode/src/tool/codesearch.ts` - Remove code search

### Medium Priority (User features)
6. `packages/opencode/src/cli/cmd/account.ts` - Remove auth commands
7. `packages/opencode/src/account/index.ts` - Disable auth flows
8. `script/stats.ts` - Remove PostHog
9. `packages/opencode/src/mcp/` - Remove OAuth
10. `packages/opencode/src/cli/cmd/github.ts` - Remove GitHub

### Low Priority (Infrastructure)
11. `packages/app/src/pages/session/use-session-commands.tsx` - Remove share UI
12. `packages/app/src/entry.tsx, layout.tsx, error.tsx, sidebar-items.tsx` - Replace URLs
13. `packages/desktop/src/menu.ts` - Update menu links
14. `infra/app.ts, console.ts, enterprise.ts, stage.ts` - Remove cloud infra
15. `sst.config.ts` - Remove deployment config

## Success Criteria
- **Build success**: No cloud dependencies in final build
- **Runtime isolation**: No network calls in offline mode
- **Model loading**: Local JSON models work via OPENCODE_MODELS_PATH
- **UI functionality**: All local features work, no broken links
- **Type safety**: All TypeScript checks pass
- **Test coverage**: Existing tests pass, new offline tests added

## Offline Distribution Strategy
For a fully offline distribution:
- **Single executable**: Per platform (Windows, macOS, Linux)
- **Bundled models**: Include local model configurations
- **No auto-updates**: Disable updater entirely
- **Self-contained**: All dependencies included
- **Privacy-focused**: No telemetry, no external calls

## Recommendations
1. **Start with flags**: Use existing OPENCODE_* flags to disable features
2. **Incremental removal**: Remove components in dependency order
3. **Test early**: Validate each phase doesn't break local functionality
4. **Document changes**: Keep track of all modifications for rollback
5. **Consider branding**: Rename project to reflect offline-only nature

## Open Questions
1. Which local models to support (Ollama, LM Studio, etc.)?
2. How to handle UI help/feedback without external sites?
3. Should project name change to indicate offline-only?
4. Any specific offline testing scenarios needed?