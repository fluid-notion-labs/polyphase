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
### Branch: ...
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

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed):
   ```bash
   cargo check -p loadfu
   cargo check -p loadfu --features bevy
   cargo test -p loadfu
   ```
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

## Merging Branches

When the user says **"merge"**, they mean **rebase to main** (not a merge commit). Use this workflow:

```bash
git checkout main
git pull
git rebase <feature-branch>
git push
```

This keeps the history linear. Never use `git merge` unless explicitly asked for a merge commit.