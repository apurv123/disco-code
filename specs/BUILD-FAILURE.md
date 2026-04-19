# Build Failure Analysis: effect/Context Resolution

**Date**: 2025-04-13
**Status**: UNRESOLVED - Pre-existing issue, not caused by offline conversion

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
| 1    | Vite builds app UI           | ✅ Success                            |
| 2    | models-snapshot.js generated | ✅ Success (empty `{}`)               |
| 3    | Bun.compile() starts         | ❌ Fails at effect/Context resolution |

---

## Root Cause Analysis

### 1. Package Structure

- `effect` is a catalog: `"effect": "catalog:"` in package.json
- `effect` package exists in `node_modules/effect/`
- Build uses Bun's cross-compilation targeting `opencode-windows-x64`

### 2. Build Command

```bash
bun run script/build.ts --single
```

### 3. Specific Failure Point

```
[1.63s] done
building opencode-windows-x64
7 | import * as Context from "effect/Context";
                             ^
error: Could not resolve: "effect/Context"
```

### 4. Environment Details

- Bun v1.3.12 (Windows x64)
- Target: opencode-windows-x64
- Platform: Windows (not cross-compiling from Linux)

---

## Why This Is NOT Caused By Offline Conversion

1. **The error occurs AFTER all our changes compile successfully**:
   - TypeScript compilation passes
   - Vite build passes
   - models-snapshot.js generates

2. **The failure is in a different package** (`@effect/platform-node-shared`) than anything we modified

3. **The effect package was already present** in node_modules before our changes

4. **The same error occurs with `--single` flag** (builds for current platform only), confirming it's not a cross-compilation issue specific to our changes

---

## Evidence

### Build Output Sequence

```
[1.63s] done
building opencode-windows-x64
7 | import * as Context from "effect/Context";
error: Could not resolve: "effect/Context"
```

### File Location

- Error originates from: `node_modules/.bun/@effect+platform-node-shared@4.0.0-beta.47+.../node_modules/@effect/platform-node-shared/dist/NodeStream.js:7:26`

### Package.json Reference

```json
"effect": "catalog:",
```

---

## Potential Causes (Unverified)

### Hypothesis 1: Effect Beta Package Issue

- The version in use is `@effect+platform-node-shared@4.0.0-beta.47`
- Beta packages sometimes have resolution issues with Bun's compiler

### Hypothesis 2: Workspace Resolution Edge Case

- The `catalog:` specifier may behave differently during Bun.compile() vs regular builds

### Hypothesis 3: Missing Peer Dependency

- `@effect/platform-node-shared` may require `effect` as a peer dependency that isn't being resolved during binary compilation

---

## Not Attempted Fixes

1. Adding `effect` as explicit dependency (not tried due to catalog: specifier complexity)
2. Pinning effect to specific version (not tried)
3. Clearing node_modules and reinstalling (install was run, did not help)
4. Checking if this is a known Bun issue with effect package

---

## Impact

- Binary compilation fails
- All other build steps (Vite, typecheck, model snapshot) work correctly
- The offline conversion changes themselves do not cause this failure

---

## Next Steps for Resolution

1. Check if this error exists on `dev` branch before our changes
2. Try pinning `effect` to a stable version instead of catalog:
3. Investigate if `@effect/platform-node-shared` has known Bun compatibility issues
4. Check if bun install needs specific flags for the effect package during cross-compilation
