//! Host-only helper for the player's guide:
//!   `guide data --preset classic|casual --out FILE`
//!   `guide words --out FILE`
//!   `guide shots`                      (one shot name per line)
//!   `guide shot NAME --out DIR`        (writes DIR/NAME.png)
//!   `guide sprites --out DIR`          (PNGs and DIR/roster.json)
//!   `guide screen KEYS --out FILE`     (any screen after pressing KEYS, see shots::screen)
use bornhack_aegg::game::engine::thresholds::Mode;
use bornhack_aegg::guide::data::dump;
use bornhack_aegg::guide::json::J;

const USAGE: &str = "usage: guide data --preset classic|casual --out FILE\n       guide words --out FILE\n       guide shots\n       guide shot NAME --out DIR\n       guide sprites --out DIR\n       guide screen KEYS --out FILE";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (json, out) = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["data", f1, v1, f2, v2] => {
            let (mut preset, mut out) = (None, None);
            for (flag, val) in [(f1, v1), (f2, v2)] {
                match *flag {
                    "--preset" => preset = Some(*val),
                    "--out" => out = Some(*val),
                    _ => usage(),
                }
            }
            let mode = match preset {
                Some("classic") => Mode::Classic,
                Some("casual") => Mode::Casual,
                _ => usage(),
            };
            let Some(out) = out else { usage() };
            (dump(mode), out.to_string())
        }
        ["shots"] => {
            for shot in bornhack_aegg::guide::shots::SHOTS {
                println!("{}", shot.name);
            }
            return;
        }
        ["shot", name, "--out", dir] => {
            match bornhack_aegg::guide::shots::run(name, std::path::Path::new(dir)) {
                Ok(s) => println!("{name} {}x{} non_white={}", s.w, s.h, s.non_white),
                Err(e) => {
                    eprintln!("guide: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        ["screen", keys, "--out", file] => {
            let file = std::path::Path::new(file);
            match bornhack_aegg::guide::shots::screen(keys, file) {
                Ok(s) => println!("{} {}x{} non_white={}", file.display(), s.w, s.h, s.non_white),
                Err(e) => {
                    eprintln!("guide: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        ["sprites", "--out", dir] => {
            let dir = std::path::Path::new(dir);
            let roster = bornhack_aegg::guide::sprites::export(dir);
            (roster, dir.join("roster.json").to_string_lossy().into_owned())
        }
        ["words", "--out", out] => (bornhack_aegg::guide::words::dump(), out.to_string()),
        _ => usage(),
    };
    write(&json, &out);
}

fn write(json: &J, out: &str) {
    let mut text = json.render();
    text.push('\n');
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("guide: cannot write {out}: {e}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}
