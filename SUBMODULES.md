# Submodules guide

Three of the firmware's biggest dependencies live as **git submodules**
under `vendor/`, consumed by Cargo as `path = "./vendor/<name>"` deps:

| Submodule | Path | Upstream | Cargo crate |
| --- | --- | --- | --- |
| MeshCore protocol | `vendor/meshcore` | <https://codeberg.org/Ranzbak/meshcore-aegg-rust> | `meshcore` |
| MeshCore companion | `vendor/meshcore-companion` | <https://codeberg.org/Ranzbak/meshcore-companion-aegg-rust> | `meshcore-companion` |
| SSD1675 e-paper driver | `vendor/ssd1675` | <https://codeberg.org/Ranzbak/ssd1675> | `ssd1675` |

Each submodule is **pinned to a specific commit** in the parent repo's
tree (the SHAs are stored alongside `.gitmodules`).  Cloning the parent
without submodules gives you empty `vendor/<name>` directories and the
build will fail at the path-dependency resolution step.

## First clone

```sh
git clone --recurse-submodules <parent-url>
```

If you already cloned without `--recurse-submodules`:

```sh
git submodule update --init --recursive
```

Either form leaves each submodule on a **detached HEAD** at the pinned
commit — that's normal for submodules and is what Cargo's path deps
resolve against.

## Pulling new commits

When the parent repo updates its submodule pin (someone bumped one of
the vendor libraries), `git pull` on the parent does **not**
automatically check out the new submodule commit.  Use either:

```sh
git pull --recurse-submodules            # one-shot
git config submodule.recurse true        # default for this repo
```

After the latter, `git pull`, `git checkout`, and `git switch` all
auto-update submodules.

## Updating a vendor library to upstream HEAD

To bump e.g. `meshcore` to its latest `main`:

```sh
cd vendor/meshcore
git checkout main
git pull
cd ../..
git add vendor/meshcore       # records the new pinned SHA
git commit -m "vendor/meshcore: bump to <sha>"
```

The `git add vendor/<name>` step is the part people forget — without
it, the parent repo still references the old commit even though the
submodule worktree is up to date.

## Hacking on a vendor library

Two-step pattern: commit in the submodule, then update the pin in the
parent.

```sh
cd vendor/meshcore
git checkout -b my-fix         # don't work on detached HEAD
# ...edit, build, test (from the parent: cargo fw)...
git commit -am "fix: …"
git push                       # to your fork or the upstream

cd ../..
git add vendor/meshcore
git commit -m "vendor/meshcore: pull <sha>"
```

The Cargo build uses whatever is checked out in `vendor/<name>`, so
local changes are picked up immediately — no need to bump the pin
during the edit/test loop.  Only commit the new pin in the parent
once the upstream change has actually landed somewhere durable.

## Interesting branches

`vendor/ssd1675` has a **`slow-but-clear`** branch with a refresh
waveform that produces noticeably cleaner e-paper updates at the cost
of a longer refresh time.  Worth poking at for screens that aren't on
the minute-tick redraw path (Watch face, PM thread, Contacts):

```sh
cd vendor/ssd1675
git checkout slow-but-clear
cd ../..
cargo fw                       # builds against the alternate branch
```

Don't commit the parent's submodule pin to the slow-but-clear branch
unless the whole project is moving to it — leave the comparison local.

## Sibling read-only checkouts

For grep / blame / cross-reference work without disturbing the build,
clone the upstreams as **siblings** of this repo:

```sh
cd ..
git clone https://codeberg.org/Ranzbak/meshcore-aegg-rust.git
git clone https://codeberg.org/Ranzbak/meshcore-companion-aegg-rust.git
git clone -b slow-but-clear https://codeberg.org/Ranzbak/ssd1675.git
```

Layout afterwards:

```
Projects/
├── cyber-aegg-v2-rust-test/        # this repo
│   └── vendor/
│       ├── meshcore               # submodule, pinned, build-time
│       ├── meshcore-companion     # submodule, pinned, build-time
│       └── ssd1675                # submodule, pinned, build-time
├── meshcore-aegg-rust/             # sibling clone, read-only
├── meshcore-companion-aegg-rust/   # sibling clone, read-only
└── ssd1675/                        # sibling clone (slow-but-clear)
```

Siblings are **independent of the build** — switching their branches
or running `git pull` in them doesn't affect anything Cargo compiles.
That makes them safe spaces to study upstream history, hunt for
patterns, or temporarily check out an unrelated branch.

## Common gotchas

- **`vendor/<name>` shows as modified after a build / merge.**  Usually
  one of: (a) the submodule moved during a merge and someone forgot
  `--recurse-submodules`; (b) you have local edits in the submodule.
  `git diff vendor/<name>` from the parent tells you which.
- **`fatal: no submodule mapping found in .gitmodules`.**  You're on a
  branch that predates a submodule.  `git checkout` a newer branch or
  `git submodule update --init` after switching back.
- **CI failures with "could not find Cargo.toml in vendor/…".**  CI is
  cloning shallow / without submodules.  Add `submodules: recursive`
  (GitHub Actions / Forgejo Actions) or pass `--recurse-submodules` to
  the clone step.
- **Detached-HEAD commits get lost.**  Always make a branch in the
  submodule (`git checkout -b ...`) before editing, and push it
  somewhere before bumping the parent's pin.
