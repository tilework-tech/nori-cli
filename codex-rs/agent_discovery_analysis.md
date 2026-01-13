# Agent Discovery Startup Delay Analysis

**Branch:** `claude/improve-agent-discovery-3xTrq`
**Date:** 2026-01-13
**Issue:** Long delay when starting a new chat with the same model as the previous session

## Executive Summary

Instrumentation of the agent discovery code reveals that **the cache prewarming is working correctly** and is **not the cause of the observed delay**. The caches initialize in ~5ms and are fully warmed before any agent operations occur. A ~4 second gap exists between cache warming completion and agent list queries, suggesting the delay originates elsewhere in the TUI startup path.

## Changes on This Branch

The commit `29d8fa5ac` introduced:

1. **Package Manager Cache** (`PACKAGE_MANAGER_CACHE`)
   - Caches result of `bun --version` detection
   - Prewarmed via `prewarm_package_manager_cache()`

2. **Agent Readiness Detection** (`AgentReadiness` enum)
   - States: `Ready` (in PATH), `Cached` (in npm/bun cache), `RequiresDownload`
   - Added `readiness` field to `AcpAgentInfo`

3. **Readiness Cache** (`READINESS_CACHE`)
   - Combines PATH lookup with package cache detection
   - Prewarmed via `prewarm_readiness_cache()`

4. **TUI Prewarming** (in `tui/src/lib.rs`)
   - Background thread warms all three caches sequentially at startup

## Hypothesis (Initial)

The initial hypothesis was that `get_agent_readiness()` is called before the background cache prewarming completes, triggering lazy initialization of the caches which spawns a `bun --version` subprocess on the main thread, blocking agent startup.

## Instrumentation Added

Tracing instrumentation was added to:
- `prewarm_package_manager_cache()`
- `prewarm_installation_cache()`
- `prewarm_readiness_cache()`
- `detect_package_manager_uncached()` (subprocess timing)
- `detect_preferred_package_manager()` (cache hit/miss)
- `init_readiness_cache()`
- `get_agent_readiness()` (cache hit/miss)
- `AcpAgentInfo::from_agent()`
- `list_available_agents()`
- Background prewarming thread in TUI

## Test Results

### Test Command
```bash
cargo t -p tui-pty-e2e --test agent_switching test_different_agents_different_subprocesses
```

### Cache Warming Timings (Session 1 - mock-model)

| Operation | Duration | Notes |
|-----------|----------|-------|
| `prewarm_package_manager_cache()` | 2.15ms | Includes `bun --version` subprocess (2.02ms) |
| `prewarm_installation_cache()` | 3.03ms | 3× `which` commands (~1ms each) |
| `prewarm_readiness_cache()` | 58µs | Uses cached installation data |
| **Total Prewarming** | **~5.3ms** | |

### Cache Warming Timings (Session 2 - mock-model-alt)

| Operation | Duration | Notes |
|-----------|----------|-------|
| `prewarm_package_manager_cache()` | 2.68ms | Includes `bun --version` subprocess (2.51ms) |
| `prewarm_installation_cache()` | 3.74ms | 3× `which` commands |
| `prewarm_readiness_cache()` | 77µs | Uses cached installation data |
| **Total Prewarming** | **~6.5ms** | |

### Cache Usage When Queried

| Operation | Cache Status | Duration |
|-----------|--------------|----------|
| `list_available_agents()` | All caches warm | 164-252µs |
| `get_agent_readiness(claude-code)` | `cache_initialized=true` | 11-16µs |
| `get_agent_readiness(codex)` | `cache_initialized=true` | 5-13µs |
| `get_agent_readiness(gemini)` | `cache_initialized=true` | 5-9µs |

### Timeline Analysis

**Session 1 (mock-model):**
```
20:42:35.954791Z - Background thread starts prewarming
20:42:35.960027Z - All caches warmed (5.2ms elapsed)
20:42:40.127245Z - list_available_agents() called
                   GAP: 4.17 seconds
```

**Session 2 (mock-model-alt):**
```
20:42:40.305523Z - Background thread starts prewarming
20:42:40.312003Z - All caches warmed (6.5ms elapsed)
20:42:44.474594Z - list_available_agents() called
                   GAP: 4.16 seconds
```

## Key Findings

### 1. Cache Prewarming Is Fast
Total prewarming takes ~5-6ms, well within acceptable limits.

### 2. No Cache Race Condition Observed
When `list_available_agents()` is called, all caches show `cache_initialized=true`, indicating the background prewarming completed successfully before any agent queries.

### 3. Unexplained 4+ Second Gap
There's a ~4.17 second gap between:
- Cache prewarming completion
- First call to `list_available_agents()`

This gap is **not caused by the agent discovery code** on this branch.

### 4. Subprocess Spawning Is Not Blocking
The `bun --version` subprocess takes ~2ms and runs in the background thread. No evidence of it blocking the main thread.

## Hypothesis Status: **NOT CONFIRMED**

The initial hypothesis that cache race conditions cause startup delays is **not supported by the evidence**. The caching mechanism works as designed:

1. Background thread warms caches in ~5-6ms
2. Caches are warm before any agent operations
3. Cache lookups are fast (~10-250µs)

## Possible Alternative Causes

The 4+ second delay likely originates from:

1. **TUI Initialization** - Terminal setup, config loading, rendering
2. **E2E Test Overhead** - `wait_for_text("›", TIMEOUT)` polling
3. **Other Startup Tasks** - Git detection, profile loading, etc.
4. **Different Scenario** - The test uses separate TUI instances; the reported issue may involve session reuse within the same TUI

## Recommendations

### 1. Clarify Reproduction Steps
The exact scenario causing the delay needs clarification:
- Is it within the same TUI session (e.g., `/new` command)?
- Is it between TUI restarts?
- What specific user actions trigger it?

### 2. Profile TUI Startup
Add timing instrumentation to the broader TUI startup path to identify where the 4+ seconds are spent.

### 3. Consider Removing Instrumentation
The added tracing instrumentation should be removed before merging, or converted to trace-level logging to avoid log noise.

## Raw Log Data

### Session 1 (mock-model) - Prewarming
```
20:42:35.954791Z [TIMING] prewarm_package_manager_cache() - START (already_initialized=false)
20:42:35.954841Z [TIMING] prewarm_package_manager_cache() - initializing cache (cache miss)
20:42:35.954850Z [TIMING] detect_package_manager_uncached() - START
20:42:35.954860Z [TIMING] detect_package_manager_uncached() - spawning 'bun --version' subprocess...
20:42:35.956882Z [TIMING] detect_package_manager_uncached() - 'bun --version' subprocess took 2.020525ms, success=true
20:42:35.956904Z [TIMING] detect_package_manager_uncached() - returning Bun (total took 2.056331ms)
20:42:35.956914Z [TIMING] prewarm_package_manager_cache() - DONE took 2.149983ms
20:42:35.956926Z [TIMING] prewarm_installation_cache() - START (already_initialized=false)
20:42:35.956933Z [TIMING] prewarm_installation_cache() - initializing cache (cache miss)
20:42:35.958294Z [TIMING] prewarm_installation_cache() - detect_agent_installation_uncached(claude-code) took 1.352307ms
20:42:35.959126Z [TIMING] prewarm_installation_cache() - detect_agent_installation_uncached(codex) took 801.093µs
20:42:35.959938Z [TIMING] prewarm_installation_cache() - detect_agent_installation_uncached(gemini) took 791.135µs
20:42:35.959960Z [TIMING] prewarm_installation_cache() - DONE took 3.034881ms
20:42:35.959969Z [TIMING] prewarm_readiness_cache() - START (already_initialized=false)
20:42:35.959975Z [TIMING] prewarm_readiness_cache() - initializing cache (cache miss)
20:42:35.959979Z [TIMING] init_readiness_cache() - START
20:42:35.959983Z [TIMING] detect_preferred_package_manager() - called (cache_initialized=true)
20:42:35.959987Z [TIMING] detect_preferred_package_manager() - returning Bun
20:42:35.959996Z [TIMING] init_readiness_cache() - detect_preferred_package_manager() took 14.016µs, result=Bun
20:42:35.960006Z [TIMING] init_readiness_cache() - detect_agent_readiness_uncached(claude-code) took 2.344µs, result=Ready
20:42:35.960011Z [TIMING] init_readiness_cache() - detect_agent_readiness_uncached(codex) took 311ns, result=Ready
20:42:35.960015Z [TIMING] init_readiness_cache() - detect_agent_readiness_uncached(gemini) took 250ns, result=Ready
20:42:35.960023Z [TIMING] init_readiness_cache() - DONE took 44.602µs
20:42:35.960027Z [TIMING] prewarm_readiness_cache() - DONE took 58.538µs
```

### Session 1 (mock-model) - Agent List Query
```
20:42:40.127245Z [TIMING] list_available_agents() - START
20:42:40.127268Z [TIMING] list_available_agents() - about to create production agent infos
20:42:40.127281Z [TIMING] AcpAgentInfo::from_agent(claude-code) - START
20:42:40.127301Z [TIMING] AcpAgentInfo::from_agent(claude-code) - detect_agent_installation took 11.972µs
20:42:40.127317Z [TIMING] get_agent_readiness(claude-code) - START (cache_initialized=true)
20:42:40.127330Z [TIMING] get_agent_readiness(claude-code) - DONE took 16.21µs, result=Ready
20:42:40.127341Z [TIMING] AcpAgentInfo::from_agent(claude-code) - get_agent_readiness took 27.27µs
20:42:40.127352Z [TIMING] AcpAgentInfo::from_agent(claude-code) - TOTAL took 73.445µs
20:42:40.127365Z [TIMING] AcpAgentInfo::from_agent(codex) - START
20:42:40.127373Z [TIMING] AcpAgentInfo::from_agent(codex) - detect_agent_installation took 1.132µs
20:42:40.127387Z [TIMING] get_agent_readiness(codex) - START (cache_initialized=true)
20:42:40.127398Z [TIMING] get_agent_readiness(codex) - DONE took 12.634µs, result=Ready
20:42:40.127407Z [TIMING] AcpAgentInfo::from_agent(codex) - get_agent_readiness took 22.081µs
20:42:40.127416Z [TIMING] AcpAgentInfo::from_agent(codex) - TOTAL took 51.264µs
20:42:40.127427Z [TIMING] AcpAgentInfo::from_agent(gemini) - START
20:42:40.127436Z [TIMING] AcpAgentInfo::from_agent(gemini) - detect_agent_installation took 1.052µs
20:42:40.127445Z [TIMING] get_agent_readiness(gemini) - START (cache_initialized=true)
20:42:40.127454Z [TIMING] get_agent_readiness(gemini) - DONE took 9.157µs, result=Ready
20:42:40.127462Z [TIMING] AcpAgentInfo::from_agent(gemini) - get_agent_readiness took 17.252µs
20:42:40.127470Z [TIMING] AcpAgentInfo::from_agent(gemini) - TOTAL took 43.23µs
20:42:40.127489Z [TIMING] list_available_agents() - DONE, created 5 agents in 252.504µs
```

## Conclusion

The agent discovery and readiness detection code added on this branch is **not causing the reported startup delay**. The caching mechanism functions correctly, completing in milliseconds before any agent queries occur. Further investigation is needed to identify the actual source of the delay, which appears to be elsewhere in the TUI startup path.
