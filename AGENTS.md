# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Project

**loadfu** — format-agnostic 3D asset loading. Single crate, Bevy behind a feature flag. Core is useful standalone (CLI tools, pipeline).

## Architecture

**Read [`docs/loadfu.md`](docs/loadfu.md) for the full design doc.** Key rules:

- **Core layer** (`src/`) = pure logic. No Bevy dependency. Parsers take `&[u8]` and return `Result<MeshData>`.
- **Bevy layer** (`src/bevy/`) = `#[cfg(feature = "bevy")]` only. Thin `AssetLoader` wrappers that delegate to core parsers. `Into<bevy::Mesh>` for `MeshData`.
- **Reference code** (`ref/`) = extracted from seaview. Read-only context for extraction, not part of the crate build.
- **Parsers are pure functions.** `&[u8] → Result<MeshData>`. No IO, no filesystem, no Bevy types.
- **`MeshData` is the universal intermediate.** Every parser produces it, every consumer (Bevy, CLI, pipeline) reads from it.
- **Feature flag is the boundary.** `cargo check -p loadfu` must succeed with zero Bevy dependency. `cargo check -p loadfu --features bevy` adds the integration layer.
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
bd show loadfu-n7x   # Load session context — architecture, state, what to do next
bd ready              # See available work
```

Issue **loadfu-n7x** is the living session context. It is always open (P0) and contains the full project state, architecture overview, open issues, and handoff notes from the last session. **Read it first.**

## `!lc` Command

When the user says **`!lc`**, check issue loadfu-n7x:

```bash
bd show loadfu-n7x
```

## `!lcc` Command

`!lcc` means, load context and continue: !lc + continue

## `!sc` Command

When the user says **`!sc`**, update loadfu-n7x's notes with the current session state:

```bash
bd update loadfu-n7x --notes "## Session Context (DATE)
### Change: <jj change id or description>
### What was done ...
### Architecture ...
### What needs to happen next ...
### Open issues ...
### Test status ..."
```

## Quick Reference

```bash
bd show loadfu-n7x    # Session context (read first!)
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Key Files

| File | Purpose |
|---|---|
| `docs/loadfu.md` | Design doc — crate structure, types, extraction plan |
| `ref/asset_loaders/stl_loader.rs` | Source for STL parser extraction |
| `ref/asset_loaders/obj_loader.rs` | Source for OBJ parser extraction |
| `ref/bin/debug_loader.rs` | GLB parsing seed code (`parse_glb_stats`) |
| `ref/bin/timeline_debug.rs` | GLB parsing seed code (`parse_glb_frame_size`) |
| `ref/sequence_loader/` | Seaview orchestration (stays in seaview, context only) |
| `docs/architecture/dependencies.md` | Dependency guidelines — what we use, what we rejected, process for adding crates |

## Verification

After any code changes, run:

```bash
# Core compiles without Bevy
cargo check -p loadfu

# Bevy feature compiles
cargo check -p loadfu --features bevy

# Tests pass
cargo test -p loadfu
```

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `jj git push` succeeds (when a remote is configured).

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** — Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed):
   ```bash
   cargo check -p loadfu
   cargo check -p loadfu --features bevy
   cargo test -p loadfu
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