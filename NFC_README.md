# NFC Signed Channel — Protocol & Reader Implementation Guide

This document describes the authenticated NFC channel between the
CyberÆgg badge firmware and a reader (e.g. the BadgeCtl Android app).
It is intended both as an introduction for first-time readers and as a
spec for anyone who wants to write a third-party reader.

The badge is open-source hardware. There is no shared secret on the
PCB. Authenticity is proven with public-key signatures (Ed25519); the
private key lives only in the reader.

---

## 1. What this is — for first-year students

The channel is **not** encryption. It is **signing**.

| | Symmetric | Asymmetric |
|---|---|---|
| **Encryption** (hide content) | AES, ChaCha20 — both sides same key | RSA, ECIES — public key encrypts, private decrypts |
| **Signing** (prove author) | HMAC — both sides same key | **Ed25519, RSA-PSS — private signs, public verifies** |

We use the **asymmetric-signing** box (bottom right). The badge holds
only the public verifying key; the matching private signing key lives
in the reader app.

Why not encryption? Because:

- We do not need to hide the command bytes. A sniffer who sees
  `"more food"` learns nothing useful.
- We need *authenticity* — only the rightful reader can make the
  badge act.
- The badge cannot keep secrets (open-source firmware, exposed debug
  port). A shared secret would be extracted in minutes.

Public-key signing fits perfectly: the reader proves it knows the
private key without revealing it; the badge proves it understood by
acting on the command.

### Replay protection — challenge-response

Even with signatures, a sniffer can record `"more food" + signature`
and replay it later. The bytes still verify against the public key.

To stop replays, the badge ties each command to a fresh random
*challenge* it picks itself. The reader signs `challenge || command`
rather than just `command`. A recorded signature is over yesterday's
random number, not today's, so replays fail.

The full handshake:

```
Reader                          Badge
──────                          ─────
                                (challenge slot empty)

  ── GET CHALLENGE ─────────►
                                1. Pull 16 random bytes from CSPRNG
                                2. Stash in session.pending_challenge
  ◄── 16 random bytes ──

  3. sig = Ed25519_sign(privkey, challenge ‖ "more food")

  ── SIGNED CMD ───────────►
     ["more food" || sig]       4. session.pending_challenge.take()
                                   (consumed regardless of outcome)
                                5. verify_strict(challenge ‖ "more food", sig)
                                6. If OK → run command → 90 00
                                   Else  → 69 82 / 6A 88 / 67 00
  ◄── 90 00 / 69 82 / … ──
```

What each piece prevents:

| Defence | Stops |
|---|---|
| Public-key signing | Forgery — attacker can't produce new valid signatures without the private key |
| Random challenge | Replay — yesterday's recording can't satisfy today's challenge |
| Challenge consumed on first verify (success or fail) | Online brute force against a known challenge |
| Challenge cleared on NFC deselect | Cross-session reuse |
| `verify_strict` (not plain `verify`) | Ed25519 signature malleability edge cases |

What is **not** protected:

- **Confidentiality** — sniffers see plaintext commands. Out of scope.
- **Reader theft** — whoever has the unlocked phone has the private
  key.
- **Badge tampering** — physical access to the firmware lets you
  replace the public key with one you control.

---

## 2. Wire format

### Transport

ISO 14443-A, Type 4 tag emulation. Standard ISO-DEP (T=CL) framing,
short-form APDU.

### APDU 1 — `GET CHALLENGE`

```
CLA   INS   P1    P2    Le
0x80  0x02  0x00  0x00  0x00
```

No data field. `Le=0` requests a "give me what you have" response.

**Response body (16 B) + status word (2 B):**

```
[challenge:16] [SW1 SW2]
              = 0x90 0x00 on success
```

The 16 returned bytes are the freshly generated challenge. The badge
also stores them internally (`session.pending_challenge`) so it can
recompute the signed message on the next SIGNED CMD.

A previously armed (but unconsumed) challenge is overwritten by the new
one — only the most recent challenge is valid.

### APDU 2 — `SIGNED CMD`

```
CLA   INS   P1    P2    Lc    [data: plaintext || signature]    Le
0x80  0x01  0x00  0x00  N     [variable]                        0x00
```

Data field layout (length = `Lc`):

```
[plaintext: 1+ bytes] [signature: 64 bytes]
```

Constraints:

- Plaintext is at least **1 byte** and at most `Lc - 64` bytes.
- Lc ≥ 65; minimum total APDU length is `4 + 1 + 1 + 64 + 1 = 71` bytes.
- Lc ≤ 255 (short-form APDU).
- `Le = 0x00` requests "any length" response.

The signature covers `challenge || plaintext` — concatenation, no
separator, no length prefix. The reader signs this exact byte string
with the Ed25519 private key whose public counterpart is on the badge.

**Response (plaintext):**

```
[response body, 0+ bytes] [SW1 SW2]
```

For station commands that succeed, the body is empty and the SW is
`90 00`. The badge surfaces success/failure to the user via on-screen
toast, not via response bytes.

### Status words

| SW | Constant | Meaning |
|---|---|---|
| `90 00` | success | command accepted and applied |
| `67 00` | wrong length | frame too short or otherwise unparseable |
| `69 82` | sec status | bad signature |
| `6A 82` | not found | plaintext didn't match a known command, or precondition failed (no active pet, cooldown) |
| `6A 88` | ref data not found | no challenge armed — caller must issue GET CHALLENGE first |

`69 82` and `6A 82` are deliberately distinct in this implementation
even though some channels collapse them; readers should treat both as
"reissue GET CHALLENGE and try again" — the failure context is
separate from cryptographic verification status.

---

## 3. Cryptography details

| Item | Value |
|---|---|
| Signature algorithm | Ed25519 (RFC 8032) |
| Public key length | 32 bytes |
| Private key (seed) length | 32 bytes |
| Signature length | 64 bytes |
| Verification mode | `verify_strict` — rejects malleable encodings |
| Hash inside Ed25519 | SHA-512 (built into Ed25519) |
| Challenge length | 16 bytes |
| Challenge entropy source | nRF52840 hardware TRNG seed → SHA-256 hash chain |

**Authorised public key** (32 bytes, hex):

```
18de9db065d8efedb636fb88a23f779ac9560d31762ca51c2ac994498f8db6ef
```

Source: `src/signed_channel.rs::AUTHORIZED_PUBLIC_KEY`. To deploy a
reader of your own, change this constant in the firmware to your
reader's public key, rebuild, and reflash. There is no runtime way to
add or rotate keys.

### Challenge generator

The badge seeds a SHA-256-based hash-chain CSPRNG once at boot from
the on-chip TRNG (direct register access, before the BLE softdevice
takes the RNG peripheral). Each `next_challenge()` call:

```
state[t+1] = SHA-256(state[t])
challenge  = state[t+1][0..16]
```

This is forward-secure — observing every emitted challenge does not
let an attacker recover the state or predict future outputs without
breaking SHA-256.

Source: `src/signed_channel.rs::Csprng`.

---

## 4. Recognised commands

> **See also:** [CLOCK.md](CLOCK.md) for the alarm/calendar system that these commands interact with.

The signed-channel dispatcher routes the verified plaintext through
`game::station::apply`. Currently recognised phrases (UTF-8, exact
byte match after lowercasing and ASCII-whitespace trimming):

| Plaintext | Effect on badge |
|---|---|
| `more food` | Set `hunger = 0` |
| `more drugs` | Set `sick = 0` |
| `more inspiration` | Set `drained = 0` |
| `sleep like a bear` | Set `tired = 0` |

Each command has a separate 5-minute cooldown. A second tap within
the cooldown returns `6A 82` and the badge displays a "wait Ns" toast.

Anything else returns `6A 82`. To extend the vocabulary, add a branch
to `src/game/station.rs::apply` and rebuild.

---

## 5. Plaintext / NDEF coexistence

The badge also serves an NFC Forum Type 4 NDEF tag at `CLA=0x00` for
phone-side tag readers (the URL `https://badge.team`). This path is
**unauthenticated** and uses the standard SELECT / READ BINARY /
UPDATE BINARY APDUs.

By default the plaintext path does **not** drive station commands —
stations are signed-channel only. A build may opt in with the
`nfc-plaintext-station` Cargo feature, after which writing certain text
records via UPDATE BINARY also triggers `station::apply`, so the same
effects can be achieved without signing — at the cost of allowing
anyone with NFC to drive the badge. Enable it only for events that hand
out buffs via plain NFC-writable tags rather than the authenticated
reader. The signed channel is the secured path for trusted readers.

The `token:` UPDATE BINARY path is unaffected by this feature and stays
on regardless.

The dispatcher (`src/fw/nfct.rs`) selects between paths purely on
`(CLA, INS)`:

- `(0x80, 0x01)` → SIGNED CMD
- `(0x80, 0x02)` → GET CHALLENGE
- anything else → plaintext NDEF dispatcher

---

## 6. Implementing a reader

### 6.1 What you need

- An NFC reader capable of ISO 14443-A Type 4 / ISO-DEP. Examples:
  - Android phone with NFC (use `android.nfc.tech.IsoDep`).
  - USB PCSC reader (e.g. ACR122U) on a host machine; libraries:
    - Python: `pyscard`
    - Java: `javax.smartcardio`
    - Rust: `pcsc` crate
- An Ed25519 signing implementation:
  - Kotlin/Java: `org.bouncycastle.crypto.signers.Ed25519Signer`
  - Python: `cryptography` (`Ed25519PrivateKey`) or `pynacl`
    (`SigningKey`)
  - Rust: `ed25519-dalek`
- The matching **private** key for the public key compiled into the
  badge. Without it, no valid SIGNED CMD can be constructed.

### 6.2 Algorithm — language-agnostic

```
1. Open ISO-DEP connection to the badge (Type 4 Tag, ISO 14443-A).

2. Send GET CHALLENGE APDU:
       80 02 00 00 00
   Read 18 bytes back.
   Verify last two bytes are 90 00.
   Take first 16 bytes as `challenge`.

3. Build the message to sign:
       signed_msg = challenge ‖ plaintext      (e.g. plaintext = b"more food")

4. Compute Ed25519 signature:
       signature = Ed25519_sign(private_key, signed_msg)        // 64 bytes

5. Build SIGNED CMD APDU:
       Lc       = len(plaintext) + 64
       data     = plaintext ‖ signature
       APDU     = 80 01 00 00 [Lc] [data] 00

6. Send APDU, read response.
       Last two bytes = SW1 SW2.
       If SW == 90 00 → success.
       Otherwise consult the status-word table.

7. (Optional) Persist the fact that you've talked to this badge. No
   counter state needs to survive across taps; each command is fully
   independent.

8. Disconnect. The badge clears its pending challenge automatically
   on the next ISO-DEP activation.
```

### 6.3 Reference — Kotlin (Android)

The shipped reader is `android_nfc/BadgeCtl/app/src/main/java/com/example/badgectl/MainActivity.kt`.
Distilled core:

```kotlin
import android.nfc.tech.IsoDep
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer

fun sendSigned(isoDep: IsoDep, privateKeySeed: ByteArray, plaintext: ByteArray): ByteArray {
    isoDep.connect()
    isoDep.timeout = 2000

    // 1. GET CHALLENGE
    val getCh = byteArrayOf(0x80.toByte(), 0x02, 0x00, 0x00, 0x00)
    val chResp = isoDep.transceive(getCh)
    require(chResp.size >= 2) { "GET CHALLENGE response too short" }
    require(chResp[chResp.size - 2] == 0x90.toByte() && chResp[chResp.size - 1] == 0x00.toByte()) {
        "GET CHALLENGE not 90 00"
    }
    val challenge = chResp.copyOfRange(0, chResp.size - 2)
    require(challenge.size == 16) { "Expected 16-byte challenge" }

    // 2. Sign challenge || plaintext
    val signedMsg = challenge + plaintext
    val signer = Ed25519Signer().apply {
        init(true, Ed25519PrivateKeyParameters(privateKeySeed, 0))
        update(signedMsg, 0, signedMsg.size)
    }
    val signature = signer.generateSignature()
    require(signature.size == 64) { "Ed25519 signature must be 64 bytes" }

    // 3. SIGNED CMD
    val payload = plaintext + signature
    require(payload.size <= 255) { "Payload too long for short-form APDU" }
    val signedCmd = byteArrayOf(
        0x80.toByte(), 0x01, 0x00, 0x00,
        payload.size.toByte(),
        *payload,
        0x00
    )
    val response = isoDep.transceive(signedCmd)
    isoDep.close()
    return response  // last two bytes are SW1 SW2
}
```

### 6.4 Reference — Python (pyscard + cryptography)

```python
from smartcard.System import readers
from smartcard.util import toBytes
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

def send_signed(connection, priv_seed: bytes, plaintext: bytes) -> tuple[bytes, int, int]:
    # 1. GET CHALLENGE — APDU: 80 02 00 00 00
    data, sw1, sw2 = connection.transmit(toBytes("80 02 00 00 00"))
    if (sw1, sw2) != (0x90, 0x00):
        raise RuntimeError(f"GET CHALLENGE failed: SW={sw1:02X}{sw2:02X}")
    challenge = bytes(data)
    assert len(challenge) == 16

    # 2. Sign challenge || plaintext
    sk = Ed25519PrivateKey.from_private_bytes(priv_seed)
    signature = sk.sign(challenge + plaintext)
    assert len(signature) == 64

    # 3. SIGNED CMD — APDU: 80 01 00 00 Lc <data> 00
    payload = plaintext + signature
    if len(payload) > 255:
        raise ValueError("Payload too long for short-form APDU")
    apdu = [0x80, 0x01, 0x00, 0x00, len(payload)] + list(payload) + [0x00]
    data, sw1, sw2 = connection.transmit(apdu)
    return bytes(data), sw1, sw2


# Example usage
r = readers()[0]
conn = r.createConnection()
conn.connect()

priv_seed = open("signing_key.bin", "rb").read()  # 32 bytes
body, sw1, sw2 = send_signed(conn, priv_seed, b"more food")
print(f"SW={sw1:02X}{sw2:02X}, body={body.hex()}")
```

### 6.5 Reference — Rust (host, pcsc + ed25519-dalek)

```rust
use ed25519_dalek::{SigningKey, Signer};
use pcsc::{Context, Scope, ShareMode, Protocols};

fn send_signed(card: &pcsc::Card, priv_seed: &[u8; 32], plaintext: &[u8]) -> anyhow::Result<(Vec<u8>, u16)> {
    // 1. GET CHALLENGE
    let mut buf = [0u8; 260];
    let resp = card.transmit(&[0x80, 0x02, 0x00, 0x00, 0x00], &mut buf)?;
    anyhow::ensure!(resp.len() >= 2);
    let sw = u16::from_be_bytes([resp[resp.len() - 2], resp[resp.len() - 1]]);
    anyhow::ensure!(sw == 0x9000, "GET CHALLENGE: SW={:04X}", sw);
    let challenge = &resp[..resp.len() - 2];
    anyhow::ensure!(challenge.len() == 16);

    // 2. Sign
    let signing_key = SigningKey::from_bytes(priv_seed);
    let mut signed_msg = Vec::with_capacity(16 + plaintext.len());
    signed_msg.extend_from_slice(challenge);
    signed_msg.extend_from_slice(plaintext);
    let signature = signing_key.sign(&signed_msg);

    // 3. SIGNED CMD
    let mut payload = Vec::with_capacity(plaintext.len() + 64);
    payload.extend_from_slice(plaintext);
    payload.extend_from_slice(&signature.to_bytes());
    anyhow::ensure!(payload.len() <= 255);

    let mut apdu = vec![0x80u8, 0x01, 0x00, 0x00, payload.len() as u8];
    apdu.extend_from_slice(&payload);
    apdu.push(0x00);

    let resp = card.transmit(&apdu, &mut buf)?;
    let sw = u16::from_be_bytes([resp[resp.len() - 2], resp[resp.len() - 1]]);
    Ok((resp[..resp.len() - 2].to_vec(), sw))
}
```

---

## 7. State on the reader side

There is **no counter to persist**. Each command is a fresh
challenge-response round, statelessly accepted by the badge.

You may freely:

- Run multiple readers with the same private key on the same badge.
- Reinstall a reader app without losing the ability to talk to the
  badge.
- Power-cycle the badge between commands.

The only state the badge keeps in RAM is the most recent unconsumed
challenge, which is wiped on every NFC field activation (i.e. every
new tap).

---

## 8. Test vectors

The unit tests in `src/signed_channel.rs::tests` use a deterministic
test keypair:

```
seed (32 B):
  de ad be ef fe ed fa ce 00 11 22 33 44 55 66 77
  88 99 aa bb cc dd ee ff 12 34 56 78 9a bc de f0
```

Derive the public key with any Ed25519 implementation:

```python
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
seed = bytes.fromhex("deadbeeffeedface00112233445566778899aabbccddeeff123456789abcdef0")
sk = Ed25519PrivateKey.from_private_bytes(seed)
pk = sk.public_key().public_bytes(
    encoding=serialization.Encoding.Raw,
    format=serialization.PublicFormat.Raw,
)
print(pk.hex())
```

To round-trip a frame against a Session for cross-implementation
testing:

1. Construct a `Session::new(pk)` with the derived public key.
2. Call `session.arm(challenge)` with a fixed 16-byte challenge.
3. Build `signed_msg = challenge || plaintext`, sign with `sk`.
4. Concatenate `frame = plaintext || signature`.
5. Call `session.verify_in_place(&frame)`. Expect `Ok(plaintext)`.

The shipped tests cover: round-trip with consumption, wrong-challenge
rejection, signature tampering, undersize frame, and missing
challenge.

---

## 9. Security caveats and rough edges

- **Private-key custody.** The reader app stores the private key in
  application-private storage (`raw/signing_key.bin` on Android,
  inside the app's signed APK). Anyone with root/jailbreak on the
  reader can extract it. Treat the private key like a physical key.
- **No key rotation in firmware.** The public key is a `const`. Key
  changes require a firmware reflash.
- **No multi-key support.** Exactly one authorised key. Multiple
  readers share the same private key.
- **CSPRNG depends on a single seed event.** If `seed_from_hardware`
  panics or is skipped, the badge will refuse to issue challenges.
- **Outer SW only — no body integrity for replies.** The badge does
  not sign its responses. Readers should not trust reply bodies as
  authoritative confirmation; rely on out-of-band observation
  (e.g. the badge displaying a toast).
- **No tag-level binding.** A reader that talks to badge A and then
  to badge B uses the same private key for both; the badge identity
  is implicit in which physical device is in the field.

---

## 10. File reference

| File | Role |
|---|---|
| `src/signed_channel.rs` | `Session`, `verify_in_place`, `Csprng`, public key constant |
| `src/fw/nfct.rs` | `handle_get_challenge`, `handle_signed`, dispatcher |
| `src/fw/iso14443.rs` | ISO-DEP (T=CL) layer |
| `src/game/station.rs` | Plaintext command dispatch |
| `src/bin/embassy.rs` | Boot order, `Csprng::seed_from_hardware()` |
| `android_nfc/BadgeCtl/app/src/main/java/com/example/badgectl/MainActivity.kt` | Reference reader |

---

## 11. References

- RFC 8032 — Edwards-Curve Digital Signature Algorithm (EdDSA)
- ISO/IEC 7816-4 — APDU format and status words
- ISO/IEC 14443-4 — Type 4 tag protocol (T=CL / ISO-DEP)
- NFC Forum Type 4 Tag Operation Specification
