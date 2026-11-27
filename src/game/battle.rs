//! Mesh Battles: pets fighting over the private SHDW channel.
//!
//! The wire packets of a v3 fight (RESULT, CHALLENGE, ACCEPT, DECLINE) and
//! [`Session`], the challenge protocol as a pure state machine, one per
//! badge. The caller hands in the pet, the friend record and the clock and
//! gets back packets and UI effects ([`Out`]); nothing here reads a global,
//! so two sessions can be wired together in a test. Every timeout is driven
//! by the `now_ms` the caller passes in.

use super::Toast;
use super::engine::combat::{self, Decisive, SNAPSHOT_LEN, Snapshot};
use super::engine::doses::{Reason, Refusal};
use super::engine::thresholds as t;
use super::engine::{Fought, GameState, PET_NAME_MAX};
use super::friends::FriendRecord;

// ---------------------------------------------------------------------------
// Wire result — sent by a challenger whose target never answered (fallback)
// ---------------------------------------------------------------------------

/// Private `GrpData` `data_type` marking a Battle result — distinct from
/// `friends::PET_BEACON_TYPE`.
pub const PET_BATTLE_TYPE: u16 = 0xBA71;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleResultMsg {
    pub challenger_id: [u8; 2],
    pub target_id: [u8; 2],
    pub challenger_won: bool,
    pub challenger_hp_pct: u8,
    pub target_hp_pct: u8,
    /// Challenger's pet species (sprite `PP` prefix), or [`KIND_UNKNOWN`] when
    /// decoded from a legacy `MIN_SIZE` packet that predates this field.
    pub challenger_kind: u8,
    /// Target's pet species, or [`KIND_UNKNOWN`] (see above).
    pub target_kind: u8,
}

/// Legacy wire layout: `2 + 2 + 1 + 1 + 1`. Still accepted on decode so a badge
/// running the older firmware can battle a new one (see module docs / spec).
const MIN_SIZE: usize = 7;
/// Current wire layout: legacy + `challenger_kind + target_kind`.
const FULL_SIZE: usize = 9;

/// Sentinel species used when a legacy packet carries no `kind` bytes. Not a
/// real pet id.
pub const KIND_UNKNOWN: u8 = 0xFF;

impl BattleResultMsg {
    pub fn to_bytes(&self) -> [u8; FULL_SIZE] {
        let mut buf = [0u8; FULL_SIZE];
        buf[0..2].copy_from_slice(&self.challenger_id);
        buf[2..4].copy_from_slice(&self.target_id);
        buf[4] = self.challenger_won as u8;
        buf[5] = self.challenger_hp_pct;
        buf[6] = self.target_hp_pct;
        buf[7] = self.challenger_kind;
        buf[8] = self.target_kind;
        buf
    }

    /// Decode a result. Accepts both the current `FULL_SIZE` layout and the
    /// legacy `MIN_SIZE` one; the species fields come back as [`KIND_UNKNOWN`]
    /// when the packet is too short to carry them (trailing bytes beyond what
    /// we read are ignored, matching how the old parser tolerated extra data).
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < MIN_SIZE {
            return None;
        }
        let (challenger_kind, target_kind) = if buf.len() >= FULL_SIZE {
            (buf[7], buf[8])
        } else {
            (KIND_UNKNOWN, KIND_UNKNOWN)
        };
        Some(Self {
            challenger_id: [buf[0], buf[1]],
            target_id: [buf[2], buf[3]],
            challenger_won: buf[4] != 0,
            challenger_hp_pct: buf[5],
            target_hp_pct: buf[6],
            challenger_kind,
            target_kind,
        })
    }
}

// ---------------------------------------------------------------------------
// Wire packets of a v3 fight: CHALLENGE, ACCEPT, DECLINE (little-endian, ids
// first, like `BattleResultMsg`). A longer buffer is tolerated; a shorter one
// or an invalid snapshot decodes to `None`.
// ---------------------------------------------------------------------------

// There is no version field in these packets, so any future layout change
// needs new `data_type` values. Flag bit 22 (the plan-8 star) is refused on
// decode (`Snapshot::from_wire`), so plan 8 must either bump these types or
// accept that fights with a plan-5 badge fall back to the beacon.
pub const PET_CHALLENGE_TYPE: u16 = 0xBA72;
pub const PET_ACCEPT_TYPE: u16 = 0xBA73;
pub const PET_DECLINE_TYPE: u16 = 0xBA74;

/// `DeclineMsg::why` when the player pressed Decline; otherwise it is the
/// first half of `Reason::pack()`.
pub const WHY_DECLINED: u8 = 0xFE;

// Byte offsets: two ids (4 bytes), then the snapshot, then the rest.
const SNAP: usize = 4;
const NONCE: usize = SNAP + SNAPSHOT_LEN;
const KIND: usize = NONCE + 2;
const NAME: usize = KIND + 1;
const NAME_LEN: usize = NAME + PET_NAME_MAX;

const CHALLENGE_LEN: usize = NAME_LEN + 1;
const ACCEPT_LEN: usize = NONCE + 2;
const DECLINE_LEN: usize = 4 + 1 + 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChallengeMsg {
    pub challenger_id: [u8; 2],
    pub target_id: [u8; 2],
    pub snap: Snapshot,
    pub nonce: u16,
    pub kind: u8,
    pub name: [u8; PET_NAME_MAX],
    pub name_len: u8,
}

impl ChallengeMsg {
    pub fn to_bytes(&self) -> [u8; CHALLENGE_LEN] {
        let mut b = [0u8; CHALLENGE_LEN];
        b[0..2].copy_from_slice(&self.challenger_id);
        b[2..4].copy_from_slice(&self.target_id);
        b[SNAP..NONCE].copy_from_slice(&self.snap.to_wire());
        b[NONCE..NONCE + 2].copy_from_slice(&self.nonce.to_le_bytes());
        b[KIND] = self.kind;
        b[NAME..NAME_LEN].copy_from_slice(&self.name);
        b[NAME_LEN] = self.name_len;
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < CHALLENGE_LEN {
            return None;
        }
        let mut name = [0u8; PET_NAME_MAX];
        name.copy_from_slice(&b[NAME..NAME_LEN]);
        Some(Self {
            challenger_id: [b[0], b[1]],
            target_id: [b[2], b[3]],
            snap: Snapshot::from_wire(&b[SNAP..NONCE])?,
            nonce: u16::from_le_bytes([b[NONCE], b[NONCE + 1]]),
            kind: b[KIND],
            name,
            name_len: b[NAME_LEN],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptMsg {
    pub challenger_id: [u8; 2],
    pub target_id: [u8; 2],
    pub snap: Snapshot,
    pub nonce: u16,
}

impl AcceptMsg {
    pub fn to_bytes(&self) -> [u8; ACCEPT_LEN] {
        let mut b = [0u8; ACCEPT_LEN];
        b[0..2].copy_from_slice(&self.challenger_id);
        b[2..4].copy_from_slice(&self.target_id);
        b[SNAP..NONCE].copy_from_slice(&self.snap.to_wire());
        b[NONCE..NONCE + 2].copy_from_slice(&self.nonce.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < ACCEPT_LEN {
            return None;
        }
        Some(Self {
            challenger_id: [b[0], b[1]],
            target_id: [b[2], b[3]],
            snap: Snapshot::from_wire(&b[SNAP..NONCE])?,
            nonce: u16::from_le_bytes([b[NONCE], b[NONCE + 1]]),
        })
    }
}

/// `why` and `secs` are the `Reason::pack` pair (`secs` is the wait of a
/// pair cooldown), or [`WHY_DECLINED`] and 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclineMsg {
    pub challenger_id: [u8; 2],
    pub target_id: [u8; 2],
    pub why: u8,
    pub secs: u16,
}

impl DeclineMsg {
    /// A decline that carries `reason`. The wait saturates at `u16::MAX`
    /// seconds (about 18 h) instead of wrapping.
    pub fn new(challenger_id: [u8; 2], target_id: [u8; 2], reason: Reason) -> Self {
        let (why, secs) = reason.pack();
        Self {
            challenger_id,
            target_id,
            why,
            secs: secs.min(u16::MAX as u32) as u16,
        }
    }

    /// The decline of a player who pressed Decline.
    pub fn declined(challenger_id: [u8; 2], target_id: [u8; 2]) -> Self {
        Self {
            challenger_id,
            target_id,
            why: WHY_DECLINED,
            secs: 0,
        }
    }

    /// The refusal reason, or `None` when the player pressed Decline.
    pub fn reason(&self) -> Option<Reason> {
        (self.why != WHY_DECLINED).then(|| Reason::unpack(self.why, self.secs as u32))
    }

    pub fn to_bytes(&self) -> [u8; DECLINE_LEN] {
        let mut b = [0u8; DECLINE_LEN];
        b[0..2].copy_from_slice(&self.challenger_id);
        b[2..4].copy_from_slice(&self.target_id);
        b[4] = self.why;
        b[5..7].copy_from_slice(&self.secs.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < DECLINE_LEN {
            return None;
        }
        Some(Self {
            challenger_id: [b[0], b[1]],
            target_id: [b[2], b[3]],
            why: b[4],
            secs: u16::from_le_bytes([b[5], b[6]]),
        })
    }
}

// ---------------------------------------------------------------------------
// The challenge protocol (spec 12.4, 12.5, 17 rows 15 to 17 and 23a)
// ---------------------------------------------------------------------------

/// The challenger waits this long for an ACCEPT (spec 12.4).
pub const ANSWER_WINDOW_MS: u32 = 20_000;
/// The target's buttons close earlier, so its ACCEPT lands inside the 20 s.
pub const TARGET_WINDOW_MS: u32 = 15_000;
/// An unattended demo target accepts after this (17 row 15).
pub const DEMO_ACCEPT_MS: u32 = 10_000;
/// After an ACCEPT, a RESULT from that peer is ignored this long.
pub const LATE_RESULT_MS: u32 = 60_000;

/// Who I am, now. `nonce` is fresh entropy for this call.
#[derive(Clone, Copy, Debug)]
pub struct Me<'a> {
    pub id: [u8; 2],
    pub kind: u8,
    pub name: &'a str,
    pub now_ms: u32,
    pub now_tick: u32,
    pub nonce: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Challenger,
    Target,
}

/// What the pre-battle screen's status line says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    Waiting,
    Declined,
    Refused(Reason),
    NotReady,
    TimedOut,
}

#[derive(Clone, Copy, Debug)]
pub struct PreBattle {
    pub role: Role,
    pub peer_id: [u8; 2],
    pub peer_kind: u8,
    pub peer_name: [u8; PET_NAME_MAX],
    pub peer_name_len: u8,
    pub mine: Snapshot,
    pub theirs: Snapshot,
    /// My chance, from my point of view.
    pub chance: u8,
    /// True until the challenger has the real snapshot: shown with "~".
    pub estimate: bool,
    pub ends_ms: u32,
    pub note: Note,
}

#[derive(Clone, Copy, Debug)]
pub struct Fight {
    pub peer_id: [u8; 2],
    pub peer_kind: u8,
    pub won: bool,
    pub my_hp: u8,
    pub their_hp: u8,
    pub prize: u32,
    pub chance: u8,
    /// `(I lost, what decided it)`, when one thing stands out.
    pub why: Option<(bool, Decisive)>,
    /// Both badges fought it (false: a fallback against the beacon).
    pub live: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Packet {
    Challenge(ChallengeMsg),
    Accept(AcceptMsg),
    Decline(DeclineMsg),
    Result(BattleResultMsg),
}

#[derive(Clone, Copy, Debug)]
pub enum Out {
    Send(Packet),
    /// The glue passes it through the one alert gate (never in demo, never muted).
    Buzz,
    /// The pre-battle screen opens or changes (`Session::view` has it).
    Show,
    /// It closes.
    Close,
    /// Play the animation, fill the result card.
    Fought(Fight),
    /// The glue calls `friends::note_fight`.
    Recorded {
        peer: [u8; 2],
        won: bool,
    },
    /// `BattleWon` / `BattleLost` for a heard result.
    Toast(Toast),
    /// The player's Fight was turned away: tell them why (never for a
    /// window that ran out).
    Refused(Reason),
}

pub type Outs = heapless::Vec<Out, 4>;

#[derive(Clone, Copy)]
struct Pending {
    pre: PreBattle,
    /// The challenger's nonce.
    nonce_a: u16,
    /// Over, but the screen stays up until `cancel` or the next challenge.
    resolved: bool,
}

impl Pending {
    /// Still waiting for something to happen.
    fn live(&self) -> bool {
        !self.resolved && self.pre.note == Note::Waiting
    }
}

/// One badge's side of the challenge protocol.
pub struct Session {
    pending: Option<Pending>,
    /// The challenger I accepted, and when.
    accepted: Option<([u8; 2], u32)>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

fn name_bytes(name: &str) -> ([u8; PET_NAME_MAX], u8) {
    let mut n = name.len().min(PET_NAME_MAX);
    while !name.is_char_boundary(n) {
        n -= 1;
    }
    let mut buf = [0u8; PET_NAME_MAX];
    buf[..n].copy_from_slice(&name.as_bytes()[..n]);
    (buf, n as u8)
}

/// The deadline `end` has passed at `now` (wrap-safe).
fn expired(now: u32, end: u32) -> bool {
    now.wrapping_sub(end) as i32 >= 0
}

/// A league mismatch, told from the challenger's side.
fn league_reason(challenger_demo: bool) -> Reason {
    Reason::Refused(if challenger_demo {
        Refusal::DemoOnly
    } else {
        Refusal::NoDemo
    })
}

fn why_for(
    o: &combat::Outcome,
    a: &Snapshot,
    b: &Snapshot,
    i_am_a: bool,
) -> Option<(bool, Decisive)> {
    combat::decisive(o, a, b).map(|(loser_is_a, d)| (loser_is_a == i_am_a, d))
}

/// Why the pet cannot take a fight now (`None` when it can). The one rule the
/// Session and the screens share. A demo pet that is asleep cannot answer; a
/// player's sleeping pet is woken by its own Fight.
pub fn target_refusal(s: &GameState) -> Option<Reason> {
    s.target_refusal()
        .or_else(|| (s.demo.on && s.is_sleeping).then_some(Reason::Refused(Refusal::Asleep)))
}

fn push_decline(outs: &mut Outs, challenger: [u8; 2], me: [u8; 2], r: Reason) {
    let _ = outs.push(Out::Send(Packet::Decline(DeclineMsg::new(
        challenger, me, r,
    ))));
}

impl Session {
    pub const fn new() -> Self {
        Self {
            pending: None,
            accepted: None,
        }
    }

    fn live(&self) -> bool {
        self.pending.is_some_and(|p| p.live())
    }

    /// Challenge `f`. `Err` says why not, in this order: busy, a guard
    /// (self or no numbers), my own refusal, league, the pair rest. A friend
    /// on an old badge is never sent a CHALLENGE (it could not answer): the
    /// screen opens at once with `Note::NotReady`.
    pub fn challenge(
        &mut self,
        me: &Me,
        s: &mut GameState,
        f: &FriendRecord,
    ) -> Result<Outs, Reason> {
        if self.live() {
            return Err(Reason::Busy);
        }
        if f.device_id == me.id || !f.has_numbers() {
            return Err(Reason::Refused(Refusal::NoNeed));
        }
        if let Some(why) = s.battle_refusal() {
            return Err(why);
        }
        if f.demo != s.demo.on {
            return Err(league_reason(s.demo.on));
        }
        if let Some(secs) = f.pair_wait_secs(me.now_tick) {
            return Err(Reason::Wait(secs));
        }
        let mine = s.snapshot();
        let theirs = f.estimate_snapshot();
        let mut pre = PreBattle {
            role: Role::Challenger,
            peer_id: f.device_id,
            peer_kind: f.pet_kind,
            peer_name: f.name,
            peer_name_len: f.name_len,
            mine,
            theirs,
            chance: combat::win_chance(&mine, &theirs),
            estimate: true,
            ends_ms: me.now_ms.wrapping_add(ANSWER_WINDOW_MS),
            note: Note::Waiting,
        };
        let mut outs = Outs::new();
        if !f.v3 {
            pre.note = Note::NotReady;
            self.pending = Some(Pending {
                pre,
                nonce_a: me.nonce,
                resolved: true,
            });
            let _ = outs.push(Out::Show);
            return Ok(outs);
        }
        let (name, name_len) = name_bytes(me.name);
        let _ = outs.push(Out::Send(Packet::Challenge(ChallengeMsg {
            challenger_id: me.id,
            target_id: f.device_id,
            snap: mine,
            nonce: me.nonce,
            kind: me.kind,
            name,
            name_len,
        })));
        let _ = outs.push(Out::Show);
        self.pending = Some(Pending {
            pre,
            nonce_a: me.nonce,
            resolved: false,
        });
        Ok(outs)
    }

    /// A CHALLENGE was heard. `pair_wait_secs` is the pair rest with that
    /// challenger from the friend record (`None` when they may fight).
    pub fn on_challenge(
        &mut self,
        me: &Me,
        s: &mut GameState,
        m: &ChallengeMsg,
        pair_wait_secs: Option<u32>,
    ) -> Outs {
        let mut outs = Outs::new();
        if m.target_id != me.id || m.challenger_id == me.id {
            return outs;
        }
        let from = m.challenger_id;
        // The same CHALLENGE heard twice: my screen for it is already up.
        if self.pending.is_some_and(|p| {
            p.live() && p.pre.role == Role::Target && p.pre.peer_id == from && p.nonce_a == m.nonce
        }) {
            return outs;
        }
        if self.live() {
            push_decline(&mut outs, from, me.id, Reason::Busy);
            return outs;
        }
        // Both badges check the league (spec 12.5), a hibernating one too.
        if m.snap.flags.demo() != s.demo.on {
            push_decline(&mut outs, from, me.id, league_reason(m.snap.flags.demo()));
            return outs;
        }
        if s.hibernating {
            // 17 row 23a: answered at once, the frozen pet untouched.
            let _ = outs.push(Out::Send(Packet::Accept(AcceptMsg {
                challenger_id: from,
                target_id: me.id,
                snap: s.snapshot(),
                nonce: me.nonce,
            })));
            return outs;
        }
        if let Some(secs) = pair_wait_secs {
            push_decline(&mut outs, from, me.id, Reason::Wait(secs));
            return outs;
        }
        // Asleep and the battle rest do not stop a target; the body does.
        if let Some(why) = s.target_refusal() {
            push_decline(&mut outs, from, me.id, why);
            return outs;
        }
        // Nobody is there to press Fight and wake a sleeping demo pet.
        if s.demo.on && s.is_sleeping {
            push_decline(&mut outs, from, me.id, Reason::Refused(Refusal::Asleep));
            return outs;
        }
        let mine = s.snapshot();
        let window = if s.demo.on {
            DEMO_ACCEPT_MS
        } else {
            TARGET_WINDOW_MS
        };
        self.pending = Some(Pending {
            pre: PreBattle {
                role: Role::Target,
                peer_id: from,
                peer_kind: m.kind,
                peer_name: m.name,
                peer_name_len: m.name_len,
                mine,
                theirs: m.snap,
                chance: 100 - combat::win_chance(&m.snap, &mine),
                estimate: false,
                ends_ms: me.now_ms.wrapping_add(window),
                note: Note::Waiting,
            },
            nonce_a: m.nonce,
            resolved: false,
        });
        let _ = outs.push(Out::Show);
        let _ = outs.push(Out::Buzz);
        outs
    }

    /// The target's player: Fight (true) or Decline (false).
    pub fn answer(&mut self, me: &Me, s: &mut GameState, fight: bool) -> Outs {
        let mut outs = Outs::new();
        let Some(p) = self.pending else {
            return outs;
        };
        if !p.live() || p.pre.role != Role::Target {
            return outs;
        }
        // A press after the window is the window running out.
        if expired(me.now_ms, p.pre.ends_ms) {
            return self.on_deadline(me, s, None);
        }
        if !fight {
            let _ = outs.push(Out::Send(Packet::Decline(DeclineMsg::declined(
                p.pre.peer_id,
                me.id,
            ))));
            s.declined();
            self.pending = None;
            let _ = outs.push(Out::Close);
            return outs;
        }
        self.fight_as_target(me, s, p, true)
    }

    /// The target fights: from `answer(true)` (`by_player`) or an unattended
    /// demo pet at the deadline.
    fn fight_as_target(&mut self, me: &Me, s: &mut GameState, p: Pending, by_player: bool) -> Outs {
        let mut outs = Outs::new();
        let peer = p.pre.peer_id;
        // The pet may have changed in the window.
        if let Some(why) = target_refusal(s) {
            push_decline(&mut outs, peer, me.id, why);
            if by_player {
                let _ = outs.push(Out::Refused(why));
            }
            self.pending = None;
            let _ = outs.push(Out::Close);
            return outs;
        }
        if s.is_sleeping {
            s.wake();
        }
        let (a, b) = (p.pre.theirs, p.pre.mine);
        let o = combat::fight(combat::seed(p.nonce_a, me.nonce, peer, me.id), &a, &b);
        let won = !o.a_won;
        let prize = s.fight_over(Fought {
            won,
            hp_left_pct: o.b_hp_pct,
            costs: true,
            paid: true,
        });
        let _ = outs.push(Out::Send(Packet::Accept(AcceptMsg {
            challenger_id: peer,
            target_id: me.id,
            snap: b,
            nonce: me.nonce,
        })));
        let _ = outs.push(Out::Recorded { peer, won });
        let _ = outs.push(Out::Fought(Fight {
            peer_id: peer,
            peer_kind: p.pre.peer_kind,
            won,
            my_hp: o.b_hp_pct,
            their_hp: o.a_hp_pct,
            prize,
            chance: p.pre.chance,
            why: why_for(&o, &a, &b, false),
            live: true,
        }));
        let _ = outs.push(Out::Close);
        self.accepted = Some((peer, me.now_ms));
        self.pending = None;
        outs
    }

    /// An ACCEPT was heard. Ignored unless it answers my live challenge in
    /// time and comes from my league.
    pub fn on_accept(&mut self, me: &Me, s: &mut GameState, m: &AcceptMsg) -> Outs {
        let mut outs = Outs::new();
        let Some(p) = self.pending else {
            return outs;
        };
        if !p.live()
            || p.pre.role != Role::Challenger
            || m.challenger_id != me.id
            || m.target_id != p.pre.peer_id
            || expired(me.now_ms, p.pre.ends_ms)
            || m.snap.flags.demo() != s.demo.on
        {
            return outs;
        }
        let peer = p.pre.peer_id;
        let (a, b) = (p.pre.mine, m.snap);
        let o = combat::fight(combat::seed(p.nonce_a, m.nonce, me.id, peer), &a, &b);
        let chance = combat::win_chance(&a, &b);
        let prize = s.fight_over(Fought {
            won: o.a_won,
            hp_left_pct: o.a_hp_pct,
            costs: true,
            paid: true,
        });
        // The exact figure replaces the "~" estimate before the animation
        // (spec 12.4); the screen stays readable through `view` for it.
        let mut pre = p.pre;
        pre.theirs = b;
        pre.chance = chance;
        pre.estimate = false;
        self.pending = Some(Pending {
            pre,
            nonce_a: p.nonce_a,
            resolved: true,
        });
        let _ = outs.push(Out::Show);
        let _ = outs.push(Out::Recorded { peer, won: o.a_won });
        let _ = outs.push(Out::Fought(Fight {
            peer_id: peer,
            peer_kind: p.pre.peer_kind,
            won: o.a_won,
            my_hp: o.a_hp_pct,
            their_hp: o.b_hp_pct,
            prize,
            chance,
            why: why_for(&o, &a, &b, true),
            live: true,
        }));
        outs
    }

    /// A DECLINE was heard: the screen shows why and stays until the player
    /// closes it. No record, no cost.
    pub fn on_decline(&mut self, me: &Me, m: &DeclineMsg) -> Outs {
        let mut outs = Outs::new();
        let Some(p) = self.pending.as_mut() else {
            return outs;
        };
        if !p.live()
            || p.pre.role != Role::Challenger
            || m.challenger_id != me.id
            || m.target_id != p.pre.peer_id
        {
            return outs;
        }
        p.pre.note = match m.reason() {
            None => Note::Declined,
            Some(r) => Note::Refused(r),
        };
        let _ = outs.push(Out::Show);
        outs
    }

    /// A 9- or 7-byte RESULT. `sender_demo`: the sender's league from its
    /// friend record, `None` if unknown (counts as a player pet).
    pub fn on_result(
        &mut self,
        me: &Me,
        s: &mut GameState,
        m: &BattleResultMsg,
        sender_demo: Option<bool>,
    ) -> Outs {
        let mut outs = Outs::new();
        if m.target_id != me.id || m.challenger_id == me.id {
            return outs;
        }
        if let Some((peer, at)) = self.accepted {
            if peer == m.challenger_id && me.now_ms.wrapping_sub(at) < LATE_RESULT_MS {
                return outs;
            }
        }
        if s.hibernating {
            return outs;
        }
        if sender_demo.is_some_and(|d| d != s.demo.on) {
            return outs;
        }
        let won = !m.challenger_won;
        s.record_only(won);
        let _ = outs.push(Out::Recorded {
            peer: m.challenger_id,
            won,
        });
        let _ = outs.push(Out::Toast(if won {
            Toast::BattleWon
        } else {
            Toast::BattleLost
        }));
        // The challenger already fought me on the beacon: its question is moot.
        if self.pending.is_some_and(|p| {
            p.live() && p.pre.role == Role::Target && p.pre.peer_id == m.challenger_id
        }) {
            self.pending = None;
            let _ = outs.push(Out::Close);
        }
        outs
    }

    /// The window ran out (a no-op before `deadline_ms`). The challenger needs
    /// the friend record for the fallback fight on the beacon numbers (17 row 17).
    pub fn on_deadline(&mut self, me: &Me, s: &mut GameState, f: Option<&FriendRecord>) -> Outs {
        let mut outs = Outs::new();
        let Some(p) = self.pending else {
            return outs;
        };
        if !p.live() || !expired(me.now_ms, p.pre.ends_ms) {
            return outs;
        }
        if p.pre.role == Role::Target {
            if s.demo.on {
                return self.fight_as_target(me, s, p, false);
            }
            self.pending = None;
            let _ = outs.push(Out::Close);
            return outs;
        }
        let peer = p.pre.peer_id;
        let usable = f.filter(|f| f.device_id == peer && f.has_numbers() && f.demo == s.demo.on);
        let Some(f) = usable else {
            if let Some(p) = self.pending.as_mut() {
                p.pre.note = Note::TimedOut;
            }
            let _ = outs.push(Out::Show);
            return outs;
        };
        let (a, b) = (p.pre.mine, f.estimate_snapshot());
        let o = combat::fight(combat::seed(p.nonce_a, 0, me.id, peer), &a, &b);
        // An older beacon gives a record only: no costs, no prize. After a
        // reboot the stored ticks are rebased and look recent, so the beacon
        // must also have been heard this boot.
        let fresh = f.heard_this_boot
            && me.now_tick.saturating_sub(f.last_seen_tick)
                <= t::BATTLE_PRIZE_BEACON_MIN() as u32 * 6;
        let prize = s.fight_over(Fought {
            won: o.a_won,
            hp_left_pct: o.a_hp_pct,
            costs: fresh,
            paid: fresh,
        });
        let _ = outs.push(Out::Send(Packet::Result(BattleResultMsg {
            challenger_id: me.id,
            target_id: peer,
            challenger_won: o.a_won,
            challenger_hp_pct: o.a_hp_pct,
            target_hp_pct: o.b_hp_pct,
            challenger_kind: me.kind,
            target_kind: f.pet_kind,
        })));
        let _ = outs.push(Out::Recorded { peer, won: o.a_won });
        let _ = outs.push(Out::Fought(Fight {
            peer_id: peer,
            peer_kind: f.pet_kind,
            won: o.a_won,
            my_hp: o.a_hp_pct,
            their_hp: o.b_hp_pct,
            prize,
            chance: combat::win_chance(&a, &b),
            why: why_for(&o, &a, &b, true),
            live: false,
        }));
        self.pending = None;
        outs
    }

    /// When `on_deadline` is due, while a challenge is waiting.
    pub fn deadline_ms(&self) -> Option<u32> {
        self.pending.filter(|p| p.live()).map(|p| p.pre.ends_ms)
    }

    /// The pre-battle screen, while there is one.
    pub fn view(&self) -> Option<PreBattle> {
        self.pending.map(|p| p.pre)
    }

    /// Demo mode was switched or the pet is gone: drop what is pending.
    pub fn cancel(&mut self) -> Outs {
        let mut outs = Outs::new();
        if self.pending.take().is_some() {
            let _ = outs.push(Out::Close);
        }
        outs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_msg_full_round_trip_preserves_species() {
        let msg = BattleResultMsg {
            challenger_id: [0xAB, 0xCD],
            target_id: [0x12, 0x34],
            challenger_won: true,
            challenger_hp_pct: 80,
            target_hp_pct: 0,
            challenger_kind: 5,
            target_kind: 1,
        };
        let bytes = msg.to_bytes();
        assert_eq!(bytes.len(), FULL_SIZE);
        let back = BattleResultMsg::from_bytes(&bytes).unwrap();
        assert_eq!(back.challenger_id, [0xAB, 0xCD]);
        assert_eq!(back.target_id, [0x12, 0x34]);
        assert!(back.challenger_won);
        assert_eq!(back.challenger_hp_pct, 80);
        assert_eq!(back.target_hp_pct, 0);
        assert_eq!(back.challenger_kind, 5);
        assert_eq!(back.target_kind, 1);
    }

    #[test]
    fn result_msg_legacy_decode_marks_species_unknown() {
        // A 7-byte packet from the old firmware must still decode, with the
        // species reported as unknown so the receiver falls back.
        let legacy = [0xAB, 0xCD, 0x12, 0x34, 1, 80, 0];
        let back = BattleResultMsg::from_bytes(&legacy).unwrap();
        assert_eq!(back.challenger_kind, KIND_UNKNOWN);
        assert_eq!(back.target_kind, KIND_UNKNOWN);
        assert!(back.challenger_won);
        // Too short → rejected.
        assert!(BattleResultMsg::from_bytes(&legacy[..MIN_SIZE - 1]).is_none());
    }

    #[test]
    fn new_receiver_ignores_extra_trailing_bytes() {
        // Forward-compat: a longer future packet still decodes the fields we know.
        let mut buf = BattleResultMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            challenger_won: false,
            challenger_hp_pct: 10,
            target_hp_pct: 90,
            challenger_kind: 2,
            target_kind: 0,
        }
        .to_bytes()
        .to_vec();
        buf.push(0x99); // hypothetical future field
        let back = BattleResultMsg::from_bytes(&buf).unwrap();
        assert_eq!(back.target_kind, 0);
        assert_eq!(back.challenger_kind, 2);
    }

    #[test]
    fn battle_result_msg_round_trips() {
        let msg = BattleResultMsg {
            challenger_id: [0x11, 0x22],
            target_id: [0x33, 0x44],
            challenger_won: true,
            challenger_hp_pct: 80,
            target_hp_pct: 0,
            challenger_kind: 0,
            target_kind: 0,
        };
        let bytes = msg.to_bytes();
        let restored = BattleResultMsg::from_bytes(&bytes).unwrap();
        assert_eq!(restored.challenger_id, [0x11, 0x22]);
        assert_eq!(restored.target_id, [0x33, 0x44]);
        assert!(restored.challenger_won);
        assert_eq!(restored.challenger_hp_pct, 80);
        assert_eq!(restored.target_hp_pct, 0);
    }
}
