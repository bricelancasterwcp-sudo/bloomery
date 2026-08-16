# smithy — the bloomery desktop (agent-first shell)

Brainstormed and approved section-by-section in conversation, 2026-08-15/16.
Amends the umbrella spec (`2026-08-14-bloomery-design.md`): the "no
userland" doctrine becomes **per-role** — the headless appliance role keeps
it absolute; an optional desktop role may carry a display stack. The core
role, the daemon, and Phases 2d–4 are untouched by this spec.

## 1. What smithy is (decision record)

smithy is bloomery's human-side userland: an **agent-first Wayland shell**
where the shell itself is the agent surface — you state intent to the
compositor, agents act through verbs and grants, the journal is your
history, and KV suspend/resume becomes "save my whole working state."
Conventional apps (Chrome included) run as guest windows from the host
distro.

Three directions were considered and this one chosen:

- (i) *Normal DE + agent HUD* — rejected: needs nothing from the bloomery
  kernel; could be an app on any distro; the industry default already.
- **(ii) Agent-first shell — chosen: the smallest surface that genuinely
  requires the kernel; every kernel primitive maps to a visible desktop
  feature (KV image → workspace resume, grants → permission prompts,
  journal → history, verbs → what the shell can do).**
- (iii) *Semantic desktop* — deferred, not foreclosed: depends on the
  semantic layer that does not exist yet; a semantic view can later grow
  inside (ii) as another way to navigate (the intent bar is where it
  would live).

Role model (the Linux analogy): same kernel, different sessions. `core` =
today's headless appliance, unchanged, remains the default. `desktop` =
an optional role. Near-term the desktop role needs no appliance image at
all — smithy runs on stock Linux as the user's Wayland session, exactly
as Phase 1 ran the daemon on stock Linux. Folding smithy into the
appliance image as a boot role is deferred (out of scope here).

smithy is a **parallel track, never a blocker**: the appliance queue
(policy plane 2d, VTT acceptance, fine-tune flywheel) proceeds
independently.

## 2. Design laws

1. **Manual-first, AI-optional.** smithy with the LLM off is a complete,
   good scrollable-tiling DE. Every agent capability is strictly
   additive; the shell never blocks on inference. (Fail-closed doctrine;
   G4 demoted the 7B live — the desktop must be worth using the day that
   happens again.)
2. **The daemon stays the kernel.** The desktop is a client of the
   daemon's HTTP API. No desktop code links into the daemon; API
   additions are designed on their own merits.
3. **Thin fork.** Own the shell, rent the compositor: the niri fork's
   delta is only what IPC cannot provide. Everything that can live
   outside the compositor does.
4. **Grants stay structural, not a sandbox** (unchanged appliance
   ruling). The shell enforces at the dispatch boundary; we are not
   building MAC policy for Chrome.
5. **Instrument honesty on resume.** A partial workspace resume is a
   visible, journaled fact (`WorkspaceResumed { matched, missing,
   substituted }`), never a silent lie. App-internal state is
   best-effort by law, not by bug report.
6. **Mutating verbs earn their way.** The desktop card gets its own
   pre-registered gate (D1, §7); mutating verbs default-off until a
   model clears it.
7. **Static per-role VRAM budget.** The boot-time budget ruling holds;
   the desktop role declares a smaller LLM budget (compositing + apps
   need their share). Gate measurements are valid only for the role they
   ran on.

## 3. Components

| Component | What it is | Where it lives |
|---|---|---|
| `bloomery-daemon` | unchanged — the kernel | bloomery repo |
| `smithy-comp` | niri fork; thin delta | own repo, upstream remote, rebase cadence |
| `smithy-shell` | new Rust process — the brain | new crate in bloomery repo (shares journal/API types) |

`smithy-comp` delta (exhaustive by intent): intent-bar overlay surface;
grant-prompt chrome (shell-owned, unspoofable by guest apps); window
metadata tagging; layout snapshot/restore hooks; extended event stream.

`smithy-shell` owns: the desktop verb card and its dispatch; the grant
brokering flow; workspace snapshot/resume; the journal-as-history view.
It sits between the daemon's native agent API and the compositor IPC.

## 4. Interaction model

- **One agent surface: the intent bar** (shell-owned overlay on a
  keybind). Typed intent returns **proposed verb calls rendered before
  they happen**, not chat prose. Read-only verbs execute immediately;
  mutating verbs follow the grant tiers:
  1. ungranted mutation → grant prompt in shell chrome;
  2. granted mutation → executes, lands in the journal;
  3. demoted model → read-only card, structurally enforced by the
     daemon (as G4 built it); the intent bar visibly degrades to
     search-and-describe, never silently fails.
- **The journal is the history UI.** Verb executions, grant decisions,
  and workspace events are already journaled by the daemon; the shell
  renders them.
- **Latency reality:** the earned candidate model (27B Q3, partial
  offload on 16GB) answers in seconds, not milliseconds. The intent bar
  streams progress; the shell stays fully interactive throughout
  (law 1). G4 measured correctness, not speed; an S2 latency
  measurement will put numbers on the UX.

## 5. Desktop verb card (v1)

Rides the existing task loop, codec ABI, and envelope-v3 unchanged —
the desktop is one new verb card plus a grants profile.

- **Read-only** (survive demotion, no grants): `desktop.describe`,
  `journal.search`, `workspace.list`.
- **Mutating, low blast radius** (grant `desktop.arrange`):
  `window.focus`, `window.move`, `window.resize`, `workspace.switch`.
- **Mutating, real blast radius** (each its own grant):
  - `app.launch` — grant `desktop.launch` **with an allowlist**
    (launching arbitrary binaries is the scariest verb on the card);
  - `workspace.snapshot` / `workspace.resume` — grant `desktop.session`
    (resume loads a KV image = spends VRAM);
  - `workspace.close` — always-confirm regardless of grant
    (destructive).
- **Absent by design:** file verbs, shell-execution verbs,
  browser-content verbs. v1 orchestrates windows, not app internals; a
  content-reaching card is a later design.

## 6. Workspace resume

A workspace snapshot is a manifest binding three things:

1. **Agent core image** — the daemon's existing KV suspend/resume,
   verbatim (semantic-restore proven in Phase 1).
2. **Window layout** — from the compositor snapshot hook: app identity
   (app-id + launch command), column/position geometry, focus state,
   metadata tags.
3. **Grants state** — active grants at snapshot; on resume a grant
   re-attaches only if it still exists in the daemon's grant store,
   otherwise the next use re-prompts (no stale authority).

Resume: the shell relaunches the manifest's apps; the compositor
restores layout as windows appear, matched by app-id and tolerant of
arrival order; the daemon restores the KV image. The agent wakes
mid-thought with its windows around it.

Scope law (law 5): we restore **shape and memory, not app guts**.
Wayland has no reliable session-management protocol; Chrome reopens its
own tabs, unsaved buffers are gone. Requested-vs-actual lands in the
journal as `WorkspaceResumed { matched, missing, substituted }`.

Storage: manifests are content-addressed files beside the KV images in
the daemon's data dir, **daemon-owned** — they reference its images, so
manifest lifecycle must be atomic with image GC. Snapshot and resume
are journal events like everything else.

## 7. Gate D1 — `desktop-tasks-v1`

Same instrument discipline as G4, new fixture set:

- Pre-registered **before** the fixture set exists
  (rigorous-experiments order).
- N=20 frozen fixtures, reference landings validated through the real
  lens, envelope-v3, greedy sampling, journals committed.
- Pass = Wilson 95% lower bound ≥ 0.8; the point estimate decides;
  fail → mutating desktop verbs stay demoted/off.
- Per-role validity (law 7): D1 runs under the desktop role's VRAM
  budget.

Candidate model: **qwen3.8-27b Q3**, which earned mutating verbs on
`codec-tasks-v1` (20/20, Wilson lower 0.839, envelope-v3; commit
`206e183`). That result does **not** auto-clear D1 — it retires the
"no capable model exists" risk, nothing more.

## 8. Slices (each independently useful)

- **S1 — the fork lives.** smithy-comp boots as the daily Wayland
  session with zero agent features; upstream-tracking discipline
  established (thin delta, rebase cadence). Pure infrastructure,
  immediately usable.
- **S2 — read-only smithy.** Intent bar + `desktop.describe` /
  `journal.search` / `workspace.list` + journal history view, wired to
  the daemon. Useful even with a demoted model. Includes the latency
  measurement (§4).
- **S3 — mutation behind the gate.** Full verb card, grants flow,
  workspace snapshot/resume, D1 pre-registered and run. Mutating verbs
  default-off until D1 passes.

## 9. Non-goals (v1)

DRM/Widevine (Netflix playback — later errand, not architecture);
Flatpak plumbing (matters only when the desktop role folds into the
appliance image, itself deferred); appliance-image integration; file /
shell / browser-content verbs; the semantic desktop (horizon, not
foreclosed); voice input (intent bar is keyboard-first).

## 10. Testing

- **`smithy-shell`:** GPU-free unit tests for all logic — verb
  dispatch, grant flow, manifest match/restore including `missing` /
  `substituted` paths, journal rendering — against mocked compositor
  IPC and daemon API. 80% bar, fmt/clippy `-D warnings`, same as the
  rest of the repo.
- **`smithy-comp`:** upstream CI covers the compositor; our delta gets
  integration tests in a nested session (niri runs nested in a window —
  also the dev loop). E2E: scripted intent → verb → grant prompt →
  window-state assertion; deterministic waits only.
- **Manual-first as a CI config, not a promise:** one path boots the
  shell with no daemon at all and asserts full DE functionality; the
  degraded modes (daemon up / model demoted, daemon down) each get
  their own path.
- **D1** is the live measurement (§7); no mutating verb ships enabled
  without it.

## 11. Risks

- **Fork drift** vs niri's velocity → thin delta (law 3), IPC-first,
  scheduled rebases in S1.
- **Latency UX** on partial offload → streaming intent bar, manual
  paths always available; S2 measures before S3 commits.
- **Oxygen theft** from the appliance track → parallel-track rule (§1),
  slices independently useful, appliance queue explicitly unblocked.
- **Resume matching flakiness** (app-id mismatches, apps that won't
  relaunch cleanly) → best-effort law + journaled deltas (law 5).
- **VRAM contention** desktop vs pager → static per-role budget
  (law 7).
