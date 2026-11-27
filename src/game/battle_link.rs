//! The glue between `battle::Session` (the pure challenge protocol) and the
//! rest of the badge: the game state, the radio, the answer timer, the
//! Battle screens and the buzzer.
//!
//! Every entry point catches the pet up, builds `Me`, calls the one
//! `Session`, then runs the `Out`s it returns. The `pub async fn on_*`
//! handlers are one-line wrappers over sync `handle_*` functions, which the
//! host tests call (the host has no `block_on`).

use super::battle::{
    AcceptMsg, BattleResultMsg, ChallengeMsg, DeclineMsg, Fight, Me, Out, Outs, Packet, PreBattle,
    Role, Session,
};
use super::engine::PET_NAME_MAX;
use super::engine::doses::{Reason, Refusal};
use super::{friends, lifecycle};
use crate::sync::SyncCell;

/// The one `Session` of this badge.
static SESSION: SyncCell<Session> = SyncCell::new(Session::new());
/// The last fight, for the result card.
static LAST_FIGHT: SyncCell<Option<Fight>> = SyncCell::new(None);

/// Wakes `battle_timer_task` when the Session's deadline moved.
#[cfg(feature = "embassy-core")]
pub static DEADLINE_CHANGED: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

fn session() -> &'static mut Session {
    unsafe { &mut *SESSION.get() }
}

/// The one gate of the challenge buzz: the same rule as every alert.
fn challenge_buzz(muted: bool, demo: bool) -> bool {
    lifecycle::alert_due(muted, demo, true)
}

/// Milliseconds from `now` until `deadline` (wrap-safe), 0 when already due.
pub fn ms_until(deadline: u32, now: u32) -> u32 {
    let d = deadline.wrapping_sub(now) as i32;
    if d <= 0 { 0 } else { d as u32 }
}

/// A screen change that came from the radio or the timer, not a button: wake
/// the display loop (the event-driven signal the toasts and the battle
/// animation use) so the e-paper redraws now, not at the next minute tick.
fn wake_display() {
    #[cfg(feature = "embassy-core")]
    crate::TOAST_SIGNAL.signal(());
}

fn changed() {
    #[cfg(feature = "embassy-core")]
    DEADLINE_CHANGED.signal(());
}

/// Run `f` on the Session with a caught-up pet, then carry out what it says.
fn run(f: impl FnOnce(&Me, &mut super::engine::GameState, &mut Session) -> Outs) {
    // Two clock bytes are weak alone; the uptime's low bits move the rest.
    let nonce = u16::from_le_bytes([crate::platform::entropy_byte(), crate::platform::entropy_byte()])
        ^ (crate::platform::uptime_ms_u32() as u16).rotate_left(5);
    let me = Me {
        id: friends::local_device_id(),
        kind: lifecycle::pet_kind().id(),
        name: lifecycle::pet_name(),
        now_ms: crate::platform::uptime_ms_u32(),
        now_tick: lifecycle::now_tick(),
        nonce,
    };
    // The challenger's name is gone from the Session once the fight is over.
    let before = session().view();
    let mut outs = Outs::new();
    lifecycle::with_game_now(|s| outs = f(&me, s, session()));
    apply(outs, before, me.now_tick);
    changed();
}

fn apply(outs: Outs, before: Option<PreBattle>, now_tick: u32) {
    let mut fought = false;
    let mut closed = false;
    let mut shown = false;
    let mut refused = None;
    for out in outs.iter() {
        match *out {
            Out::Send(p) => send(&p),
            Out::Buzz => {
                let muted = crate::GAME_MUTE.load(core::sync::atomic::Ordering::Relaxed);
                if challenge_buzz(muted, lifecycle::is_demo()) {
                    #[cfg(feature = "embassy-core")]
                    crate::fw::buzzer::play(crate::PET_WARN_INDEX);
                }
            }
            Out::Show => {
                shown = true;
                super::battle_view::open_pre_battle();
                wake_display();
            }
            Out::Close => closed = true,
            Out::Fought(f) => {
                fought = true;
                // The challenger's ACCEPT pushes `Show` (the exact chance)
                // just before `Fought`: queue that screen so the display
                // draws it once before the animation takes over (spec 12.4).
                if shown
                    && let Some(pre) = session().view().filter(|p| p.role == Role::Challenger)
                {
                    super::battle_view::queue_pre_show(pre);
                }
                let (name, len) = peer_name(&f, before);
                unsafe { *LAST_FIGHT.get() = Some(f) };
                super::battle_view::show_fight(name, len, f);
                super::show_battle_anim(
                    lifecycle::pet_kind().id(),
                    f.peer_kind,
                    f.won,
                    f.my_hp,
                    f.their_hp,
                    f.peer_id,
                );
            }
            Out::Recorded { peer, won } => friends::note_fight(peer, won, now_tick),
            Out::Toast(t) => super::show_toast(t),
            Out::Refused(why) => refused = Some(why),
        }
    }
    if fought {
        // The result card replaces the pre-battle screen, so a `Close` must
        // not shut it. The challenger's side emits no `Close`: its resolved
        // screen is dropped here, since the fight is played.
        if !closed {
            let _ = session().cancel();
        }
    } else if closed {
        super::battle_view::close();
        wake_display();
    }
    // After the screen is closed, so the toast is raised over the game screen.
    if let Some(why) = refused {
        super::show_reason(why);
    }
}

/// The peer's name for the card: the screen that was up, else the friend list.
fn peer_name(f: &Fight, before: Option<PreBattle>) -> ([u8; PET_NAME_MAX], u8) {
    if let Some(p) = before.filter(|p| p.peer_id == f.peer_id) {
        return (p.peer_name, p.peer_name_len);
    }
    friends::name_of(f.peer_id).unwrap_or(([0; PET_NAME_MAX], 0))
}

/// Put a packet on the SHDW channel, zero-hop (never flooded).
#[cfg(feature = "mesh")]
fn send(p: &Packet) {
    use super::battle::{PET_ACCEPT_TYPE, PET_BATTLE_TYPE, PET_CHALLENGE_TYPE, PET_DECLINE_TYPE};
    let mut data: heapless::Vec<u8, { crate::fw::mesh::MAX_CHANNEL_DATA }> = heapless::Vec::new();
    let data_type = match p {
        Packet::Challenge(m) => {
            let _ = data.extend_from_slice(&m.to_bytes());
            PET_CHALLENGE_TYPE
        }
        Packet::Accept(m) => {
            let _ = data.extend_from_slice(&m.to_bytes());
            PET_ACCEPT_TYPE
        }
        Packet::Decline(m) => {
            let _ = data.extend_from_slice(&m.to_bytes());
            PET_DECLINE_TYPE
        }
        Packet::Result(m) => {
            let _ = data.extend_from_slice(&m.to_bytes());
            PET_BATTLE_TYPE
        }
    };
    let _ = crate::fw::mesh::tx_send(crate::fw::mesh::TxRequest::ChannelData(
        crate::fw::mesh::TxChannelData {
            channel_idx: crate::fw::mesh::channels::SHDW_SLOT,
            data_type,
            // path_len == 0 sends RouteType::Direct with an empty path
            // (zero-hop). Not OUT_PATH_UNKNOWN (0xFF), which would flood.
            path_len: 0,
            path: heapless::Vec::new(),
            data,
        },
    ));
}

/// No radio here (the host simulator, or a build without the mesh).
#[cfg(not(feature = "mesh"))]
fn send(_p: &Packet) {}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Challenge the friend at `friend_index`. `Err` says why not (the Battle
/// screen shows it); "Friend not ready" stays the view's own check.
pub fn challenge(friend_index: usize) -> Result<(), Reason> {
    // Firmware without the mesh cannot send, so no Session is left waiting
    // for an answer that can never come. The Battle screen checks this first
    // and says "No radio on badge"; this branch is for other callers.
    #[cfg(all(feature = "embassy-core", not(feature = "mesh")))]
    {
        let _ = friend_index;
        Err(Reason::Refused(Refusal::NoNeed))
    }
    #[cfg(not(all(feature = "embassy-core", not(feature = "mesh"))))]
    challenge_inner(friend_index)
}

#[cfg(not(all(feature = "embassy-core", not(feature = "mesh"))))]
fn challenge_inner(friend_index: usize) -> Result<(), Reason> {
    if !lifecycle::is_started() {
        return Err(Reason::Gone);
    }
    let Some(f) = friends::get(friend_index) else {
        return Err(Reason::Refused(Refusal::NoNeed));
    };
    let mut res = Ok(());
    run(|me, s, sess| match sess.challenge(me, s, &f) {
        Ok(outs) => outs,
        Err(why) => {
            res = Err(why);
            Outs::new()
        }
    });
    res
}

/// The target's player pressed Fight (true) or Decline (false).
pub fn answer(fight: bool) {
    run(|me, s, sess| sess.answer(me, s, fight));
}

pub async fn on_challenge(data: &[u8]) {
    handle_challenge(data)
}

pub async fn on_accept(data: &[u8]) {
    handle_accept(data)
}

pub async fn on_decline(data: &[u8]) {
    handle_decline(data)
}

pub async fn on_result(data: &[u8]) {
    handle_result(data)
}

fn handle_challenge(data: &[u8]) {
    let Some(m) = ChallengeMsg::from_bytes(data) else {
        return;
    };
    let now = lifecycle::now_tick();
    let wait = friends::by_id(m.challenger_id).and_then(|f| f.pair_wait_secs(now));
    run(|me, s, sess| sess.on_challenge(me, s, &m, wait));
}

fn handle_accept(data: &[u8]) {
    let Some(m) = AcceptMsg::from_bytes(data) else {
        return;
    };
    run(|me, s, sess| sess.on_accept(me, s, &m));
}

fn handle_decline(data: &[u8]) {
    let Some(m) = DeclineMsg::from_bytes(data) else {
        return;
    };
    run(|me, _s, sess| sess.on_decline(me, &m));
}

fn handle_result(data: &[u8]) {
    let Some(m) = BattleResultMsg::from_bytes(data) else {
        return;
    };
    let sender_demo = friends::by_id(m.challenger_id).map(|f| f.demo);
    run(|me, s, sess| sess.on_result(me, s, &m, sender_demo));
}

/// The firmware timer fired at the Session's deadline.
pub fn on_deadline() {
    let peer = session().view().map(|p| p.peer_id);
    let f = peer.and_then(friends::by_id);
    run(|me, s, sess| sess.on_deadline(me, s, f.as_ref()));
}

/// The pre-battle screen, while there is one.
pub fn view() -> Option<PreBattle> {
    session().view()
}

/// The last fight, for the result card.
pub fn last_fight() -> Option<Fight> {
    unsafe { *LAST_FIGHT.get() }
}

/// The screen was dismissed, or demo mode was switched: drop what is pending.
pub fn cancel() {
    // A live incoming challenge is declined (busy), not left to time out.
    if let Some(p) = session().view().filter(|p| {
        p.role == super::battle::Role::Target && p.note == super::battle::Note::Waiting
    }) {
        if session().deadline_ms().is_some() {
            send(&Packet::Decline(DeclineMsg::new(
                p.peer_id,
                friends::local_device_id(),
                Reason::Busy,
            )));
        }
    }
    let outs = session().cancel();
    if !outs.is_empty() {
        super::battle_view::close();
        wake_display();
    }
    changed();
}

/// A result card or notice was dismissed: forget the finished screen, but
/// never a challenge that is still waiting for an answer.
pub fn dismiss_finished() {
    if deadline_ms().is_none() {
        cancel();
    }
}

/// When the firmware timer must wake (uptime ms), while a challenge waits.
pub fn deadline_ms() -> Option<u32> {
    session().deadline_ms()
}

/// Host-only: a pending challenge from a made-up friend, for the guide shot.
#[cfg(feature = "simulator")]
pub fn inject_challenge_for_shot(name: &str, kind: u8) {
    let mut buf = [0u8; PET_NAME_MAX];
    let n = name.len().min(PET_NAME_MAX);
    buf[..n].copy_from_slice(&name.as_bytes()[..n]);
    run(|me, s, sess| {
        let m = ChallengeMsg {
            challenger_id: [0xB0, 0x01],
            target_id: me.id,
            snap: s.snapshot(),
            nonce: 0x1234,
            kind,
            name: buf,
            name_len: n as u8,
        };
        sess.on_challenge(me, s, &m, None)
    });
}

/// Test hook: a fresh Session and no remembered fight.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    unsafe {
        *SESSION.get() = Session::new();
        *LAST_FIGHT.get() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::{GameState, PetKind, Phase};
    use crate::game::friends::{self, FriendRecord};
    use crate::game::{lifecycle, lock_test_state};

    fn pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    fn friend(id: [u8; 2]) -> FriendRecord {
        FriendRecord {
            device_id: id,
            pet_kind: 1,
            name: *b"Bram\0\0\0\0\0\0\0\0",
            name_len: 4,
            first_seen_tick: 1,
            last_boost_tick: 1,
            last_seen_tick: 1,
            power: 20,
            guard: 10,
            speed: 30,
            hp: 50,
            stamina: 5,
            focus: 0,
            demo: false,
            v3: true,
            fought_tick: 0,
            wins: 0,
            losses: 0,
            heard_this_boot: true,
        }
    }

    #[test]
    fn challenge_buzz_follows_the_gate() {
        assert!(challenge_buzz(false, false));
        assert!(!challenge_buzz(true, false));
        assert!(!challenge_buzz(true, true));
        assert!(!challenge_buzz(false, true));
    }

    #[test]
    fn toggling_demo_cancels_a_challenge() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        inject_challenge_for_shot("Bram", 1);
        assert!(view().is_some(), "the injected challenge is on screen");
        assert!(lifecycle::toggle_demo().is_ok());
        assert!(view().is_none());
        assert!(deadline_ms().is_none());
        lifecycle::set_state_for_test(None);
        reset_for_test();
    }

    #[test]
    fn heard_result_updates_the_friend() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        friends::insert_for_test(friend([0xA1, 0x08]));
        let msg = crate::game::battle::BattleResultMsg {
            challenger_id: [0xA1, 0x08],
            target_id: friends::local_device_id(),
            challenger_won: true,
            challenger_hp_pct: 40,
            target_hp_pct: 0,
            challenger_kind: 1,
            target_kind: 0,
        };
        handle_result(&msg.to_bytes());
        let f = friends::by_id([0xA1, 0x08]).unwrap();
        assert_eq!((f.wins, f.losses), (0, 1), "their win is my loss");
        assert!(f.fought_tick != 0);
        lifecycle::set_state_for_test(None);
        reset_for_test();
    }

    #[test]
    fn beacon_carries_the_snapshot() {
        let _guard = lock_test_state();
        lifecycle::set_state_for_test(None);
        assert!(friends::beacon_now().is_none(), "no pet, no beacon");
        let mut s = pet();
        s.demo.on = true;
        lifecycle::set_state_for_test(Some(s));
        let b = friends::beacon_now().expect("a pet is active");
        let snap = lifecycle::snapshot().unwrap();
        assert_eq!(
            (b.power, b.guard, b.speed, b.hp, b.stamina, b.focus),
            (snap.power, snap.guard, snap.speed, snap.hp, snap.stamina, snap.focus)
        );
        assert!(b.v3);
        assert_eq!(b.demo, lifecycle::is_demo());
        assert!(b.demo);
        assert_eq!(b.to_bytes().len(), 25);
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn dismissing_a_card_keeps_a_live_challenge() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        crate::game::battle_view::show_result_for_test();
        inject_challenge_for_shot("Bram", 1);
        assert!(deadline_ms().is_some(), "the challenge is live");
        dismiss_finished();
        assert!(view().is_some() && deadline_ms().is_some());
        assert!(!crate::game::battle_view::wants_full_refresh(), "no old card reopens");
        lifecycle::set_state_for_test(None);
        reset_for_test();
        crate::game::battle_view::reset_for_test();
    }

    #[test]
    fn dismissing_a_finished_screen_clears_it() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        inject_challenge_for_shot("Bram", 1);
        answer(true);
        assert!(view().is_none());
        assert!(last_fight().is_some(), "the target fought");
        dismiss_finished();
        assert!(view().is_none());
        lifecycle::set_state_for_test(None);
        reset_for_test();
        crate::game::battle_view::reset_for_test();
    }

    #[test]
    fn a_malformed_packet_is_dropped() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        handle_challenge(&[1, 2, 3]);
        assert!(view().is_none());
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn an_accept_plays_the_fight_and_records_it() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        let id = [0xA2, 0x09];
        friends::insert_for_test(friend(id));
        assert!(challenge(0).is_ok());
        assert!(deadline_ms().is_some());
        let snap = lifecycle::snapshot().unwrap();
        let acc = crate::game::battle::AcceptMsg {
            challenger_id: friends::local_device_id(),
            target_id: id,
            snap,
            nonce: 7,
        };
        handle_accept(&acc.to_bytes());
        assert!(last_fight().is_some());
        assert!(view().is_none(), "the challenger's screen is dropped");
        assert!(friends::by_id(id).unwrap().fought_tick != 0);
        lifecycle::set_state_for_test(None);
        reset_for_test();
        crate::game::battle_view::reset_for_test();
    }

    #[test]
    fn challenging_without_a_pet_says_gone() {
        let _guard = lock_test_state();
        reset_for_test();
        lifecycle::set_state_for_test(None);
        friends::insert_for_test(friend([0xA3, 0x0A]));
        assert_eq!(challenge(0), Err(Reason::Gone));
    }

    #[test]
    fn deadline_wait_is_wrap_safe() {
        assert_eq!(ms_until(1_000, 400), 600);
        assert_eq!(ms_until(400, 1_000), 0, "already due");
        assert_eq!(ms_until(100, u32::MAX - 99), 200, "across the wrap");
    }

    #[test]
    fn an_accept_queues_the_exact_frame_for_the_challenger_only() {
        let _guard = lock_test_state();
        reset_for_test();
        crate::game::battle_view::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        let id = [0xA4, 0x0B];
        friends::insert_for_test(friend(id));
        assert!(challenge(0).is_ok());
        let snap = lifecycle::snapshot().unwrap();
        let theirs = crate::game::engine::combat::derive(&crate::game::engine::combat::Body {
            fit: 90,
            ..crate::game::engine::combat::Body::REFERENCE
        });
        let acc = crate::game::battle::AcceptMsg {
            challenger_id: friends::local_device_id(),
            target_id: id,
            snap: theirs,
            nonce: 7,
        };
        handle_accept(&acc.to_bytes());
        assert!(crate::game::battle_view::take_pre_show(), "a frame was queued");
        let q = crate::game::battle_view::queued_pre_for_test().expect("the frame");
        assert!(!q.estimate, "the exact chance, not the estimate");
        assert_eq!(q.chance, crate::game::engine::combat::win_chance(&snap, &theirs));
        assert_eq!(q.role, crate::game::battle::Role::Challenger);
        // The target fights without a pre-frame.
        crate::game::battle_view::reset_for_test();
        crate::game::battle_view::clear_queued_pre_for_test();
        inject_challenge_for_shot("Bram", 1);
        answer(true);
        assert!(last_fight().is_some());
        assert!(!crate::game::battle_view::take_pre_show());
        assert!(crate::game::battle_view::queued_pre_for_test().is_none());
        lifecycle::set_state_for_test(None);
        reset_for_test();
        crate::game::battle_view::reset_for_test();
    }
}
