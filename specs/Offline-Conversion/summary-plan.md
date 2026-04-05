# Summary Plan: Convert OpenCode to Offline-Only Version

## Overview
Transform the OpenCode project from a cloud-connected AI coding assistant to a fully offline version that works exclusively with local models, removing all online integrations, telemetry, and cloud dependencies.

## Key Objectives
- **Remove Online Features**: Eliminate all cloud model providers (OpenAI, Claude, etc.), telemetry (PostHog), sharing/sync, external APIs, and cloud infrastructure
- **Preserve Core Functionality**: Maintain CLI tools, local file operations, SQLite database, Git integration, and editing capabilities
- **Ensure Privacy**: No data transmission to external services, complete offline operation
- **Leverage Existing Flags**: Use built-in offline flags where possible, add new master offline mode

## High-Level Phases
1. **Documentation & Setup** (Current): Create analysis files and document all changes
2. **Provider Removal**: Strip out 23+ online AI model providers, keep local-compatible only
3. **Tool Cleanup**: Remove web search and code search tools
4. **Sharing & Auth Removal**: Disable session sharing, accounts, and authentication
5. **Telemetry Elimination**: Remove analytics and tracking
6. **UI Updates**: Clean up cloud features from interfaces
7. **Infrastructure Removal**: Delete cloud deployment configs and services
8. **Build & Test**: Update configurations and validate offline operation

## Success Criteria
- Project builds without cloud dependencies
- No network calls in offline mode
- Local models load from OPENCODE_MODELS_PATH
- All core features work offline
- Type checking and tests pass

## Estimated Effort
- **Total Time**: 22-31 hours across 12 phases
- **Risk Level**: Medium (many existing offline flags reduce complexity)
- **Dependencies**: 45-55% reduction in node_modules size possible

## Next Steps
Execute Phase 2: Provider System Removal to begin core modifications.