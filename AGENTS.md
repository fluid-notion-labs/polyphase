# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Project

**polyphase** — format-agnostic 3D asset loading. Single crate, Bevy behind a feature flag. Core is useful standalone (CLI tools, pipeline).

## Architecture

**Read [`docs/polyphase.md`](docs/polyphase.md) for the full design doc.** Key rules:

- **Core layer** (`src/`) = pure logic. No Bevy dependency. Parsers take `&[u8]` and return `Result<MeshData>`.
- **Bevy layer** (`src/bevy/`) = `#[cfg(feature = "bevy")]` only. Thin `AssetLoader` wrappers that delegate to core parsers. `Into<bevy::Mesh>` for `MeshData`.
- **Reference code** (`ref/`) = extracted from seaview. Read-only context for extraction, not part of the crate build.
- **Parsers are pure functions.** `&[u8] → Result<MeshData>`. No IO, no filesystem, no Bevy types.
- **`MeshData` is the universal intermediate.** Every parser produces it, every consumer (Bevy, CLI, pipeline) reads from it.
- **Feature flag is the boundary.** `cargo check -p polyphase` must succeed with zero Bevy dependency. `cargo check -p polyphase --features bevy` adds the integration layer.
- **File size signals.** Parsers >300 lines → split into submodules. Bevy wrappers should be thin (<100 lines each).
- **Idiomatic patterns.** Use idiomatic APIs from dependency crates (e.g. `thiserror` derives instead of hand-rolled `Display`/`Error` impls). Don't reinvent what a dependency already provides. Keep the macro/crate footprint lean — only add a derive crate if it genuinely reduces boilerplate at our scale.
- **Dependencies.** Read [`docs/architecture/dependencies.md`](docs/architecture/dependencies.md) for full rationale. Summary rules:
  - **Prefer `[f32; 3]` over math-library types** for `MeshData` internals — zero-dep, bytemuck-compatible, universally convertible.
  - **No math library in core.** glam comes in via bevy feature; core parsers use raw arrays.
  - **mint interop is opt-in** — add behind a `mint` feature flag when needed, not as a core dep.
  - **When adding a crate**, update `docs/architecture/dependencies.md` with: what it does, why we chose it, what we evaluated and rejected.

## Version Control — jj (Jujutsu)

This repo uses **jj** (colocated with git). All VCS operations use jj commands. Do NOT use raw `git` commands except where noted.

### Agent Setup

The repo has `ui.pager = "cat"` configured so jj commands don't block in non-interactive terminals. If jj commands hang or produce repeated output, the pager is fighting. Fix:

```bash
jj config set --repo ui.pager "cat"
```

### Core Concepts

- **`@`** is always the working-copy change. jj auto-snapshots on every operation — there is no "staging area".
- **Changes, not branches.** Work happens on `@`. Use `jj new` to start a new change, `jj describe` to add a message.
- **Bookmarks** (jj's name for branches) mark integration points. `main` is the stable line.
- **History is always rewritable.** `jj rebase`, `jj squash`, `jj split` — use freely.

### Daily Workflow

```bash
# See what you're working on
jj st                          # Working copy status
jj log --limit 10              # Recent history

# Describe the current change
jj describe -m "Epic 2: STL parser with tests"

# Start a new change on top of current
jj new                         # New empty change on top of @-
jj new -m "next thing"         # With description

# Fold working copy into parent (squash up)
jj squash                      # Merge @ into @-
jj squash --into <rev>         # Merge @ into a specific change

# Split a change that got too big
jj split                       # Interactive split of @

# Rebase onto a different parent
jj rebase -d main              # Rebase @ onto main bookmark
jj rebase -r @ -d <rev>        # Rebase @ onto specific revision
```

### Bookmarks (Branches)

```bash
# Create/move a bookmark to current change
jj bookmark set <name>         # Point bookmark at @
jj bookmark set <name> -r @-   # Point bookmark at parent

# List bookmarks
jj bookmark list --all

# Delete a bookmark
jj bookmark delete <name>
```

### Pushing to Remote

```bash
# Push bookmarks to git remote
jj git push                    # Push all tracked bookmarks
jj git push -b <name>          # Push specific bookmark

# Fetch from remote
jj git fetch
```

### Interop with bd (beads)

bd uses git refs under the hood. jj colocated mode auto-exports to git, so `bd sync` works directly:

```bash
jj describe -m "your message"  # Describe the change
bd sync                        # beads reads git state — works because colocated
```

## Starting a Session

**Always start here:**

```bash
bd show polyphase-n7x   # Load session context — architecture, state, what to do next
bd ready              # See available work
```

Issue **polyphase-n7x** is the living session context. It is always open (P0) and contains the full project state, architecture overview, open issues, and handoff notes from the last session. **Read it first.**

## `!lc` Command

When the user says **`!lc`**, check issue polyphase-n7x:

```bash
bd show polyphase-n7x
```

## `!lcc` Command

`!lcc` means, load context and continue: !lc + continue

## `!sc` Command

When the user says **`!sc`**, update polyphase-n7x's notes with the current session state:

```bash
bd update polyphase-n7x --notes "## Session Context (DATE)
### Change: <jj change id or description>
### What was done ...
### Architecture ...
### What needs to happen next ...
### Open issues ...
### Test status ..."
```

## Quick Reference

```bash
bd show polyphase-n7x    # Session context (read first!)
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Key Files

| File | Purpose |
|---|---|
| `docs/polyphase.md` | Design doc — crate structure, types, extraction plan |
| `ref/asset_loaders/stl_loader.rs` | Source for STL parser extraction |
| `ref/asset_loaders/obj_loader.rs` | Source for OBJ parser extraction |
| `ref/bin/debug_loader.rs` | GLB parsing seed code (`parse_glb_stats`) |
| `ref/bin/timeline_debug.rs` | GLB parsing seed code (`parse_glb_frame_size`) |
| `ref/sequence_loader/` | Seaview orchestration (stays in seaview, context only) |
| `docs/architecture/dependencies.md` | Dependency guidelines — what we use, what we rejected, process for adding crates |

## Testing

### Test Runner

We use **cargo-nextest** for fast, parallel test execution. Config lives in `.config/nextest.toml`.

```bash
cargo nextest run -p polyphase          # Run all tests (default profile)
cargo nextest run -p polyphase -E 'test(stl)'  # Filter by name
cargo nextest run -p polyphase --profile ci     # CI profile (quiet, failures only)
```

### Test Layout

Tests live in three places:

| Location | What | Example |
|---|---|---|
| `src/<module>.rs` (`#[cfg(test)] mod tests`) | Unit tests — co-located with the code they test | `src/parsers/stl.rs`, `src/mesh_data.rs` |
| `tests/pipeline.rs` | Integration tests — cross-module pipeline (parse → validate → capabilities) | `stl_binary_full_pipeline`, `format_detect_and_parse_glb` |
| `tests/fixtures/` | Binary/text fixture files loaded by tests | `triangle.stl`, `cube.glb`, `quad.obj` |

Every `src/` module with logic has a `#[cfg(test)] mod tests` block at the bottom, separated by a `// ---` divider.

Shared test helpers live in `src/lib.rs` → `test_util` (`#[cfg(test)] pub(crate) mod`). This provides `fixture()`, `sample_triangle()`, and `sample_triangle_bare()` so unit tests don't duplicate helpers.

### Fixtures

Fixture files in `tests/fixtures/` are real 3D assets (small, hand-crafted). Parsers load them via `include_bytes!` in tests — no filesystem IO at test time.

Current fixtures:
- **STL:** `triangle.stl` (binary), `triangle_ascii.stl`, `quad.stl` (binary)
- **OBJ:** `triangle.obj`, `triangle_normals.obj`, `triangle_full.obj` (normals+UVs), `quad.obj`, `two_objects.obj`
- **GLB:** `cube.glb`, `cube_textured.glb` (materials + embedded texture + sampler), `cube_default_sampler.glb` (texture with no explicit sampler), `quad_meshopt.glb` (EXT_meshopt_compression; regenerate via `cargo run --features meshopt --example gen_meshopt_fixture`)
- **PLY:** `triangle.ply` (ASCII, normals), `triangle_binary.ply` (binary LE, normals), `triangle_colors.ply` (ASCII, normals+vertex colors), `quad.ply` (ASCII, n-gon triangulation test)

When adding a new parser or feature, add a minimal fixture that exercises it.

### Test Conventions

- **Parser tests are pure.** Feed `&[u8]` in, assert on `MeshData` fields. No filesystem, no Bevy.
- **Test names describe the scenario:** `parse_binary_triangle`, `empty_stl_returns_error`, `smooth_normals_single_triangle`.
- **Error paths get tests too.** Every `LoadError` variant should have at least one test that triggers it.
- **Validation round-trip.** After parsing, call `.validate()` and assert `Ok(())` — proves the parser produces structurally valid `MeshData`.

### Nextest Profiles

| Profile | Use | Behavior |
|---|---|---|
| `default` | Local dev | Shows all pass/fail, immediate failure output, slow-test warning at 5s |
| `ci` | CI pipelines | Only shows failures, quiet on success |

### Test Counts

Track rough test counts here so agents know what to expect:
- **194 tests** as of multi-asset steps 1–4 + sampler capture + meshopt decode (both feature configs pass the same suite; the two meshopt tests swap by cfg)

## Verification

After any code changes, run:

```bash
# Core compiles without Bevy
cargo check -p polyphase

# Bevy feature compiles
cargo check -p polyphase --features bevy

# Tests pass (nextest: hierarchical, colored, parallel)
cargo nextest run -p polyphase
```

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `jj git push` succeeds (when a remote is configured).

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** — Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed):
   ```bash
   cargo check -p polyphase
   cargo check -p polyphase --features bevy
   cargo nextest run -p polyphase
   ```
3. **Update issue status** — Close finished work, update in-progress items
4. **Describe and push changes** — This is MANDATORY:
   ```bash
   jj describe -m "meaningful description of this change"
   jj new                    # Move @ forward so described change is immutable
   jj bookmark set main -r @-  # Advance main to the completed change (if landing to main)
   bd sync                   # Sync beads
   jj git push               # Push to remote (when remote is configured)
   jj log --limit 5          # Verify state
   ```
5. **Clean up** — Abandon empty changes: `jj abandon` if `@` is empty after `jj new`
6. **Verify** — All changes described and pushed
7. **Hand off** — Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until changes are described, bookmarks advanced, and pushed
- NEVER stop before pushing — that leaves work stranded locally
- NEVER say "ready to push when you are" — YOU must push
- If push fails, resolve and retry until it succeeds

## Merging / Rebasing

When the user says **"merge"**, they mean **rebase to main** (linear history). Use this workflow:

```bash
# Rebase a change onto main
jj rebase -r <change> -d main

# Or rebase a chain of changes
jj rebase -s <base-of-chain> -d main

# Advance the main bookmark
jj bookmark set main -r <new-tip>

# Push
jj git push
```

This keeps the history linear. Never create merge commits unless explicitly asked.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:46cd31e7 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->
## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->
