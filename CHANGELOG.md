# Changelog

CyberÆgg badge firmware (`Ranzbak/bornhack-firmware-2026-ce`) and its MeshCore
crates (`Ranzbak/meshcore-aegg-rust` → `vendor/meshcore`,
`Ranzbak/meshcore-companion-aegg-rust` → `vendor/meshcore-companion`).

Newest first. Commit hashes refer to the firmware repo unless a crate is named.
The earlier one-off changelog is in
[CHANGELOG-2026-07-14-to-16.md](CHANGELOG-2026-07-14-to-16.md).

## MeshCore 1.16 → 1.18 — 2026-07-27 … 2026-11-11

Brings the badge from companion protocol v11 (MeshCore 1.15) to v14
(MeshCore 1.18), plus the 1.16/1.17 mesh and radio fixes and a few
post-1.18 upstream fixes. Pull requests: firmware #1, meshcore #3,
companion #2.

### Breaking
- **Companion error codes use upstream `ERR_CODE_*` numbering** (`da265a9`,
  companion `b764170`): UNSUPPORTED_CMD 1, NOT_FOUND 2, TABLE_FULL 3,
  BAD_STATE 4, FILE_IO_ERROR 5, ILLEGAL_ARG 6. Every error reply now
  matches what upstream `MyMesh` answers for the same input.
- **`PUSH_CODE_ADVERT` (0x80) is `[0x80][pub_key:32]`** like upstream
  (companion `263b858`).
- **`transport_codes[1]` is kept on parse and re-encode** instead of
  being zeroed (meshcore `c00c3e9`).

### Added
- **Companion protocol v14** (`e76a6aa`, companion `d0446cd`): the
  commands upstream gated behind versions 12–14, including un-scoped
  floods (`SET_FLOOD_SCOPE` 0x01), the default flood scope, and text CLI
  over the companion link (`RUN_CLI_COMMAND`).
- **Remaining 1.18 commands the badge can answer** (`7a9db5e`, companion
  `c42a527`): `EXPORT_CONTACT` for our own advert, `SIGN_START/DATA/FINISH`
  (up to 8 KiB, signed with the device key), `GET_CUSTOM_VARS` (empty),
  honest `HAS_CONNECTION`/`LOGOUT`, and `GET_STATS` core and packet
  counters (the radio now counts sent, received and CRC-failed packets).
- **`CMD_SEND_RAW_DATA` (0x19)** (`0f2397e`, companion `34b3c49`), and
  `CMD_GET_DEVICE_TIME` (0x05) (companion `9d3528e`).
- **`TXT_TYPE_CLI_COMMAND`** (MeshCore 1.18) on the air (meshcore
  `9e1ea77`). Incoming CLI messages go to the app; the badge does not run
  them itself.
- **Six-byte receiver ACKs for plain messages** like MeshCore 1.16
  (`ffef23b`, meshcore `a4584bb`), so each retry gets a distinct ACK that
  repeaters don't drop as a duplicate.
- **Region support**:
  - `ANON_REQ`, so the app's "Discover Regions" works (`f4ccdde`,
    companion `32596c0`).
  - The incoming region filter is optional and off by default (`10e9488`),
    as upstream companions never filter by region.
  - Per-channel region override; the region filter applies per channel
    (`9515a65`).
  - The global region is kept separate from the app's live send scope
    (`be28b84`).
- **Half- and quarter-hour UTC offsets** like MeshCore 1.18 (`73fd0f3`):
  `set tz.offset 5.5` / `5.75`; the menu shows `UTC+5:30` and steps one
  hour. The old whole-hour setting carries over.
- **Conformance tests against upstream C++**: differential vectors for
  the mesh crate (meshcore `65a76f4`) and 174 companion command steps from
  companion-v1.18.0 (companion `00d54de`). See `SUBMODULES.md` (`c1c7d18`).

### Fixed
- **Adverts carried timestamp 0** (`32657e0`). Upstream drops an advert
  that isn't newer than the last one it saw, so nodes that already knew
  the badge ignored its name changes and new paths. Adverts now use the
  clock.
- **Contacts with an all-zero public key are refused** (`fc8e2e0`, from
  upstream dev 060fd219). Such a record could take a slot, or evict a real
  contact when the store was full.
- **Boost RX gain applies straight away** (`06c0d57`) instead of at the
  next transmit. Frequency, bandwidth, SF, CR and TX power were already
  applied live.
- **Channel hash collisions** (`ff4caae`, meshcore `267bbf8`): every
  channel whose one-byte hash matches is tried until a MAC verifies, so a
  second channel with a colliding hash receives its messages.
- **Unsolicited responses** reach the app as `PUSH_CODE_BINARY_RESPONSE`,
  and login replies only count for the node we logged in to (`e5dc39c`).
- **CLI text messages** are stamped with our own strictly increasing
  clock and expect no ACK, like the reference companion (`b7d661e`).
  This fixes repeater admin from the app timing out or tripping replay
  protection.
- **Flood-scope key** is derived from the region name with a leading `#`,
  like upstream (`5659334`, meshcore `dd91b54`).
- **Packet validation like upstream**:
  - invalid `path_len` encodings are rejected (`a9acba3`, meshcore
    `539bf69`);
  - packets with a reserved payload version are dropped (meshcore
    `331c721`);
  - group data is capped at 165 B (`MAX_GROUP_DATA_LENGTH`), and oversized
    `SEND_CHANNEL_DATA` is refused up front instead of being dropped
    silently (`b5c8668`, meshcore `b9d6d4d`);
  - advert app data is bounded to 32 bytes (meshcore `2385d63`), login
    passwords to 15 bytes (meshcore `ff25d85`), and replies to the
    upstream datagram bound (meshcore `bc1b743`);
  - text messages are composed and received like upstream
    `composeMsgPacket` (meshcore `4c32ca0`).
- **Companion command checks like upstream `handleCmdFrame`** (companion
  `004a442`, `be1cf94`, `7a52ff9`, `62626d8`, `3ac0940`, `823dce6`,
  `e8186ef`; firmware `927581d` … `d3fb469`):
  - `SEND_CHANNEL_DATA` field order, and 2- and 3-byte path hashes
    (`b9f97ec`);
  - `SET_RADIO_PARAMS` accepts frequencies down to 150 MHz (`5d6bf07`);
  - a channel send to a missing slot answers NOT_FOUND (`8cd1a17`);
  - `SEND_TXT_MSG` over the maximum length answers TABLE_FULL (`5699c72`);
  - `SET_DEVICE_TIME` into the past answers ILLEGAL_ARG (`2260990`);
  - `RESET_PATH` on an unknown contact answers NOT_FOUND (`927581d`);
  - `GET_STATS` with an unknown type answers ILLEGAL_ARG (`f773096`);
  - `ADD_UPDATE_CONTACT` answers OK only (`d3fb469`);
  - 256-bit `SET_CHANNEL`, `SET_DEFAULT_FLOOD_SCOPE`, trace-path and
    path-discovery length checks, and the `LOGIN_FAIL` reserved byte.

### Radio (SX1262)
- **MeshCore 1.17 RX watchdog and listen-before-talk** (`c31bfdb`). A
  false preamble detect no longer keeps the channel "busy" and defers
  every transmit by 4 s. The second receive phase now has a timeout.
- **Bounded TxDone wait** (`b2588c0`): a lost TxDone no longer hangs the
  mesh task, and RX is re-armed when a transmit fails.

### Known gaps
- An out-of-range `GET_CHANNEL` answers "no more messages"; upstream
  answers NOT_FOUND.
- The optional `since` timestamp on `GET_CONTACTS` is ignored, so every
  sync sends the full contact list.
- Not implemented: private-key export/import, device PIN, and running
  remote CLI commands for contacts with the remote-CLI permission flag.
