# Contacts screen + PM inbox — meshcore on-badge UI

> **See also:** [README.md](README.md) for project overview, [CLOCK.md](CLOCK.md) for the watch/alarm/calendar app, [GAME.md](GAME.md) for player-facing game instructions, [GAMES.md](GAMES.md) for mini-game developer reference, [NFC_README.md](NFC_README.md) for NFC signed channel protocol, [HWTEST.md](HWTEST.md) for the factory hardware-test firmware, [License.md](License.md) for licensing.

The badge is more than a passive Bluetooth companion — it has a working
chat UI on its own.  This doc captures the design and current behaviour
of the meshcore-related on-device screens: a discovery-sorted **Contacts**
view that surfaces saved contacts and recently-heard adverts side by
side, and a **PM inbox** with per-peer threads.

It supersedes the original `SCREEN_ADVERT` (single-record "last advert
seen") and `SCREEN_PM` (single-record "last received message").

## Goals

- **Discovery first.**  At a hacker camp the badge's most useful job is
  "who is near me right now?"  The screen sorts so live nodes float to
  the top.
- **One-press to chat.**  Friend's badge sends an advert → it appears in
  the list → two button presses (Fire on the row, Fire on PM) opens the
  compose keyboard.
- **Stay consistent with the rest of the badge.**  Up/Down scrolls,
  Left/Right is the global screen-swipe carousel — no per-screen
  rebinding of nav keys.  Per-contact actions live behind a popup
  ("click").
- **Curate-friendly.**  Auto-discovered contacts sit alongside saved
  ones; explicit Save/Add promotes them to the persistent address book,
  Forget removes.

---

## 1. Contacts screen (`fw::mesh::contacts_screen`)

Sources merged into one list:

- **Persistent `ContactStore`** (300-slot kv-backed flash storage).
  Saved entries — those manually added via the BLE companion or
  promoted via the popup's `Add` action.
- **`fw::mesh::discovery::CACHE`** (32-entry RAM ring, per-boot).
  Adverts heard but not yet promoted to the persistent store.
  Populated by `meshcore::log_advert` for every received advert.
- **`OBSERVATIONS`** (64-entry RAM ring, per-boot).  Local
  `(pub_key → seconds-since-boot)` so "Last:" rendering doesn't depend
  on the sender's clock (most badges advertise `timestamp = 0` until
  their wall clock is seeded).

The Contacts screen merges all three at cache-rebuild time into
`CACHED_CONTACTS` (50-entry sync-readable in-RAM cache) and renders
from there.

### Row layout (~18 px tall, 7 rows visible)

```
┌────────────────────────────────────────────────┐
│ Contacts                                [85%]  │  ← header
├────────────────────────────────────────────────┤
│ ●  *alice                                 3m   │  ← saved + favorite
│       bob                                12m   │  ← saved, not favorite
│ ●  +carol                                 2m   │  ← discovery (heard, not saved)
│    R  borncamp-rep                       1h    │  ← repeater (role glyph)
│       dave                              ydy    │
└────────────────────────────────────────────────┘
```

| Element                  | When                                                      |
|--------------------------|-----------------------------------------------------------|
| Live dot ●               | observed within the last 5 minutes (red)                  |
| Role glyph (R / # / S)   | only when `node_type ≠ Chat Node`                         |
| `*` prefix               | saved + `FLAG_FAVORITE` set                               |
| `+` prefix               | discovery row (not yet in `ContactStore`)                 |
| Name (truncated)         | always — `(unknown)` fallback for empty names             |
| Last-seen (right-edge)   | `now / 3m / 1h / ydy / 3d / 9w` from local observation    |

Sort: heard-this-session entries float above never-heard entries; within
each group, by recency.

### Filtering

Up while the cursor is at row 0 opens a **Filter picker** overlay (same
look as the action popup):

| Filter      | Shows                                |
|-------------|--------------------------------------|
| **All**     | Everything (default)                 |
| Favorites   | Only saved + `FLAG_FAVORITE`         |
| People      | Only `node_type == Chat Node`        |
| Repeaters   | Only `node_type == Repeater`         |
| Rooms       | Only `node_type == Room Server`      |
| Sensors     | Only `node_type == Sensor`           |

Filter resets on screen exit (discovery-first default).  Active filter
is appended to the header: `Contacts · Repeaters`.

### Buttons (list view)

| Button       | Action                                                 |
|--------------|--------------------------------------------------------|
| Up           | Scroll up — at row 0, opens the filter picker          |
| Down         | Scroll down                                            |
| Left / Right | **Screen-swipe to neighbour screen — unchanged.**      |
| Fire / Execute | Open per-contact popup                               |
| Cancel       | Back to home / main screen                             |

### Per-contact popup

Modal overlay built from the row's `(node_type, is_saved, is_favorite)`.
Index 0 is preselected.  Picker is scrollable: caps at 5 visible rows
with `^` / `v` chevrons in the right margin (`ui::draw_picker_menu`).

| Saved? | Role            | Items (primary → cancel)                        |
|--------|-----------------|-------------------------------------------------|
| no     | Chat Node       | **Add** · PM · Info · Cancel                    |
| no     | Repeater/Room/Sensor | **Add** · Info · Cancel                    |
| yes    | Chat Node       | **PM** · Info · Save\|Unsave · Forget · Cancel  |
| yes    | Repeater/Room/Sensor | **Info** · Save\|Unsave · Forget · Cancel  |

Actions (`PopupAction` enum):

- **PM** — opens the on-screen keyboard primed for compose to that
  pub_key.  Submit builds a `TxPrivateMsg`, mirrors the outgoing entry
  into the PM inbox, and pushes via `tx_send`.  Works for *any* chat
  node (saved or discovery).
- **Info** — drills into the per-contact detail view.
- **Save / Unsave** — toggles `FLAG_FAVORITE` via
  `ContactStore::set_favorite`.  Saved-state rows that were never in the
  store get implicit-saved by the very act of toggling `FLAG_FAVORITE`
  on a slot that exists; saved-no-favorite rows just flip the flag.
- **Add** — promotes a discovery row by building a `Contact` from the
  cached advert metadata and calling `ContactStore::add_or_update`.
- **Forget** — deletes the slot via `ContactStore::delete`.
- **Cancel** — close popup.

Mutations bridge sync-vs-async via `MUTATION_QUEUE` (a 4-deep channel)
drained by `mutation_persister_task`.  The cache also receives a
synchronous in-place edit (`cached_with` / `cached_remove`) so the UI
reflects the change instantly without waiting on flash.

### Detail view (Info)

Reachable from the popup's Info item.  Single-screen layout:

```
┌────────────────────────────────────────────────┐
│ alice                                   [85%]  │
├────────────────────────────────────────────────┤
│  Chat Node                                     │
│  ──────────────────────────────────────────    │
│  Last: 3m                                      │
│  Hops: 2                                       │
│  Key: 573b0ec30476993d                         │
│  GPS: 55.612N 12.999E                          │
└────────────────────────────────────────────────┘
```

`Hops:` reads `out_path_len & 0x3F`; shows `?` when path is unknown
(flood-only) or `0 (direct)` for direct neighbours.  `GPS:` only
appears when the advert carried a position.  Key is the first 8 bytes
of the pub_key in lowercase hex on a single line (`crate::hex_prefix`).

| Button         | Action                                       |
|----------------|----------------------------------------------|
| Fire / Execute | Open PM thread (chat nodes only)             |
| Left / Right   | Prev / next contact in the filtered view     |
| Cancel         | Back to the Contacts list                    |

---

## 2. PM inbox + per-peer threads (`fw::mesh::pm_inbox`)

Replaces the old single-record `SCREEN_PM`.  RAM ring of recent PMs
(incoming + outgoing combined), grouped per peer.

- `INBOX` — heapless Vec, cap 32.  FIFO eviction by observation time.
  Per-boot.
- `READ_CURSORS` — heapless Vec, cap 16.  Per-peer "last read" timestamp
  for unread-badge tracking.

Wiring:

- **Incoming.**  `meshcore::log_advert`'s plain-TxtMsg branch calls
  `pm_inbox::note_incoming(pub_key, peer_name, text)` after the existing
  `verify_mac` check.
- **Outgoing.**  `contacts_screen::on_pm_compose_done` mirrors the
  outgoing entry into the inbox **before** `tx_send` so the user's own
  thread shows what they tried to send (even if the queue rejected it).
  The BLE companion's `SEND_TXT_MSG` path also mirrors phone-originated
  PMs.

### Inbox view

One row per distinct peer, sorted by latest-message time.  Two visible
sub-rows per peer (peer name + preview line beneath).  `^` / `v`
chevrons in the right margin when more peers exist than fit.

```
┌────────────────────────────────────────────────┐
│ Messages                                [85%]  │
├────────────────────────────────────────────────┤
│ alice                                  (2)     │  ← unread count
│ < hello, are you at C-camp?                    │
│ bob                                            │
│ > sure, on my way!                             │  ← last sent by us
└────────────────────────────────────────────────┘
```

### Thread view

Selected via Fire on a peer.  `mark_read` clears the unread badge.
Header shows `PM: <name>` (truncated to 16 chars).

Per-message layout: arrow + 3-char relative time prefix on the same row
as the first body chunk; continuation lines indent under the body.
Word-aware wrapping breaks on space boundaries when possible (hard-break
only for words longer than the line width).  Footer hint
`Fire reply  Esc back`.

```
< 3m  hello there how
      are you doing
      today?
> 1m  sure, on my way!
< 5m  ok see u there
```

### Buttons

| Mode    | Button         | Action                              |
|---------|----------------|-------------------------------------|
| Inbox   | Up / Down      | Scroll peers                        |
| Inbox   | Fire / Execute | Open thread, mark read              |
| Inbox   | Cancel         | Back to home                        |
| Inbox   | Left / Right   | Screen swipe                        |
| Thread  | Up / Down      | Scroll within long thread           |
| Thread  | Fire / Execute | Reply (opens compose keyboard)      |
| Thread  | Cancel         | Back to inbox                       |
| Thread  | Left / Right   | Screen swipe                        |

Reply reuses `contacts_screen::start_pm_compose` so the sync-vs-async
bridge stays in one place.

---

## 3. Cache rebuild + energy

`refresh_cache_task` (Embassy task) rebuilds `CACHED_CONTACTS` on
demand.  Wakes on `REBUILD_SIGNAL` (a dedicated single-waiter signal,
separate from `ADVERT_SIGNAL` to avoid waker-overwrite races with the
UI redraw loop).  Debounces bursts with a 1 s quiet-window.

Two paths controlled by `contacts::STORE_DIRTY`:

- **Slow path** (300-slot kv rescan): when the persistent store has
  actually been mutated.  Set by every `ContactStore` mutation method
  (`add_or_update`, `set_favorite`, `delete`, `update_path`,
  `update_sync_since`, `clear_all`).
- **Fast path** (RAM-only): keep saved rows from the existing cache,
  refresh `observed_at_secs` from `OBSERVATIONS`, re-merge discovery
  overlay.  Zero flash I/O.  Used for advert-driven rebuilds, which is
  the common case.

This means an advert burst at a busy event no longer triggers ~300 ms
of QSPI activity per rebuild — saved-row metadata is updated only when
the store actually changes.

`mark_dirty` (in `contacts.rs`) signals `REBUILD_SIGNAL` automatically,
so BLE-companion-driven mutations also wake the refresh task without
needing an `ADVERT_SIGNAL` to coincide.

---

## Implementation map

| Module                              | Role                                                |
|-------------------------------------|-----------------------------------------------------|
| `fw::mesh::contacts`                | Persistent slot store, mutation methods, dirty flag |
| `fw::mesh::contacts_screen`         | Contacts list + popup + detail + filter picker      |
| `fw::mesh::discovery`               | RAM ring of recently-heard advert metadata          |
| `fw::mesh::pm_inbox`                | PM ring, peer summaries, inbox/thread state machine |
| `fw::mesh::time_fmt`                | `fmt_relative_secs` shared by both screens          |
| `lib::hex_prefix`, `lib::truncate_str` | Shared formatting / UTF-8-safe truncation        |
| `ui::draw_picker_menu`              | Scrollable picker overlay (popup, filter, threads)  |

## Out of scope (for the current PR)

- **LED chirp on new advert** — Settings toggle, off by default.  Easy
  follow-up.
- **Passive-screen `+N new` indicator** — same.
- **Sensor "Read" action** — needs sensor-payload support upstream in
  the meshcore crate.
- **Room Server "Join room" deep-link** — currently the popup only
  exposes Info/Save/Forget for room servers; the join flow re-uses the
  existing channel browser.
- **Persistent unread tracking** — `READ_CURSORS` is per-boot.  Could
  move to flash if the freshness vs. wear trade-off makes sense.
