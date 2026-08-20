//! The tuning tables, and the one function that turns their text into something
//! the generator can trust.
//!
//! [`parse`] is the only way in. It resolves every cross-table key and checks
//! every range, so no reader below it needs a guard and none has a panic path.
//! That is the same contract `crates/sprites` states for sprite manifests, and
//! it is why the generator can index rather than look up.
//!
//! This module takes table *text*, never a path: the crate does no I/O, so the
//! frontend reads through Godot and the preview tool through `std::fs`.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use serde::Deserialize;

use crate::tile::{MaterialId, TileFlags};

/// Every generated height, so movement's `i8` arithmetic cannot overflow and the
/// frontend can size its cliff art.
pub const HEIGHT_RANGE: RangeInclusive<i8> = -32..=32;

/// Which site class, as an index into the site class table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SiteClassId(pub u8);

/// Which kind of site, as an index into the site table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SiteId(pub u16);

/// Which biome, as an index into the biome table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BiomeId(pub u16);

/// The four table texts. The caller supplies the strings.
#[derive(Debug, Clone, Copy)]
pub struct Tables<'a> {
    pub world: &'a str,
    pub tiers: &'a str,
    pub materials: &'a str,
    pub biomes: &'a str,
    pub site_classes: &'a str,
    pub sites: &'a str,
}

/// Why a table set is not usable. Names the table, the row and the field, so the
/// message points at the cell to edit.
#[derive(Debug)]
pub struct Error {
    table: &'static str,
    /// 1-based and counted over data rows, matching what a spreadsheet shows
    /// once the header is discounted. `None` for a whole-table problem.
    row: Option<usize>,
    detail: String,
}

impl Error {
    fn table(table: &'static str, detail: impl Into<String>) -> Self {
        Self {
            table,
            row: None,
            detail: detail.into(),
        }
    }

    fn row(table: &'static str, row: usize, detail: impl Into<String>) -> Self {
        Self {
            table,
            row: Some(row),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.row {
            Some(row) => write!(f, "{} row {}: {}", self.table, row, self.detail),
            None => write!(f, "{}: {}", self.table, self.detail),
        }
    }
}

impl std::error::Error for Error {}

/// A ground material, and what standing on it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialRow {
    pub name: String,
    pub flags: TileFlags,
}

/// One biome: which tier offers it, and how its ground looks and undulates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeRow {
    pub name: String,
    pub tier: u8,
    /// Relative chance of being picked among its tier's biomes.
    pub weight: u16,
    pub ground: MaterialId,
    /// Peak height in steps. The field runs from `-amp` to `+amp`.
    pub height_amp: i8,
    /// Tiles across one terrain feature. A designer-facing period, so the table
    /// says "features about 140 tiles wide" instead of a noise frequency.
    pub height_period: u32,
}

/// One class of point of interest, and the rules for spacing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteClassRow {
    pub name: String,
    /// Lattice pitch in tiles, so also the average gap between sites.
    pub spacing: i32,
    /// The gap the placement guarantees, whatever the roll. Must be under
    /// `spacing`, or a cell would have nowhere to put its site.
    pub separation: i32,
    /// Chance in 100 that a cell of this class holds a site at all.
    pub fill_pct: u8,
    /// No site of this class closer to the origin than this.
    pub min_from_spawn: i64,
    pub tier_lo: u8,
    pub tier_hi: u8,
}

/// One kind of site, and how big it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRow {
    pub name: String,
    pub class: SiteClassId,
    /// Relative chance among its class's kinds.
    pub weight: u16,
    /// Side length in tiles. Odd, so it has a centre tile to be placed on.
    pub footprint: i32,
}

/// One difficulty band: where it starts, and how far a region inside it may
/// stray from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierRow {
    pub tier: u8,
    /// Distance from the origin at which this band begins.
    pub inner_tiles: i64,
    pub harder_stray: u8,
    pub easier_stray: u8,
    /// Chance in 100 that a region in this band strays at all.
    pub stray_pct: u8,
}

/// Validated tuning data. Every accessor is total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRules {
    region_pitch: i32,
    region_jitter_pct: u32,
    home_bubble: i64,
    /// Ascending by `inner_tiles`, contiguous, and starting at tier 0.
    tiers: Vec<TierRow>,
    materials: Vec<MaterialRow>,
    biomes: Vec<BiomeRow>,
    /// Indexed by tier. Non-empty for every tier, which `parse` enforces.
    by_tier: Vec<Vec<BiomeId>>,
    site_classes: Vec<SiteClassRow>,
    sites: Vec<SiteRow>,
    /// Indexed by site class. Non-empty for every class.
    by_class: Vec<Vec<SiteId>>,
}

impl WorldRules {
    /// Distance between region lattice points, in tiles.
    #[must_use]
    pub fn region_pitch(&self) -> i32 {
        self.region_pitch
    }

    /// How far a region point may sit from its cell centre, as a percentage of
    /// half the pitch. At most 100, or the nearest point could fall outside the
    /// searched 3 by 3 block.
    #[must_use]
    pub fn region_jitter_pct(&self) -> u32 {
        self.region_jitter_pct
    }

    /// Radius around the origin that is always tier 0.
    #[must_use]
    pub fn home_bubble(&self) -> i64 {
        self.home_bubble
    }

    #[must_use]
    pub fn max_tier(&self) -> u8 {
        // `parse` rejects an empty table, so the last row exists.
        self.tiers.last().map_or(0, |row| row.tier)
    }

    /// Which band `distance` falls in. The last band runs to infinity, so the
    /// frontier never runs out of world.
    #[must_use]
    pub fn tier_for(&self, distance_tiles: i64) -> u8 {
        self.tiers
            .iter()
            .rev()
            .find(|row| distance_tiles >= row.inner_tiles)
            .map_or(0, |row| row.tier)
    }

    /// The stray rules for a tier.
    ///
    /// # Panics
    /// If `tier` exceeds [`Self::max_tier`]. Tiers are contiguous from 0, so any
    /// tier this crate produces is in range.
    #[must_use]
    pub fn band_of(&self, tier: u8) -> &TierRow {
        &self.tiers[tier as usize]
    }

    /// The biomes a tier can offer. Never empty.
    ///
    /// # Panics
    /// If `tier` exceeds [`Self::max_tier`].
    #[must_use]
    pub fn biomes_in(&self, tier: u8) -> &[BiomeId] {
        &self.by_tier[tier as usize]
    }

    /// # Panics
    /// If `id` did not come from these rules.
    #[must_use]
    pub fn biome(&self, id: BiomeId) -> &BiomeRow {
        &self.biomes[id.0 as usize]
    }

    /// # Panics
    /// If `id` did not come from these rules.
    #[must_use]
    pub fn material(&self, id: MaterialId) -> &MaterialRow {
        &self.materials[id.0 as usize]
    }

    /// Every site class, in table order.
    pub fn site_classes(&self) -> impl Iterator<Item = SiteClassId> + '_ {
        (0..self.site_classes.len()).map(|i| SiteClassId(i as u8))
    }

    /// # Panics
    /// If `id` did not come from these rules.
    #[must_use]
    pub fn site_class(&self, id: SiteClassId) -> &SiteClassRow {
        &self.site_classes[id.0 as usize]
    }

    /// # Panics
    /// If `id` did not come from these rules.
    #[must_use]
    pub fn site(&self, id: SiteId) -> &SiteRow {
        &self.sites[id.0 as usize]
    }

    /// The kinds a class can place. Never empty.
    ///
    /// # Panics
    /// If `id` did not come from these rules.
    #[must_use]
    pub fn sites_in(&self, id: SiteClassId) -> &[SiteId] {
        &self.by_class[id.0 as usize]
    }

    /// Looks a material up by the name its table row carries. For tools and
    /// tests; the generator indexes instead.
    #[must_use]
    pub fn material_named(&self, name: &str) -> Option<MaterialId> {
        self.materials
            .iter()
            .position(|row| row.name == name)
            .and_then(|i| u8::try_from(i).ok())
            .map(MaterialId)
    }
}

// Serde shapes, one per table. Separate from the validated rows above so the
// wire column names can differ from the field names the generator reads.

#[derive(Deserialize)]
struct WorldCsv {
    region_pitch_tiles: i32,
    region_jitter_pct: u32,
    home_bubble_tiles: i64,
}

#[derive(Deserialize)]
struct TierCsv {
    tier: u8,
    inner_tiles: i64,
    harder_stray: u8,
    easier_stray: u8,
    stray_pct: u8,
}

#[derive(Deserialize)]
struct MaterialCsv {
    material: String,
    blocks_walk: u8,
    blocks_jump: u8,
    blocks_shot: u8,
}

#[derive(Deserialize)]
struct SiteClassCsv {
    class: String,
    spacing: i32,
    separation: i32,
    fill_pct: u8,
    min_from_spawn: i64,
    tier_lo: u8,
    tier_hi: u8,
}

#[derive(Deserialize)]
struct SiteCsv {
    site: String,
    class: String,
    weight: u16,
    footprint: i32,
}

#[derive(Deserialize)]
struct BiomeCsv {
    biome: String,
    tier: u8,
    weight: u16,
    ground: String,
    height_amp: i8,
    height_period: u32,
}

/// Reads one table. Tab separated, `#` starts a comment, and a short row is an
/// error rather than a default: the `trailing-whitespace` hook strips a trailing
/// tab, and silently reading a default would turn that into a wrong world.
fn rows<T: for<'de> Deserialize<'de>>(table: &'static str, text: &str) -> Result<Vec<T>, Error> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .comment(Some(b'#'))
        .flexible(false)
        .from_reader(text.as_bytes());
    let mut out = Vec::new();
    for (i, record) in reader.deserialize().enumerate() {
        out.push(record.map_err(|e| Error::row(table, i + 1, e.to_string()))?);
    }
    if out.is_empty() {
        return Err(Error::table(table, "no data rows"));
    }
    Ok(out)
}

/// Reads a table set and checks that everything the generator assumes is true.
///
/// # Errors
/// The first broken invariant, naming the table, the row and the field.
pub fn parse(tables: Tables<'_>) -> Result<WorldRules, Error> {
    let world = one_world(tables.world)?;
    let tiers = tier_rows(tables.tiers)?;
    let materials = material_rows(tables.materials)?;
    let biomes = biome_rows(tables.biomes, &materials, tiers.len())?;
    let by_tier = group_by_tier(&biomes, &tiers)?;
    let site_classes = site_class_rows(tables.site_classes, tiers.len())?;
    let sites = site_rows(tables.sites, &site_classes)?;
    let by_class = group_by_class(&sites, &site_classes)?;

    Ok(WorldRules {
        region_pitch: world.region_pitch_tiles,
        region_jitter_pct: world.region_jitter_pct,
        home_bubble: world.home_bubble_tiles,
        tiers,
        materials,
        biomes,
        by_tier,
        site_classes,
        sites,
        by_class,
    })
}

fn site_class_rows(text: &str, tier_count: usize) -> Result<Vec<SiteClassRow>, Error> {
    const TABLE: &str = "site_classes.tsv";
    let rows: Vec<SiteClassCsv> = rows(TABLE, text)?;
    if u8::try_from(rows.len()).is_err() {
        return Err(Error::table(
            TABLE,
            "at most 255 site classes, since a class id is a byte",
        ));
    }
    let mut out: Vec<SiteClassRow> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let line = i + 1;
        if out.iter().any(|seen| seen.name == row.class) {
            return Err(Error::row(
                TABLE,
                line,
                format!("duplicate class {:?}", row.class),
            ));
        }
        if row.separation <= 0 {
            return Err(Error::row(TABLE, line, "separation must be positive"));
        }
        // The placement divides by `spacing - separation`, so equality would be a
        // division by zero and a larger separation would underflow into a huge
        // modulus that silently destroys the gap guarantee.
        if row.separation >= row.spacing {
            return Err(Error::row(
                TABLE,
                line,
                "separation must be below spacing, or a cell has nowhere to place its site",
            ));
        }
        if row.fill_pct == 0 || row.fill_pct > 100 {
            return Err(Error::row(TABLE, line, "fill_pct must be 1 to 100"));
        }
        if row.min_from_spawn < 0 {
            return Err(Error::row(
                TABLE,
                line,
                "min_from_spawn must not be negative",
            ));
        }
        if row.tier_lo > row.tier_hi {
            return Err(Error::row(TABLE, line, "tier_lo must not exceed tier_hi"));
        }
        if usize::from(row.tier_hi) >= tier_count {
            return Err(Error::row(
                TABLE,
                line,
                format!("tier_hi {} has no row in tiers.tsv", row.tier_hi),
            ));
        }
        out.push(SiteClassRow {
            name: row.class,
            spacing: row.spacing,
            separation: row.separation,
            fill_pct: row.fill_pct,
            min_from_spawn: row.min_from_spawn,
            tier_lo: row.tier_lo,
            tier_hi: row.tier_hi,
        });
    }
    Ok(out)
}

fn site_rows(text: &str, classes: &[SiteClassRow]) -> Result<Vec<SiteRow>, Error> {
    const TABLE: &str = "sites.tsv";
    let rows: Vec<SiteCsv> = rows(TABLE, text)?;
    if u16::try_from(rows.len()).is_err() {
        return Err(Error::table(TABLE, "at most 65535 sites"));
    }
    let mut out: Vec<SiteRow> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let line = i + 1;
        if out.iter().any(|seen| seen.name == row.site) {
            return Err(Error::row(
                TABLE,
                line,
                format!("duplicate site {:?}", row.site),
            ));
        }
        if row.weight == 0 {
            return Err(Error::row(
                TABLE,
                line,
                "weight must be positive, or the site can never be picked",
            ));
        }
        if row.footprint <= 0 {
            return Err(Error::row(TABLE, line, "footprint must be positive"));
        }
        if row.footprint % 2 == 0 {
            return Err(Error::row(
                TABLE,
                line,
                "footprint must be odd, so the site has a centre tile to sit on",
            ));
        }
        let position = classes
            .iter()
            .position(|c| c.name == row.class)
            .and_then(|p| u8::try_from(p).ok())
            .ok_or_else(|| {
                Error::row(
                    TABLE,
                    line,
                    format!("class {:?} has no row in site_classes.tsv", row.class),
                )
            })?;
        // Two footprints at the minimum gap must not overlap, or sites of one
        // class can be placed touching however large the separation is.
        let separation = classes[usize::from(position)].separation;
        if row.footprint > separation {
            return Err(Error::row(
                TABLE,
                line,
                format!(
                    "footprint {} is wider than class {:?}'s separation of {separation}",
                    row.footprint, row.class
                ),
            ));
        }
        out.push(SiteRow {
            name: row.site,
            class: SiteClassId(position),
            weight: row.weight,
            footprint: row.footprint,
        });
    }
    Ok(out)
}

/// One list of site ids per class, so picking a kind is an index rather than a
/// scan. Rejects a class no site claims, which would be a lattice placing
/// nothing.
fn group_by_class(sites: &[SiteRow], classes: &[SiteClassRow]) -> Result<Vec<Vec<SiteId>>, Error> {
    let mut grouped: BTreeMap<u8, Vec<SiteId>> = BTreeMap::new();
    for (i, site) in sites.iter().enumerate() {
        let id = u16::try_from(i).map_err(|_| Error::table("sites.tsv", "too many sites"))?;
        grouped.entry(site.class.0).or_default().push(SiteId(id));
    }
    classes
        .iter()
        .enumerate()
        .map(|(i, class)| {
            grouped.remove(&(i as u8)).ok_or_else(|| {
                Error::table(
                    "sites.tsv",
                    format!("class {:?} has no site using it", class.name),
                )
            })
        })
        .collect()
}

fn one_world(text: &str) -> Result<WorldCsv, Error> {
    const TABLE: &str = "world.tsv";
    let mut rows: Vec<WorldCsv> = rows(TABLE, text)?;
    if rows.len() > 1 {
        return Err(Error::table(TABLE, "expected exactly one row"));
    }
    // `rows` rejects an empty table, so there is exactly one row here.
    let row = rows.remove(0);
    if row.region_pitch_tiles <= 0 {
        return Err(Error::row(TABLE, 1, "region_pitch_tiles must be positive"));
    }
    if row.region_jitter_pct > 100 {
        return Err(Error::row(
            TABLE,
            1,
            "region_jitter_pct must be at most 100, which is half the pitch",
        ));
    }
    if row.home_bubble_tiles < 0 {
        return Err(Error::row(
            TABLE,
            1,
            "home_bubble_tiles must not be negative",
        ));
    }
    Ok(row)
}

fn tier_rows(text: &str) -> Result<Vec<TierRow>, Error> {
    const TABLE: &str = "tiers.tsv";
    let rows: Vec<TierCsv> = rows(TABLE, text)?;
    let mut out: Vec<TierRow> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let line = i + 1;
        // Contiguous from zero, so `band_of` and `biomes_in` can index by tier.
        if usize::from(row.tier) != i {
            return Err(Error::row(
                TABLE,
                line,
                format!("expected tier {i}, so tiers run contiguously from 0"),
            ));
        }
        if i == 0 && row.inner_tiles != 0 {
            return Err(Error::row(
                TABLE,
                line,
                "the first band must start at the origin",
            ));
        }
        if out.last().is_some_and(|p| row.inner_tiles <= p.inner_tiles) {
            return Err(Error::row(
                TABLE,
                line,
                "inner_tiles must ascend, so a distance lands in one band",
            ));
        }
        if row.stray_pct > 100 {
            return Err(Error::row(TABLE, line, "stray_pct must be at most 100"));
        }
        out.push(TierRow {
            tier: row.tier,
            inner_tiles: row.inner_tiles,
            harder_stray: row.harder_stray,
            easier_stray: row.easier_stray,
            stray_pct: row.stray_pct,
        });
    }
    Ok(out)
}

fn material_rows(text: &str) -> Result<Vec<MaterialRow>, Error> {
    const TABLE: &str = "materials.tsv";
    let rows: Vec<MaterialCsv> = rows(TABLE, text)?;
    if u8::try_from(rows.len()).is_err() {
        return Err(Error::table(
            TABLE,
            "at most 255 materials, since an id is a byte",
        ));
    }
    let mut out: Vec<MaterialRow> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let line = i + 1;
        if out.iter().any(|seen| seen.name == row.material) {
            return Err(Error::row(
                TABLE,
                line,
                format!("duplicate material {:?}", row.material),
            ));
        }
        let mut flags = TileFlags::NONE;
        for (set, flag) in [
            (row.blocks_walk, TileFlags::BLOCKS_WALK),
            (row.blocks_jump, TileFlags::BLOCKS_JUMP),
            (row.blocks_shot, TileFlags::BLOCKS_SHOT),
        ] {
            if set > 1 {
                return Err(Error::row(TABLE, line, "flag columns are 0 or 1"));
            }
            if set == 1 {
                flags = flags.with(flag);
            }
        }
        out.push(MaterialRow {
            name: row.material,
            flags,
        });
    }
    Ok(out)
}

fn biome_rows(
    text: &str,
    materials: &[MaterialRow],
    tier_count: usize,
) -> Result<Vec<BiomeRow>, Error> {
    const TABLE: &str = "biomes.tsv";
    let rows: Vec<BiomeCsv> = rows(TABLE, text)?;
    if u16::try_from(rows.len()).is_err() {
        return Err(Error::table(
            TABLE,
            "at most 65535 biomes, since an id is two bytes",
        ));
    }
    let mut out: Vec<BiomeRow> = Vec::with_capacity(rows.len());
    for (i, row) in rows.into_iter().enumerate() {
        let line = i + 1;
        if out.iter().any(|seen| seen.name == row.biome) {
            return Err(Error::row(
                TABLE,
                line,
                format!("duplicate biome {:?}", row.biome),
            ));
        }
        if usize::from(row.tier) >= tier_count {
            return Err(Error::row(
                TABLE,
                line,
                format!("tier {} has no row in tiers.tsv", row.tier),
            ));
        }
        if row.weight == 0 {
            return Err(Error::row(
                TABLE,
                line,
                "weight must be positive, or the biome can never be picked",
            ));
        }
        if !HEIGHT_RANGE.contains(&row.height_amp) {
            return Err(Error::row(
                TABLE,
                line,
                format!(
                    "height_amp must be within {}..={}",
                    HEIGHT_RANGE.start(),
                    HEIGHT_RANGE.end()
                ),
            ));
        }
        if row.height_period == 0 {
            return Err(Error::row(
                TABLE,
                line,
                "height_period must be positive, since it divides a coordinate",
            ));
        }
        let position = materials
            .iter()
            .position(|m| m.name == row.ground)
            .and_then(|p| u8::try_from(p).ok())
            .ok_or_else(|| {
                Error::row(
                    TABLE,
                    line,
                    format!("ground {:?} has no row in materials.tsv", row.ground),
                )
            })?;
        // A whole biome of unwalkable ground would be a region the player can
        // see and never enter. Blocking materials are cliff faces, not floors.
        if materials[usize::from(position)].flags.blocks_walk() {
            return Err(Error::row(
                TABLE,
                line,
                format!(
                    "ground {:?} blocks walking, so it cannot be a floor",
                    row.ground
                ),
            ));
        }
        let ground = MaterialId(position);
        out.push(BiomeRow {
            name: row.biome,
            tier: row.tier,
            weight: row.weight,
            ground,
            height_amp: row.height_amp,
            height_period: row.height_period,
        });
    }
    Ok(out)
}

/// One list of biome ids per tier, so picking is an index rather than a scan.
///
/// Rejects a tier no biome claims: the generator would have nothing to place
/// there, and a silent fallback would hide the missing content.
fn group_by_tier(biomes: &[BiomeRow], tiers: &[TierRow]) -> Result<Vec<Vec<BiomeId>>, Error> {
    // Ordered, so the grouping cannot depend on hash iteration order.
    let mut grouped: BTreeMap<u8, Vec<BiomeId>> = BTreeMap::new();
    for (i, biome) in biomes.iter().enumerate() {
        let id = u16::try_from(i).map_err(|_| Error::table("biomes.tsv", "too many biomes"))?;
        grouped.entry(biome.tier).or_default().push(BiomeId(id));
    }
    tiers
        .iter()
        .map(|row| {
            grouped.remove(&row.tier).ok_or_else(|| {
                Error::table(
                    "biomes.tsv",
                    format!("tier {} has no biome offering it", row.tier),
                )
            })
        })
        .collect()
}
