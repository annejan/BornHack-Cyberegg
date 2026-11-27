//! Desktop helpers shared by the simulator window and the guide's headless
//! shots: the tri-colour draw adapter, the repo's pet roster, and one-frame
//! rendering to a PNG.
extern crate embedded_graphics as eg;
extern crate embedded_graphics_simulator as simulator;

use std::path::Path;

use eg::pixelcolor::Rgb888;
use eg::prelude::*;
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

use crate::{TriColor, draw_graphics};

/// Adapter that translates TriColor draw calls to an Rgb888 SimulatorDisplay.
pub struct TriColorDisplay<'a>(pub &'a mut SimulatorDisplay<Rgb888>);

impl DrawTarget for TriColorDisplay<'_> {
    type Color = TriColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<TriColor>>,
    {
        self.0
            .draw_iter(
                pixels
                    .into_iter()
                    .map(|Pixel(p, c)| Pixel(p, Rgb888::from(c))),
            )
            .unwrap();
        Ok(())
    }
}

impl OriginDimensions for TriColorDisplay<'_> {
    fn size(&self) -> Size {
        self.0.size()
    }
}

/// The badge reads `PETS.CFG` off its FAT12 partition at boot; the simulator
/// has no such partition, so it compiles the repo's copy in and installs the
/// same roster from it. Not a fallback for the firmware — that deliberately
/// has none — just the sim's stand-in for the USB volume, kept pointed at the
/// one file so a pet added there shows up in both.
pub fn install_pets_from_repo_cfg() {
    install_pets_from_cfg_text(include_str!("../assets/to-badge/PETS.CFG"));
}

/// An old badge: `copy_assets --update` never edits a badge's `PETS.CFG`, so
/// it still lists `8=Jellyfish` after the Jellyfish sprites were removed.
/// The repo's roster plus that line, with the sprites dropped as at boot.
pub fn install_pets_from_old_badge_cfg() {
    let mut text = String::from(include_str!("../assets/to-badge/PETS.CFG"));
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("8=Jellyfish\n");
    install_pets_from_cfg_text(&text);
}

/// Install the roster from `PETS.CFG` text, then drop every pet the
/// simulator has no idle sprite for (as the boot does). Once per process.
pub fn install_pets_from_cfg_text(text: &str) {
    use crate::game::pet_registry::{self, MAX_PETS, PetDef};

    let mut pets: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();
    crate::game::pets_cfg::parse(text.as_bytes(), &mut pets);
    crate::game::pets_cfg::migrate_old_format(&mut pets);
    pet_registry::install(&pets);
    pet_registry::drop_without_sprites(|id| {
        let idle = crate::game::engine::anim_files::anim_id_for(crate::game::engine::DisplayAnim::Idle);
        crate::game::sprite_loader::count_anim_frames(id, idle) > 0
    });
}

/// Enlargement of the saved PNGs: the 152 px screen becomes 608 px.
pub const PNG_SCALE: u32 = 4;

/// Draw one frame of the game on a cleared white 152 x 152 display, as the
/// simulator window does.
pub fn render() -> SimulatorDisplay<Rgb888> {
    let mut display: SimulatorDisplay<Rgb888> = SimulatorDisplay::new(Size::new(152, 152));
    display.clear(Rgb888::new(255, 255, 255)).unwrap();
    draw_graphics(&mut TriColorDisplay(&mut display), "sim", &85).unwrap();
    display
}

/// Save `d` as a PNG, enlarged by [`PNG_SCALE`], with no pixel gaps.
pub fn save_png(d: &SimulatorDisplay<Rgb888>, path: &Path) -> Result<(), String> {
    let settings = OutputSettingsBuilder::new()
        .scale(PNG_SCALE)
        .pixel_spacing(0)
        .build();
    d.to_rgb_output_image(&settings)
        .save_png(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// How many pixels of `d` are not pure white.
pub fn non_white(d: &SimulatorDisplay<Rgb888>) -> usize {
    let white = Rgb888::new(255, 255, 255);
    let size = d.size();
    (0..size.height as i32)
        .flat_map(|y| (0..size.width as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| d.get_pixel(Point::new(x, y)) != white)
        .count()
}
