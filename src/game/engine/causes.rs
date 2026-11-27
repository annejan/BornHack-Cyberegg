//! Cause events: why a dose or a wake went badly.
//!
//! Rules push a [`Cause`] where they fire (`doses::apply`, `advance::wake`).
//! The queue is RAM-only and never saved; the UI drains it with
//! `GameState::take_causes` and shows the words as a toast and a diary line.

use heapless::Deque;

/// What a strained or torn muscle blames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrainWhy {
    Hungry,
    Thirsty,
    Tired,
    Heavy,
    WornOut,
    Overdone,
    /// A battle's rough handling (plan 5).
    Fight,
}

impl StrainWhy {
    pub const ALL: [StrainWhy; 7] = [
        StrainWhy::Hungry,
        StrainWhy::Thirsty,
        StrainWhy::Tired,
        StrainWhy::Heavy,
        StrainWhy::WornOut,
        StrainWhy::Overdone,
        StrainWhy::Fight,
    ];

    fn why(self) -> &'static str {
        match self {
            StrainWhy::Hungry => "sport: hungry",
            StrainWhy::Thirsty => "sport: thirsty",
            StrainWhy::Tired => "sport: tired",
            StrainWhy::Heavy => "sport: heavy",
            StrainWhy::WornOut => "sport: worn out",
            StrainWhy::Overdone => "sport: too much",
            StrainWhy::Fight => "in a fight",
        }
    }
}

/// What kept a sleeper from resting (see `regime::unsettled_by`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unsettled {
    Drinks,
    Hungry,
    Stuffed,
    Thirsty,
    Waterlogged,
    Wired,
    Withdrawal,
    Unknown,
}

impl Unsettled {
    pub const ALL: [Unsettled; 8] = [
        Unsettled::Drinks,
        Unsettled::Hungry,
        Unsettled::Stuffed,
        Unsettled::Thirsty,
        Unsettled::Waterlogged,
        Unsettled::Wired,
        Unsettled::Withdrawal,
        Unsettled::Unknown,
    ];

    fn why(self) -> &'static str {
        match self {
            Unsettled::Drinks => "late drinks",
            Unsettled::Hungry => "hungry",
            Unsettled::Stuffed => "stuffed",
            Unsettled::Thirsty => "thirsty",
            Unsettled::Waterlogged => "waterlogged",
            Unsettled::Wired => "wired",
            Unsettled::Withdrawal => "withdrawal",
            Unsettled::Unknown => "slept badly",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    FoodComa,
    TreatTooSoon,
    NoTreatBuzz,
    DrinkDulled,
    OneTooMany,
    DryMouth,
    Relapse,
    Sloshing,
    TornMuscle(StrainWhy),
    Strained(StrainWhy),
    Grumpy,
    BadNight(Unsettled),
    Groggy,
    WornOut,
    StillHungOver,
    OnlyNumbed,
    OverRelaxed,
    WellRested,
    /// Demo mode: the player's care helped the pet.
    Thanks,
    /// Coffee ended the grogginess.
    PerkedUp,
    /// One coffee too many: the pet is wired.
    CoffeeWired,
}

/// The causes without a payload.
const PLAIN: [Cause; 18] = [
    Cause::FoodComa,
    Cause::TreatTooSoon,
    Cause::NoTreatBuzz,
    Cause::DrinkDulled,
    Cause::OneTooMany,
    Cause::DryMouth,
    Cause::Relapse,
    Cause::Sloshing,
    Cause::Grumpy,
    Cause::Groggy,
    Cause::WornOut,
    Cause::StillHungOver,
    Cause::OnlyNumbed,
    Cause::OverRelaxed,
    Cause::WellRested,
    Cause::Thanks,
    Cause::PerkedUp,
    Cause::CoffeeWired,
];

impl Cause {
    /// The short headline of a toast (at most 12 columns).
    pub fn what(self) -> &'static str {
        match self {
            Cause::FoodComa => "food coma",
            Cause::TreatTooSoon | Cause::DrinkDulled => "barely felt",
            Cause::NoTreatBuzz => "no buzz",
            Cause::OneTooMany => "sloppy",
            Cause::DryMouth => "dry mouth",
            Cause::Relapse => "relapse!",
            Cause::Sloshing => "sloshing",
            Cause::TornMuscle(_) => "torn muscle",
            Cause::Strained(_) => "strained",
            Cause::Grumpy => "grumpy",
            Cause::BadNight(_) => "bad night",
            Cause::Groggy => "groggy",
            Cause::WornOut => "worn out",
            Cause::StillHungOver => "hung over",
            Cause::OnlyNumbed => "only numbed",
            Cause::OverRelaxed => "bored",
            Cause::WellRested => "well rested",
            Cause::Thanks => "thanks!",
            Cause::PerkedUp => "perked up",
            Cause::CoffeeWired => "wired",
        }
    }

    /// The reason line of a toast (at most 15 columns).
    pub fn why(self) -> &'static str {
        match self {
            Cause::FoodComa => "ate too much",
            Cause::TreatTooSoon => "treat too soon",
            Cause::NoTreatBuzz => "on Ozempic",
            Cause::DrinkDulled => "drunk already",
            Cause::OneTooMany => "one too many",
            Cause::DryMouth => "booze, no water",
            Cause::Relapse => "rehab lost",
            Cause::Sloshing => "drank too much",
            Cause::TornMuscle(w) | Cause::Strained(w) => w.why(),
            Cause::Grumpy => "woken too early",
            Cause::BadNight(u) => u.why(),
            Cause::Groggy => "overslept",
            Cause::WornOut => "short nights",
            Cause::StillHungOver => "needs water",
            Cause::OnlyNumbed => "not sick",
            Cause::OverRelaxed => "too much relax",
            Cause::WellRested => "good sleep",
            Cause::Thanks => "that helped",
            Cause::PerkedUp => "had a coffee",
            Cause::CoffeeWired => "too much coffee",
        }
    }

    /// Everything but a good night, a thank-you and a perk is a warning.
    pub fn is_warning(self) -> bool {
        !matches!(self, Cause::WellRested | Cause::Thanks | Cause::PerkedUp)
    }

    /// Every variant, payloads included, once.
    pub fn all() -> impl Iterator<Item = Cause> {
        PLAIN
            .into_iter()
            .chain(StrainWhy::ALL.into_iter().map(Cause::TornMuscle))
            .chain(StrainWhy::ALL.into_iter().map(Cause::Strained))
            .chain(Unsettled::ALL.into_iter().map(Cause::BadNight))
    }

    /// A one-byte code, so a cause can cross a task boundary in an atomic.
    /// An exhaustive match: a new variant does not compile until it has one.
    pub fn pack(self) -> u8 {
        match self {
            Cause::FoodComa => 0,
            Cause::TreatTooSoon => 1,
            Cause::NoTreatBuzz => 2,
            Cause::DrinkDulled => 3,
            Cause::OneTooMany => 4,
            Cause::DryMouth => 5,
            Cause::Relapse => 6,
            Cause::Sloshing => 7,
            Cause::Grumpy => 8,
            Cause::Groggy => 9,
            Cause::WornOut => 10,
            Cause::StillHungOver => 11,
            Cause::OnlyNumbed => 12,
            Cause::OverRelaxed => 13,
            Cause::WellRested => 14,
            Cause::Thanks => 15,
            Cause::PerkedUp => 16,
            Cause::CoffeeWired => 17,
            Cause::TornMuscle(w) => TORN_BASE + w as u8,
            Cause::Strained(w) => STRAINED_BASE + w as u8,
            Cause::BadNight(u) => BAD_NIGHT_BASE + u as u8,
        }
    }

    /// Inverse of [`pack`](Self::pack).
    pub fn unpack(v: u8) -> Option<Cause> {
        let at = |base: u8| v.checked_sub(base).map(usize::from);
        match v {
            0..TORN_BASE => PLAIN.get(v as usize).copied(),
            TORN_BASE..STRAINED_BASE => StrainWhy::ALL
                .get(at(TORN_BASE)?)
                .map(|w| Cause::TornMuscle(*w)),
            STRAINED_BASE..BAD_NIGHT_BASE => StrainWhy::ALL
                .get(at(STRAINED_BASE)?)
                .map(|w| Cause::Strained(*w)),
            _ => Unsettled::ALL
                .get(at(BAD_NIGHT_BASE)?)
                .map(|u| Cause::BadNight(*u)),
        }
    }
}

/// Room for plain causes below it; the codes never leave RAM, so the bases
/// may move.
const TORN_BASE: u8 = 24;
const STRAINED_BASE: u8 = 32;
const BAD_NIGHT_BASE: u8 = 48;

/// How many causes wait for the UI.
pub const CAUSE_QUEUE: usize = 4;

/// The newest [`CAUSE_QUEUE`] causes, oldest first. Never saved.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CauseQueue(Deque<Cause, CAUSE_QUEUE>);

impl CauseQueue {
    /// Queue a cause; when full the oldest is dropped.
    pub fn push(&mut self, c: Cause) {
        #[cfg(feature = "simulator")]
        super::sim::note_cause();
        self.keep(c);
    }

    fn keep(&mut self, c: Cause) {
        if self.0.is_full() {
            self.0.pop_front();
        }
        let _ = self.0.push_back(c);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Cause> {
        self.0.iter()
    }

    /// Queue every cause of `other`, in order.
    pub fn extend_from(&mut self, other: &CauseQueue) {
        for &c in other.iter() {
            self.keep(c); // already counted when `other` queued it
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
