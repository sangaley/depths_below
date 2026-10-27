//! Saved gun designs: a built gun as data.
//!
//! A design is the gun's parts, laid out relative to its Firing Piece as if
//! the gun faced the bow (+x), plus the shell it fires. Designs live one per
//! file in `designs/guns/*.json`, like ship designs: save one from a built
//! gun's window, stamp it into a hull or onto a turret deck in build mode
//! (T to pick, Enter to place), or write one by hand -- faction guns are
//! meant to be drawn, the same as faction ships.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::shell::ShellDesign;
use crate::components::{Module, ModuleType, Rotation};

pub const GUN_DESIGN_DIR: &str = "designs/guns";

/// Where gun designs are read and written. DEPTHS_GUN_DESIGN_DIR moves it
/// (tests write somewhere that isn't the repo).
pub fn gun_design_dir() -> std::path::PathBuf {
    std::env::var("DEPTHS_GUN_DESIGN_DIR").map(Into::into).unwrap_or_else(|_| GUN_DESIGN_DIR.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GunDesignPart {
    pub module_type: ModuleType,
    /// From the firing piece, with the gun facing +x.
    pub offset: IVec2,
    pub rotation: Rotation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GunDesign {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Every part, the firing piece included (at 0, 0, facing East).
    pub parts: Vec<GunDesignPart>,
    /// None: the default shell.
    #[serde(default)]
    pub shell: Option<ShellDesign>,
    /// The faction whose ships carry it (an AiShipType name, e.g.
    /// "TerranHegemony"); None for the player's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    /// Fitted on point defence: shoots incoming missiles, never ships
    /// (combat::missile_defence).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub point_defence: bool,
}

/// Clockwise quarter turns from East.
fn turns(r: Rotation) -> u8 {
    match r {
        Rotation::East => 0,
        Rotation::South => 1,
        Rotation::West => 2,
        Rotation::North => 3,
    }
}

/// `v` turned `k` quarter turns clockwise.
fn turn(v: IVec2, k: u8) -> IVec2 {
    (0..k % 4).fold(v, |v, _| IVec2::new(v.y, -v.x))
}

fn turn_rot(r: Rotation, k: u8) -> Rotation {
    (0..k % 4).fold(r, |r, _| r.rotate_cw())
}

impl GunDesign {
    /// A design from a built gun's blocks (its firing piece among them),
    /// turned so it faces +x.
    pub fn capture(name: impl Into<String>, blocks: &[Module], shell: Option<ShellDesign>) -> Option<Self> {
        let fp = blocks.iter().find(|m| m.module_type == ModuleType::FiringPiece)?;
        let undo = (4 - turns(fp.rotation)) % 4;
        let mut parts: Vec<GunDesignPart> = blocks
            .iter()
            .map(|m| GunDesignPart {
                module_type: m.module_type,
                offset: turn(m.grid_position - fp.grid_position, undo),
                rotation: turn_rot(m.rotation, undo),
            })
            .collect();
        parts.sort_by_key(|p| (p.offset.x, p.offset.y));
        Some(Self {
            name: name.into(),
            description: String::new(),
            parts,
            shell: shell.filter(|s| *s != ShellDesign::default()),
            faction: None,
            point_defence: false,
        })
    }

    /// The parts laid down with the firing piece at `origin`, facing `facing`.
    pub fn placed(&self, origin: IVec2, facing: Rotation) -> Vec<(ModuleType, IVec2, Rotation)> {
        let k = turns(facing);
        self.parts
            .iter()
            .map(|p| (p.module_type, origin + turn(p.offset, k), turn_rot(p.rotation, k)))
            .collect()
    }

    pub fn cost(&self, registry: &crate::building::ModuleRegistry) -> u32 {
        self.parts.iter().map(|p| registry.get(p.module_type).cost).sum()
    }

    /// What it adds up to, worked out the same way a built one is.
    pub fn assembled(&self) -> Option<super::gun_assembly::GunAssembly> {
        let modules: Vec<(Entity, Module)> = self
            .placed(IVec2::ZERO, Rotation::East)
            .into_iter()
            .enumerate()
            .map(|(i, (t, at, r))| {
                let m = Module {
                    module_type: t, health: 1.0, max_health: 1.0, power_consumption: 0.0, power_generation: 0.0,
                    is_active: true, grid_position: at, size: IVec2::ONE, rotation: r,
                };
                (Entity::from_raw_u32(i as u32 + 1).unwrap(), m)
            })
            .collect();
        let shells = self
            .shell
            .clone()
            .and_then(|s| modules.iter().find(|(_, m)| m.module_type == ModuleType::FiringPiece).map(|(e, _)| (*e, s)))
            .into_iter()
            .collect();
        super::gun_assembly::assemble_with_shells(&modules, &shells).into_iter().next().map(|(_, g)| g)
    }

    /// "280 mm, 6 m barrel, 3 loaders, 27.8 rpm".
    pub fn summary(&self) -> String {
        match self.assembled() {
            Some(g) => format!(
                "{:.0} mm, {} m barrel, {} loader(s), {:.0} m/s, {:.1} rpm, {} parts",
                g.stats.gauge_mm, g.parts.barrel_m, g.parts.loaders.len(), g.stats.velocity_mps, g.stats.rpm, self.parts.len()
            ),
            None => format!("{} parts, no firing piece", self.parts.len()),
        }
    }

    /// A file name from the design's name.
    pub fn file_stem(&self) -> String {
        let s: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let s = s.trim_matches('_').to_string();
        if s.is_empty() { "gun".into() } else { s }
    }
}

/// Every gun design on disk, by name. Re-read when one is saved.
#[derive(Resource, Default, Debug)]
pub struct GunDesigns(pub Vec<GunDesign>);

pub fn read_gun_designs() -> Vec<GunDesign> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(gun_design_dir()) else { return out };
    let mut paths: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    for path in paths {
        match std::fs::read_to_string(&path).map(|s| serde_json::from_str::<GunDesign>(&s)) {
            Ok(Ok(d)) => out.push(d),
            Ok(Err(e)) => warn!("gun design {:?} doesn't read: {}", path, e),
            Err(e) => warn!("gun design {:?}: {}", path, e),
        }
    }
    out
}

/// Write a design to `designs/guns/<name>.json`, never over another: a name
/// already taken gets a number.
pub fn write_gun_design(design: &GunDesign) -> std::io::Result<std::path::PathBuf> {
    let dir = gun_design_dir();
    std::fs::create_dir_all(&dir)?;
    let stem = design.file_stem();
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}_{n}.json"));
        n += 1;
    }
    let json = serde_json::to_string_pretty(design).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

pub fn load_gun_designs(mut designs: ResMut<GunDesigns>) {
    designs.0 = read_gun_designs();
    info!("{} gun designs in {:?}", designs.0.len(), gun_design_dir());
}

#[cfg(test)]
mod gun_design_tests {
    use super::*;
    use ModuleType::*;

    fn m(t: ModuleType, x: i32, y: i32, r: Rotation) -> Module {
        Module {
            module_type: t, health: 1.0, max_health: 1.0, power_consumption: 0.0, power_generation: 0.0,
            is_active: true, grid_position: IVec2::new(x, y), size: IVec2::ONE, rotation: r,
        }
    }

    /// A small gun facing North (barrel up the +y axis) at (5, 5).
    fn north_gun() -> Vec<Module> {
        vec![
            m(FiringPiece, 5, 5, Rotation::North),
            m(GunBarrel, 5, 6, Rotation::North),
            m(GunBarrel, 5, 7, Rotation::North),
            m(Loader, 5, 4, Rotation::North),
            m(GaugeIncreaser, 4, 5, Rotation::North),
        ]
    }

    #[test]
    fn a_captured_gun_is_stored_facing_the_bow() {
        let d = GunDesign::capture("test", &north_gun(), None).unwrap();
        let fp = d.parts.iter().find(|p| p.module_type == FiringPiece).unwrap();
        assert_eq!((fp.offset, fp.rotation), (IVec2::ZERO, Rotation::East));
        assert!(d.parts.contains(&GunDesignPart { module_type: GunBarrel, offset: IVec2::new(2, 0), rotation: Rotation::East }));
        assert!(d.parts.contains(&GunDesignPart { module_type: Loader, offset: IVec2::new(-1, 0), rotation: Rotation::East }));
        // Left of a north-facing gun is port; facing the bow, port is +y.
        assert!(d.parts.iter().any(|p| p.module_type == GaugeIncreaser && p.offset == IVec2::new(0, 1)));
    }

    #[test]
    fn placing_turns_it_back_whichever_way_it_faces() {
        let original = north_gun();
        let d = GunDesign::capture("test", &original, None).unwrap();
        let mut back = d.placed(IVec2::new(5, 5), Rotation::North);
        let mut want: Vec<_> = original.iter().map(|m| (m.module_type, m.grid_position, m.rotation)).collect();
        back.sort_by_key(|(_, c, _)| (c.x, c.y));
        want.sort_by_key(|(_, c, _)| (c.x, c.y));
        assert_eq!(back, want);
        let west = d.placed(IVec2::ZERO, Rotation::West);
        assert!(west.contains(&(GunBarrel, IVec2::new(-2, 0), Rotation::West)));
    }

    #[test]
    fn a_design_adds_up_like_the_gun_it_came_from() {
        let d = GunDesign::capture("test", &north_gun(), None).unwrap();
        let g = d.assembled().unwrap();
        assert_eq!(g.stats.gauge_mm, 115.0);
        assert_eq!(g.parts.barrel_m, 2);
    }

    #[test]
    fn the_default_shell_isnt_written_down() {
        let d = GunDesign::capture("test", &north_gun(), Some(ShellDesign::default())).unwrap();
        assert_eq!(d.shell, None);
    }

    #[test]
    fn a_design_from_the_gun_foundry_reads() {
        // What tools/gun-editor saves: the game's fields plus its own.
        let json = r#"{"name":"T","description":"","parts":[{"module_type":"FiringPiece","offset":[0,0],"rotation":"East"}],
            "shell":{"base":"Tracer","casings":3,"bodies":["He"],"head":"Ap"},"faction":"TerranHegemony","savedAt":"2026-10-26T10:00:00Z"}"#;
        let d: GunDesign = serde_json::from_str(json).unwrap();
        assert_eq!(d.faction.as_deref(), Some("TerranHegemony"));
        assert_eq!(d.shell.unwrap().casings, 3);
    }

    #[test]
    fn names_become_safe_file_names() {
        let d = GunDesign { name: "280 mm HE / Terran!".into(), description: String::new(), parts: vec![], shell: None, faction: None, point_defence: false };
        assert_eq!(d.file_stem(), "280_mm_he___terran");
    }
}
