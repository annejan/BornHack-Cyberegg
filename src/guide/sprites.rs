//! The pet sprites for the booklet (`guide sprites`): every frame of the
//! animations the guide shows, for every pet in `PETS.CFG`, as a PNG cropped to
//! the pet, one PNG pixel per badge pixel, with a clear ground, plus `roster.json` listing what exists. Sponsor art (prefix
//! 03) and menu icons (prefix 04) are never in the roster, so never exported.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::path::Path;

use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::SimulatorDisplay;

use super::json::J;
use super::shots::HERO_NAME;
use crate::game::engine::anim_files::{build_filename, frame_count, start_screen_filename};
use crate::game::engine::{DisplayAnim, PetKind};
use crate::game::pet_registry;
use crate::game::sprite_loader::{blit_pcx_sim, pcx_size_sim};
use crate::sim_support::{self, TriColorDisplay};

/// The animations the guide shows, under the names the booklet asks for.
pub const ANIMS: [(&str, DisplayAnim); 15] = [
    ("idle", DisplayAnim::Idle),
    ("happy", DisplayAnim::Happy),
    ("sleeping", DisplayAnim::Sleeping),
    ("feeding", DisplayAnim::Feeding),
    ("drinking", DisplayAnim::Drinking),
    ("playing", DisplayAnim::Playing),
    ("exercising", DisplayAnim::Exercising),
    ("healing", DisplayAnim::Healing),
    ("hungry", DisplayAnim::WarningHungry),
    ("tired", DisplayAnim::WarningTired),
    ("sick", DisplayAnim::WarningSick),
    ("critical", DisplayAnim::CriticalSick),
    ("hibernating", DisplayAnim::Hibernating),
    ("egg", DisplayAnim::Hatching { ticks_remaining: 0 }),
    ("gone", DisplayAnim::Gone),
];

/// The prefix as it appears in file names: two uppercase hex digits.
fn hex(prefix: u8) -> String {
    alloc::format!("{prefix:02X}")
}

/// Draw the PCX `name` and save it cropped to the pet: one PNG pixel per badge
/// pixel (the booklet scales by whole numbers), white pixels connected to the
/// edge made clear, white pixels inside the outline kept white.
fn save_pcx(name: &[u8; 11], path: &Path) -> Result<(), String> {
    let (w, h) = pcx_size_sim(name).ok_or_else(|| {
        alloc::format!(
            "missing or unreadable sprite {}",
            String::from_utf8_lossy(name)
        )
    })?;
    let mut display: SimulatorDisplay<Rgb888> = SimulatorDisplay::new(Size::new(w, h));
    display.clear(Rgb888::new(255, 255, 255)).unwrap();
    blit_pcx_sim(&mut TriColorDisplay(&mut display), name, 0, 0, false);
    let white = Rgb888::new(255, 255, 255);
    let px = |x: u32, y: u32| display.get_pixel(Point::new(x as i32, y as i32));
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px(x, y) != white {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return Err(alloc::format!("sprite {} is blank", path.display()));
    }
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut clear = alloc::vec![false; (cw * ch) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for y in 0..ch {
        for x in 0..cw {
            if (x == 0 || y == 0 || x == cw - 1 || y == ch - 1) && px(x0 + x, y0 + y) == white {
                stack.push((x, y));
            }
        }
    }
    while let Some((x, y)) = stack.pop() {
        let i = (y * cw + x) as usize;
        if clear[i] || px(x0 + x, y0 + y) != white {
            continue;
        }
        clear[i] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if x + 1 < cw {
            stack.push((x + 1, y));
        }
        if y + 1 < ch {
            stack.push((x, y + 1));
        }
    }
    let img = image::RgbaImage::from_fn(cw, ch, |x, y| {
        let c = px(x0 + x, y0 + y);
        let a = if clear[(y * cw + x) as usize] { 0 } else { 255 };
        image::Rgba([c.r(), c.g(), c.b(), a])
    });
    img.save(path)
        .map_err(|e| alloc::format!("cannot write {}: {e}", path.display()))
}

/// Write every sprite PNG into `out_dir` and return the roster JSON.
pub fn export(out_dir: &Path) -> J {
    sim_support::install_pets_from_repo_cfg();
    std::fs::create_dir_all(out_dir).expect("create the sprite folder");
    let mut pets: Vec<J> = Vec::new();
    for &kind in PetKind::roster() {
        let prefix = hex(kind.0);
        let mut anims: Vec<(String, J)> = Vec::new();
        for (name, anim) in ANIMS {
            let frames = frame_count(kind, anim);
            for f in 0..frames {
                let path = out_dir.join(alloc::format!("{prefix}-{name}-{f}.png"));
                if let Err(e) = save_pcx(&build_filename(kind, anim, f), &path) {
                    panic!("{e}");
                }
            }
            if frames > 0 {
                anims.push((name.to_string(), J::Int(frames as i64)));
            }
        }
        pets.push(J::Obj(alloc::vec![
            ("prefix".to_string(), J::Str(prefix)),
            (
                "name".to_string(),
                J::Str(pet_registry::name_of(kind.0).to_string())
            ),
            ("anims".to_string(), J::Obj(anims)),
        ]));
    }
    save_pcx(&start_screen_filename(), &out_dir.join("start.png")).expect("the start screen");
    let hero = PetKind::roster().first().map_or(0, |k| k.0);
    J::Obj(alloc::vec![
        (
            "hero".to_string(),
            J::Obj(alloc::vec![
                ("name".to_string(), J::Str(HERO_NAME.to_string())),
                ("prefix".to_string(), J::Str(hex(hero))),
            ]),
        ),
        ("pets".to_string(), J::Arr(pets)),
        ("start".to_string(), J::Str("start.png".to_string())),
    ])
}
