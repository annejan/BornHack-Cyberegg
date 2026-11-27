//! The background behind the pet (spec 14.2 layers 1 and 2): the sky, the
//! floor items, a cloud by day and the poorly cross. Everything is drawn from
//! primitives and small const bitmaps, no image files. The same function
//! draws into the firmware framebuffer and the simulator display.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

use super::engine::scene::{BackgroundState, FLOOR_ITEMS, Sky};
use crate::{BLACK, RED, TriColor, WHITE};

/// First and last sky row (the pet area starts under the separator at y 34).
pub const SKY_TOP: i32 = super::PET_AREA_TOP as i32;
pub const SKY_BOTTOM: i32 = 99;
/// First and last floor row.
pub const FLOOR_TOP: i32 = 100;
pub const FLOOR_BOTTOM: i32 = 109;

pub const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
/// Black where `BAYER4[y % 4][x % 4] < level`; one level per 16-row band from
/// `SKY_TOP` (the last band runs to `SKY_BOTTOM`). Denser towards the top,
/// about 53 % black overall.
pub const NIGHT_LEVELS: [u8; 4] = [10, 9, 8, 7];
/// Rows per dither band.
const BAND_ROWS: i32 = 16;

/// Where the sun or the moon sits.
pub const SUN_MOON_BOX: Rectangle = Rectangle::new(Point::new(129, 49), Size::new(19, 19));
/// Centre of the sun and of the moon.
const SUN_MOON_CENTRE: Point = Point::new(138, 58);
/// Top-left of each night star (5 x 5).
pub const STARS: [Point; 6] = [
    Point::new(18, 44),
    Point::new(50, 40),
    Point::new(86, 46),
    Point::new(110, 62),
    Point::new(28, 74),
    Point::new(72, 68),
];

/// Slot boxes in slot order: crumbs, peel, can, sock, pizza box (bottom row
/// y 109).
pub const FLOOR_SLOTS: [Rectangle; FLOOR_ITEMS] = [
    Rectangle::new(Point::new(4, 107), Size::new(7, 3)),
    Rectangle::new(Point::new(120, 104), Size::new(11, 6)),
    Rectangle::new(Point::new(16, 104), Size::new(10, 6)),
    Rectangle::new(Point::new(136, 102), Size::new(9, 8)),
    Rectangle::new(Point::new(29, 105), Size::new(16, 5)),
];

/// Black pixels of each floor item, one `u16` per row, the leftmost column in
/// the highest of the slot's `width` bits.
const CRUMBS: [u16; 3] = [0b0100010, 0b1010100, 0b0010001];
const PEEL: [u16; 6] = [
    0b00000110000,
    0b00011001001,
    0b01100000101,
    0b10010001010,
    0b10001110010,
    0b01110000000,
];
const CAN: [u16; 6] = [
    0b0011111110,
    0b0100000001,
    0b1010011100,
    0b1010011100,
    0b0100000001,
    0b0011111110,
];
const SOCK: [u16; 8] = [
    0b011111000,
    0b010001000,
    0b010001000,
    0b010001000,
    0b010001110,
    0b010000010,
    0b010000001,
    0b001111111,
];
const PIZZA_BOX: [u16; 5] = [
    0b1111111111111111,
    0b1000000000000001,
    0b1000110011000001,
    0b1000000000000001,
    0b1111111111111111,
];
const FLOOR_BITMAPS: [&[u16]; FLOOR_ITEMS] = [&CRUMBS, &PEEL, &CAN, &SOCK, &PIZZA_BOX];

/// Where the day cloud sits (22 x 10): left of the sun, 3 px clear of its
/// rays, below the HAX row and above the floor.
pub const CLOUD_BOX: Rectangle = Rectangle::new(Point::new(107, 59), Size::new(22, 10));
/// Cumulus shape in `CLOUD_BOX` coordinates: a flat base and three bumps
/// (centre x, centre y, radius).
const CLOUD_BUMPS: [(i32, i32, i32); 3] = [(4, 6, 3), (10, 4, 4), (16, 6, 3)];
/// The flat base spans these columns and rows.
const CLOUD_BASE_X: core::ops::RangeInclusive<i32> = 1..=20;
const CLOUD_BASE_Y: core::ops::RangeInclusive<i32> = 6..=9;

/// Where the poorly cross sits (9 x 9); the plus inside is 7 long, 3 wide.
pub const CROSS_BOX: Rectangle = Rectangle::new(Point::new(140, 70), Size::new(9, 9));

/// Draw the background of `bg` into the pet area: sky, floor items, then the
/// red cross.
pub fn draw_background<D: DrawTarget<Color = TriColor>>(
    display: &mut D,
    bg: &BackgroundState,
) -> Result<(), D::Error> {
    match bg.sky {
        Sky::Plain => {}
        Sky::Day => {
            draw_sun(display)?;
            draw_cloud(display)?;
        }
        Sky::Night => draw_night(display)?,
    }
    draw_floor_items(display, bg.floor)?;
    if bg.poorly {
        draw_cross(display)?;
    }
    Ok(())
}

/// Is (`x`, `y`) inside the disc of radius `r` around `c`?
fn in_disc(x: i32, y: i32, c: Point, r: i32) -> bool {
    let (dx, dy) = (x - c.x, y - c.y);
    dx * dx + dy * dy <= r * r
}

/// A filled black disc of radius 4 and eight 2-px rays.
fn draw_sun<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error> {
    let c = SUN_MOON_CENTRE;
    let disc = SUN_MOON_BOX
        .points()
        .filter(|p| in_disc(p.x, p.y, c, 4))
        .map(|p| Pixel(p, BLACK));
    // Straight rays 3..=4 px clear of the disc edge, diagonal rays on the
    // 45 degree lines.
    const RAYS: [(i32, i32); 8] = [
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    let rays = RAYS.iter().flat_map(move |&(dx, dy)| {
        let (near, far) = if dx != 0 && dy != 0 { (5, 6) } else { (6, 7) };
        [near, far]
            .into_iter()
            .map(move |d| Pixel(Point::new(c.x + dx * d, c.y + dy * d), BLACK))
    });
    display.draw_iter(disc.chain(rays))
}

/// The Bayer dither, the stars and the moon.
fn draw_night<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error> {
    let sky = (SKY_TOP..=SKY_BOTTOM).flat_map(|y| {
        let band = (((y - SKY_TOP) / BAND_ROWS) as usize).min(NIGHT_LEVELS.len() - 1);
        let level = NIGHT_LEVELS[band];
        (0..152).map(move |x| {
            let black = BAYER4[(y % 4) as usize][(x % 4) as usize] < level;
            Pixel(Point::new(x, y), if black { BLACK } else { WHITE })
        })
    });
    display.draw_iter(sky)?;
    for s in STARS {
        let star = Rectangle::new(s, Size::new(5, 5)).points().map(|p| {
            let (dx, dy) = (p.x - s.x, p.y - s.y);
            let plus = (dx == 2 && (1..=3).contains(&dy)) || (dy == 2 && (1..=3).contains(&dx));
            Pixel(p, if plus { WHITE } else { BLACK })
        });
        display.draw_iter(star)?;
    }
    let c = SUN_MOON_CENTRE;
    let bite = Point::new(141, 56);
    let moon = SUN_MOON_BOX.points().map(|p| {
        let white = in_disc(p.x, p.y, c, 7) && !in_disc(p.x, p.y, bite, 6);
        Pixel(p, if white { WHITE } else { BLACK })
    });
    display.draw_iter(moon)
}

/// The black pixels of a bitmap `width` columns wide at `origin`.
fn bitmap<D: DrawTarget<Color = TriColor>>(
    display: &mut D,
    origin: Point,
    width: i32,
    rows: &[u16],
) -> Result<(), D::Error> {
    let px = rows.iter().enumerate().flat_map(move |(dy, bits)| {
        (0..width)
            .filter(move |dx| bits & (1 << (width - 1 - dx)) != 0)
            .map(move |dx| Pixel(origin + Point::new(dx, dy as i32), BLACK))
    });
    display.draw_iter(px)
}

/// The first `n` floor items, each in its own slot.
fn draw_floor_items<D: DrawTarget<Color = TriColor>>(
    display: &mut D,
    n: u8,
) -> Result<(), D::Error> {
    for (slot, rows) in FLOOR_SLOTS.iter().zip(FLOOR_BITMAPS).take(n as usize) {
        bitmap(display, slot.top_left, slot.size.width as i32, rows)?;
    }
    Ok(())
}

/// Is (`x`, `y`), relative to `CLOUD_BOX`, inside the cloud shape?
fn in_cloud(x: i32, y: i32) -> bool {
    (CLOUD_BASE_X.contains(&x) && CLOUD_BASE_Y.contains(&y))
        || CLOUD_BUMPS
            .iter()
            .any(|&(cx, cy, r)| in_disc(x, y, Point::new(cx, cy), r))
}

/// The cloud: the shape's 1 px black outline, the inside stays sky white.
fn draw_cloud<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error> {
    let o = CLOUD_BOX.top_left;
    let px = CLOUD_BOX
        .points()
        .map(|p| p - o)
        .filter(|p| {
            in_cloud(p.x, p.y)
                && ![(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .all(|(dx, dy)| in_cloud(p.x + dx, p.y + dy))
        })
        .map(|p| Pixel(p + o, BLACK));
    display.draw_iter(px)
}

/// `CROSS_BOX` white with a red plus, arms 3 px wide and 7 px long.
fn draw_cross<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error> {
    CROSS_BOX
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    let c = CROSS_BOX.center();
    for (dx, dy, w, h) in [(-3, -1, 7, 3), (-1, -3, 3, 7)] {
        Rectangle::new(c + Point::new(dx, dy), Size::new(w, h))
            .into_styled(PrimitiveStyle::with_fill(RED))
            .draw(display)?;
    }
    Ok(())
}

#[cfg(test)]
mod draw {
    use super::*;
    use crate::game::test_canvas::Canvas;

    const PET_AREA: Rectangle = Rectangle::new(Point::new(0, 35), Size::new(152, 75));

    fn states() -> impl Iterator<Item = BackgroundState> {
        [Sky::Plain, Sky::Day, Sky::Night]
            .into_iter()
            .flat_map(|sky| {
                (0..=FLOOR_ITEMS as u8).flat_map(move |floor| {
                    [false, true]
                        .into_iter()
                        .map(move |poorly| BackgroundState { sky, floor, poorly })
                })
            })
    }

    fn drawn(bg: BackgroundState) -> Canvas {
        let mut c = Canvas::new();
        draw_background(&mut c, &bg).unwrap();
        c
    }

    fn state(sky: Sky, floor: u8, poorly: bool) -> BackgroundState {
        BackgroundState { sky, floor, poorly }
    }

    fn non_white(c: &Canvas, area: Rectangle) -> usize {
        area.points().filter(|p| c.get(p.x, p.y) != WHITE).count()
    }

    fn black_share(c: &Canvas, y0: i32, y1: i32, skip: &[Rectangle]) -> f32 {
        let (mut black, mut all) = (0, 0);
        for y in y0..=y1 {
            for x in 0..152 {
                let p = Point::new(x, y);
                if skip.iter().any(|r| r.contains(p)) {
                    continue;
                }
                all += 1;
                black += (c.get(x, y) == BLACK) as u32;
            }
        }
        black as f32 / all as f32
    }

    fn star_boxes() -> [Rectangle; 6] {
        STARS.map(|p| Rectangle::new(p, Size::new(5, 5)))
    }

    #[test]
    fn background_stays_in_the_pet_area() {
        for bg in states() {
            let c = drawn(bg);
            for y in 0..152 {
                for x in 0..152 {
                    if !PET_AREA.contains(Point::new(x, y)) {
                        assert_eq!(c.get(x, y), WHITE, "{bg:?} at ({x},{y})");
                    }
                }
            }
        }
    }

    #[test]
    fn plain_sky_is_white() {
        let c = drawn(state(Sky::Plain, 0, false));
        assert_eq!(
            non_white(&c, Rectangle::new(Point::zero(), Size::new(152, 152))),
            0
        );
    }

    #[test]
    fn night_is_about_half_black_and_denser_on_top() {
        let c = drawn(state(Sky::Night, 0, false));
        let skip = [SUN_MOON_BOX];
        let top = black_share(&c, 35, 50, &skip);
        let bottom = black_share(&c, 83, 99, &skip);
        assert!(top > bottom, "top {top} bottom {bottom}");
        let mut skip = star_boxes().to_vec();
        skip.push(SUN_MOON_BOX);
        let all = black_share(&c, 35, 99, &skip);
        assert!((0.45..=0.60).contains(&all), "overall {all}");
    }

    #[test]
    fn day_has_a_sun_and_night_a_moon() {
        let day = drawn(state(Sky::Day, 0, false));
        assert!(day.any(SUN_MOON_BOX, BLACK), "sun");
        assert_eq!(day.get(138, 58), BLACK, "disc centre");
        let sky = Rectangle::new(Point::new(0, 35), Size::new(152, 65));
        let cloud = |p: &Point| CLOUD_BOX.contains(*p);
        for p in sky
            .points()
            .filter(|p| !SUN_MOON_BOX.contains(*p) && !cloud(p))
        {
            assert_eq!(day.get(p.x, p.y), WHITE, "day sky at {p:?}");
        }
        let night = drawn(state(Sky::Night, 0, false));
        assert!(night.any(SUN_MOON_BOX, WHITE), "crescent");
        assert_eq!(
            night.get(SUN_MOON_BOX.top_left.x, SUN_MOON_BOX.top_left.y),
            BLACK
        );
        // The bite is dark again: the black disc inside the white one.
        assert_eq!(night.get(141, 56), BLACK);
        assert_eq!(night.get(133, 58), WHITE, "crescent body");
    }

    #[test]
    fn night_stars_are_white_plus_signs_on_black() {
        let c = drawn(state(Sky::Night, 0, false));
        for s in STARS {
            assert_eq!(c.get(s.x, s.y), BLACK, "corner {s:?}");
            assert_eq!(c.get(s.x + 2, s.y + 2), WHITE, "centre {s:?}");
            assert_eq!(c.get(s.x + 2, s.y + 1), WHITE);
            assert_eq!(c.get(s.x + 1, s.y + 2), WHITE);
            assert_eq!(c.get(s.x + 4, s.y + 4), BLACK);
        }
    }

    #[test]
    fn floor_items_are_cumulative_and_never_move() {
        let all = drawn(state(Sky::Plain, 5, false));
        for n in 0..=FLOOR_ITEMS as u8 {
            let c = drawn(state(Sky::Plain, n, false));
            for (k, slot) in FLOOR_SLOTS.iter().enumerate() {
                let shown = non_white(&c, *slot) > 0;
                assert_eq!(shown, (k as u8) < n, "slot {k} with {n} items");
                if shown {
                    for p in slot.points() {
                        assert_eq!(
                            c.get(p.x, p.y),
                            all.get(p.x, p.y),
                            "slot {k} moved at {p:?}"
                        );
                    }
                }
            }
            let outside = Rectangle::new(Point::new(0, 35), Size::new(152, 75));
            for p in outside.points() {
                if !FLOOR_SLOTS.iter().any(|s| s.contains(p)) {
                    assert_eq!(c.get(p.x, p.y), WHITE, "stray pixel {p:?} with {n} items");
                }
            }
        }
    }

    #[test]
    fn slots_and_cross_never_overlap() {
        for (i, a) in FLOOR_SLOTS.iter().enumerate() {
            let bb = a.bottom_right().unwrap();
            assert!(
                a.top_left.y >= FLOOR_TOP && bb.y <= FLOOR_BOTTOM,
                "slot {i}"
            );
            assert!(a.top_left.x >= 0 && bb.x <= 151, "slot {i}");
            for b in &FLOOR_SLOTS[i + 1..] {
                assert!(a.intersection(b).is_zero_sized(), "{a:?} meets {b:?}");
            }
        }
        assert!(CROSS_BOX.intersection(&SUN_MOON_BOX).is_zero_sized());
        for r in [CROSS_BOX, SUN_MOON_BOX] {
            let bb = r.bottom_right().unwrap();
            assert!(r.top_left.y >= SKY_TOP && bb.y <= SKY_BOTTOM);
        }
        cloud_stays_clear();
        // The cross and the sun sit clear of the HAX row and the battery.
        assert!(CROSS_BOX.top_left.y > 45 && SUN_MOON_BOX.top_left.y > 45);
    }

    /// The cloud sits in the sky, left of the sun, clear of the sun's
    /// disc and rays, the HAX row and battery (y 45 and up), the cross and
    /// the floor.
    fn cloud_stays_clear() {
        let bb = CLOUD_BOX.bottom_right().unwrap();
        assert!((20..=26).contains(&CLOUD_BOX.size.width), "{CLOUD_BOX:?}");
        assert!((8..=12).contains(&CLOUD_BOX.size.height), "{CLOUD_BOX:?}");
        assert!(CLOUD_BOX.top_left.y > 45 && bb.y <= SKY_BOTTOM);
        assert!(CLOUD_BOX.top_left.x >= 0 && bb.x < SUN_MOON_BOX.top_left.x);
        assert!(CLOUD_BOX.intersection(&SUN_MOON_BOX).is_zero_sized());
        assert!(CLOUD_BOX.intersection(&CROSS_BOX).is_zero_sized());
        assert!(!CLOUD_BOX.contains(SUN_MOON_CENTRE));
        for s in &FLOOR_SLOTS {
            assert!(CLOUD_BOX.intersection(s).is_zero_sized());
        }
    }

    #[test]
    fn the_cloud_is_a_black_outline_by_day_only() {
        let day = drawn(state(Sky::Day, 0, false));
        assert!(day.any(CLOUD_BOX, BLACK), "cloud");
        assert!(!day.any(CLOUD_BOX, RED));
        // The base row is a flat 1 px line, the inside stays white.
        let bottom = CLOUD_BOX.bottom_right().unwrap().y;
        let base = (CLOUD_BOX.top_left.x..=CLOUD_BOX.bottom_right().unwrap().x)
            .filter(|&x| day.get(x, bottom) == BLACK)
            .count();
        assert!(base >= 14, "base {base}");
        let mid = CLOUD_BOX.center();
        assert_eq!(day.get(mid.x, mid.y), WHITE, "white fill");
        // The sun is drawn exactly as without the cloud: no sun pixel moved.
        let night = drawn(state(Sky::Night, 0, false));
        // (A night star may sit in the box: it is not a cloud.)
        for p in CLOUD_BOX
            .points()
            .filter(|p| !star_boxes().iter().any(|s| s.contains(*p)))
        {
            let level = NIGHT_LEVELS[(((p.y - SKY_TOP) / BAND_ROWS) as usize).min(3)];
            let black = BAYER4[(p.y % 4) as usize][(p.x % 4) as usize] < level;
            let want = if black { BLACK } else { WHITE };
            assert_eq!(night.get(p.x, p.y), want, "night at {p:?}");
        }
        let plain = drawn(state(Sky::Plain, 0, false));
        assert!(!plain.any(CLOUD_BOX, BLACK));
    }

    #[test]
    fn only_the_cross_is_red() {
        for bg in states() {
            let c = drawn(bg);
            let red_outside = PET_AREA
                .points()
                .any(|p| c.get(p.x, p.y) == RED && !CROSS_BOX.contains(p));
            assert!(!red_outside, "{bg:?}");
            assert_eq!(c.any(CROSS_BOX, RED), bg.poorly, "{bg:?}");
        }
        let c = drawn(state(Sky::Night, 0, true));
        assert_eq!(c.get(144, 74), RED, "centre");
        assert_eq!(c.get(144, 71), RED);
        assert_eq!(c.get(147, 74), RED);
        assert_eq!(
            c.get(141, 71),
            WHITE,
            "corner of the box is white, not dither"
        );
    }
}

/// What the background adds to the refresh and battery cost (plan 7, task 4).
/// The SSD1680 runs every runtime refresh as `Fast`, which never flashes. The
/// SSD1675 promotes a partial refresh to a flashing full one once the vendor's
/// changed-pixel counter passes `full_after_screens` screens of changed
/// pixels, so what matters there is changed pixels per idle frame.
#[cfg(test)]
mod budget {
    use super::*;
    use crate::game::engine::anim_files::{anim_filename, frame_count};
    use crate::game::engine::{DisplayAnim, PetKind};
    use crate::game::pacing;
    use crate::game::pet_registry::{MAX_PETS, PetDef};
    use crate::game::sprite_loader::blit_pcx_sim;
    use crate::game::test_canvas::Canvas;

    /// Vendor `PartialConfig::default().full_after_screens` (3) full screens.
    const SSD1675_FULL_AFTER_PX: u32 = 3 * 152 * 152;
    const IDLE_FRAMES_PER_DAY: u32 = 24 * 3600 / pacing::IDLE_FRAME_SECS as u32;

    const IDLE_PACED: [DisplayAnim; 10] = [
        DisplayAnim::Idle,
        DisplayAnim::Happy,
        DisplayAnim::Sleeping,
        DisplayAnim::WarningSick,
        DisplayAnim::WarningTired,
        DisplayAnim::WarningHungry,
        DisplayAnim::WarningMiserable,
        DisplayAnim::CriticalSick,
        DisplayAnim::CriticalTired,
        DisplayAnim::CriticalHungry,
    ];

    /// Differing pixels in the pet area rows (y 35 to 109).
    fn changed_px(a: &Canvas, b: &Canvas) -> u32 {
        let mut n = 0;
        for y in SKY_TOP..=FLOOR_BOTTOM {
            for x in 0..152 {
                n += (a.get(x, y) != b.get(x, y)) as u32;
            }
        }
        n
    }

    fn sky_only(sky: Sky) -> BackgroundState {
        BackgroundState {
            sky,
            floor: 0,
            poorly: false,
        }
    }

    /// The backdrop of `sky`, then the sprite file on top, as the game draws.
    fn frame(sky: Sky, name: &[u8; 11]) -> Canvas {
        let mut c = Canvas::new();
        draw_background(&mut c, &sky_only(sky)).unwrap();
        blit_pcx_sim(&mut c, name, 0, SKY_TOP, false);
        c
    }

    /// The pets of the repo roster, parsed from the shipped `PETS.CFG`. The
    /// registry installs once per process, so it is never touched here.
    fn roster() -> std::vec::Vec<PetKind> {
        let mut pets: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();
        crate::game::pets_cfg::parse(include_bytes!("../../assets/to-badge/PETS.CFG"), &mut pets);
        pets.iter().map(|p| PetKind(p.id)).collect()
    }

    /// Worst changed pixels between two consecutive frames of any idle-paced
    /// animation of any roster pet, over `sky`, and how many (pet, anim) pairs
    /// were measured. The loop wraps.
    fn worst_frame_step(sky: Sky) -> (u32, u32) {
        let (mut worst, mut pairs) = (0, 0);
        for kind in roster() {
            for anim in IDLE_PACED {
                let n = frame_count(kind, anim);
                if n < 2 {
                    continue;
                }
                pairs += 1;
                for f in 0..n {
                    let a = frame(sky, &anim_filename(kind, anim, f));
                    let b = frame(sky, &anim_filename(kind, anim, (f + 1) % n));
                    worst = worst.max(changed_px(&a, &b));
                }
            }
        }
        (worst, pairs)
    }

    #[test]
    fn the_sky_switch_is_cheap() {
        let px = changed_px(&drawn_sky(Sky::Day), &drawn_sky(Sky::Night));
        eprintln!("sky switch changes {px} px");
        assert!(px <= 6_000, "{px} px");
    }

    fn drawn_sky(sky: Sky) -> Canvas {
        let mut c = Canvas::new();
        draw_background(&mut c, &sky_only(sky)).unwrap();
        c
    }

    /// Night is 21:00 to 07:00 (`scene::is_night`): 120 of the 288 idle frames.
    fn night_frames() -> u32 {
        let night_min = (0..1440u16).filter(|&m| crate::game::engine::scene::is_night(m)).count() as u32;
        night_min * 60 / pacing::IDLE_FRAME_SECS as u32
    }

    #[test]
    fn ssd1675_promotions_stay_bounded() {
        assert_eq!(IDLE_FRAMES_PER_DAY, 288);
        assert_eq!(night_frames(), 120);
        let day_frames = IDLE_FRAMES_PER_DAY - night_frames();
        let switch_px = changed_px(&drawn_sky(Sky::Day), &drawn_sky(Sky::Night));
        let (plain_worst, pairs) = worst_frame_step(Sky::Plain);
        let (day_worst, day_pairs) = worst_frame_step(Sky::Day);
        let (night_worst, night_pairs) = worst_frame_step(Sky::Night);
        // The roster has 6 pets, each with several animated idle-paced anims.
        assert!(
            pairs >= 20 && day_pairs == pairs && night_pairs == pairs,
            "{pairs} pairs measured"
        );
        assert!(plain_worst > 1_000, "plain worst {plain_worst} px");
        let per_day = |px: u32| px as f32 / SSD1675_FULL_AFTER_PX as f32;
        // Worst step on every frame: an upper bound. Baseline = backgrounds off.
        let base = per_day(IDLE_FRAMES_PER_DAY * plain_worst);
        let real = per_day(day_frames * day_worst + night_frames() * night_worst + 2 * switch_px);
        let day_only = per_day(IDLE_FRAMES_PER_DAY * day_worst);
        let added = real - base;
        eprintln!(
            "ssd1675 worst frame step ({pairs} pairs): plain {plain_worst} px, day {day_worst} px, night {night_worst} px"
        );
        eprintln!(
            "ssd1675 full refreshes a day: real hours with background {real:.2}, without {base:.2} (added {added:.2}); \
             day only with background {day_only:.2}, without {base:.2}"
        );
        assert!(added <= 0.5, "the background adds {added} a day");
    }
}
