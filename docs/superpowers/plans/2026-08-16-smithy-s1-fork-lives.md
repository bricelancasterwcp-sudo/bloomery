# smithy S1 — "The Fork Lives" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fork niri into `smithy-comp`, boot it as a selectable daily Wayland session on this box with zero agent features, and establish the upstream-tracking discipline (thin delta, rebase cadence).

**Architecture:** `smithy-comp` is a GitHub fork of YaLTeR/niri pinned to the release tag `v26.04` on a long-lived `smithy` branch. The S1 delta is **additive files only** (FORK.md, `.smithy-base`, a sync script, a session desktop file) — zero modifications to upstream source, so rebases are trivial. Session integration is install-time packaging (binaries to `/usr/local`, a `Smithy` entry in GDM), never source changes. GNOME remains the default session and the fallback.

**Tech Stack:** Rust (stable 1.96.0 on this box), cargo, smithay-based niri v26.04, gh CLI, GDM/systemd user session, Ubuntu 26.04 LTS.

**Spec:** `docs/superpowers/specs/2026-08-16-smithy-desktop-design.md` (sections 1, 3, 8 §S1, 10, 11). The umbrella context is `docs/superpowers/specs/2026-08-14-bloomery-design.md`.

## Global Constraints

- **Thin fork (spec law 3):** the fork's delta is only what IPC cannot provide. In S1 that means *additive files only* — do not modify any upstream source file, do not rename the `niri` binary or crates. Smithy identity lives in the session desktop file's `Name=` only.
- **Zero agent features in S1** (spec §8): no intent bar, no verbs, no daemon connection. S1 is pure infrastructure.
- **Manual-first (spec law 1):** nothing in S1 may touch `bloomery-daemon` or depend on it.
- **GNOME stays installed and default.** Never uninstall/disable the GNOME session or modify GDM defaults; Smithy is an *additional* login-screen option. Rollback from a broken Smithy session is "pick Ubuntu at the GDM session picker."
- **Box gotchas:** never wrap builds/tests in `timeout` (uutils segfault, exit 139 = wrapper crash). GPU is shared — do not kill any in-flight model run; building niri is CPU-only and safe.
- **Environment facts (verified 2026-08-16):** Ubuntu 26.04 LTS; GNOME **Wayland** session under GDM works today on NVIDIA driver 595.84 (so Wayland-on-NVIDIA is proven on this box); rustc 1.96.0; `niri` and `xwayland-satellite` not currently installed; upstream latest release = `v26.04`, default branch `main`.
- Fork repo will be **public** (forks of public repos are public; bloomery itself is public — fine).
- Commit style: single-line conventional commits, no attribution trailers (matches bloomery house style).

## File Structure

New repo `~/workspace/smithy-comp` (fork of YaLTeR/niri), branch `smithy` based on tag `v26.04`:

- `FORK.md` — create. The fork constitution: what the delta is allowed to contain, current delta inventory, rebase cadence, base tag.
- `.smithy-base` — create. Single line: the upstream tag the `smithy` branch is currently based on. Read by the sync script.
- `scripts/sync-upstream.sh` — create. Rebases the smithy delta onto the newest upstream release tag; `--dry-run` mode prints the plan without acting.
- `resources/smithy.desktop` — create. Wayland session entry (Name=Smithy) execing the standard `niri-session`.
- **No upstream file is modified.**

System install targets (Task 5, not in any repo): `/usr/local/bin/niri`, `/usr/local/bin/niri-session`, `/etc/systemd/user/niri.service`, `/etc/systemd/user/niri-shutdown.target`, `/usr/local/share/wayland-sessions/smithy.desktop`.

User config (not in the fork repo): `~/.config/niri/config.kdl`.

---

### Task 1: Fork niri and pin the baseline

**Files:**
- Create: `~/workspace/smithy-comp/` (git clone of the new fork)

**Interfaces:**
- Consumes: nothing (first task).
- Produces: repo at `~/workspace/smithy-comp` with remotes `origin` = `bricelancasterwcp-sudo/smithy-comp`, `upstream` = `YaLTeR/niri`; local branch `smithy` checked out at tag `v26.04`. All later tasks run inside this directory.

- [ ] **Step 1: Fork and clone**

```bash
cd ~/workspace
gh repo fork YaLTeR/niri --fork-name smithy-comp --clone
cd smithy-comp
git remote -v
```

Expected: clone succeeds; `origin` points at `bricelancasterwcp-sudo/smithy-comp`, `upstream` at `YaLTeR/niri`. (If `upstream` is missing: `git remote add upstream https://github.com/YaLTeR/niri.git`.)

- [ ] **Step 2: Fetch tags and create the `smithy` branch at the release tag**

```bash
cd ~/workspace/smithy-comp
git fetch upstream --tags
git switch -c smithy v26.04
git log --oneline -1
```

Expected: `smithy` branch created; `git log` shows the `v26.04` tag commit (a release commit dated 2026-04-25).

- [ ] **Step 3: Push the branch and make it the fork's default**

```bash
git push -u origin smithy
gh repo edit bricelancasterwcp-sudo/smithy-comp --default-branch smithy
gh repo view bricelancasterwcp-sudo/smithy-comp --json defaultBranchRef --jq .defaultBranchRef.name
```

Expected: final command prints `smithy`.

- [ ] **Step 4: Verify origin sync**

```bash
git fetch origin && git status -sb
```

Expected: `## smithy...origin/smithy` with no ahead/behind markers (per the house after-any-push rule).

### Task 2: Build deps, release build, upstream test baseline

**Files:**
- Modify: none (system packages + build artifacts only)

**Interfaces:**
- Consumes: Task 1's repo at `~/workspace/smithy-comp`, branch `smithy`.
- Produces: `target/release/niri` binary; a recorded green `cargo test` baseline that Task 3's FORK.md cites and the sync script re-runs after every rebase.

- [ ] **Step 1: Install system build dependencies**

```bash
sudo apt-get update
sudo apt-get install -y gcc clang libudev-dev libgbm-dev libxkbcommon-dev \
  libegl1-mesa-dev libwayland-dev libinput-dev libdbus-1-dev libsystemd-dev \
  libseat-dev libpipewire-0.3-dev libpango1.0-dev libdisplay-info-dev
```

Expected: all packages install (this is the niri wiki's Ubuntu list). If a later build step fails with a pkg-config error naming `<something>.pc`, the fix is `apt-get install lib<something>-dev` — resolve by the exact `.pc` name in the error, and record any extra package in FORK.md's build-deps note (Task 3).

- [ ] **Step 2: Release build**

```bash
cd ~/workspace/smithy-comp
cargo build --release
```

Expected: completes with warnings at most, no errors. First build takes several minutes. Do NOT wrap in `timeout`.

- [ ] **Step 3: Binary smoke check**

```bash
./target/release/niri --version
```

Expected: prints a niri version string containing `26.04`.

- [ ] **Step 4: Run the upstream test suite and record the baseline**

```bash
cargo test --release 2>&1 | tail -5
```

Expected: final summary shows `0 failed`. Record the exact pass count — it goes in FORK.md (Task 3) as the upstream baseline. If anything fails on a pristine tag checkout, STOP and report — that's an environment problem to diagnose, not a fork problem to patch (we never modify upstream source).

### Task 3: Fork-discipline delta (FORK.md, base pin, sync script, session file)

**Files:**
- Create: `FORK.md`
- Create: `.smithy-base`
- Create: `scripts/sync-upstream.sh`
- Create: `resources/smithy.desktop`

**Interfaces:**
- Consumes: Task 2's test-baseline number (fill it into FORK.md where marked).
- Produces: `.smithy-base` (read by `scripts/sync-upstream.sh`); `resources/smithy.desktop` (installed by Task 5); FORK.md (the constitution every future smithy-comp session reads first).

- [ ] **Step 1: Write `.smithy-base`**

```bash
cd ~/workspace/smithy-comp
echo v26.04 > .smithy-base
```

- [ ] **Step 2: Write `FORK.md`**

Write exactly this content, replacing `<N>` with Task 2's recorded pass count:

```markdown
# smithy-comp — fork constitution

Fork of [YaLTeR/niri](https://github.com/YaLTeR/niri) for
[bloomery](https://github.com/bricelancasterwcp-sudo/bloomery)'s desktop
track (smithy). Spec: bloomery
`docs/superpowers/specs/2026-08-16-smithy-desktop-design.md`.

## The thin-fork law

The delta on the `smithy` branch may contain ONLY what niri's IPC cannot
provide (spec law 3). Everything that can live outside the compositor
lives in `smithy-shell` (bloomery repo). Planned delta ceiling: intent-bar
overlay surface, grant-prompt chrome, window metadata tagging, layout
snapshot/restore hooks, extended event stream. S1 delta: additive files
only, zero upstream-source modifications, no renames — smithy identity is
`resources/smithy.desktop` `Name=` only.

## Branch and rebase discipline

- `smithy` = the delta branch, based on the upstream release tag recorded
  in `.smithy-base` (currently v26.04). Default branch of this repo.
- `main` = untouched mirror of upstream main. Never commit to it.
- Cadence: when upstream publishes a release tag, run
  `scripts/sync-upstream.sh` (use `--dry-run` first). Rebase onto release
  tags only, never onto upstream main.
- After any rebase: build + full test suite green before force-pushing
  (`git push --force-with-lease origin smithy` — never plain `--force`).

## Upstream test baseline

`cargo test --release` on pristine v26.04 (Ubuntu 26.04, rustc 1.96.0,
NVIDIA 595.84): <N> passed, 0 failed. A rebase that changes the failure
count from 0 blocks the push.

## Build deps (Ubuntu 26.04)

gcc clang libudev-dev libgbm-dev libxkbcommon-dev libegl1-mesa-dev
libwayland-dev libinput-dev libdbus-1-dev libsystemd-dev libseat-dev
libpipewire-0.3-dev libpango1.0-dev libdisplay-info-dev

## Delta inventory (keep current)

- FORK.md (this file)
- .smithy-base
- scripts/sync-upstream.sh
- resources/smithy.desktop
```

- [ ] **Step 3: Write `scripts/sync-upstream.sh`**

```bash
#!/usr/bin/env bash
# Rebase the smithy delta onto the newest upstream release tag.
# Usage: scripts/sync-upstream.sh [--dry-run]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

[ "$(git branch --show-current)" = "smithy" ] || { echo "ERROR: run on the smithy branch" >&2; exit 1; }
git diff --quiet || { echo "ERROR: working tree not clean" >&2; exit 1; }

git fetch upstream --tags --quiet
current_base=$(<.smithy-base)
latest_tag=$(git tag --list 'v*' --sort=-version:refname | head -1)

if [ "$current_base" = "$latest_tag" ]; then
  echo "Already based on $current_base — nothing to do."
  exit 0
fi

echo "Rebase plan: $current_base -> $latest_tag" \
  "($(git rev-list --count "$current_base".."$latest_tag") upstream commits)"
if [ "${1:-}" = "--dry-run" ]; then
  echo "(dry run — no changes made)"
  exit 0
fi

git rebase --onto "$latest_tag" "$current_base" smithy
echo "$latest_tag" > .smithy-base
git add .smithy-base
git commit -m "chore: rebase smithy delta onto $latest_tag"
cargo build --release
cargo test --release
echo "OK: rebased onto $latest_tag, build+tests green."
echo "Review, then: git push --force-with-lease origin smithy"
```

Then: `chmod +x scripts/sync-upstream.sh`

- [ ] **Step 4: Write `resources/smithy.desktop`**

```ini
[Desktop Entry]
Name=Smithy
Comment=bloomery desktop (niri fork)
Exec=niri-session
Type=Application
DesktopNames=niri
```

(`DesktopNames=niri` is deliberate — portals and app compatibility key off the compositor's real identity; only the human-visible `Name` says Smithy. `Exec=niri-session` is upstream's own session launcher, installed in Task 5.)

- [ ] **Step 5: Test the sync script's no-op and dry-run paths**

```bash
cd ~/workspace/smithy-comp
./scripts/sync-upstream.sh
```

Expected: prints `Already based on v26.04 — nothing to do.` and exits 0 (v26.04 is the newest tag today, so the no-op path runs; this also proves branch-check, clean-tree-check, and `.smithy-base` parsing work). Then verify the guard: `git switch main && ./scripts/sync-upstream.sh; git switch smithy` — expected: `ERROR: run on the smithy branch`, nonzero exit.

- [ ] **Step 6: Validate the desktop file**

```bash
desktop-file-validate resources/smithy.desktop && echo VALID
```

Expected: `VALID` (install `desktop-file-utils` via apt if the command is missing).

- [ ] **Step 7: Commit and push**

```bash
git add FORK.md .smithy-base scripts/sync-upstream.sh resources/smithy.desktop
git commit -m "chore: smithy S1 fork discipline — FORK.md, base pin, sync script, session entry"
git push origin smithy
git fetch origin && git status -sb
```

Expected: status shows `## smithy...origin/smithy`, no ahead/behind.

### Task 4: Nested smoke run + user config

**Files:**
- Create: `~/.config/niri/config.kdl` (user config, not in the repo)

**Interfaces:**
- Consumes: Task 2's `target/release/niri`.
- Produces: a working user config that Task 6's real login uses; proof the compositor renders on this GPU/driver.

- [ ] **Step 1: Install the default terminal and XWayland bridge**

```bash
sudo apt-get install -y alacritty
sudo apt-get install -y xwayland-satellite || cargo install --locked xwayland-satellite
```

Expected: alacritty installs (niri's default config binds Mod+T to it). If the apt install of xwayland-satellite fails (not packaged), the cargo fallback needs `libxcb-cursor-dev libxcb-composite0-dev` first — install those and retry. niri v26.04 auto-launches xwayland-satellite when the binary is on PATH; X11 apps then Just Work.

- [ ] **Step 2: Seed the user config from upstream's default**

```bash
mkdir -p ~/.config/niri
cp -n ~/workspace/smithy-comp/resources/default-config.kdl ~/.config/niri/config.kdl
```

Expected: file exists afterward. `-n` preserves any pre-existing config. No edits needed — the default binds Mod+T→alacritty, Mod+Shift+E→quit, Mod+Shift+/→hotkey help.

- [ ] **Step 3: Validate the config**

```bash
cd ~/workspace/smithy-comp
./target/release/niri validate
```

Expected: exits 0 (prints nothing or an OK line). A parse error here means the config copy went wrong — fix before proceeding.

- [ ] **Step 4: Nested smoke run inside the GNOME session**

Run (this opens a window; it needs the graphical session, so run it from a terminal inside GNOME, not over SSH):

```bash
./target/release/niri
```

Expected: a window titled like "niri" opens showing the niri wallpaper/workspace. Inside it press **Mod+T** — an alacritty terminal opens and renders text. Press **Mod+Shift+/** — the hotkey overlay appears. Press **Mod+Shift+E** to quit. Any GPU/EGL crash on startup: capture the full stderr and STOP — that's the NVIDIA risk from spec §11 materializing, and it needs diagnosis before session install makes sense.

- [ ] **Step 5: Record the smoke result**

No commit (nothing in-repo changed). Note pass/fail and any stderr warnings in the task report — warnings about missing portals or DBus services are normal in nested mode.

### Task 5: System session install

**Files:**
- Create (system): `/usr/local/bin/niri`, `/usr/local/bin/niri-session`, `/etc/systemd/user/niri.service`, `/etc/systemd/user/niri-shutdown.target`, `/usr/local/share/wayland-sessions/smithy.desktop`

**Interfaces:**
- Consumes: Task 2's binary, Task 3's `resources/smithy.desktop`, upstream's `resources/niri-session`, `resources/niri.service`, `resources/niri-shutdown.target`.
- Produces: a "Smithy" entry in GDM's session picker for Task 6.

- [ ] **Step 1: Install binaries and session launcher**

```bash
cd ~/workspace/smithy-comp
sudo install -Dm755 target/release/niri /usr/local/bin/niri
sudo install -Dm755 resources/niri-session /usr/local/bin/niri-session
```

- [ ] **Step 2: Install systemd user units (path-fixed to /usr/local)**

```bash
sed 's|/usr/bin/niri|/usr/local/bin/niri|' resources/niri.service | sudo tee /etc/systemd/user/niri.service >/dev/null
sudo install -Dm644 resources/niri-shutdown.target /etc/systemd/user/niri-shutdown.target
grep ExecStart /etc/systemd/user/niri.service
```

Expected: `ExecStart=/usr/local/bin/niri --session` (the sed is install-time packaging, not a source change — thin-fork law intact).

- [ ] **Step 3: Install the Smithy session entry**

```bash
sudo install -Dm644 resources/smithy.desktop /usr/local/share/wayland-sessions/smithy.desktop
ls /usr/local/share/wayland-sessions/
```

Expected: `smithy.desktop` listed. GDM reads `/usr/local/share/wayland-sessions` via XDG data dirs; the entry appears at next GDM load.

- [ ] **Step 4: Sanity-check the installed stack headlessly**

```bash
/usr/local/bin/niri --version && /usr/local/bin/niri validate
systemd-analyze --user verify /etc/systemd/user/niri.service 2>&1 | grep -v '^$' || echo "unit OK"
```

Expected: version prints, validate exits 0, unit verify emits no errors (warnings about missing `niri-shutdown.target` ordering are acceptable; errors are not).

- [ ] **Step 5: Confirm GNOME is untouched**

```bash
ls /usr/share/wayland-sessions/
```

Expected: `ubuntu.desktop` (GNOME) still present. Nothing in this task modified or removed it — verify, don't assume.

### Task 6: Manual acceptance — Brice logs into Smithy (BLOCKING: human step)

**Files:** none.

**Interfaces:**
- Consumes: everything above.
- Produces: S1 acceptance verdict; go/no-go for S2 planning.

- [ ] **Step 1: Hand off to Brice with this exact checklist**

This step cannot be automated — an agent must stop here and present:

> Log out of GNOME. At the GDM login screen, click the gear/session picker and choose **Smithy**, then log in. Checklist:
> 1. Desktop appears (niri wallpaper, no crash back to GDM).
> 2. **Mod+T** opens a terminal; typing works.
> 3. **Mod+Shift+/** shows the hotkey help; browse the basics (Mod+H/L to focus columns, Mod+F maximize).
> 4. Launch a browser from the terminal (`google-chrome` or `firefox`) — it opens and renders (proves the app path; Firefox from apt exercises xwayland-satellite if it starts as X11).
> 5. Audio/network/keyboard layout behave normally.
> 6. Log out (**Mod+Shift+E**), log back into **Ubuntu** (GNOME) — the old world is intact.
>
> If Smithy fails at login: pick Ubuntu at the session picker (that's the whole rollback), and capture `journalctl --user -u niri.service -b --no-pager | tail -50` from the GNOME session for diagnosis.

- [ ] **Step 2: On Brice's pass verdict, record S1 complete**

In the bloomery repo (clean master worktree, house practice): append one line to the S1 plan doc's header — `**Status: S1 ACCEPTED <date> — smithy-comp <commit> daily-driveable.**` — commit as `docs: smithy S1 accepted`, push, verify origin sync. If Brice reports failures, they become the input for a fix cycle — do not mark accepted.

---

## Self-review notes (already applied)

- Spec §8 S1 requires: fork boots as daily session ✓ (Tasks 4–6), zero agent features ✓ (none anywhere), upstream discipline ✓ (Task 3), thin delta ✓ (additive files only, install-time sed instead of source edits).
- Spec §10 fork-testing requires upstream CI + nested-session checks: upstream suite baseline ✓ (Task 2 Step 4), nested run ✓ (Task 4 Step 4). E2E/intent tests are S2+ scope — none exist in S1 because no agent surface exists.
- Type/name consistency: `.smithy-base`, `smithy` branch, `smithy.desktop`, `/usr/local/bin/niri` used identically across Tasks 1–6.
