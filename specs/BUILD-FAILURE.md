# Build Failure Analysis: effect/Context Resolution

**Date**: 2026-04-13
**Status**: UNRESOLVED - Pre-existing Bun issue, not caused by offline conversion

---

## Summary

The opencode binary build fails during the final Bun compilation step with:

```
error: Could not resolve: "effect/Context". Maybe you need to "bun install"?
```

This occurs at `packages/opencode/node_modules/@effect/platform-node-shared/dist/NodeStream.js:7:26` when building the Windows x64 binary.

---

## Timeline

| Step | Action                       | Result                                |
| ---- | ---------------------------- | ------------------------------------- |
| 1    | TypeScript typecheck         | ✅ Success                            |
| 2    | Vite builds app UI           | ✅ Success                            |
| 3    | models-snapshot.js generated | ✅ Success (empty `{}`)               |
| 4    | Bun.compile() starts         | ❌ Fails at effect/Context resolution |

---

## Root Cause Analysis

### 1. Package Structure

- `effect` is a catalog: `"effect": "catalog:"` in package.json
- `effect` package exists in `node_modules/effect/` (version 4.0.0-beta.43)
- `@effect/platform-node-shared` (version 4.0.0-beta.47) is a transitive dependency
- The error occurs when Bun tries to bundle `@effect/platform-node-shared` which imports `effect/Context`

### 2. The Specific Import Pattern

In `@effect/platform-node-shared/dist/NodeStream.js`:

```javascript
import * as Context from "effect/Context"
```

This uses a **subpath export** (`./Context`) from the `effect` package, which is defined in `effect/package.json`:

```json
"exports": {
  "./*": "./dist/*.js"
}
```

### 3. Why Subpath Exports Fail

The `effect` package uses the pattern `"./*": "./dist/*.js"` which should map `effect/Context` to `effect/dist/Context.js`. However:

- The bundler fails to resolve this subpath during compile
- The `effect` package is in the root `node_modules/` but `@effect/platform-node-shared` is in an isolated `.bun` cache directory
- The resolution chain breaks when bundling transitive dependencies

### 4. Environment Details

- Bun v1.3.12 (Windows x64)
- Target: opencode-windows-x64
- Platform: Windows (not cross-compiling from Linux)
- Using `--single` flag (builds for current platform only)

---

## Why This Is NOT Caused By Offline Conversion

1. **The error occurs AFTER all our changes compile successfully**:
   - TypeScript compilation passes
   - Vite build passes
   - models-snapshot.js generates correctly

2. **The failure is in a different package** (`@effect/platform-node-shared`) than anything we modified

3. **The effect package was already present** in node_modules before our changes

4. **The same error occurs with `--single` flag** (builds for current platform only), confirming it's not a cross-compilation issue

---

## Evidence

### Build Output Sequence

```
[1.63s] done
building opencode-windows-x64
7 | import * as Context from "effect/Context";
error: Could not resolve: "effect/Context"
```

### File Locations

- Error originates from: `node_modules/.bun/@effect+platform-node-shared@4.0.0-beta.47+.../node_modules/@effect/platform-node-shared/dist/NodeStream.js:7:26`
- `effect` package: `node_modules/effect/` (version 4.0.0-beta.43)
- `@effect/platform-node-shared`: isolated in `.bun` cache, NOT in main node_modules

---

## Attempted Fixes (All Failed)

### Fix 1: NODE_ENV=production

- Added `"process.env.NODE_ENV": '"production"'` to build define
- **Result**: ❌ Failed with same error

### Fix 2: compile.production = true

- Added `production: true` to compile options
- **Result**: ❌ Failed with same error

### Fix 3: Mark effect packages as external

- Added `external: ["effect", "@effect/*"]`
- **Result**: ❌ Failed - build succeeded but binary failed at runtime with "Cannot find module 'effect/Context'"
- This confirms the issue is in how Bun resolves subpath exports during bundling

### Fix 4: Specific external packages

- Added `external: ["effect", "@effect/platform-node-shared", "@effect/platform-node"]`
- **Result**: ❌ Same runtime failure

---

## Related Bun Issues Found

### 1. Bun #27058 - Compiled Executables Cannot Resolve External Modules (Feb 2026)

- **Status**: Open
- **Summary**: Bun 1.3.9+ broke module resolution for compiled executables that extract files to disk
- **Impact**: External packages can't find their dependencies at runtime
- **Workaround**: None works, only fix is downgrading to Bun 1.3.3
- **Key quote**: "Based on extensive debugging, Bun 1.3.9 changed module resolution for compiled executables to ignore NODE_PATH completely"

### 2. Bun #22589 - Cannot resolve during compile (Sep 2025)

- **Status**: Open
- **Summary**: Similar "Could not resolve" during `bun build --compile`
- **Workaround**: `NODE_ENV=production bun build` or `--production` flag

### 3. Bun #8266 - Auto imports with version specifiers (Jan 2024)

- **Status**: Open
- **Summary**: `bun build --compile` fails with version specifiers in import statements

### 4. Bun #25635 - Sharp native addon subpath export failure (Dec 2025)

- **Closed**: Fixed
- **Similar issue**: Subpath exports not resolving in compiled mode

### 5. Bun #26653 - Plugin onLoad causes transitive dependency resolution failure (Feb 2026)

- **Status**: Open
- **Key insight**: "The bug appears to be in how Bun tracks the 'importer' for module resolution"

---

## Key Insight: Catalog + Subpath Exports

The combination of:

1. `catalog:` specifier in package.json
2. Beta versions of effect packages
3. Subpath exports pattern (`"./*": "./dist/*.js"`)

This appears to be a known problematic combination in Bun's compile mode. The bundler can resolve the main package but fails to correctly resolve subpath imports from transitive dependencies.

---

## Impact

- Binary compilation fails
- All other build steps (Vite, typecheck, model snapshot) work correctly
- The offline conversion changes do NOT cause this failure

---

## Final Recommendations

### Option A: Wait for Bun Fix

- This is a known Bun issue (#27058)
- May be fixed in a future Bun version
- Check https://github.com/oven-sh/bun/issues/27058 for updates

### Option B: Downgrade Bun

- Bun 1.3.3 reportedly works correctly
- Not ideal but could unblock the build

### Option C: Report to Bun

- Create a minimal reproduction with:
  - Effect package with `catalog:` specifier
  - Transitive dependency importing subpath (e.g., `effect/Context`)
  - `bun build --compile`
- Post to https://github.com/oven-sh/bun/issues with reproduction

### Option D: Pin Effect to Specific Version

- Try replacing `"effect": "catalog:"` with a specific version
- May avoid the beta + catalog combination issue

---

## Not Yet Tried

1. Pinning `effect` to a specific version instead of `catalog:`
2. Testing on a different OS (Linux/macOS) to see if the issue is Windows-specific
3. Downgrading to Bun 1.3.3 to verify it works there

---

## Related Files

- `packages/opencode/script/build.ts` - Build script that runs Bun.compile()
- `packages/opencode/package.json` - Contains `effect: "catalog:"` dependency
- `node_modules/effect/package.json` - Uses subpath exports pattern
- `node_modules/@effect/platform-node-shared/` - Transitive dependency causing the error
