//! Runs the onigiri welcome outside the TUI.
//!
//!   onigiri --live  [--tier true|256|16] [--static]
//!   onigiri --bench [--tier true|256|16]
//!   onigiri --cast FILE [--size WxH] [--from MS] [--until MS] [--at MS]
//!                       [--frame MS] [--tier true|256|16] [--static]
//!
//! `--live` plays it in this terminal with a performance overlay; press any
//! key to quit. `--bench` prints per-frame cost and output size. `--cast`
//! writes an asciicast v2 recording at the scene's own frame cadence unless
//! `--frame` fixes one.

mod bench;
mod cast;
mod live;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use nori_tui_components::onigiri::Caption;
use nori_tui_components::onigiri::Onigiri;
use nori_tui_components::onigiri::Tier;

pub struct Opts {
    pub tier: Tier,
    pub settled: bool,
    pub size: (u16, u16),
    pub from: u64,
    pub until: u64,
    pub frame: Option<u64>,
}

impl Opts {
    pub fn scene(&self) -> Onigiri {
        let caption = Caption {
            version: "v0.42.0".to_string(),
            cwd: "~/tidepool".to_string(),
        };
        let scene = Onigiri::new(self.tier, caption);
        if self.settled { scene.settled() } else { scene }
    }
}

enum Mode {
    Live,
    Bench,
    Cast(String),
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut mode = None;
    let mut o = Opts {
        tier: Tier::detect(),
        settled: false,
        size: (100, 30),
        from: 0,
        until: 8000,
        frame: None,
    };
    let mut at = None;
    while let Some(arg) = args.next() {
        let mut val = || args.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--live" => mode = Some(Mode::Live),
            "--bench" => mode = Some(Mode::Bench),
            "--cast" => mode = Some(Mode::Cast(val()?)),
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').context("--size takes WxH")?;
                o.size = (w.parse()?, h.parse()?);
            }
            "--from" => o.from = val()?.parse()?,
            "--until" => o.until = val()?.parse()?,
            "--at" => at = Some(val()?.parse::<u64>()?),
            "--frame" => o.frame = Some(val()?.parse()?),
            "--tier" => o.tier = Tier::parse(&val()?).context("--tier takes true, 256 or 16")?,
            "--static" => o.settled = true,
            other => bail!("unknown argument {other}; see the header of src/onigiri/demo/main.rs"),
        }
    }
    if o.settled {
        o.until = o.from;
    }
    if let Some(at) = at {
        (o.from, o.until) = (at, at);
    }
    match mode {
        Some(Mode::Live) => live::run(&o),
        Some(Mode::Bench) => bench::run(&o),
        Some(Mode::Cast(path)) => cast::write(&path, &o),
        None => bail!("pick one of --live, --bench or --cast FILE"),
    }
}
